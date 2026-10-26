//! Pure submission policy. Recognition and timers never submit directly.
use crate::Activity;
use nanatype_settings::SpeechMode;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Off,
    Starting,
    Listening,
    Recognizing,
    Countdown,
    Paused,
    Error,
}

impl Phase {
    pub fn active(self) -> bool {
        self != Self::Off
    }
}

#[derive(Debug, Default)]
pub struct Controller {
    pub generation: u64,
    pub revision: u64,
    pub phase: Phase,
    pub partial: String,
    pub remaining: u32,
    manual_line: bool,
    has_text: bool,
    pending: BTreeMap<u64, Option<String>>,
    next: u64,
}

impl Controller {
    pub fn start(&mut self, existing_draft: bool) -> u64 {
        self.invalidate();
        self.phase = Phase::Starting;
        self.manual_line = existing_draft;
        self.has_text = false;
        self.next = 1;
        self.generation
    }
    fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.revision = self.revision.wrapping_add(1);
        self.pending.clear();
        self.partial.clear();
        self.remaining = 0;
    }
    /// A completed line does not restart capture or reload the model. Keep
    /// utterance IDs increasing so delayed results cannot enter the next line.
    pub fn continue_after_send(&mut self, service_pending: usize) -> bool {
        if !self.active() || !self.pending.is_empty() || service_pending != 0 {
            return false;
        }
        self.revision = self.revision.wrapping_add(1);
        self.manual_line = false;
        self.has_text = false;
        self.remaining = 0;
        self.partial.clear();
        if self.phase != Phase::Starting {
            self.phase = Phase::Listening;
        }
        true
    }
    pub fn stop(&mut self) {
        self.invalidate();
        self.phase = Phase::Off;
    }
    pub fn pause(&mut self) {
        if self.active() {
            self.invalidate();
            self.phase = Phase::Paused;
            self.manual_line = true;
        }
    }
    pub fn fail(&mut self) {
        self.pause();
        self.phase = Phase::Error;
    }
    pub fn active(&self) -> bool {
        self.phase.active()
    }
    pub fn running(&self) -> bool {
        !matches!(self.phase, Phase::Paused | Phase::Error | Phase::Off)
    }
    pub fn pending(&mut self, id: u64) {
        if self.running() && id >= self.next {
            self.pending.entry(id).or_insert(None);
            self.phase = Phase::Recognizing;
        }
    }
    /// Commit in microphone order, including when native callbacks finish out of order.
    pub fn final_text(&mut self, id: u64, text: String) -> Vec<String> {
        let Some(result) = self.pending.get_mut(&id) else {
            return Vec::new();
        };
        result.get_or_insert(text);
        let mut committed = Vec::new();
        while matches!(self.pending.get(&self.next), Some(Some(_))) {
            let text = self.pending.remove(&self.next).flatten().unwrap();
            self.next += 1;
            if !text.trim().is_empty() {
                self.has_text = true;
                committed.push(text);
            }
        }
        self.partial.clear();
        committed
    }
    /// The caller also checks the service's pending counter: a freshly captured
    /// utterance may not yet have reached the application's event queue.
    pub fn tick(
        &mut self,
        mode: SpeechMode,
        seconds: u32,
        now: Instant,
        activity: Activity,
    ) -> bool {
        let Activity {
            last_voice,
            speaking,
            pending: service_pending,
        } = activity;
        if !self.running() || self.phase == Phase::Starting {
            return false;
        }
        if speaking {
            self.remaining = 0;
            self.phase = Phase::Recognizing;
            return false;
        }
        if mode == SpeechMode::Manual || self.manual_line || !self.has_text {
            self.phase = if self.pending.is_empty() && service_pending == 0 {
                Phase::Listening
            } else {
                Phase::Recognizing
            };
            return false;
        }
        let Some(last) = last_voice else { return false };
        let elapsed = now.saturating_duration_since(last);
        let wait = Duration::from_secs(u64::from(seconds));
        self.remaining = wait.saturating_sub(elapsed).as_secs_f64().ceil() as u32;
        if !self.pending.is_empty() || service_pending != 0 {
            self.phase = Phase::Recognizing;
            return false;
        }
        if elapsed < wait {
            self.phase = Phase::Countdown;
            return false;
        }
        // Consume eligibility before dispatch, so even a second timer cannot send twice.
        self.has_text = false;
        self.manual_line = true;
        self.phase = Phase::Paused;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn audio(at: Instant, speaking: bool, pending: usize) -> Activity {
        Activity {
            last_voice: Some(at),
            speaking,
            pending,
        }
    }
    fn line() -> (Controller, Instant) {
        let mut c = Controller::default();
        c.start(false);
        c.phase = Phase::Listening;
        c.pending(1);
        assert_eq!(c.final_text(1, "hello".into()), ["hello"]);
        (c, Instant::now())
    }
    #[test]
    fn activation_is_derived_from_phase_and_pause_keeps_voice_enabled() {
        let mut c = Controller::default();
        assert!(!c.active());
        c.start(false);
        assert!(c.active() && c.running());
        c.pause();
        assert!(c.active() && !c.running());
        c.fail();
        assert!(c.active() && !c.running());
        c.stop();
        assert!(!c.active() && !c.running());
    }
    #[test]
    fn consecutive_lines_keep_the_backend_session_and_invalidate_old_timer_revisions() {
        let (mut c, at) = line();
        let session = c.generation;
        let revision = c.revision;
        assert!(c.tick(
            SpeechMode::Auto,
            3,
            at + Duration::from_secs(3),
            audio(at, false, 0)
        ));
        assert!(c.continue_after_send(0));
        assert_eq!(c.generation, session);
        assert_ne!(c.revision, revision);
        assert!(c.running());
        assert!(c.final_text(1, "late duplicate".into()).is_empty());
        assert!(!c.tick(
            SpeechMode::Auto,
            3,
            at + Duration::from_secs(4),
            audio(at, false, 0)
        ));
        c.pending(2);
        assert!(!c.continue_after_send(1));
        assert_eq!(c.final_text(2, "next line".into()), ["next line"]);
        assert!(c.tick(
            SpeechMode::Auto,
            3,
            at + Duration::from_secs(8),
            audio(at + Duration::from_secs(5), false, 0)
        ));
    }
    #[test]
    fn silence_requires_final_results_and_sends_once() {
        let (mut c, at) = line();
        assert!(!c.tick(
            SpeechMode::Auto,
            3,
            at + Duration::from_secs(2),
            audio(at, false, 0)
        ));
        assert_eq!(c.phase, Phase::Countdown);
        assert!(!c.tick(
            SpeechMode::Auto,
            3,
            at + Duration::from_secs(4),
            audio(at, false, 1)
        ));
        assert!(c.tick(
            SpeechMode::Auto,
            3,
            at + Duration::from_secs(4),
            audio(at, false, 0)
        ));
        assert!(!c.tick(
            SpeechMode::Auto,
            3,
            at + Duration::from_secs(5),
            audio(at, false, 0)
        ));
    }
    #[test]
    fn edits_and_new_voice_cancel_a_deadline() {
        let (mut c, at) = line();
        assert!(!c.tick(
            SpeechMode::Auto,
            3,
            at + Duration::from_secs(3),
            audio(at, true, 0)
        ));
        let old = c.generation;
        c.pause();
        assert_ne!(old, c.generation);
        assert!(!c.tick(
            SpeechMode::Auto,
            3,
            at + Duration::from_secs(8),
            audio(at, false, 0)
        ));
    }
    #[test]
    fn existing_draft_and_manual_mode_never_auto_send() {
        for mode in [SpeechMode::Manual, SpeechMode::Auto] {
            let (mut c, at) = line();
            c.manual_line = true;
            assert!(!c.tick(mode, 3, at + Duration::from_secs(9), audio(at, false, 0)));
        }
    }
    #[test]
    fn results_are_ordered_and_duplicates_and_late_results_ignored() {
        let mut c = Controller::default();
        c.start(false);
        c.pending(1);
        c.pending(2);
        assert!(c.final_text(2, "two".into()).is_empty());
        c.pending(2);
        assert!(c.final_text(2, "duplicate".into()).is_empty());
        assert_eq!(c.final_text(1, "one".into()), ["one", "two"]);
        assert!(c.final_text(1, "again".into()).is_empty());
        c.pause();
        assert!(c.final_text(3, "late".into()).is_empty());
    }
    #[test]
    fn late_final_uses_audio_time_instead_of_result_time() {
        let (mut c, at) = line();
        c.pending(2);
        assert!(!c.tick(
            SpeechMode::Auto,
            3,
            at + Duration::from_secs(4),
            audio(at, false, 0)
        ));
        c.final_text(2, "world".into());
        assert!(c.tick(
            SpeechMode::Auto,
            3,
            at + Duration::from_secs(4),
            audio(at, false, 0)
        ));
    }
}
