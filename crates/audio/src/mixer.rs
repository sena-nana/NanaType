//! The real-time part. `Mixer::render` runs inside the device callback, so it
//! never allocates after warm-up, never locks and never blocks.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use crate::Pcm;

pub const MAX_VOICES: usize = 32;

/// Opaque sound name chosen by the host (`"skin:sounds/type-1.ogg"`).
pub type SoundId = Arc<str>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bus {
    Typing,
    Effects,
    Ui,
    /// Keystrokes in the input window.
    Keys,
}

impl Bus {
    fn index(self) -> usize {
        match self {
            Self::Typing => 0,
            Self::Effects => 1,
            Self::Ui => 2,
            Self::Keys => 3,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Voice {
    pub sound: SoundId,
    pub bus: Bus,
    pub gain: f32,
    /// Playback-rate multiplier; 1.0 is the original pitch.
    pub pitch: f32,
}

#[derive(Debug, Clone)]
pub struct Scheduled {
    pub at: Instant,
    pub voice: Voice,
}

struct Playing {
    pcm: Arc<Pcm>,
    pos: f64,
    step: f64,
    gain: f32,
    bus: usize,
    /// Frames of silence before the voice starts, inside the current buffer.
    delay: usize,
}

pub(crate) enum Command {
    Load(SoundId, Arc<Pcm>),
    Unload(SoundId),
    Play(Voice),
    Schedule { epoch: u64, items: Vec<Scheduled> },
    Cancel { epoch: u64 },
    Master(f32),
    BusGain(Bus, f32),
}

pub(crate) struct Mixer {
    rate: u32,
    sounds: HashMap<SoundId, Arc<Pcm>>,
    voices: Vec<Playing>,
    pending: Vec<(u64, Scheduled)>,
    master: f32,
    buses: [f32; 4],
}

impl Mixer {
    pub fn new(rate: u32) -> Self {
        Self {
            rate,
            sounds: HashMap::new(),
            voices: Vec::with_capacity(MAX_VOICES),
            pending: Vec::with_capacity(256),
            master: 0.8,
            buses: [1.0; 4],
        }
    }

    pub fn apply(&mut self, command: Command) {
        match command {
            Command::Load(id, pcm) => {
                self.sounds.insert(id, pcm);
            }
            Command::Unload(id) => {
                self.sounds.remove(&id);
            }
            Command::Play(voice) => self.start(&voice, 0),
            Command::Schedule { epoch, items } => {
                self.pending.extend(items.into_iter().map(|s| (epoch, s)))
            }
            Command::Cancel { epoch } => self.pending.retain(|(e, _)| *e != epoch),
            Command::Master(g) => self.master = g.clamp(0.0, 1.0),
            Command::BusGain(bus, g) => self.buses[bus.index()] = g.clamp(0.0, 1.0),
        }
    }

    fn start(&mut self, voice: &Voice, delay: usize) {
        let Some(pcm) = self.sounds.get(&voice.sound).cloned() else {
            return;
        };
        if self.voices.len() >= MAX_VOICES {
            // Steal the voice closest to its end.
            if let Some((i, _)) = self.voices.iter().enumerate().max_by(|a, b| {
                let ra = a.1.pos / a.1.pcm.frames().max(1) as f64;
                let rb = b.1.pos / b.1.pcm.frames().max(1) as f64;
                ra.total_cmp(&rb)
            }) {
                self.voices.swap_remove(i);
            }
        }
        let step = f64::from(pcm.rate) / f64::from(self.rate.max(1))
            * f64::from(voice.pitch.clamp(0.25, 4.0));
        self.voices.push(Playing {
            pcm,
            pos: 0.0,
            step,
            gain: voice.gain.max(0.0),
            bus: voice.bus.index(),
            delay,
        });
    }

    /// Mix into interleaved `out` with `channels` channels. `now` is when the
    /// first frame of `out` reaches the speaker (approximately).
    pub fn render(&mut self, out: &mut [f32], channels: usize, now: Instant) {
        out.fill(0.0);
        let frames = out.len() / channels.max(1);
        let buffer_s = frames as f64 / f64::from(self.rate.max(1));
        // Start scheduled voices due within this buffer at their exact frame.
        let mut i = 0;
        while i < self.pending.len() {
            let at = self.pending[i].1.at;
            let offset_s = if at <= now {
                0.0
            } else {
                (at - now).as_secs_f64()
            };
            if offset_s < buffer_s {
                let (_, s) = self.pending.swap_remove(i);
                let delay = (offset_s * f64::from(self.rate)) as usize;
                self.start(&s.voice, delay.min(frames.saturating_sub(1)));
            } else {
                i += 1;
            }
        }
        let buses = self.buses;
        let master = self.master;
        self.voices.retain_mut(|v| {
            let gain = v.gain * buses[v.bus] * master;
            let len = v.pcm.frames() as f64;
            let start = v.delay.min(frames);
            v.delay = v.delay.saturating_sub(frames);
            for f in start..frames {
                if v.pos >= len - 1.0 {
                    return false;
                }
                let l = v.pcm.sample(v.pos, 0) * gain;
                let r = v.pcm.sample(v.pos, 1) * gain;
                let base = f * channels;
                match channels {
                    1 => out[base] += (l + r) * 0.5,
                    _ => {
                        out[base] += l;
                        out[base + 1] += r;
                    }
                }
                v.pos += v.step;
            }
            v.pos < len - 1.0
        });
        // Soft limiter: transparent below ~0.6, saturating smoothly above.
        for s in out.iter_mut() {
            let x = *s;
            *s = if x.abs() < 0.6 {
                x
            } else {
                x.signum() * (0.6 + 0.4 * ((x.abs() - 0.6) / 0.4).tanh())
            };
        }
    }

    #[cfg(test)]
    pub fn voices(&self) -> usize {
        self.voices.len()
    }

    #[cfg(test)]
    pub fn pending(&self) -> usize {
        self.pending.len()
    }
}
