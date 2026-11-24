//! Crash guard for skin shaders. A program is written down as on trial
//! before its first draw and cleared once it has drawn for a while; one still
//! on trial at the next launch was running when the process or the GPU driver
//! died, so it stays blocked until the operator turns advanced effects off
//! and on again (or the skin changes the program).
//!
//! Disk persistence runs on a single debounced background writer thread so
//! `watch` / `draw` never block the UI or render thread with atomic file I/O.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, mpsc};
use std::time::{Duration, Instant};

/// A program is healthy after this many draws, or this long and one more
/// draw after its first.
const HEALTHY_DRAWS: u32 = 120;
const HEALTHY_AFTER: Duration = Duration::from_secs(2);
const COALESCE_WINDOW: Duration = Duration::from_millis(50);

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Record {
    trial: BTreeSet<String>,
    blocked: BTreeSet<String>,
}

enum WriteCmd {
    Write { path: PathBuf, bytes: Vec<u8> },
    Flush(mpsc::Sender<()>),
}

struct GuardWriter {
    tx: mpsc::Sender<WriteCmd>,
}

static WRITER: LazyLock<GuardWriter> = LazyLock::new(GuardWriter::start);

impl GuardWriter {
    fn start() -> Self {
        let (tx, rx) = mpsc::channel::<WriteCmd>();
        let spawned = std::thread::Builder::new()
            .name("nanatype-shader-guard".into())
            .spawn(move || Self::run(rx));
        if let Err(err) = spawned {
            nanatype_diag::report!(crate::diag::SHADER_GUARD_FAILED; "{err}");
        }
        Self { tx }
    }

    fn run(rx: mpsc::Receiver<WriteCmd>) {
        fn enqueue(
            c: WriteCmd,
            pending: &mut HashMap<PathBuf, Vec<u8>>,
            flush_waiters: &mut Vec<mpsc::Sender<()>>,
        ) {
            match c {
                WriteCmd::Write { path, bytes } => {
                    pending.insert(path, bytes);
                }
                WriteCmd::Flush(done) => {
                    flush_waiters.push(done);
                }
            }
        }
        let mut pending: HashMap<PathBuf, Vec<u8>> = HashMap::new();
        let mut last_write: HashMap<PathBuf, Instant> = HashMap::new();
        while let Ok(cmd) = rx.recv() {
            let mut flush_waiters: Vec<mpsc::Sender<()>> = Vec::new();
            enqueue(cmd, &mut pending, &mut flush_waiters);
            while let Ok(next) = rx.try_recv() {
                enqueue(next, &mut pending, &mut flush_waiters);
            }
            if flush_waiters.is_empty() {
                let recently_written = pending.keys().any(|p| {
                    last_write
                        .get(p)
                        .is_some_and(|at| at.elapsed() < COALESCE_WINDOW)
                });
                if recently_written {
                    let deadline = Instant::now() + COALESCE_WINDOW;
                    while let Some(rem) = deadline.checked_duration_since(Instant::now()) {
                        match rx.recv_timeout(rem) {
                            Ok(next) => {
                                let is_flush = matches!(next, WriteCmd::Flush(_));
                                enqueue(next, &mut pending, &mut flush_waiters);
                                if is_flush {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                    while let Ok(next) = rx.try_recv() {
                        enqueue(next, &mut pending, &mut flush_waiters);
                    }
                }
            }
            for (path, bytes) in pending.drain() {
                write_record(&path, &bytes);
                last_write.insert(path, Instant::now());
            }
            for done in flush_waiters {
                let _ = done.send(());
            }
        }
    }

    fn enqueue(&self, path: PathBuf, bytes: Vec<u8>) {
        let _ = self.tx.send(WriteCmd::Write { path, bytes });
    }

    fn flush(&self) {
        let (done, wait) = mpsc::channel();
        if self.tx.send(WriteCmd::Flush(done)).is_ok() {
            let _ = wait.recv_timeout(Duration::from_secs(5));
        }
    }
}

fn write_record(path: &Path, bytes: &[u8]) {
    if let Err(e) = nanatype_settings::write_atomic(path, bytes) {
        // Degrades to this run's in-memory guard; rendering goes on.
        nanatype_diag::report!(crate::diag::SHADER_GUARD_FAILED; "{e}");
    }
}

/// Without a path (tests, captures) the guard keeps its state in memory.
#[derive(Default)]
pub(super) struct Guard {
    path: Option<PathBuf>,
    record: Record,
    trials: HashMap<String, (Instant, u32)>,
    passed: HashSet<String>,
}

impl Guard {
    /// Load the record; whatever was on trial when the last run ended is
    /// blocked from now on. Returns those newly blocked hashes.
    pub(super) fn open(path: PathBuf) -> (Self, Vec<String>) {
        WRITER.flush();
        let mut record: Record = nanatype_settings::load_json(&path).value;
        let crashed: Vec<String> = std::mem::take(&mut record.trial).into_iter().collect();
        record.blocked.extend(crashed.iter().cloned());
        let guard = Self {
            path: Some(path),
            record,
            ..Default::default()
        };
        if !crashed.is_empty() {
            guard.save_async();
            guard.flush();
        }
        (guard, crashed)
    }

    pub(super) fn blocked(&self) -> impl Iterator<Item = &String> {
        self.record.blocked.iter()
    }

    pub(super) fn is_blocked(&self, hash: &str) -> bool {
        self.record.blocked.contains(hash)
    }

    /// Called when a program is about to be compiled or first seen. Writes it
    /// down as on trial without counting a healthy draw: compiling can take
    /// longer than [`HEALTHY_AFTER`].
    pub(super) fn watch(&mut self, hash: &str) -> bool {
        if self.is_blocked(hash) {
            return false;
        }
        if self.passed.contains(hash) || self.trials.contains_key(hash) {
            return true;
        }
        if self.record.trial.insert(hash.to_owned()) {
            self.save_async();
        }
        true
    }

    /// Called before each draw of `hash`; false when it must not draw.
    pub(super) fn draw(&mut self, hash: &str, now: Instant) -> bool {
        if self.is_blocked(hash) {
            return false;
        }
        if self.passed.contains(hash) {
            return true;
        }
        match self.trials.get_mut(hash) {
            None => {
                self.trials.insert(hash.to_owned(), (now, 1));
                // Written down before the GPU first sees the program.
                self.record.trial.insert(hash.to_owned());
                self.save_async();
            }
            Some((start, draws)) => {
                *draws += 1;
                if *draws >= HEALTHY_DRAWS || now.duration_since(*start) >= HEALTHY_AFTER {
                    self.trials.remove(hash);
                    self.passed.insert(hash.to_owned());
                    self.record.trial.remove(hash);
                    self.save_async();
                }
            }
        }
        true
    }

    /// Forget every block and trial: the operator asked to retry effects.
    pub(super) fn reset(&mut self) {
        self.trials.clear();
        self.passed.clear();
        if self.record != Record::default() {
            self.record = Record::default();
            self.save_async();
            self.flush();
        }
    }

    /// A clean exit: nothing on trial took the process down.
    pub(super) fn close(&mut self) {
        if !self.record.trial.is_empty() {
            self.record.trial.clear();
            self.save_async();
        }
        self.flush();
    }

    pub(super) fn flush(&self) {
        if self.path.is_some() {
            WRITER.flush();
        }
    }

    fn save_async(&self) {
        let Some(path) = &self.path else {
            return;
        };
        match serde_json::to_vec_pretty(&self.record) {
            Ok(bytes) => WRITER.enqueue(path.clone(), bytes),
            Err(e) => nanatype_diag::report!(crate::diag::SHADER_GUARD_FAILED; "{e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_left_on_trial_is_blocked_at_the_next_launch_until_reset() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shader-guard.json");
        let start = Instant::now();
        let (mut guard, crashed) = Guard::open(path.clone());
        assert!(crashed.is_empty());
        assert!(guard.draw("crashy", start));
        assert!(guard.draw("healthy", start));
        assert!(guard.draw("healthy", start + HEALTHY_AFTER));
        guard.flush();
        // The process dies here: no `close`.
        drop(guard);
        let (mut guard, crashed) = Guard::open(path.clone());
        assert_eq!(crashed, ["crashy"]);
        assert!(!guard.draw("crashy", start));
        assert!(guard.draw("healthy", start));
        guard.close();
        let (mut guard, crashed) = Guard::open(path.clone());
        assert!(crashed.is_empty(), "a clean exit ends every trial");
        assert!(guard.is_blocked("crashy"));
        guard.reset();
        let (guard, _) = Guard::open(path);
        assert!(!guard.is_blocked("crashy"));
    }

    #[test]
    fn enough_draws_end_a_trial_without_waiting() {
        let mut guard = Guard::default();
        let now = Instant::now();
        for _ in 0..HEALTHY_DRAWS {
            assert!(guard.draw("p", now));
        }
        assert!(guard.passed.contains("p") && guard.record.trial.is_empty());
    }

    #[test]
    fn a_slow_compile_does_not_end_a_trial_before_the_first_draw() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shader-guard.json");
        let (mut guard, _) = Guard::open(path.clone());
        let start = Instant::now();
        assert!(guard.watch("m"));
        guard.flush();
        assert!(
            std::fs::read_to_string(&path).unwrap().contains("m"),
            "first sighting was not written down"
        );
        // A compile longer than HEALTHY_AFTER, then the admit that precedes
        // the first draw, must leave the program on trial.
        assert!(guard.watch("m"));
        assert!(guard.draw("m", start + HEALTHY_AFTER));
        assert!(
            guard.record.trial.contains("m"),
            "the first real draw after a slow compile must still be on trial"
        );
        assert!(guard.draw("m", start + HEALTHY_AFTER + Duration::from_secs(2)));
        assert!(guard.passed.contains("m") && guard.record.trial.is_empty());
    }
}
