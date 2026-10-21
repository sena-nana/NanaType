use crate::services::updater::{self, Event};
use crate::ui::feedback::{self, ToastRequest};
use crate::{app::App, msg::Msg};
use nanatype_update::{Asset, Checked, Error, Kind};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Automatic checks run at most this often.
const CHECK_EVERY: Duration = Duration::from_secs(24 * 3600);
/// Startup checks wait for the app to settle.
const STARTUP_DELAY: Duration = Duration::from_secs(10);

/// What the About page shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available {
        version: String,
        size: u64,
    },
    Downloading {
        version: String,
        done: u64,
        total: u64,
        source: String,
    },
    Ready {
        version: String,
        kind: Kind,
        path: PathBuf,
    },
    Failed {
        message: String,
        detail: String,
    },
}

/// The release on offer and the download chosen from it.
pub type Pending = Option<(Checked, Asset)>;

impl App {
    pub(super) fn update_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Update(event) => self.update_event(event),
            Msg::CheckUpdate => self.check_update(Duration::ZERO, true),
            Msg::DownloadUpdate => self.download_update(),
            Msg::CancelUpdate => self.updater.cancel(),
            Msg::InstallUpdate => self.install_update(),
            Msg::SkipUpdate => self.skip_update(),
            Msg::OpenUpdateNotes => {
                if let Some((checked, _)) = &self.pending_update {
                    crate::services::open_url(&checked.manifest.notes_url());
                }
            }
            _ => unreachable!("not an update message"),
        }
    }

    /// The daily check, when enabled.
    pub(super) fn check_update_on_start(&mut self) {
        let last = self.settings.update.last_check.unwrap_or(0);
        if self.settings.general.check_updates
            && unix_now().saturating_sub(last) >= CHECK_EVERY.as_secs()
        {
            self.check_update(STARTUP_DELAY, false);
        }
    }

    fn check_update(&mut self, delay: Duration, manual: bool) {
        if matches!(self.update_state(), State::Downloading { .. }) {
            return;
        }
        if manual {
            self.set_update_state(State::Checking);
        }
        self.updater.check(&self.settings.update, delay, manual);
    }

    fn update_event(&mut self, event: Event) {
        match event {
            Event::Checked { manual, result } => self.update_checked(manual, result),
            Event::Progress(p) => {
                if let State::Downloading { version, .. } = self.update_state() {
                    self.set_update_state(State::Downloading {
                        version,
                        done: p.done,
                        total: p.total,
                        source: p.source.label(),
                    });
                }
            }
            Event::Downloaded(result) => self.update_downloaded(result),
        }
    }

    fn update_checked(&mut self, manual: bool, result: Result<Checked, Error>) {
        if !manual {
            self.edit(|s| s.update.last_check = Some(unix_now()));
        }
        let checked = match result {
            Ok(checked) => checked,
            // Nothing published yet reads the same as being current.
            Err(Error::NoRelease) => return self.set_update_state(State::UpToDate),
            Err(e) => {
                if manual {
                    self.update_failed(e);
                }
                return;
            }
        };
        let version = checked.manifest.version.clone();
        if !nanatype_update::is_newer(&version, nanatype_update::current()) {
            return self.set_update_state(State::UpToDate);
        }
        let Some(asset) = updater::pick(&checked) else {
            if manual {
                self.update_failed(Error::NoAsset);
            }
            return;
        };
        let skipped = self.settings.update.skipped_version.as_deref() == Some(version.as_str());
        if manual || !skipped {
            self.set_update_state(State::Available {
                version: version.clone(),
                size: asset.size,
            });
        }
        if !manual && !skipped {
            let text = self.ui.tr("update-toast", &[("version", version.into())]);
            self.toasts.push(ToastRequest::new(
                text,
                nana_ui::ToastTone::Info,
                feedback::NOTICE_MS,
                true,
            ));
        }
        self.pending_update = Some((checked, asset));
    }

    fn download_update(&mut self) {
        let Some((checked, asset)) = &self.pending_update else {
            return;
        };
        self.updater.download(checked, asset);
        self.set_update_state(State::Downloading {
            version: checked.manifest.version.clone(),
            done: 0,
            total: asset.size,
            source: String::new(),
        });
    }

    fn update_downloaded(&mut self, result: Result<PathBuf, Error>) {
        let Some((checked, asset)) = &self.pending_update else {
            return;
        };
        let version = checked.manifest.version.clone();
        match result {
            Ok(path) => self.set_update_state(State::Ready {
                version,
                kind: asset.kind,
                path,
            }),
            Err(Error::Cancelled) => {
                let size = asset.size;
                self.set_update_state(State::Available { version, size });
            }
            Err(e) => self.update_failed(e),
        }
    }

    fn install_update(&mut self) {
        let State::Ready { kind, path, .. } = self.update_state() else {
            return;
        };
        match updater::install(&path, kind) {
            Ok(true) => self.quit(),
            Ok(false) => {}
            Err(e) => self.update_failed(Error::Io(e.to_string())),
        }
    }

    fn skip_update(&mut self) {
        let Some((checked, _)) = &self.pending_update else {
            return;
        };
        let version = checked.manifest.version.clone();
        self.updater.cancel();
        self.edit(|s| s.update.skipped_version = Some(version));
        self.set_update_state(State::Idle);
    }

    fn update_failed(&mut self, error: Error) {
        let key = match &error {
            Error::Unreachable(_) => "update-error-unreachable",
            Error::BadManifest(_) => "update-error-rejected",
            Error::NoAsset => "update-error-no-asset",
            Error::Download(_) => "update-error-download",
            Error::NoRelease | Error::Io(_) | Error::Cancelled => "update-error-other",
        };
        self.set_update_state(State::Failed {
            message: self.ui.now_t(key),
            detail: error.to_string(),
        });
    }

    fn update_state(&self) -> State {
        (*self.ui.update.get_untracked()).clone()
    }

    fn set_update_state(&mut self, state: State) {
        self.ui.update.set(Arc::new(state));
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
