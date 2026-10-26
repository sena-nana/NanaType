// Desktop SAPI, without MSIX identity or a second microphone capture path.
#include <windows.h>
#include <sapi.h>
#include <wrl/client.h>

#include <atomic>
#include <condition_variable>
#include <deque>
#include <mutex>
#include <string>
#include <vector>
#include <algorithm>
#include <cstdio>
#include <cstring>

using Microsoft::WRL::ComPtr;
template<class T> static HRESULT create(REFCLSID clsid, ComPtr<T>& result) {
    return CoCreateInstance(clsid, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(result.GetAddressOf()));
}
static HRESULT SpEnumTokens(const wchar_t *category, const wchar_t *required,
        const wchar_t *optional, IEnumSpObjectTokens **tokens) {
    ComPtr<ISpObjectTokenCategory> cat;
    HRESULT hr = create(CLSID_SpObjectTokenCategory, cat);
    if (SUCCEEDED(hr)) hr = cat->SetId(category, FALSE);
    return SUCCEEDED(hr) ? cat->EnumTokens(required, optional, tokens) : hr;
}
// Each event owns the pointer returned by GetEvents until the loop iteration ends.
struct Event : SPEVENT {
    ~Event() {
        if (elParamType == SPET_LPARAM_IS_TOKEN || elParamType == SPET_LPARAM_IS_OBJECT) {
            if (lParam) reinterpret_cast<IUnknown *>(lParam)->Release();
        } else if (elParamType == SPET_LPARAM_IS_POINTER || elParamType == SPET_LPARAM_IS_STRING) {
            CoTaskMemFree(reinterpret_cast<void *>(lParam));
        }
    }
};
typedef void (*NTCallback)(void *, int, const char *);
typedef void (*NTRelease)(void *);
static std::string utf8(const wchar_t *value) {
    int n = WideCharToMultiByte(CP_UTF8, 0, value, -1, nullptr, 0, nullptr, nullptr);
    if (n <= 0) return {};
    std::vector<char> result(n);
    WideCharToMultiByte(CP_UTF8, 0, value, -1, result.data(), n, nullptr, nullptr);
    return std::string(result.data());
}
struct Apartment {
    HRESULT result = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    ~Apartment() { if (SUCCEEDED(result)) CoUninitialize(); }
};
// SAPI reads on its own thread. EOF wakes that thread on pause/stop.
class AudioStream final : public IStream {
    std::atomic<ULONG> refs{1};
    std::mutex mutex;
    std::condition_variable ready;
    std::deque<unsigned char> bytes;
    bool ended = false;
public:
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID iid, void **out) override {
        if (!out) return E_POINTER;
        *out = nullptr;
        if (iid == IID_IUnknown || iid == IID_ISequentialStream || iid == IID_IStream) {
            *out = static_cast<IStream *>(this); AddRef(); return S_OK;
        }
        return E_NOINTERFACE;
    }
    ULONG STDMETHODCALLTYPE AddRef() override { return ++refs; }
    ULONG STDMETHODCALLTYPE Release() override { ULONG n = --refs; if (!n) delete this; return n; }
    HRESULT STDMETHODCALLTYPE Read(void *out, ULONG length, ULONG *read) override {
        std::unique_lock<std::mutex> lock(mutex);
        ready.wait(lock, [&] { return ended || bytes.size() >= length; });
        ULONG n = static_cast<ULONG>((std::min)(static_cast<size_t>(length), bytes.size()));
        for (ULONG i = 0; i < n; ++i) { static_cast<unsigned char *>(out)[i] = bytes.front(); bytes.pop_front(); }
        if (read) *read = n;
        return n == length ? S_OK : S_FALSE;
    }
    bool append(const float *data, size_t length) {
        std::lock_guard<std::mutex> lock(mutex);
        if (ended || bytes.size() + length * 2 > 16000 * 2 * 30) return false;
        for (size_t i = 0; i < length; ++i) {
            float value = (std::max)(-1.0f, (std::min)(1.0f, data[i]));
            short sample = static_cast<short>(value * 32767);
            bytes.push_back(static_cast<unsigned char>(sample & 255));
            bytes.push_back(static_cast<unsigned char>((sample >> 8) & 255));
        }
        ready.notify_one(); return true;
    }
    void end() { std::lock_guard<std::mutex> lock(mutex); ended = true; ready.notify_all(); }
    HRESULT STDMETHODCALLTYPE Write(const void *, ULONG, ULONG *) override { return STG_E_ACCESSDENIED; }
    HRESULT STDMETHODCALLTYPE Seek(LARGE_INTEGER offset, DWORD origin, ULARGE_INTEGER *position) override {
        if (origin == STREAM_SEEK_CUR && offset.QuadPart == 0) { if (position) position->QuadPart = 0; return S_OK; }
        return STG_E_INVALIDFUNCTION;
    }
    HRESULT STDMETHODCALLTYPE SetSize(ULARGE_INTEGER) override { return E_NOTIMPL; }
    HRESULT STDMETHODCALLTYPE CopyTo(IStream *, ULARGE_INTEGER, ULARGE_INTEGER *, ULARGE_INTEGER *) override { return E_NOTIMPL; }
    HRESULT STDMETHODCALLTYPE Commit(DWORD) override { return S_OK; }
    HRESULT STDMETHODCALLTYPE Revert() override { return E_NOTIMPL; }
    HRESULT STDMETHODCALLTYPE LockRegion(ULARGE_INTEGER, ULARGE_INTEGER, DWORD) override { return E_NOTIMPL; }
    HRESULT STDMETHODCALLTYPE UnlockRegion(ULARGE_INTEGER, ULARGE_INTEGER, DWORD) override { return E_NOTIMPL; }
    HRESULT STDMETHODCALLTYPE Stat(STATSTG *stat, DWORD) override {
        if (!stat) return E_POINTER;
        memset(stat, 0, sizeof(*stat)); stat->type = STGTY_STREAM; stat->grfMode = STGM_READ; return S_OK;
    }
    HRESULT STDMETHODCALLTYPE Clone(IStream **) override { return E_NOTIMPL; }
};
static HRESULT find_token(const char *language, ISpObjectToken **out) {
    wchar_t tag[128] = {};
    if (!MultiByteToWideChar(CP_UTF8, 0, language, -1, tag, 128)) return E_INVALIDARG;
    LCID id = LocaleNameToLCID(tag, 0);
    if (!id) return E_INVALIDARG;
    wchar_t attributes[64];
    swprintf_s(attributes, L"Language=%x", static_cast<unsigned>(LANGIDFROMLCID(id)));
    ComPtr<IEnumSpObjectTokens> tokens;
    HRESULT hr = SpEnumTokens(SPCAT_RECOGNIZERS, attributes, nullptr, &tokens);
    if (FAILED(hr)) return hr;
    return tokens->Next(1, out, nullptr) == S_OK ? S_OK : E_FAIL;
}
struct Recognition {
    Apartment apartment;
    ComPtr<ISpRecognizer> recognizer;
    ComPtr<ISpRecoContext> context;
    ComPtr<ISpRecoGrammar> grammar;
    ComPtr<ISpStream> stream;
    AudioStream *audio = new AudioStream;
    void *delivery;
    NTCallback callback;
    NTRelease releaseContext;
    std::string text;
    bool delivered = false;
    Recognition(void *d, NTCallback c, NTRelease r) : delivery(d), callback(c), releaseContext(r) {}
    ~Recognition() {
        audio->end();
        if (recognizer) recognizer->SetRecoState(SPRST_INACTIVE_WITH_PURGE);
        audio->Release(); releaseContext(delivery);
    }
};
extern "C" {
int nt_microphone_permission() { return 1; } // WASAPI reports the OS privacy/device error on opening.
int nt_system_permission() { return 1; }
size_t nt_system_languages(char *buffer, size_t length) {
    Apartment apartment;
    ComPtr<IEnumSpObjectTokens> tokens;
    std::string text;
    if (SUCCEEDED(SpEnumTokens(SPCAT_RECOGNIZERS, nullptr, nullptr, &tokens))) {
        ComPtr<ISpObjectToken> token;
        while (tokens->Next(1, &token, nullptr) == S_OK) {
            ComPtr<ISpDataKey> attributes;
            LPWSTR value = nullptr;
            if (SUCCEEDED(token->OpenKey(L"Attributes", &attributes)) && SUCCEEDED(attributes->GetStringValue(L"Language", &value))) {
                wchar_t tag[LOCALE_NAME_MAX_LENGTH] = {};
                LCID id = static_cast<LCID>(wcstoul(value, nullptr, 16));
                if (LCIDToLocaleName(id, tag, LOCALE_NAME_MAX_LENGTH, 0)) { text += utf8(tag); text += '\n'; }
                CoTaskMemFree(value);
            }
            token.Reset();
        }
    }
    if (length > text.size()) memcpy(buffer, text.c_str(), text.size() + 1);
    return text.size();
}
int nt_system_available(const char *language) {
    Apartment apartment;
    ComPtr<ISpObjectToken> token;
    return SUCCEEDED(find_token(language, &token));
}
void *nt_system_create(const char *language, void *delivery, NTCallback callback, NTRelease releaseContext) {
    auto session = new Recognition(delivery, callback, releaseContext);
    ComPtr<ISpObjectToken> token;
    HRESULT hr = find_token(language, &token);
    if (SUCCEEDED(hr)) hr = create(CLSID_SpInprocRecognizer, session->recognizer);
    if (SUCCEEDED(hr)) hr = session->recognizer->SetRecognizer(token.Get());
    if (SUCCEEDED(hr)) hr = create(CLSID_SpStream, session->stream);
    WAVEFORMATEX format = { WAVE_FORMAT_PCM, 1, 16000, 32000, 2, 16, 0 };
    if (SUCCEEDED(hr)) hr = session->stream->SetBaseStream(session->audio, SPDFID_WaveFormatEx, &format);
    if (SUCCEEDED(hr)) hr = session->recognizer->SetInput(session->stream.Get(), FALSE);
    if (SUCCEEDED(hr)) hr = session->recognizer->CreateRecoContext(&session->context);
    if (SUCCEEDED(hr)) hr = session->context->SetNotifyWin32Event();
    ULONGLONG interest = SPFEI(SPEI_RECOGNITION) | SPFEI(SPEI_HYPOTHESIS) | SPFEI(SPEI_END_SR_STREAM);
    if (SUCCEEDED(hr)) hr = session->context->SetInterest(interest, interest);
    if (SUCCEEDED(hr)) hr = session->context->CreateGrammar(1, &session->grammar);
    if (SUCCEEDED(hr)) hr = session->grammar->LoadDictation(nullptr, SPLO_STATIC);
    if (SUCCEEDED(hr)) hr = session->grammar->SetDictationState(SPRS_ACTIVE);
    if (FAILED(hr)) { delete session; return nullptr; }
    return session;
}
int nt_system_append(void *handle, const float *samples, size_t length) {
    return static_cast<Recognition *>(handle)->audio->append(samples, length) ? 1 : 0;
}
void nt_system_end(void *handle) {
    auto session = static_cast<Recognition *>(handle);
    session->audio->end();
}
void nt_system_poll(void *handle) {
    auto session = static_cast<Recognition *>(handle);
    for (;;) {
        Event event{};
        ULONG count = 0;
        if (FAILED(session->context->GetEvents(1, &event, &count))) {
            session->callback(session->delivery, 2, "speech-system-error");
            return;
        }
        if (!count) break;
        if (event.eEventId == SPEI_RECOGNITION || event.eEventId == SPEI_HYPOTHESIS) {
            LPWSTR value = nullptr;
            if (SUCCEEDED(reinterpret_cast<ISpRecoResult *>(event.lParam)->GetText(SP_GETWHOLEPHRASE, SP_GETWHOLEPHRASE, TRUE, &value, nullptr))) {
                std::string text = utf8(value);
                CoTaskMemFree(value);
                if (event.eEventId == SPEI_RECOGNITION) {
                    if (!session->text.empty()) session->text += " ";
                    session->text += text;
                }
                std::string partial = session->text;
                if (event.eEventId == SPEI_HYPOTHESIS) { if (!partial.empty()) partial += " "; partial += text; }
                session->callback(session->delivery, 0, partial.c_str());
            }
        } else if (event.eEventId == SPEI_END_SR_STREAM && !session->delivered) {
            session->delivered = true;
            if (FAILED(static_cast<HRESULT>(event.lParam))) session->callback(session->delivery, 2, "speech-system-error");
            else session->callback(session->delivery, 1, session->text.c_str());
        }
    }
}
void nt_system_close(void *handle) { delete static_cast<Recognition *>(handle); }
}
