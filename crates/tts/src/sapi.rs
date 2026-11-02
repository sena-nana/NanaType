//! SAPI 5 (`ISpVoice`), polled for its events on the worker thread.
use crate::{Engine, Event, Utterance, Voice, utf16_range};
use std::mem::ManuallyDrop;
use windows::Win32::Globalization::LCIDToLocaleName;
use windows::Win32::Media::Speech::{
    IEnumSpObjectTokens, ISpObjectToken, ISpObjectTokenCategory, ISpVoice, SPCAT_VOICES,
    SPEI_END_INPUT_STREAM, SPEI_START_INPUT_STREAM, SPEI_WORD_BOUNDARY, SPEVENT, SPF_ASYNC,
    SPF_IS_NOT_XML, SPF_PURGEBEFORESPEAK, SpObjectTokenCategory, SpVoice,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    CoUninitialize,
};
use windows::core::{PCWSTR, PWSTR, w};

struct Pending {
    id: u64,
    stream: u32,
    text: String,
}

pub(crate) struct Sapi {
    /// Released before COM is uninitialized.
    voice: ManuallyDrop<ISpVoice>,
    pending: Vec<Pending>,
    /// This thread's COM initialization is ours to undo.
    com: bool,
}

/// `SPFEI(id)` for the events read: start, end and word boundaries.
const INTEREST: u64 = (1 << SPEI_START_INPUT_STREAM.0)
    | (1 << SPEI_END_INPUT_STREAM.0)
    | (1 << SPEI_WORD_BOUNDARY.0)
    | (1 << 30)
    | (1 << 33);

impl Sapi {
    pub fn new() -> Option<Self> {
        unsafe {
            let com = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            let voice = CoCreateInstance::<_, ISpVoice>(&SpVoice, None, CLSCTX_ALL)
                .and_then(|v| v.SetInterest(INTEREST, INTEREST).map(|()| v));
            match voice {
                Ok(voice) => Some(Self {
                    voice: ManuallyDrop::new(voice),
                    pending: Vec::new(),
                    com,
                }),
                Err(_) => {
                    if com {
                        CoUninitialize();
                    }
                    None
                }
            }
        }
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A string SAPI allocated, freed after reading.
unsafe fn take(p: PWSTR) -> String {
    let s = unsafe { p.to_string() }.unwrap_or_default();
    unsafe { CoTaskMemFree(Some(p.0 as *const _)) };
    s
}

fn tokens() -> Vec<ISpObjectToken> {
    unsafe {
        let Ok(category) =
            CoCreateInstance::<_, ISpObjectTokenCategory>(&SpObjectTokenCategory, None, CLSCTX_ALL)
        else {
            return Vec::new();
        };
        if category.SetId(SPCAT_VOICES, false).is_err() {
            return Vec::new();
        }
        let Ok(list): windows::core::Result<IEnumSpObjectTokens> =
            category.EnumTokens(PCWSTR::null(), PCWSTR::null())
        else {
            return Vec::new();
        };
        let mut count = 0u32;
        if list.GetCount(&mut count).is_err() {
            return Vec::new();
        }
        (0..count).filter_map(|i| list.Item(i).ok()).collect()
    }
}

fn token_id(token: &ISpObjectToken) -> Option<String> {
    unsafe { token.GetId().ok().map(|p| take(p)) }
}

/// The first LCID of the token's `Language` attribute ("409;9") as a tag.
fn language(token: &ISpObjectToken) -> String {
    unsafe {
        let Ok(attributes) = token.OpenKey(w!("Attributes")) else {
            return String::new();
        };
        let Ok(value) = attributes.GetStringValue(w!("Language")) else {
            return String::new();
        };
        let value = take(value);
        let Some(lcid) = value
            .split(';')
            .next()
            .and_then(|v| u32::from_str_radix(v.trim(), 16).ok())
        else {
            return String::new();
        };
        let mut name = [0u16; 85];
        let n = LCIDToLocaleName(lcid, Some(&mut name), 0);
        if n <= 1 {
            return String::new();
        }
        String::from_utf16_lossy(&name[..n as usize - 1])
    }
}

/// A multiple of the normal pace as SAPI's -10..10, where 10 is three times as fast.
fn sapi_rate(rate: f32) -> i32 {
    (10.0 * rate.max(0.01).ln() / 3f32.ln())
        .round()
        .clamp(-10.0, 10.0) as i32
}

impl Engine for Sapi {
    fn speak(&mut self, u: Utterance) {
        unsafe {
            let token = u.voice.as_deref().and_then(|id| {
                tokens()
                    .into_iter()
                    .find(|t| token_id(t).as_deref() == Some(id))
            });
            // An unknown voice falls back to the default one.
            let _ = self.voice.SetVoice(token.as_ref());
            let _ = self.voice.SetRate(sapi_rate(u.rate));
            let _ = self.voice.SetVolume((u.volume * 100.0).round() as u16);
            let text = wide(&u.text);
            let mut stream = 0u32;
            if self
                .voice
                .Speak(
                    PCWSTR(text.as_ptr()),
                    (SPF_ASYNC.0 | SPF_IS_NOT_XML.0) as u32,
                    Some(&mut stream),
                )
                .is_ok()
            {
                self.pending.push(Pending {
                    id: u.id,
                    stream,
                    text: u.text,
                });
            }
        }
    }

    fn stop(&mut self) {
        unsafe {
            let _ = self.voice.Speak(
                PCWSTR::null(),
                (SPF_ASYNC.0 | SPF_PURGEBEFORESPEAK.0) as u32,
                None,
            );
        }
        self.pending.clear();
        let mut ignore = |_| {};
        // Drop the events of what was purged.
        self.poll(&mut ignore);
    }

    fn voices(&mut self) -> Vec<Voice> {
        let mut voices: Vec<Voice> = tokens()
            .iter()
            .filter_map(|t| {
                let id = token_id(t)?;
                let name = unsafe { t.GetStringValue(PCWSTR::null()).ok().map(|p| take(p)) }
                    .unwrap_or_else(|| id.rsplit('\\').next().unwrap_or_default().to_owned());
                Some(Voice {
                    id,
                    name,
                    language: language(t),
                })
            })
            .collect();
        voices.sort_by(|a, b| (&a.language, &a.name).cmp(&(&b.language, &b.name)));
        voices
    }

    fn poll(&mut self, out: &mut dyn FnMut(Event)) {
        loop {
            let mut event = SPEVENT::default();
            let mut fetched = 0u32;
            if unsafe { self.voice.GetEvents(1, &mut event, &mut fetched) }.is_err() || fetched == 0
            {
                return;
            }
            // The low 16 bits are the event id. None of these carry an
            // allocated lParam, so nothing needs freeing.
            let kind = event._bitfield & 0xffff;
            let Some(i) = self
                .pending
                .iter()
                .position(|p| p.stream == event.ulStreamNum)
            else {
                continue;
            };
            let p = &self.pending[i];
            if kind == SPEI_START_INPUT_STREAM.0 {
                out(Event::Started(p.id));
            } else if kind == SPEI_WORD_BOUNDARY.0 {
                let (start, len) = utf16_range(&p.text, event.lParam.0 as usize, event.wParam.0);
                out(Event::Word {
                    id: p.id,
                    start,
                    len,
                });
            } else if kind == SPEI_END_INPUT_STREAM.0 {
                out(Event::Finished(p.id));
                self.pending.remove(i);
            }
        }
    }

    fn busy(&self) -> bool {
        !self.pending.is_empty()
    }
}

impl Drop for Sapi {
    fn drop(&mut self) {
        self.stop();
        unsafe {
            ManuallyDrop::drop(&mut self.voice);
            if self.com {
                CoUninitialize();
            }
        }
    }
}
