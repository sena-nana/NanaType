use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::mixer::{Command, Mixer};
use crate::*;

fn wav(rate: u32, samples: &[i16]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

fn click() -> Arc<Pcm> {
    Arc::new(Pcm {
        rate: 48_000,
        channels: 1,
        samples: vec![0.5; 480],
    })
}

#[test]
fn wav_decodes_to_float_pcm() {
    let pcm = decode(wav(22_050, &[0, 16_384, -16_384, 0]), Some("wav")).unwrap();
    assert_eq!(pcm.rate, 22_050);
    assert_eq!(pcm.channels, 1);
    assert_eq!(pcm.frames(), 4);
    assert!((pcm.samples[1] - 0.5).abs() < 0.01);
    assert!(decode(b"not audio at all".to_vec(), Some("wav")).is_err());
}

#[test]
fn a_played_voice_mixes_and_ends() {
    let mut m = Mixer::new(48_000);
    let id: SoundId = "c".into();
    m.apply(Command::Load(id.clone(), click()));
    m.apply(Command::Master(1.0));
    m.apply(Command::Play(Voice {
        sound: id,
        bus: Bus::Typing,
        gain: 1.0,
        pitch: 1.0,
    }));
    let mut out = vec![0.0f32; 256 * 2];
    m.render(&mut out, 2, Instant::now());
    assert!((out[0] - 0.5).abs() < 1e-6);
    assert_eq!(m.voices(), 1);
    let mut out = vec![0.0f32; 512 * 2];
    m.render(&mut out, 2, Instant::now());
    assert_eq!(m.voices(), 0, "480 frames are done");
}

#[test]
fn scheduled_voices_start_on_their_frame_and_cancel_by_epoch() {
    let mut m = Mixer::new(48_000);
    let id: SoundId = "c".into();
    m.apply(Command::Load(id.clone(), click()));
    m.apply(Command::Master(1.0));
    let now = Instant::now();
    let voice = Voice {
        sound: id,
        bus: Bus::Typing,
        gain: 1.0,
        pitch: 1.0,
    };
    m.apply(Command::Schedule {
        epoch: 1,
        items: vec![Scheduled {
            at: now + Duration::from_micros(2_000),
            voice: voice.clone(),
        }],
    });
    m.apply(Command::Schedule {
        epoch: 2,
        items: vec![Scheduled {
            at: now + Duration::from_secs(5),
            voice,
        }],
    });
    let mut out = vec![0.0f32; 480];
    m.render(&mut out, 1, now);
    let first = out.iter().position(|s| *s != 0.0).unwrap();
    assert!(
        (95..=97).contains(&first),
        "2 ms at 48 kHz ≈ frame 96, got {first}"
    );
    m.apply(Command::Cancel { epoch: 2 });
    assert_eq!(m.pending(), 0);
}

#[test]
fn bus_gain_and_limiter_bound_the_output() {
    let mut m = Mixer::new(48_000);
    let id: SoundId = "c".into();
    m.apply(Command::Load(id.clone(), click()));
    m.apply(Command::Master(1.0));
    for _ in 0..40 {
        m.apply(Command::Play(Voice {
            sound: id.clone(),
            bus: Bus::Effects,
            gain: 1.0,
            pitch: 1.0,
        }));
    }
    assert_eq!(m.voices(), mixer::MAX_VOICES);
    let mut out = vec![0.0f32; 64];
    m.render(&mut out, 1, Instant::now());
    assert!(out.iter().all(|s| s.abs() <= 1.0));
    m.apply(Command::BusGain(Bus::Effects, 0.0));
    m.render(&mut out, 1, Instant::now());
    assert!(out.iter().all(|s| *s == 0.0));
}

#[test]
fn pitch_changes_the_playback_rate() {
    let mut m = Mixer::new(48_000);
    let id: SoundId = "c".into();
    m.apply(Command::Load(id.clone(), click()));
    m.apply(Command::Play(Voice {
        sound: id,
        bus: Bus::Ui,
        gain: 1.0,
        pitch: 2.0,
    }));
    let mut out = vec![0.0f32; 300];
    m.render(&mut out, 1, Instant::now());
    assert_eq!(m.voices(), 0, "480 frames at double speed end within 300");
}

#[test]
fn keys_bus_has_its_own_gain() {
    let mut m = Mixer::new(48_000);
    let id: SoundId = "c".into();
    m.apply(Command::Load(id.clone(), click()));
    m.apply(Command::BusGain(Bus::Keys, 0.0));
    m.apply(Command::Play(Voice {
        sound: id.clone(),
        bus: Bus::Keys,
        gain: 1.0,
        pitch: 1.0,
    }));
    let mut out = vec![0.0f32; 64];
    m.render(&mut out, 1, Instant::now());
    assert!(out.iter().all(|s| *s == 0.0));
    m.apply(Command::Play(Voice {
        sound: id,
        bus: Bus::Typing,
        gain: 1.0,
        pitch: 1.0,
    }));
    m.render(&mut out, 1, Instant::now());
    assert!(out.iter().any(|s| *s != 0.0), "other buses keep playing");
}

use crate::mixer;
