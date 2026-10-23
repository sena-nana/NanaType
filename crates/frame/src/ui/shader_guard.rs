//! Crash guard for skin shaders. A program is written down as on trial
//! before its first draw and cleared once it has drawn for a while; one still
//! on trial at the next launch was running when the process or the GPU driver
//! died, so it stays blocked until the operator turns advanced effects off
//! and on again (or the skin changes the program).
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// A program is healthy after this many draws, or this long and one more
/// draw after its first.
const HEALTHY_DRAWS: u32 = 120;
const HEALTHY_AFTER: Duration = Duration::from_secs(2);

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Record {
    trial: BTreeSet<String>,
    blocked: BTreeSet<String>,
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
        let mut record: Record = nanatype_settings::load_json(&path).value;
        let crashed: Vec<String> = std::mem::take(&mut record.trial).into_iter().collect();
        record.blocked.extend(crashed.iter().cloned());
        let guard = Self {
            path: Some(path),
            record,
            ..Default::default()
        };
        if !crashed.is_empty() {
            guard.save();
        }
        (guard, crashed)
    }

    pub(super) fn blocked(&self) -> impl Iterator<Item = &String> {
        self.record.blocked.iter()
    }

    pub(super) fn is_blocked(&self, hash: &str) -> bool {
        self.record.blocked.contains(hash)
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
                self.save();
            }
            Some((start, draws)) => {
                *draws += 1;
                if *draws >= HEALTHY_DRAWS || now.duration_since(*start) >= HEALTHY_AFTER {
                    self.trials.remove(hash);
                    self.passed.insert(hash.to_owned());
                    self.record.trial.remove(hash);
                    self.save();
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
            self.save();
        }
    }

    /// A clean exit: nothing on trial took the process down.
    pub(super) fn close(&mut self) {
        if !self.record.trial.is_empty() {
            self.record.trial.clear();
            self.save();
        }
    }

    fn save(&self) {
        let Some(path) = &self.path else {
            return;
        };
        let written = serde_json::to_vec_pretty(&self.record)
            .map_err(|e| e.to_string())
            .and_then(|bytes| {
                nanatype_settings::write_atomic(path, &bytes).map_err(|e| e.to_string())
            });
        if let Err(e) = written {
            // Degrades to this run's in-memory guard; rendering goes on.
            nanatype_diag::report!(crate::diag::SHADER_GUARD_FAILED; "{e}");
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
}
