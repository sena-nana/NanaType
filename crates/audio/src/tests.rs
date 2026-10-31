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
use crate::supervise::{self, Fault, Supervisor};

fn lost() -> Fault {
    Fault::Lost("device not available".into())
}

#[test]
fn a_missing_device_is_reported_once_then_retried_quietly_with_backoff() {
    let mut s = Supervisor::default();
    let (report, first) = s.open_failed("no audio output device".into());
    assert_eq!(
        report,
        Some(AudioStatus::Failed("no audio output device".into()))
    );
    let mut last = first;
    for _ in 0..20 {
        let (report, wait) = s.open_failed("no audio output device".into());
        assert_eq!(report, None, "the failure is already on screen");
        assert!(wait >= last, "the delay never shrinks");
        last = wait;
    }
    assert_eq!(last, supervise::BACKOFF_MAX);
    assert!(first < Duration::from_secs(1), "the first retry is quick");
}

#[test]
fn a_lost_device_is_reported_once_even_when_reopening_keeps_failing() {
    let mut s = Supervisor::default();
    let t0 = Instant::now();
    s.opened(t0, false);
    let (report, wait) = s.stream_failed(t0 + supervise::STABLE, lost()).unwrap();
    assert!(matches!(report, Some(AudioStatus::Lost(_))));
    assert_eq!(wait, supervise::FIRST_RETRY);
    let (report, _) = s.open_failed("no audio output device".into());
    assert_eq!(report, None, "losing it and failing to reopen is one story");
}

#[test]
fn a_stream_that_keeps_dying_right_away_is_not_reported_each_time() {
    let mut s = Supervisor::default();
    let mut now = Instant::now();
    s.opened(now, false);
    now += supervise::STABLE;
    let (report, _) = s.stream_failed(now, lost()).unwrap();
    assert!(report.is_some());
    let mut waits = Vec::new();
    for _ in 0..6 {
        s.opened(now, false);
        now += Duration::from_millis(50);
        let (report, wait) = s.stream_failed(now, lost()).unwrap();
        assert_eq!(report, None);
        waits.push(wait);
    }
    assert!(waits.windows(2).all(|w| w[0] <= w[1]), "{waits:?}");
    assert!(*waits.last().unwrap() > supervise::FIRST_RETRY);
}

#[test]
fn a_stream_that_played_steadily_makes_the_next_loss_news() {
    let mut s = Supervisor::default();
    let t0 = Instant::now();
    s.open_failed("no audio output device".into());
    s.open_failed("no audio output device".into());
    s.opened(t0, false);
    let (report, wait) = s.stream_failed(t0 + supervise::STABLE, lost()).unwrap();
    assert!(matches!(report, Some(AudioStatus::Lost(_))));
    assert_eq!(wait, supervise::FIRST_RETRY, "the backoff starts over");
}

#[test]
fn glitches_keep_the_stream_and_reroutes_rebuild_it_quietly() {
    let mut s = Supervisor::default();
    let t0 = Instant::now();
    s.opened(t0, false);
    assert_eq!(s.stream_failed(t0, Fault::Glitch), None);
    let (report, wait) = s
        .stream_failed(t0 + supervise::STABLE, Fault::Rerouted)
        .unwrap();
    assert_eq!(report, None, "a new default device is not an error");
    assert_eq!(wait, supervise::FIRST_RETRY);
    // A reroute that then fails to reopen is reported.
    let (report, _) = s.open_failed("no audio output device".into());
    assert!(matches!(report, Some(AudioStatus::Failed(_))));
}

#[test]
fn choosing_a_device_makes_its_trouble_news_again() {
    let mut s = Supervisor::default();
    assert!(s.open_failed("busy".into()).0.is_some());
    assert!(s.open_failed("busy".into()).0.is_none());
    s.reset();
    let (report, wait) = s.open_failed("busy".into());
    assert!(report.is_some());
    assert_eq!(wait, supervise::FIRST_RETRY);
}

#[test]
fn the_chosen_device_is_looked_for_while_the_default_stands_in() {
    let mut s = Supervisor::default();
    let t0 = Instant::now();
    assert_eq!(
        s.opened(t0, false),
        None,
        "no fallback, nothing to look for"
    );
    assert_eq!(s.opened(t0, true), Some(supervise::PREFERRED_POLL));
    // It is back but still does not open: look less often.
    s.preferred_back();
    assert_eq!(s.opened(t0, true), Some(supervise::PREFERRED_POLL * 2));
    for _ in 0..10 {
        s.preferred_back();
    }
    assert_eq!(s.opened(t0, true), Some(supervise::PREFERRED_POLL_MAX));
    // It opened: the next fallback looks quickly again.
    assert_eq!(s.opened(t0, false), None);
    assert_eq!(s.opened(t0, true), Some(supervise::PREFERRED_POLL));
}

#[test]
fn cpal_errors_are_sorted_by_what_they_mean_for_the_stream() {
    use cpal::ErrorKind as K;
    let fault = |k| supervise::classify(&cpal::Error::new(k));
    assert_eq!(fault(K::Xrun), Fault::Glitch);
    assert_eq!(fault(K::DeviceChanged), Fault::Glitch);
    assert_eq!(fault(K::RealtimeDenied), Fault::Glitch);
    assert_eq!(fault(K::StreamInvalidated), Fault::Rerouted);
    assert!(matches!(fault(K::DeviceNotAvailable), Fault::Lost(_)));
    assert!(matches!(fault(K::BackendError), Fault::Lost(_)));
}

#[test]
fn backoff_grows_and_caps() {
    assert_eq!(supervise::backoff(0), supervise::FIRST_RETRY);
    assert_eq!(supervise::backoff(1), supervise::FIRST_RETRY);
    assert_eq!(supervise::backoff(2), Duration::from_secs(1));
    assert_eq!(supervise::backoff(3), Duration::from_secs(2));
    assert_eq!(supervise::backoff(u32::MAX), supervise::BACKOFF_MAX);
}
