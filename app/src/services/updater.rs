//! Update checks and downloads off the UI thread (`nanatype_update`).

use nanatype_settings::UpdateSettings;
use nanatype_update::{Asset, Checked, Client, Error, Kind, Progress, Source, Target};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug)]
pub enum Event {
    Checked {
        manual: bool,
        result: Result<Checked, Error>,
    },
    Progress(Progress),
    Downloaded(Result<PathBuf, Error>),
}

pub struct Updater {
    dir: PathBuf,
    post: Arc<dyn Fn(Event) + Send + Sync>,
    /// The running download's stop flag.
    cancel: Mutex<Arc<AtomicBool>>,
}

impl Updater {
    /// Downloads from earlier runs were installed or abandoned: start empty.
    pub fn new(dir: PathBuf, post: impl Fn(Event) + Send + Sync + 'static) -> Self {
        let _ = std::fs::remove_dir_all(&dir);
        Self {
            dir,
            post: Arc::new(post),
            cancel: Mutex::default(),
        }
    }

    /// Check after `delay` (startup checks wait for the app to settle).
    pub fn check(&self, settings: &UpdateSettings, delay: Duration, manual: bool) {
        // A user's own proxy first, then GitHub and the built-in ones at once.
        let sources: Vec<Source> = Source::prefix(&settings.proxy)
            .into_iter()
            .chain([Source::Direct])
            .chain(Source::builtin())
            .collect();
        let post = self.post.clone();
        std::thread::spawn(move || {
            std::thread::sleep(delay);
            let result = Client::new().check(&sources);
            post(Event::Checked { manual, result });
        });
    }

    /// Download `asset`, stopping any earlier download.
    pub fn download(&self, checked: &Checked, asset: &Asset) {
        let cancel = Arc::new(AtomicBool::new(false));
        let previous = std::mem::replace(&mut *self.lock(), cancel.clone());
        previous.store(true, Ordering::Relaxed);
        let (checked, asset, dir) = (checked.clone(), asset.clone(), self.dir.clone());
        let post = self.post.clone();
        std::thread::spawn(move || {
            let result = Client::new().download(
                &checked.manifest,
                &asset,
                &checked.sources,
                &dir,
                |p| post(Event::Progress(p)),
                &cancel,
            );
            post(Event::Downloaded(result));
        });
    }

    pub fn cancel(&self) {
        self.lock().store(true, Ordering::Relaxed);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Arc<AtomicBool>> {
        self.cancel.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// The download for this installation, if the release has one.
pub fn pick(checked: &Checked) -> Option<Asset> {
    let target = Target::current()?;
    let kinds: &[Kind] = match target {
        Target::WindowsX86_64 if installed() => &[Kind::Installer],
        Target::WindowsX86_64 => &[Kind::Portable],
        _ => &[Kind::Dmg, Kind::AppZip],
    };
    checked.manifest.select(target, kinds).cloned()
}

/// Whether this copy came from the installer: Inno Setup leaves its
/// uninstaller beside the program.
fn installed() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("unins000.exe").is_file()))
        .unwrap_or(false)
}

/// Hand a verified download to the system. Returns whether the app should
/// quit so the installer can replace its files.
pub fn install(path: &Path, kind: Kind) -> std::io::Result<bool> {
    match kind {
        Kind::Installer => {
            std::process::Command::new(path).spawn()?;
            Ok(true)
        }
        Kind::Dmg => {
            #[cfg(target_os = "macos")]
            std::process::Command::new("open").arg(path).spawn()?;
            Ok(false)
        }
        _ => {
            if let Some(dir) = path.parent() {
                super::reveal(dir);
            }
            Ok(false)
        }
    }
}
