//! Microphone lifetime and recognition are independent of application windows.
mod capture;
pub mod controller;
pub mod model;
mod native;
mod worker;

use nanatype_settings::SpeechSettings;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub key: &'static str,
    pub detail: String,
}
impl Error {
    pub fn key(key: &'static str) -> Self {
        Self {
            key,
            detail: String::new(),
        }
    }
    pub(crate) fn detail(key: &'static str, e: impl std::fmt::Display) -> Self {
        Self {
            key,
            detail: e.to_string(),
        }
    }
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    System,
    Local,
}
#[derive(Debug, Clone)]
pub enum Event {
    Ready(Backend),
    Pending(u64),
    Partial(u64, String),
    Final(u64, String),
    Error(Error),
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inventory {
    pub devices: Vec<String>,
    pub system_languages: Vec<String>,
}

/// Updated before posting events, also consulted by the auto-send boundary.
#[derive(Debug, Clone, Copy, Default)]
pub struct Activity {
    pub last_voice: Option<Instant>,
    pub speaking: bool,
    pub pending: usize,
}
pub(crate) type Deliver = Arc<dyn Fn(u64, Event) + Send + Sync>;

pub struct Service {
    directory: PathBuf,
    deliver: Deliver,
    cancel: Arc<AtomicBool>,
    activity: Arc<Mutex<Activity>>,
}
impl Service {
    pub fn new(directory: PathBuf, deliver: impl Fn(u64, Event) + Send + Sync + 'static) -> Self {
        Self {
            directory,
            deliver: Arc::new(deliver),
            cancel: Arc::new(AtomicBool::new(false)),
            activity: Arc::new(Mutex::new(Activity::default())),
        }
    }
    pub fn start(&mut self, settings: SpeechSettings, language: String, generation: u64) {
        self.stop();
        self.cancel = Arc::new(AtomicBool::new(false));
        self.activity = Arc::new(Mutex::new(Activity::default()));
        let cancel = self.cancel.clone();
        let activity = self.activity.clone();
        let directory = self.directory.clone();
        let deliver = self.deliver.clone();
        let failure = deliver.clone();
        if let Err(e) = std::thread::Builder::new()
            .name("nanatype-speech".into())
            .spawn(move || {
                if let Err(e) = worker::run(
                    settings, language, &directory, generation, &cancel, &activity, &deliver,
                ) && !cancel.load(Ordering::Acquire)
                {
                    cancel.store(true, Ordering::Release);
                    deliver(generation, Event::Error(e));
                }
            })
        {
            failure(
                generation,
                Event::Error(Error::detail("speech-system-error", e)),
            );
        }
    }
    /// Non-blocking: capture notices cancellation independently of inference.
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::Release);
    }
    pub fn stopped(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }
    pub fn activity(&self) -> Activity {
        *self.activity.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn model_path(&self) -> PathBuf {
        model::path(&self.directory)
    }
    pub fn directory(&self) -> &Path {
        &self.directory
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn inventory() -> Inventory {
    Inventory {
        devices: capture::devices(),
        system_languages: native::languages(),
    }
}
pub fn local_languages() -> Vec<(String, String)> {
    (0..=whisper_rs::get_lang_max_id())
        .filter_map(|id| {
            Some((
                whisper_rs::get_lang_str(id)?.to_string(),
                whisper_rs::get_lang_str_full(id)?.to_string(),
            ))
        })
        .collect()
}
