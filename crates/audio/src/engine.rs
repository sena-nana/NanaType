use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{Receiver, RecvError, Sender, after, bounded, never, unbounded};

use crate::mixer::{Command, Mixer};
use crate::supervise::{Fault, PREFERRED_POLL, Supervisor, classify};
use crate::{Bus, Pcm, Scheduled, SoundId, Voice};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioError {
    NoDevice,
    Device(String),
}

impl std::fmt::Display for AudioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoDevice => write!(f, "no audio output device"),
            Self::Device(e) => write!(f, "audio device error: {e}"),
        }
    }
}

impl std::error::Error for AudioError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioStatus {
    Running {
        device: String,
        rate: u32,
    },
    /// The device disappeared; the engine is reopening the default one.
    /// Sent once: until a stream plays steadily again, later losses and
    /// failed reopens are retried quietly.
    Lost(String),
    /// No device could be opened. Sent once, like `Lost`; the engine keeps
    /// retrying with a growing delay and sends `Running` when one opens.
    Failed(String),
}

/// Cheap to clone; every method is non-blocking.
#[derive(Clone)]
pub struct AudioHandle {
    tx: Sender<Command>,
    control: Sender<Control>,
}

enum Control {
    Device(Option<String>),
    Stop,
}

impl AudioHandle {
    fn send(&self, command: Command) {
        let _ = self.tx.send(command);
    }

    pub fn load(&self, id: SoundId, pcm: Arc<Pcm>) {
        self.send(Command::Load(id, pcm));
    }

    pub fn unload(&self, id: SoundId) {
        self.send(Command::Unload(id));
    }

    pub fn play(&self, voice: Voice) {
        self.send(Command::Play(voice));
    }

    /// Queue sounds at instants; a later `cancel(epoch)` drops those not started.
    pub fn schedule(&self, epoch: u64, items: Vec<Scheduled>) {
        if !items.is_empty() {
            self.send(Command::Schedule { epoch, items });
        }
    }

    pub fn cancel(&self, epoch: u64) {
        self.send(Command::Cancel { epoch });
    }

    pub fn set_master(&self, gain: f32) {
        self.send(Command::Master(gain));
    }

    pub fn set_bus(&self, bus: Bus, gain: f32) {
        self.send(Command::BusGain(bus, gain));
    }

    /// Switch output device (`None` = system default).
    pub fn set_device(&self, name: Option<String>) {
        let _ = self.control.send(Control::Device(name));
    }

    pub fn stop(&self) {
        let _ = self.control.send(Control::Stop);
    }
}

pub struct AudioEngine;

/// Output device names for the settings picker.
pub fn list_devices() -> Vec<String> {
    let host = cpal::default_host();
    host.output_devices()
        .map(|devices| devices.filter_map(|d| device_name(&d)).collect())
        .unwrap_or_default()
}

fn device_name(device: &cpal::Device) -> Option<String> {
    device.description().ok().map(|d| d.name().to_owned())
}

fn find_device(host: &cpal::Host, name: &str) -> Option<cpal::Device> {
    host.output_devices()
        .ok()?
        .find(|d| device_name(d).as_deref() == Some(name))
}

/// Whether an output device with this name is plugged in and enabled.
fn device_present(name: &str) -> bool {
    find_device(&cpal::default_host(), name).is_some()
}

struct Opened {
    stream: cpal::Stream,
    device: String,
    rate: u32,
    /// Stream errors, already classified.
    faults: Receiver<Fault>,
    /// The chosen device was missing or failed; the default stands in.
    fell_back: bool,
}

/// Open `name`, or the system default when it is `None`, missing or broken.
fn open(name: Option<&str>, rx: &Receiver<Command>) -> Result<Opened, AudioError> {
    let host = cpal::default_host();
    let mut preferred_error = None;
    if let Some(name) = name
        && let Some(device) = find_device(&host, name)
    {
        match open_device(&device, rx) {
            Ok(opened) => return Ok(opened),
            Err(e) => preferred_error = Some(e),
        }
    }
    let device = host.default_output_device().ok_or(AudioError::NoDevice);
    match device.and_then(|device| open_device(&device, rx)) {
        Ok(opened) => Ok(Opened {
            fell_back: name.is_some(),
            ..opened
        }),
        // The chosen device's own failure says more than "no default".
        Err(e) => Err(preferred_error.unwrap_or(e)),
    }
}

fn open_device(device: &cpal::Device, rx: &Receiver<Command>) -> Result<Opened, AudioError> {
    let device_name = device_name(device).unwrap_or_else(|| "default".into());
    let config = device
        .default_output_config()
        .map_err(|e| AudioError::Device(e.to_string()))?;
    let rate = config.sample_rate();
    let channels = usize::from(config.channels());
    let mut mixer = Mixer::new(rate);
    let stream_config: cpal::StreamConfig = config.config();
    let (fault_tx, faults) = bounded::<Fault>(4);
    // Runs on cpal's thread, which `Stream::drop` joins: never block it.
    let on_error = move |e: cpal::Error| {
        let _ = fault_tx.try_send(classify(&e));
    };
    let rx = rx.clone();
    let mut scratch: Vec<f32> = Vec::new();
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_output_stream(
            stream_config,
            move |out: &mut [f32], info: &cpal::OutputCallbackInfo| {
                while let Ok(c) = rx.try_recv() {
                    mixer.apply(c);
                }
                let ts = info.timestamp();
                let ahead = ts.playback.duration_since(ts.callback);
                mixer.render(out, channels, Instant::now() + ahead);
            },
            on_error,
            None,
        ),
        other => device.build_output_stream_raw(
            stream_config,
            other,
            move |data: &mut cpal::Data, info: &cpal::OutputCallbackInfo| {
                while let Ok(c) = rx.try_recv() {
                    mixer.apply(c);
                }
                let len = data.len();
                scratch.resize(len, 0.0);
                let ts = info.timestamp();
                let ahead = ts.playback.duration_since(ts.callback);
                mixer.render(&mut scratch, channels, Instant::now() + ahead);
                if let Some(out) = data.as_slice_mut::<i16>() {
                    for (o, s) in out.iter_mut().zip(&scratch) {
                        *o = (s.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
                    }
                } else if let Some(out) = data.as_slice_mut::<u16>() {
                    for (o, s) in out.iter_mut().zip(&scratch) {
                        *o = ((s.clamp(-1.0, 1.0) * 0.5 + 0.5) * f32::from(u16::MAX)) as u16;
                    }
                } else if let Some(out) = data.as_slice_mut::<i32>() {
                    for (o, s) in out.iter_mut().zip(&scratch) {
                        *o = (f64::from(s.clamp(-1.0, 1.0)) * f64::from(i32::MAX)) as i32;
                    }
                }
            },
            on_error,
            None,
        ),
    }
    .map_err(|e| AudioError::Device(e.to_string()))?;
    stream
        .play()
        .map_err(|e| AudioError::Device(e.to_string()))?;
    Ok(Opened {
        stream,
        device: device_name,
        rate,
        faults,
        fell_back: false,
    })
}

/// Apply a control message; `false` means the thread should stop.
fn control(
    msg: Result<Control, RecvError>,
    wanted: &mut Option<String>,
    supervisor: &mut Supervisor,
) -> bool {
    match msg {
        Ok(Control::Device(name)) => {
            *wanted = name;
            // An explicit choice: trouble with it is worth reporting again.
            supervisor.reset();
            true
        }
        Ok(Control::Stop) | Err(_) => false,
    }
}

impl AudioEngine {
    /// Start the audio thread. `status` is called from that thread whenever
    /// the device state changes; the host forwards it to its UI.
    ///
    /// Trouble is reported once: while no device works the engine stays
    /// silent and retries with a growing delay, and a chosen device that is
    /// missing is replaced by the default until it comes back.
    pub fn start(
        device: Option<String>,
        status: impl Fn(AudioStatus) + Send + 'static,
    ) -> AudioHandle {
        let (tx, rx) = unbounded::<Command>();
        let (control_tx, control_rx) = unbounded::<Control>();
        thread::Builder::new()
            .name("nanatype-audio".into())
            .spawn(move || {
                let mut wanted = device;
                let mut supervisor = Supervisor::default();
                loop {
                    let (stream, mut faults, mut timer) = match open(wanted.as_deref(), &rx) {
                        Ok(opened) => {
                            let poll = supervisor.opened(Instant::now(), opened.fell_back);
                            status(AudioStatus::Running {
                                device: opened.device,
                                rate: opened.rate,
                            });
                            (
                                Some(opened.stream),
                                opened.faults,
                                poll.map_or_else(never, after),
                            )
                        }
                        Err(e) => {
                            let (report, wait) = supervisor.open_failed(e.to_string());
                            if let Some(report) = report {
                                status(report);
                            }
                            (None, never(), after(wait))
                        }
                    };
                    // Wait for a control message, a stream error or the timer.
                    let delay = loop {
                        crossbeam_channel::select! {
                            recv(control_rx) -> msg => {
                                if !control(msg, &mut wanted, &mut supervisor) {
                                    return;
                                }
                                break Duration::ZERO;
                            },
                            recv(faults) -> fault => {
                                let Ok(fault) = fault else {
                                    faults = never();
                                    continue;
                                };
                                if let Some((report, delay)) =
                                    supervisor.stream_failed(Instant::now(), fault)
                                {
                                    if let Some(report) = report {
                                        status(report);
                                    }
                                    break delay;
                                }
                            },
                            recv(timer) -> _ => {
                                if stream.is_none() {
                                    break Duration::ZERO;
                                }
                                // Playing on the default: is the chosen device back?
                                if wanted.as_deref().is_some_and(device_present) {
                                    supervisor.preferred_back();
                                    break Duration::ZERO;
                                }
                                timer = after(PREFERRED_POLL);
                            },
                        }
                    };
                    drop(stream);
                    if !delay.is_zero() {
                        crossbeam_channel::select! {
                            recv(control_rx) -> msg => {
                                if !control(msg, &mut wanted, &mut supervisor) {
                                    return;
                                }
                            },
                            default(delay) => {}
                        }
                    }
                    // Drop commands meant for the dead stream: sounds played while
                    // nothing could hear them must not burst out of the next one.
                    // The mixer is rebuilt from scratch, so the host reloads banks
                    // on `Running`.
                    while rx.try_recv().is_ok() {}
                }
            })
            .expect("spawn audio thread");
        AudioHandle {
            tx,
            control: control_tx,
        }
    }
}
