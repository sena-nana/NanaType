[简体中文](../voice-input.md) · [English](voice-input.md) · [日本語](../ja/voice-input.md)

# Voice input

> **Purpose:** Configure microphone input and choose between system and on-device speech recognition.
>
> **On this page:** [Voice modes](#voice-input) · [Recognition methods](#recognition-method-and-language) · [Model management](#recognition-method-and-language) · [Implementation and checks](#implementation-and-verification)


The microphone button in the input window's bottom bar starts and stops voice input; the menu next to it switches the mode, resumes recognition and opens the settings. **Settings › Input › Voice** gathers the recognition method, microphone, language and pause length in one place, and manages the local model.

The voice mode is independent of the existing **Compose & send** / **Live mirror** modes:

- **Edit manually** (the default on first run): confirmed recognized text goes into the draft and you send it yourself.
- **Send automatically**: once you stop speaking, NanaType waits 3 seconds by default (configurable from 1 to 10 seconds) and, as soon as recognition has finished, submits the current line. Speaking again cancels the countdown.

Typing, IME composition, pasting, deleting, undo/redo, formatting, inserting a phrase or recalling an earlier line all pause recognition and cancel the automatic send for that line.

Listening resumes after a successful manual send; you can also click **Resume recognition** after clearing or keeping the draft.

Merely moving the cursor, selecting text or switching between the rich-text and source views does not pause it.

If there is already a draft when you start, voice results are appended to the end and that line must be sent manually.

Changing recognition settings also pauses recognition, and the current draft has to be confirmed manually first.

Interim results are shown in the input window's status area and only written into the draft once confirmed.

Recognized text is always inserted as plain text: saying `[pause]` out loud will not run any markup.

Once turned on, it keeps listening even when the input window loses focus, is hidden or is minimized. The tray shows the current state and offers a way to stop. The microphone is never turned on automatically after restarting the app; pausing, stopping, device errors and quitting all release the capture device. Stopping keeps the text already confirmed and does not send the current draft.

Automatic sending goes through the normal send path, so it respects the playback queue, online-session permissions and any chat sync you have turned on. If a submission is rejected, the draft is kept and recognition pauses; there is no automatic retry. Set up chat sync according to what you actually need.

## Recognition method and language

**Prefer system** first uses the system recognizer for the selected language and falls back to the installed local model when that isn't available; you can also choose system or local explicitly.

- **macOS**: uses the Speech framework's `SFSpeechRecognizer`; the first run asks for microphone and speech recognition permission. When the system supports on-device recognition it is processed on the device; otherwise Apple's online service may be used. Permissions can be managed in System Settings.
- **Windows**: uses the recognition engines installed for desktop SAPI; MSIX is not required. The available languages depend on the engines installed on the system. If no engine is available, pick the local model. If the microphone is unavailable, check Windows' microphone permission for desktop apps and the device connection.
- **Local model**: Whisper base multilingual Q5_1, 59,707,625 bytes, CPU inference, no Python or external recognition process needed. Once installed, audio never leaves the device. Chinese, English and Japanese are listed first; the other languages come from the backend's supported list. Automatic language detection is only available with the local model and never translates.

The default recognition language follows the UI language. If a backend error occurs during recognition, NanaType does not silently carry on with another backend, to avoid mixed or duplicated results; the draft is kept and you can continue after changing the recognition method.

The model is downloaded on demand with **Download / retry**, or installed via **Import model file…** from the same file obtained upstream.

Cancelling and deleting are supported; while a download is running, no other installation can start.

The file is stored as `speech/ggml-base-q5_1.bin` in the app data directory.

Downloads and imports are first written to a temporary file and installed only after verification.

Any other model, and any corrupted file, is rejected.

Upstream file: [ggerganov/whisper.cpp](https://huggingface.co/ggerganov/whisper.cpp/blob/main/ggml-base-q5_1.bin)

SHA-256: `422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898`

The app does not keep recordings and never writes recognized text to the diagnostic logs. Third-party licenses ship with the app; see `THIRD_PARTY_NOTICES.md`.

## Implementation and verification

Capture, resampling and voice activity detection are independent of the UI and of the local inference thread.

Input is mono 16 kHz, with 20 ms VAD frames and 200 ms of pre-roll audio; 500 ms of silence ends a recognition segment, and segments are cut at 20 seconds at most.

During long sentences the local model tries to refresh the interim result every 3 seconds, and final results take precedence over interim refreshes.

After a normal send, the listener and model are reused, and a draft version number invalidates stale timer tasks.

The automatic-send pause is independent of recognition segmentation and is measured from voice activity timestamps.

The start/stop state is derived from the recognition phase alone; pending and finished segments share one ordered table. The digest is verified while the model is being copied, so the file doesn't have to be read back after installation; audio format conversion and mono downmixing happen in the same pass.

Capture and inference use bounded queues; overload, device errors and recognition timeouts all pause and keep the draft. A session generation counter isolates stale callbacks; segments are merged in audio order and de-duplicated. Input routing raises an edit barrier early, and both timers and result callbacks check it, so nothing is sent automatically while the IME still has uncommitted text.

Automated tests cover the state machine, settings compatibility, model verification, edit-key classification, escaping of markup in the text, and the real input/settings views. The real-model-file test is run separately:

```sh
NANATYPE_SPEECH_FIXTURES=/absolute/fixture/directory \
  cargo test -p nanatype-speech offline_fixtures -- --ignored --nocapture
```

The directory contains the fixed model above plus `jfk.wav`, `zh.wav` and `ja.wav` (16 kHz, mono PCM16). This test checks real local decoding and prints load time, inference time and real-time factor; it does not replace testing with a real microphone, permission prompts, system recognition quality or long live streams.
