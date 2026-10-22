//! The active skin's sounds, decoded off the UI thread and handed to the
//! mixer. Skins without typing sounds get a synthesized set of soft clicks.
//! The input window's keystroke clicks ride along in every bank, so a fresh
//! output stream gets them back with the rest.

use std::sync::{Arc, LazyLock};

use nanatype_audio::{AudioHandle, Bus, Pcm, SoundId, Voice, decode};
use nanatype_core::DEFAULT_TYPING_VARIANTS;
use nanatype_playback::CueKind;
use nanatype_settings::KeystrokeScheme;
use nanatype_theme::LoadedSkin;

pub type Bank = Vec<(SoundId, Arc<Pcm>)>;

pub fn typing_id(variant: u8) -> SoundId {
    format!("typing:{variant}").into()
}

pub fn se_id(name: &str) -> SoundId {
    format!("se:{name}").into()
}

pub const OPEN: &str = "ui:open";
pub const ADVANCE: &str = "ui:advance";

/// One set of keystroke recordings (see `app/assets/sounds/keys`).
struct Scheme {
    name: &'static str,
    keys: &'static [&'static [u8]],
    enter: &'static [u8],
    /// The sound Backspace plays, and its pitch: neither set has its own.
    backspace: (Sound, f32),
}

#[derive(Clone, Copy)]
enum Sound {
    Key(u8),
    Enter,
}

macro_rules! key_files {
    ($dir:literal: $($n:literal),+) => {
        &[$(include_bytes!(concat!("../../assets/sounds/keys/", $dir, "/key-", $n, ".wav"))),+]
    };
}

const SCHEMES: [Scheme; 2] = [
    // jim-ph's 1986 DOS keyboard, CC0. Backspace is the enter key slowed
    // down: same keyboard, duller thud.
    Scheme {
        name: "mechanical",
        keys: key_files!("mechanical": 1, 2, 3, 4),
        enter: include_bytes!("../../assets/sounds/keys/mechanical/enter.wav"),
        backspace: (Sound::Enter, 0.88),
    },
    // Glaneur de sons' bubbles, CC BY 3.0. Enter is a long stream of
    // bubbles, so Backspace lowers the shortest pop instead.
    Scheme {
        name: "bubble",
        keys: key_files!("bubble": 1, 2, 3, 4, 5, 6, 7, 8),
        enter: include_bytes!("../../assets/sounds/keys/bubble/enter.wav"),
        backspace: (Sound::Key(6), 0.8),
    },
];

fn scheme(kind: KeystrokeScheme) -> &'static Scheme {
    &SCHEMES[match kind {
        KeystrokeScheme::Mechanical => 0,
        KeystrokeScheme::Bubble => 1,
    }]
}

fn key_id(scheme: &Scheme, sound: Sound) -> SoundId {
    match sound {
        Sound::Key(n) => format!("key:{}:{n}", scheme.name).into(),
        Sound::Enter => format!("key:{}:enter", scheme.name).into(),
    }
}

static KEY_BANK: LazyLock<Bank> = LazyLock::new(|| {
    let decoded = |bytes: &[u8]| Arc::new(decode(bytes.to_vec(), Some("wav")).expect("key sound"));
    let mut bank = Bank::new();
    for scheme in &SCHEMES {
        for (i, bytes) in scheme.keys.iter().enumerate() {
            bank.push((key_id(scheme, Sound::Key(i as u8)), decoded(bytes)));
        }
        bank.push((key_id(scheme, Sound::Enter), decoded(scheme.enter)));
    }
    bank
});

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keystroke {
    Normal,
    Enter,
    Backspace,
}

impl Keystroke {
    /// Classify a key by its logical name.
    pub fn of(logical: &str) -> Self {
        match logical {
            "Enter" => Self::Enter,
            "Backspace" | "Delete" => Self::Backspace,
            _ => Self::Normal,
        }
    }
}

/// Picks keystroke voices: normal keys draw a random recording, never the
/// same one twice in a row, with a little pitch jitter.
pub struct Keys {
    seed: u32,
    last: u8,
}

impl Default for Keys {
    fn default() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        Self {
            seed: nanos | 1,
            last: u8::MAX,
        }
    }
}

impl Keys {
    fn roll(&mut self) -> u32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed
    }

    pub fn voice(&mut self, kind: KeystrokeScheme, key: Keystroke) -> Voice {
        let scheme = scheme(kind);
        let (sound, pitch) = match key {
            Keystroke::Enter => (Sound::Enter, 1.0),
            Keystroke::Backspace => scheme.backspace,
            Keystroke::Normal => {
                let count = scheme.keys.len() as u32;
                let mut variant = (self.roll() % count) as u8;
                if variant == self.last {
                    variant = ((u32::from(variant) + 1 + self.roll() % (count - 1)) % count) as u8;
                }
                self.last = variant;
                let r = (self.roll() >> 8) as f32 / (1u32 << 24) as f32;
                (Sound::Key(variant), 1.0 + 0.03 * (r * 2.0 - 1.0))
            }
        };
        let sound = key_id(scheme, sound);
        Voice {
            sound,
            bus: Bus::Keys,
            gain: 1.0,
            pitch,
        }
    }
}

/// A short, soft key click: a decaying noise burst through a resonant tone.
fn synth_click(variant: u8) -> Pcm {
    let rate = 48_000u32;
    let len = (rate as f32 * 0.045) as usize;
    let tone = [1_650.0f32, 1_900.0, 1_420.0][usize::from(variant) % 3];
    let mut seed = 0x1234_5678u32 ^ u32::from(variant).wrapping_mul(0x9E37_79B9);
    let mut samples = Vec::with_capacity(len);
    let mut lp = 0.0f32;
    for i in 0..len {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let noise = (seed as f32 / u32::MAX as f32) * 2.0 - 1.0;
        let t = i as f32 / rate as f32;
        let env = (-t * 140.0).exp();
        lp += 0.35 * (noise - lp);
        let body = (t * tone * std::f32::consts::TAU).sin() * (-t * 90.0).exp();
        samples.push((lp * 0.55 + body * 0.45) * env * 0.5);
    }
    Pcm {
        rate,
        channels: 1,
        samples,
    }
}

/// Decode every sound of `skin` (runs on a worker thread).
pub fn load(skin: &LoadedSkin) -> (Bank, Vec<String>) {
    let mut bank: Bank = Vec::new();
    let mut errors = Vec::new();
    let mut read = |path: &str| -> Option<Arc<Pcm>> {
        let ext = path.rsplit('.').next().map(str::to_owned);
        match skin.files.read(path) {
            Ok(bytes) => match decode(bytes, ext.as_deref()) {
                Ok(pcm) => Some(Arc::new(pcm)),
                Err(e) => {
                    errors.push(format!("{path}: {e}"));
                    None
                }
            },
            Err(e) => {
                errors.push(format!("{path}: {e}"));
                None
            }
        }
    };
    let sounds = &skin.skin.sounds;
    if sounds.typing.files.is_empty() {
        for v in 0..DEFAULT_TYPING_VARIANTS {
            bank.push((typing_id(v), Arc::new(synth_click(v))));
        }
    } else {
        for (i, file) in sounds.typing.files.iter().enumerate() {
            if let Some(pcm) = read(file) {
                bank.push((typing_id(i as u8), pcm));
            }
        }
    }
    for (name, file) in &sounds.se {
        if let Some(pcm) = read(file) {
            bank.push((se_id(name), pcm));
        }
    }
    if let Some(file) = &sounds.open
        && let Some(pcm) = read(file)
    {
        bank.push((OPEN.into(), pcm));
    }
    if let Some(file) = &sounds.advance
        && let Some(pcm) = read(file)
    {
        bank.push((ADVANCE.into(), pcm));
    }
    bank.extend(KEY_BANK.iter().cloned());
    (bank, errors)
}

pub fn install(audio: &AudioHandle, previous: &Bank, next: &Bank) {
    for (id, _) in previous {
        if !next.iter().any(|(n, _)| n == id) {
            audio.unload(id.clone());
        }
    }
    for (id, pcm) in next {
        audio.load(id.clone(), pcm.clone());
    }
}

/// The voice a cue plays, if it plays one.
pub fn voice(cue: &CueKind, skin: &LoadedSkin) -> Option<Voice> {
    match cue {
        CueKind::TypeSound { variant, seed, .. } => {
            let t = &skin.skin.sounds.typing;
            let r = (seed.wrapping_mul(0x2545_F491) >> 8) as f32 / (1u32 << 24) as f32;
            Some(Voice {
                sound: typing_id(*variant),
                bus: Bus::Typing,
                gain: if t.files.is_empty() { 0.8 } else { t.volume },
                pitch: 1.0 + t.pitch_jitter * (r * 2.0 - 1.0),
            })
        }
        CueKind::Sound { sound, gain } => Some(Voice {
            sound: se_id(sound),
            bus: Bus::Effects,
            gain: *gain,
            pitch: 1.0,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_sounds_decode() {
        assert_eq!(KEY_BANK.len(), 5 + 9);
        for (id, pcm) in KEY_BANK.iter() {
            assert!(pcm.frames() > 2_000, "{id}");
        }
    }

    #[test]
    fn keys_are_classified_by_logical_name() {
        assert_eq!(Keystroke::of("Enter"), Keystroke::Enter);
        assert_eq!(Keystroke::of("Backspace"), Keystroke::Backspace);
        assert_eq!(Keystroke::of("Delete"), Keystroke::Backspace);
        assert_eq!(Keystroke::of("a"), Keystroke::Normal);
        assert_eq!(Keystroke::of(" "), Keystroke::Normal);
    }

    #[test]
    fn every_voice_names_a_loaded_sound() {
        let mut keys = Keys::default();
        for kind in [KeystrokeScheme::Mechanical, KeystrokeScheme::Bubble] {
            let mut last = keys.voice(kind, Keystroke::Normal).sound;
            for _ in 0..200 {
                let v = keys.voice(kind, Keystroke::Normal);
                assert_ne!(v.sound, last, "never the same recording twice");
                assert!(KEY_BANK.iter().any(|(id, _)| *id == v.sound));
                assert!((0.97..=1.03).contains(&v.pitch));
                last = v.sound;
            }
            for key in [Keystroke::Enter, Keystroke::Backspace] {
                let v = keys.voice(kind, key);
                assert!(KEY_BANK.iter().any(|(id, _)| *id == v.sound), "{}", v.sound);
            }
        }
    }
}
