//! AVSpeechSynthesizer. Its delegate is called on the main run loop, so it only
//! queues what happened; the worker turns that into events when it polls.
use crate::{Engine, Event, Utterance, Voice, utf16_range};
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::ProtocolObject;
use objc2::{AnyThread, DefinedClass, define_class, msg_send, sel};
use objc2_avf_audio::{
    AVSpeechBoundary, AVSpeechSynthesisVoice, AVSpeechSynthesisVoiceTraits, AVSpeechSynthesizer,
    AVSpeechSynthesizerDelegate, AVSpeechUtterance, AVSpeechUtteranceDefaultSpeechRate,
    AVSpeechUtteranceMaximumSpeechRate, AVSpeechUtteranceMinimumSpeechRate,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSRange, NSString};
use std::sync::Mutex;
use std::time::{Duration, Instant};

enum Raw {
    Started,
    Word(NSRange),
    Finished,
}

/// Delegate calls, keyed by the utterance's address.
type Inbox = Mutex<Vec<(usize, Raw)>>;

define_class!(
    // SAFETY: NSObject has no subclassing requirements and Delegate has no Drop.
    #[unsafe(super(NSObject))]
    #[name = "NanaTypeSpeechDelegate"]
    #[ivars = Inbox]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl AVSpeechSynthesizerDelegate for Delegate {
        #[unsafe(method(speechSynthesizer:didStartSpeechUtterance:))]
        fn did_start(&self, _: &AVSpeechSynthesizer, utterance: &AVSpeechUtterance) {
            self.push(utterance, Raw::Started);
        }

        #[unsafe(method(speechSynthesizer:didFinishSpeechUtterance:))]
        fn did_finish(&self, _: &AVSpeechSynthesizer, utterance: &AVSpeechUtterance) {
            self.push(utterance, Raw::Finished);
        }

        #[unsafe(method(speechSynthesizer:didCancelSpeechUtterance:))]
        fn did_cancel(&self, _: &AVSpeechSynthesizer, utterance: &AVSpeechUtterance) {
            self.push(utterance, Raw::Finished);
        }

        #[unsafe(method(speechSynthesizer:willSpeakRangeOfSpeechString:utterance:))]
        fn will_speak(
            &self,
            _: &AVSpeechSynthesizer,
            range: NSRange,
            utterance: &AVSpeechUtterance,
        ) {
            self.push(utterance, Raw::Word(range));
        }
    }
);

impl Delegate {
    fn create() -> Retained<Self> {
        let this = Self::alloc().set_ivars(Mutex::new(Vec::new()));
        unsafe { msg_send![super(this), init] }
    }
    fn push(&self, utterance: &AVSpeechUtterance, raw: Raw) {
        let key = utterance as *const AVSpeechUtterance as usize;
        self.ivars()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((key, raw));
    }
    fn take(&self) -> Vec<(usize, Raw)> {
        std::mem::take(&mut *self.ivars().lock().unwrap_or_else(|e| e.into_inner()))
    }
}

struct Pending {
    id: u64,
    text: String,
    utterance: Retained<AVSpeechUtterance>,
    started: bool,
    queued_at: Instant,
}

/// The delegate is called from the main run loop; where none runs (a test
/// binary) the speaking flag ends utterances instead, after this long idle.
const IDLE_GRACE: Duration = Duration::from_millis(600);

pub(crate) struct Mac {
    synth: Retained<AVSpeechSynthesizer>,
    delegate: Retained<Delegate>,
    pending: Vec<Pending>,
    idle_since: Option<Instant>,
}

impl Mac {
    pub fn new() -> Self {
        autoreleasepool(|_| {
            let synth = unsafe { AVSpeechSynthesizer::new() };
            let delegate = Delegate::create();
            unsafe { synth.setDelegate(Some(ProtocolObject::from_ref(&*delegate))) };
            Self {
                synth,
                delegate,
                pending: Vec::new(),
                idle_since: None,
            }
        })
    }
}

/// A multiple of the normal pace as AVSpeechUtterance's 0–1 rate, which
/// puts normal at its default (0.5) and runs fast above it.
fn av_rate(rate: f32) -> f32 {
    let (min, normal, max) = unsafe {
        (
            AVSpeechUtteranceMinimumSpeechRate,
            AVSpeechUtteranceDefaultSpeechRate,
            AVSpeechUtteranceMaximumSpeechRate,
        )
    };
    let v = if rate <= 1.0 {
        normal * rate
    } else {
        normal + (max - normal) * (rate - 1.0)
    };
    v.clamp(min, max)
}

impl Engine for Mac {
    fn speak(&mut self, u: Utterance) {
        autoreleasepool(|_| {
            let utterance = unsafe {
                AVSpeechUtterance::speechUtteranceWithString(&NSString::from_str(&u.text))
            };
            unsafe {
                if let Some(id) = &u.voice
                    && let Some(voice) =
                        AVSpeechSynthesisVoice::voiceWithIdentifier(&NSString::from_str(id))
                {
                    utterance.setVoice(Some(&voice));
                }
                utterance.setRate(av_rate(u.rate));
                utterance.setVolume(u.volume);
                self.synth.speakUtterance(&utterance);
            }
            self.pending.push(Pending {
                id: u.id,
                text: u.text,
                utterance,
                started: false,
                queued_at: Instant::now(),
            });
        });
    }

    fn stop(&mut self) {
        autoreleasepool(|_| unsafe {
            self.synth
                .stopSpeakingAtBoundary(AVSpeechBoundary::Immediate);
        });
        self.pending.clear();
        self.idle_since = None;
        // Cancel callbacks of what was stopped name nothing pending any more.
        self.delegate.take();
    }

    fn voices(&mut self) -> Vec<Voice> {
        autoreleasepool(|_| {
            let mut voices: Vec<Voice> = unsafe { AVSpeechSynthesisVoice::speechVoices() }
                .iter()
                .filter(|v| !novelty(v))
                .map(|v| unsafe {
                    Voice {
                        id: v.identifier().to_string(),
                        name: v.name().to_string(),
                        language: v.language().to_string(),
                    }
                })
                .collect();
            voices.sort_by(|a, b| (&a.language, &a.name).cmp(&(&b.language, &b.name)));
            voices
        })
    }

    fn poll(&mut self, out: &mut dyn FnMut(Event)) {
        for (key, raw) in self.delegate.take() {
            let Some(i) = self
                .pending
                .iter()
                .position(|p| Retained::as_ptr(&p.utterance) as usize == key)
            else {
                continue;
            };
            let p = &mut self.pending[i];
            match raw {
                Raw::Started if !p.started => {
                    p.started = true;
                    out(Event::Started(p.id));
                }
                Raw::Started => {}
                Raw::Word(range) => {
                    let (start, len) = utf16_range(&p.text, range.location, range.length);
                    out(Event::Word {
                        id: p.id,
                        start,
                        len,
                    });
                }
                Raw::Finished => {
                    let id = p.id;
                    self.pending.remove(i);
                    out(Event::Finished(id));
                }
            }
        }
        if self.pending.is_empty() {
            self.idle_since = None;
            return;
        }
        let speaking = unsafe { self.synth.isSpeaking() };
        if speaking {
            self.idle_since = None;
            if let Some(p) = self.pending.first_mut()
                && !p.started
            {
                p.started = true;
                out(Event::Started(p.id));
            }
            return;
        }
        let now = Instant::now();
        let idle = *self.idle_since.get_or_insert(now);
        let queued = self
            .pending
            .iter()
            .map(|p| p.queued_at)
            .max()
            .unwrap_or(now);
        if now.duration_since(idle) >= IDLE_GRACE && now.duration_since(queued) >= IDLE_GRACE {
            for p in self.pending.drain(..) {
                if !p.started {
                    out(Event::Started(p.id));
                }
                out(Event::Finished(p.id));
            }
            self.idle_since = None;
        }
    }

    fn busy(&self) -> bool {
        !self.pending.is_empty()
    }
}

/// Bells, Bubbles and the other joke voices (macOS 14 tells them apart).
fn novelty(voice: &AVSpeechSynthesisVoice) -> bool {
    voice.respondsToSelector(sel!(voiceTraits))
        && unsafe { voice.voiceTraits() }.contains(AVSpeechSynthesisVoiceTraits::IsNoveltyVoice)
}

impl Drop for Mac {
    fn drop(&mut self) {
        autoreleasepool(|_| unsafe {
            self.synth
                .stopSpeakingAtBoundary(AVSpeechBoundary::Immediate);
            self.synth.setDelegate(None);
        });
    }
}
