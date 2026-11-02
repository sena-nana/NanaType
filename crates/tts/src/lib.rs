//! Lines read aloud by the operating system's own voices: AVSpeechSynthesizer
//! on macOS, SAPI on Windows. No network and no models; other systems stay
//! silent. One worker thread owns the engine, so callers never block on it.
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod sapi;
mod text;

pub use text::speakable;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::Duration;

/// An installed voice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Voice {
    /// Stable across launches; what settings keep.
    pub id: String,
    pub name: String,
    /// BCP 47 tag, `en-US`.
    pub language: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Started(u64),
    /// A word is about to be spoken: a byte range of the utterance's text.
    Word {
        id: u64,
        start: usize,
        len: usize,
    },
    /// Spoken to the end, or stopped.
    Finished(u64),
    /// Answer to [`Tts::request_voices`].
    Voices(Vec<Voice>),
}

/// Rates [`Speak::speak`] accepts: a multiple of the voice's own pace.
pub const RATE_RANGE: (f32, f32) = (0.5, 2.0);

/// What the application needs from an engine, so its policy can be tested
/// with a fake one.
pub trait Speak {
    /// Say `text` after whatever is still being said. `voice` is a
    /// [`Voice::id`]; `None` (or an unknown one) is the system default.
    /// Returns the utterance's id for [`Event`]s.
    fn speak(&mut self, text: &str, voice: Option<&str>, rate: f32, volume: f32) -> u64;
    /// Silence what is said and drop what is queued.
    fn stop(&mut self);
}

#[derive(Debug, Clone)]
pub(crate) struct Utterance {
    pub id: u64,
    pub text: String,
    pub voice: Option<String>,
    pub rate: f32,
    pub volume: f32,
}

/// A platform synthesizer, driven from the worker thread only.
pub(crate) trait Engine {
    fn speak(&mut self, utterance: Utterance);
    /// Stop everything; the worker reports the unfinished ones as finished.
    fn stop(&mut self);
    fn voices(&mut self) -> Vec<Voice>;
    /// Hand over the events since the last call.
    fn poll(&mut self, out: &mut dyn FnMut(Event));
    /// Something is queued or being said, so the worker keeps polling.
    fn busy(&self) -> bool;
}

enum Command {
    Speak(Utterance),
    Stop,
    Voices(Option<Sender<Vec<Voice>>>),
}

type Deliver = Arc<dyn Fn(Event) + Send + Sync>;

/// How often a busy worker collects engine events (word boundaries).
const POLL: Duration = Duration::from_millis(15);

/// The handle. Dropping it stops speech and ends the worker.
pub struct Tts {
    tx: Sender<Command>,
    next: u64,
}

impl Tts {
    pub fn new(deliver: impl Fn(Event) + Send + Sync + 'static) -> Self {
        let (tx, rx) = crossbeam_channel::unbounded();
        let deliver: Deliver = Arc::new(deliver);
        // Without a thread nothing is said; the handle still works.
        let _ = std::thread::Builder::new()
            .name("nanatype-tts".into())
            .spawn(move || run(rx, deliver));
        Self { tx, next: 0 }
    }

    /// Whether this system has an engine at all.
    pub const fn available() -> bool {
        cfg!(any(target_os = "macos", windows))
    }

    /// The installed voices, waiting for the worker (at most two seconds).
    pub fn voices(&self) -> Vec<Voice> {
        let (tx, rx) = crossbeam_channel::bounded(1);
        if self.tx.send(Command::Voices(Some(tx))).is_err() {
            return Vec::new();
        }
        rx.recv_timeout(Duration::from_secs(2)).unwrap_or_default()
    }

    /// The installed voices, delivered later as [`Event::Voices`].
    pub fn request_voices(&self) {
        let _ = self.tx.send(Command::Voices(None));
    }
}

impl Speak for Tts {
    fn speak(&mut self, text: &str, voice: Option<&str>, rate: f32, volume: f32) -> u64 {
        self.next += 1;
        let _ = self.tx.send(Command::Speak(Utterance {
            id: self.next,
            text: text.to_owned(),
            voice: voice.map(str::to_owned),
            rate: clamp_or(rate, RATE_RANGE.0, RATE_RANGE.1, 1.0),
            volume: clamp_or(volume, 0.0, 1.0, 1.0),
        }));
        self.next
    }

    fn stop(&mut self) {
        let _ = self.tx.send(Command::Stop);
    }
}

fn clamp_or(v: f32, lo: f32, hi: f32, fallback: f32) -> f32 {
    if v.is_finite() {
        v.clamp(lo, hi)
    } else {
        fallback
    }
}

fn engine() -> Option<Box<dyn Engine>> {
    #[cfg(target_os = "macos")]
    return Some(Box::new(macos::Mac::new()));
    #[cfg(windows)]
    return sapi::Sapi::new().map(|e| Box::new(e) as Box<dyn Engine>);
    #[cfg(not(any(target_os = "macos", windows)))]
    None
}

fn run(rx: Receiver<Command>, deliver: Deliver) {
    let mut engine = engine();
    // Said or queued and not reported finished yet, oldest first.
    let mut open: Vec<u64> = Vec::new();
    loop {
        let busy = engine.as_ref().is_some_and(|e| e.busy()) || !open.is_empty();
        let command = if busy {
            match rx.recv_timeout(POLL) {
                Ok(c) => Some(c),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match rx.recv() {
                Ok(c) => Some(c),
                Err(_) => break,
            }
        };
        let Some(engine) = engine.as_mut() else {
            match command {
                Some(Command::Voices(Some(reply))) => drop(reply.send(Vec::new())),
                Some(Command::Voices(None)) => deliver(Event::Voices(Vec::new())),
                _ => {}
            }
            continue;
        };
        match command {
            Some(Command::Speak(u)) if u.text.trim().is_empty() => {
                deliver(Event::Started(u.id));
                deliver(Event::Finished(u.id));
            }
            Some(Command::Speak(u)) => {
                open.push(u.id);
                engine.speak(u);
            }
            Some(Command::Stop) => {
                engine.stop();
                for id in open.drain(..) {
                    deliver(Event::Finished(id));
                }
            }
            Some(Command::Voices(reply)) => {
                let voices = engine.voices();
                match reply {
                    Some(reply) => drop(reply.send(voices)),
                    None => deliver(Event::Voices(voices)),
                }
            }
            None => {}
        }
        engine.poll(&mut |event| {
            // Stopped utterances were already reported finished.
            match &event {
                Event::Finished(id) => {
                    let Some(i) = open.iter().position(|o| o == id) else {
                        return;
                    };
                    open.remove(i);
                }
                Event::Started(id) | Event::Word { id, .. } if !open.contains(id) => return,
                _ => {}
            }
            deliver(event);
        });
    }
    if let Some(engine) = engine.as_mut() {
        engine.stop();
    }
}

/// Byte offset in `text` of UTF-16 offset `units` (engines count in UTF-16).
pub(crate) fn utf16_to_byte(text: &str, units: usize) -> usize {
    let mut seen = 0;
    for (byte, c) in text.char_indices() {
        if seen >= units {
            return byte;
        }
        seen += c.len_utf16();
    }
    text.len()
}

/// A UTF-16 range of `text` as a byte range `(start, len)`.
pub(crate) fn utf16_range(text: &str, start: usize, len: usize) -> (usize, usize) {
    let from = utf16_to_byte(text, start);
    let to = utf16_to_byte(text, start + len);
    (from, to.saturating_sub(from))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_ranges_become_byte_ranges() {
        let text = "a漢😀b";
        assert_eq!(utf16_range(text, 0, 1), (0, 1));
        assert_eq!(utf16_range(text, 1, 1), (1, 3));
        // The emoji is two UTF-16 units and four bytes.
        assert_eq!(utf16_range(text, 2, 2), (4, 4));
        assert_eq!(utf16_range(text, 4, 1), (8, 1));
        assert_eq!(utf16_range(text, 9, 3), (9, 0));
    }

    #[test]
    fn rate_and_volume_are_clamped() {
        assert_eq!(clamp_or(9.0, RATE_RANGE.0, RATE_RANGE.1, 1.0), 2.0);
        assert_eq!(clamp_or(f32::NAN, 0.0, 1.0, 1.0), 1.0);
    }

    /// Says a line through the real engine; run by hand with `--ignored`.
    #[test]
    #[ignore = "speaks through the speakers"]
    fn system_engine_lists_voices_and_speaks() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let mut tts = Tts::new(move |e| drop(tx.send(e)));
        let voices = tts.voices();
        println!("{} voices, e.g. {:?}", voices.len(), voices.first());
        assert!(!voices.is_empty());
        let english = voices.iter().find(|v| v.language.starts_with("en"));
        let id = tts.speak(
            "Hello from NanaType.",
            english.map(|v| v.id.as_str()),
            1.0,
            0.8,
        );
        let mut events = Vec::new();
        while let Ok(e) = rx.recv_timeout(Duration::from_secs(10)) {
            println!("{e:?}");
            let done = e == Event::Finished(id);
            events.push(e);
            if done {
                break;
            }
        }
        assert!(events.contains(&Event::Finished(id)));
        let id = tts.speak("This one is stopped right away.", None, 1.0, 0.8);
        std::thread::sleep(Duration::from_millis(400));
        tts.stop();
        assert!(
            std::iter::from_fn(|| rx.recv_timeout(Duration::from_secs(3)).ok())
                .any(|e| e == Event::Finished(id))
        );
    }
}
