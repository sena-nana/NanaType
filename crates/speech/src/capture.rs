use crate::{Error, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{Receiver, Sender, bounded};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Instant;

type Samples = (Vec<f32>, Instant);

pub fn devices() -> Vec<String> {
    cpal::default_host()
        .input_devices()
        .map(|devices| {
            devices
                .filter_map(|d| d.description().ok().map(|d| d.name().to_owned()))
                .collect()
        })
        .unwrap_or_default()
}
pub struct Capture {
    pub samples: Receiver<Samples>,
    pub failed: Arc<AtomicBool>,
    pub rate: u32,
    _stream: cpal::Stream,
}
impl Capture {
    pub fn open(name: Option<&str>, overflow: Arc<AtomicBool>) -> Result<Self> {
        let host = cpal::default_host();
        let device = if let Some(name) = name {
            host.input_devices()
                .map_err(|e| Error::detail("speech-device-error", e))?
                .find(|d| d.description().is_ok_and(|d| d.name() == name))
        } else {
            host.default_input_device()
        }
        .ok_or_else(|| Error::key("speech-device-error"))?;
        let config = device
            .default_input_config()
            .map_err(|e| Error::detail("speech-device-error", e))?;
        let failed = Arc::new(AtomicBool::new(false));
        let channels = usize::from(config.channels());
        if channels == 0 || channels > 32 || config.sample_rate() > 384000 {
            return Err(Error::key("speech-device-error"));
        }
        let (tx, samples) = bounded(64);
        let stream_config: cpal::StreamConfig = config.into();
        let build = match config.sample_format() {
            cpal::SampleFormat::F32 => build::<f32>,
            cpal::SampleFormat::I16 => build::<i16>,
            cpal::SampleFormat::I32 => build::<i32>,
            cpal::SampleFormat::U16 => build::<u16>,
            cpal::SampleFormat::U8 => build::<u8>,
            cpal::SampleFormat::F64 => build::<f64>,
            _ => return Err(Error::key("speech-device-error")),
        };
        let stream = build(&device, &stream_config, tx, overflow, failed.clone())
            .map_err(|e| Error::detail("speech-device-error", e))?;
        stream
            .play()
            .map_err(|e| Error::detail("speech-device-error", e))?;
        Ok(Self {
            samples,
            rate: config.sample_rate(),
            failed,
            _stream: stream,
        })
    }
}

fn build<T: cpal::SizedSample + Copy>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    tx: Sender<Samples>,
    overflow: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
) -> std::result::Result<cpal::Stream, cpal::Error>
where
    f32: cpal::FromSample<T>,
{
    let channels = usize::from(config.channels);
    device.build_input_stream(
        *config,
        move |values: &[T], _| {
            // Convert and downmix in one pass; the audio callback never blocks.
            if values.len() > 192000 {
                overflow.store(true, Ordering::Release);
                return;
            }
            let mono = values
                .chunks_exact(channels)
                .map(|frame| {
                    frame.iter().map(|v| v.to_sample::<f32>()).sum::<f32>() / channels as f32
                })
                .collect();
            if tx.try_send((mono, Instant::now())).is_err() {
                overflow.store(true, Ordering::Release);
            }
        },
        move |_| {
            failed.store(true, Ordering::Release);
        },
        None,
    )
}
