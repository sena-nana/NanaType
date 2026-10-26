use crate::{
    Activity, Backend, Deliver, Error, Event, Result,
    capture::Capture,
    model,
    native::{self, Native, NativeEvent},
};
use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use nanatype_settings::{SpeechBackend, SpeechSettings};
use rubato::{FftFixedInOut, Resampler};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

const RATE: usize = 16000;
const FRAME: usize = 320;
const SEGMENT_MAX: usize = RATE * 20;
const SILENCE_FRAMES: usize = 25; // Finalize after 500 ms; auto-send uses the independently configured pause.
struct Chunk {
    id: u64,
    audio: Vec<f32>,
    finalized: bool,
}
struct LocalWorker {
    tx: Sender<Chunk>,
    queued: Receiver<Chunk>,
}
impl LocalWorker {
    fn submit(&self, chunk: Chunk) -> Result<()> {
        let finalized = chunk.finalized;
        match self.tx.try_send(chunk) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) if !finalized => Ok(()), // Provisional refreshes may be coalesced.
            Err(TrySendError::Full(chunk)) => {
                if let Ok(old) = self.queued.try_recv()
                    && old.finalized
                {
                    return Err(Error::key("speech-overloaded"));
                }
                self.tx
                    .try_send(chunk)
                    .map_err(|_| Error::key("speech-overloaded"))
            }
            Err(TrySendError::Disconnected(_)) => Err(Error::key("speech-system-error")),
        }
    }
}

unsafe extern "C" fn abort(data: *mut std::ffi::c_void) -> bool {
    unsafe { &*data.cast::<AtomicBool>() }.load(Ordering::Acquire)
}
fn local_worker(
    model_file: &std::path::Path,
    language: &str,
    cancel: Arc<AtomicBool>,
    results: Sender<NativeEvent>,
) -> Result<LocalWorker> {
    // Upstream enables token-level trace logging in debug builds even when
    // print_realtime is false. Never forward native logs containing dictation.
    static LOGS: std::sync::Once = std::sync::Once::new();
    LOGS.call_once(whisper_rs::install_logging_hooks);
    model::verify(model_file).map_err(|_| Error::key("speech-model-invalid"))?;
    let params = WhisperContextParameters {
        use_gpu: false,
        ..Default::default()
    };
    let context = WhisperContext::new_with_params(model_file, params)
        .map_err(|e| Error::detail("speech-model-invalid", e))?;
    let language = if language == "auto" {
        None
    } else {
        let short = language.split('-').next().unwrap_or(language);
        if whisper_rs::get_lang_id(short).is_none() {
            return Err(Error::key("speech-language-unavailable"));
        }
        Some(short.to_string())
    };
    let (tx, rx) = bounded::<Chunk>(1);
    let queued = rx.clone();
    std::thread::Builder::new()
        .name("nanatype-speech-inference".into())
        .spawn(move || {
            while let Ok(mut chunk) = rx.recv() {
                if cancel.load(Ordering::Acquire) {
                    break;
                }
                let result = (|| {
                    let mut state = context.create_state()?;
                    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
                    params.set_language(language.as_deref());
                    params.set_translate(false);
                    params.set_no_context(true);
                    params.set_no_timestamps(true);
                    params.set_print_special(false);
                    params.set_print_progress(false);
                    params.set_print_realtime(false);
                    params.set_print_timestamps(false);
                    params.set_n_threads(
                        std::thread::available_parallelism().map_or(2, |n| n.get().clamp(1, 4))
                            as i32,
                    );
                    // The Arc outlives full(); abort runs synchronously on the inference threads.
                    unsafe {
                        params.set_abort_callback(Some(abort));
                        params.set_abort_callback_user_data(Arc::as_ptr(&cancel).cast_mut().cast());
                    }
                    if chunk.audio.len() < RATE {
                        chunk.audio.resize(RATE, 0.0);
                    }
                    state.full(params, &chunk.audio)?;
                    let mut text = String::new();
                    for segment in state.as_iter() {
                        text.push_str(segment.to_str()?);
                    }
                    Ok::<_, whisper_rs::WhisperError>(text.trim().to_string())
                })();
                if cancel.load(Ordering::Acquire) {
                    break;
                }
                let event = match result {
                    Ok(text) => NativeEvent {
                        id: chunk.id,
                        kind: if chunk.finalized { 1 } else { 0 },
                        text,
                    },
                    Err(_) => NativeEvent {
                        id: chunk.id,
                        kind: 2,
                        text: String::new(),
                    },
                };
                if results.send(event).is_err() {
                    break;
                }
            }
        })
        .map_err(|e| Error::detail("speech-system-error", e))?;
    Ok(LocalWorker { tx, queued })
}

pub(crate) fn run(
    settings: SpeechSettings,
    language: String,
    directory: &std::path::Path,
    generation: u64,
    cancel: &Arc<AtomicBool>,
    activity: &Arc<Mutex<Activity>>,
    deliver: &Deliver,
) -> Result<()> {
    if cancel.load(Ordering::Acquire) {
        return Ok(());
    }
    let model_file = model::path(directory);
    let system_language = if language.contains('-') {
        language.clone()
    } else {
        native::languages()
            .into_iter()
            .find(|tag| tag.split('-').next() == Some(language.as_str()))
            .unwrap_or_else(|| language.clone())
    };
    let system_permission = if language != "auto" && settings.backend != SpeechBackend::Local {
        native::system_permission()
    } else {
        Err(Error::key("speech-system-unavailable"))
    };
    let system = system_permission.is_ok() && native::available(&system_language);
    let backend = if system {
        Backend::System
    } else {
        if settings.backend == SpeechBackend::System {
            return Err(system_permission
                .err()
                .unwrap_or_else(|| Error::key("speech-system-unavailable")));
        }
        if !model_file.is_file() {
            return Err(system_permission
                .err()
                .filter(|e| e.key == "speech-permission")
                .unwrap_or_else(|| Error::key("speech-model-needed")));
        }
        Backend::Local
    };
    let (result_tx, results) = bounded::<NativeEvent>(64);
    let local = if backend == Backend::Local {
        Some(local_worker(
            &model_file,
            &language,
            cancel.clone(),
            result_tx.clone(),
        )?)
    } else {
        None
    };
    if cancel.load(Ordering::Acquire) {
        return Ok(());
    }
    native::microphone_permission()?;
    if cancel.load(Ordering::Acquire) {
        return Ok(());
    }
    let overflow = Arc::new(AtomicBool::new(false));
    let capture = Capture::open(settings.device.as_deref(), overflow.clone())?;
    let mut resampler =
        FftFixedInOut::<f32>::new(capture.rate as usize, RATE, (capture.rate / 50) as usize, 1)
            .map_err(|e| Error::detail("speech-device-error", e))?;
    let mut raw = VecDeque::<f32>::new();
    let mut normalized = VecDeque::<f32>::new();
    let mut preroll = VecDeque::<f32>::new();
    let mut vad = webrtc_vad::Vad::new_with_rate_and_mode(
        webrtc_vad::SampleRate::Rate16kHz,
        webrtc_vad::VadMode::Aggressive,
    );
    let mut native_tasks = BTreeMap::<u64, (Native, Option<Instant>)>::new();
    let mut current: Option<Chunk> = None;
    let mut pending = BTreeSet::new();
    let mut next = 1;
    let mut partial_at = RATE * 3;
    let mut last_capture = Instant::now();
    let mut quiet = 0;
    let mut voiced = 0;
    deliver(generation, Event::Ready(backend));
    while !cancel.load(Ordering::Acquire) {
        if capture.failed.load(Ordering::Acquire) || last_capture.elapsed() > Duration::from_secs(5)
        {
            return Err(Error::key("speech-device-error"));
        }
        if overflow.load(Ordering::Acquire) {
            return Err(Error::key("speech-overloaded"));
        }
        for (task, ended) in native_tasks.values() {
            task.poll();
            if ended.is_some_and(|at| at.elapsed() > Duration::from_secs(15)) {
                return Err(Error::key("speech-recognition-timeout"));
            }
        }
        while let Ok(event) = results.try_recv() {
            if !pending.contains(&event.id) {
                continue;
            }
            if event.kind == 0 {
                deliver(generation, Event::Partial(event.id, event.text));
                continue;
            }
            if event.kind == 2 {
                return Err(Error::key("speech-system-error"));
            }
            pending.remove(&event.id);
            if current.as_ref().is_some_and(|chunk| chunk.id == event.id) {
                current = None;
                voiced = 0;
            }
            native_tasks.remove(&event.id);
            {
                let mut a = activity.lock().unwrap_or_else(|e| e.into_inner());
                a.pending = a.pending.saturating_sub(1);
            }
            deliver(generation, Event::Final(event.id, event.text));
        }
        let (samples, captured_at) = match capture.samples.recv_timeout(Duration::from_millis(20)) {
            Ok(samples) => samples,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
            Err(_) => return Err(Error::key("speech-device-error")),
        };
        last_capture = captured_at;
        if captured_at.elapsed() > Duration::from_millis(250) {
            return Err(Error::key("speech-overloaded"));
        }
        raw.extend(samples);
        while raw.len() >= resampler.input_frames_next() {
            let input: Vec<f32> = raw.drain(..resampler.input_frames_next()).collect();
            let output = resampler
                .process(&[input], None)
                .map_err(|e| Error::detail("speech-device-error", e))?;
            normalized.extend(output.into_iter().next().unwrap_or_default());
        }
        while normalized.len() >= FRAME {
            let frame: Vec<f32> = normalized.drain(..FRAME).collect();
            let pcm: Vec<i16> = frame
                .iter()
                .map(|v| (v.clamp(-1.0, 1.0) * 32767.0) as i16)
                .collect();
            let voice = vad
                .is_voice_segment(&pcm)
                .map_err(|_| Error::key("speech-device-error"))?;
            if voice {
                voiced += 1;
                quiet = 0;
                let mut a = activity.lock().unwrap_or_else(|e| e.into_inner());
                a.last_voice = captured_at.checked_sub(Duration::from_secs_f64(
                    normalized.len() as f64 / RATE as f64,
                ));
                a.speaking = true;
            } else {
                voiced = 0;
                quiet += 1;
                // The last actual voice frame anchors the countdown, including VAD hangover.
                activity.lock().unwrap_or_else(|e| e.into_inner()).speaking = false;
            }
            if current.is_none() && voiced >= 3 {
                if native_tasks.len() >= 3 {
                    return Err(Error::key("speech-overloaded"));
                }
                let chunk = Chunk {
                    id: next,
                    audio: preroll.drain(..).collect(),
                    finalized: false,
                };
                partial_at = RATE * 3;
                next += 1;
                activity.lock().unwrap_or_else(|e| e.into_inner()).pending += 1;
                pending.insert(chunk.id);
                deliver(generation, Event::Pending(chunk.id));
                if backend == Backend::System {
                    let task = Native::new(
                        &system_language,
                        chunk.id,
                        result_tx.clone(),
                        overflow.clone(),
                    )?;
                    task.append(&chunk.audio)?;
                    native_tasks.insert(chunk.id, (task, None));
                }
                current = Some(chunk);
            }
            if let Some(chunk) = &mut current {
                chunk.audio.extend_from_slice(&frame);
                if let Some((task, _)) = native_tasks.get(&chunk.id) {
                    task.append(&frame)?;
                }
                if quiet >= SILENCE_FRAMES || chunk.audio.len() >= SEGMENT_MAX {
                    let mut chunk = current.take().unwrap();
                    chunk.finalized = true;
                    if let Some(local) = &local {
                        local.submit(chunk)?;
                    } else if let Some((task, ended)) = native_tasks.get_mut(&chunk.id) {
                        task.end();
                        *ended = Some(Instant::now());
                    }
                    voiced = 0;
                } else if chunk.audio.len() >= partial_at {
                    if let Some(local) = &local {
                        local.submit(Chunk {
                            id: chunk.id,
                            audio: chunk.audio.clone(),
                            finalized: false,
                        })?;
                    }
                    partial_at += RATE * 3;
                }
            } else {
                preroll.extend(frame);
                while preroll.len() > RATE / 5 {
                    preroll.pop_front();
                }
            }
        }
    }
    // Capture is dropped here even if the independently cancellable inference is busy.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn final_jobs_replace_provisional_jobs_and_never_silently_drop_final_audio() {
        let (tx, rx) = bounded(1);
        let worker = LocalWorker {
            tx,
            queued: rx.clone(),
        };
        worker
            .submit(Chunk {
                id: 1,
                audio: vec![0.0],
                finalized: false,
            })
            .unwrap();
        worker
            .submit(Chunk {
                id: 1,
                audio: vec![1.0],
                finalized: false,
            })
            .unwrap();
        worker
            .submit(Chunk {
                id: 1,
                audio: vec![2.0],
                finalized: true,
            })
            .unwrap();
        let final_job = rx.recv().unwrap();
        assert!(final_job.finalized);
        assert_eq!(final_job.audio, [2.0]);
        worker
            .submit(Chunk {
                id: 2,
                audio: vec![],
                finalized: true,
            })
            .unwrap();
        assert!(
            worker
                .submit(Chunk {
                    id: 3,
                    audio: vec![],
                    finalized: true
                })
                .is_err()
        );
    }
    /// Opt-in, real model test. Fixtures and model are deliberately not shipped.
    /// NANATYPE_SPEECH_FIXTURES=<directory> cargo test -p nanatype-speech offline_fixtures -- --ignored --nocapture
    #[test]
    #[ignore = "requires the pinned model and 16 kHz mono PCM16 en/zh/ja WAV fixtures"]
    fn offline_fixtures() {
        let directory = std::path::PathBuf::from(
            std::env::var_os("NANATYPE_SPEECH_FIXTURES").expect("fixture directory"),
        );
        for (file, language) in [
            ("jfk.wav", "en-US"),
            ("zh.wav", "zh-CN"),
            ("ja.wav", "ja-JP"),
        ] {
            let wav = std::fs::read(directory.join(file)).unwrap();
            let audio = pcm16_wave(&wav);
            let duration = audio.len() as f64 / RATE as f64;
            let cancel = Arc::new(AtomicBool::new(false));
            let (tx, rx) = bounded(2);
            let load = Instant::now();
            let worker = local_worker(&model::path(&directory), language, cancel, tx).unwrap();
            let load_ms = load.elapsed().as_secs_f64() * 1000.0;
            let provisional = Instant::now();
            worker
                .submit(Chunk {
                    id: 1,
                    audio: audio.clone(),
                    finalized: false,
                })
                .unwrap();
            let partial = rx.recv_timeout(Duration::from_secs(60)).unwrap();
            assert_eq!(partial.kind, 0);
            let partial_seconds = provisional.elapsed().as_secs_f64();
            let inference = Instant::now();
            worker
                .submit(Chunk {
                    id: 1,
                    audio,
                    finalized: true,
                })
                .unwrap();
            let result = rx.recv_timeout(Duration::from_secs(60)).unwrap();
            let elapsed = inference.elapsed().as_secs_f64();
            assert_eq!(result.kind, 1);
            assert!(!result.text.trim().is_empty(), "{language}");
            println!(
                "{language}: audio={duration:.2}s load={load_ms:.0}ms provisional={partial_seconds:.2}s inference={elapsed:.2}s rtf={:.3} text={}",
                elapsed / duration,
                result.text
            );
            drop(worker);
        }
    }
    fn pcm16_wave(wav: &[u8]) -> Vec<f32> {
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        let mut offset = 12;
        while offset + 8 <= wav.len() {
            let length =
                u32::from_le_bytes(wav[offset + 4..offset + 8].try_into().unwrap()) as usize;
            let data = &wav[offset + 8..offset + 8 + length];
            match &wav[offset..offset + 4] {
                b"fmt " => {
                    assert_eq!(u16::from_le_bytes(data[..2].try_into().unwrap()), 1);
                    assert_eq!(u16::from_le_bytes(data[2..4].try_into().unwrap()), 1);
                    assert_eq!(
                        u32::from_le_bytes(data[4..8].try_into().unwrap()),
                        RATE as u32
                    );
                    assert_eq!(u16::from_le_bytes(data[14..16].try_into().unwrap()), 16);
                }
                b"data" => {
                    return data
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|s| i16::from_le_bytes([s[0], s[1]]) as f32 / 32768.0)
                        .collect();
                }
                _ => {}
            }
            offset += 8 + length + length % 2;
        }
        panic!("no WAV data");
    }
}
