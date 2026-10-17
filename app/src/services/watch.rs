//! File watching: a skin folder in hot-reload mode and sticker folders.
//! Bursts of events (an editor saving, a copy of fifty files) collapse into
//! one message after a short quiet period.

use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::msg::Msg;

const QUIET: Duration = Duration::from_millis(300);

pub struct Watch {
    /// Held only to keep watching: dropping it stops the watch.
    _watcher: Option<RecommendedWatcher>,
    paths: Vec<PathBuf>,
}

impl Watch {
    /// Watch `paths`; `msg` builds the message posted after changes settle.
    pub fn start(
        paths: Vec<PathBuf>,
        post: impl Fn(Msg) + Send + 'static,
        msg: fn() -> Msg,
    ) -> Self {
        Self::start_excluding(paths, Vec::new(), post, msg)
    }

    pub fn start_excluding(
        paths: Vec<PathBuf>,
        excluded: Vec<PathBuf>,
        post: impl Fn(Msg) + Send + 'static,
        msg: fn() -> Msg,
    ) -> Self {
        let (tx, rx) = mpsc::channel::<()>();
        let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            if event.is_ok_and(|e| {
                !e.kind.is_access()
                    && (e.paths.is_empty()
                        || e.paths.iter().any(|p| {
                            !excluded.iter().any(|root| {
                                crate::stickers::folder(p)
                                    .starts_with(crate::stickers::folder(root))
                            })
                        }))
            }) {
                let _ = tx.send(());
            }
        });
        let mut watcher = match watcher {
            Ok(w) => w,
            Err(_) => {
                return Self {
                    _watcher: None,
                    paths,
                };
            }
        };
        for p in &paths {
            let _ = watcher.watch(p, RecursiveMode::Recursive);
        }
        thread::Builder::new()
            .name("nanatype-watch".into())
            .spawn(move || {
                while rx.recv().is_ok() {
                    // Wait for quiet.
                    loop {
                        match rx.recv_timeout(QUIET) {
                            Ok(()) => continue,
                            Err(mpsc::RecvTimeoutError::Timeout) => break,
                            Err(mpsc::RecvTimeoutError::Disconnected) => return,
                        }
                    }
                    post(msg());
                }
            })
            .expect("spawn watch thread");
        Self {
            _watcher: Some(watcher),
            paths,
        }
    }

    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }
}

/// One scan in flight; newer changes supersede its result and coalesce into
/// one follow-up. Catalogue mutations also invalidate an older snapshot.
#[derive(Default)]
pub struct ReloadGate {
    revision: u64,
    running: bool,
}

pub struct PreparedSkins {
    pub registry: nanatype_theme::SkinRegistry,
    pub loaded: Result<nanatype_theme::LoadedSkin, Vec<nanatype_theme::SkinDiagnostic>>,
    pub scanned: bool,
    pub image_urls: Option<std::collections::HashMap<String, String>>,
}
impl ReloadGate {
    pub fn invalidate(&mut self) {
        self.revision += 1;
    }
    pub fn request(&mut self) -> Option<u64> {
        self.invalidate();
        if self.running {
            None
        } else {
            self.running = true;
            Some(self.revision)
        }
    }
    /// Ok commits; Err starts the newest scan without opening a second worker.
    pub fn finish(&mut self, ticket: u64) -> Result<(), u64> {
        if ticket == self.revision {
            self.running = false;
            Ok(())
        } else {
            Err(self.revision)
        }
    }
    pub fn cancel(&mut self) {
        self.running = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reload_bursts_and_catalogue_mutations_never_commit_a_stale_scan() {
        let mut gate = ReloadGate::default();
        let first = gate.request().unwrap();
        for _ in 0..100 {
            assert!(gate.request().is_none());
        }
        let latest = gate.finish(first).unwrap_err();
        gate.invalidate(); // install/remove/duplicate while that worker runs
        let newest = gate.finish(latest).unwrap_err();
        assert!(gate.finish(newest).is_ok());
        assert!(gate.request().is_some());
    }
}
