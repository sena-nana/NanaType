use std::sync::Arc;

use nanatype_model::{AfterLine, LineId};

use crate::{Cue, CueKind, SegmentEnd, Timeline};

/// Length of the fade-out once a fading line's hold time is over.
pub const FADE_OUT_MS: u32 = 450;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeReason {
    SegmentEnd,
    Settled,
    AutoContinue,
    AfterLine,
    ExitDone,
    AutoAdvance,
    /// Typing waits for speech: no word came in time, so it types on its own.
    SpeechTimeout,
}

/// How long typing waits for the first spoken word before it types on its own.
pub const SPEECH_WAIT_MS: u32 = 300;
/// Gap between the units a spoken word releases, and the most a word's
/// units take in all.
const SPEECH_GAP_MS: u32 = 35;
const SPEECH_WORD_MS: u32 = 240;

/// Typing paced by speech: units wait until a spoken word reaches them.
#[derive(Debug, Clone)]
struct Speech {
    /// When each unit of the line was released (host ms); `None` waits.
    release: Vec<Option<u64>>,
    /// A word arrived; the timeout no longer applies.
    heard: bool,
}

impl Speech {
    /// Release units up to `through` (exclusive) from `now`, spread a little
    /// so a word still types in. Units released already keep their time.
    fn release(&mut self, through: u32, now: u64) {
        let through = (through as usize).min(self.release.len());
        let held: Vec<usize> = (0..through)
            .filter(|u| self.release[*u].is_none())
            .collect();
        let gap = if held.is_empty() {
            0
        } else {
            SPEECH_GAP_MS.min(SPEECH_WORD_MS / held.len() as u32)
        };
        for (k, u) in held.into_iter().enumerate() {
            self.release[u] = Some(now + u64::from(gap) * k as u64);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExitStyle {
    #[default]
    Instant,
    Fade {
        ms: u32,
    },
    /// Skin scramble or material exit.
    Presentation {
        ms: u16,
    },
}

/// Timing and exit behaviour for one playing line, resolved from settings and skin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaybackProfile {
    /// Glyph entrance / scramble settle duration held at segment end.
    pub intro_ms: u32,
    pub exit: ExitStyle,
    pub after: AfterLine,
    pub auto_advance_ms: Option<u32>,
}

impl PlaybackProfile {
    pub fn new(after: AfterLine, auto_advance_ms: Option<u32>) -> Self {
        Self {
            intro_ms: 0,
            exit: ExitStyle::Instant,
            after,
            auto_advance_ms,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Revealing,
    /// Waiting for the last revealed glyph's entrance/scramble to settle.
    Settling,
    /// Waiting at a pause or page marker for the continue key.
    Gated,
    /// Fully revealed; holding or counting down to an exit.
    Done,
    Exiting,
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitAction {
    PageNext(usize),
    FinishLine,
}

/// Instructions for the host. Each is idempotent for its `epoch`; a newer
/// epoch makes every earlier wake and cue stale.
#[derive(Debug, Clone, PartialEq)]
pub enum PlayerEffect {
    /// Animate segment `seg` from `origin_ms`. Units before `segment.units`
    /// are already visible; units from `visible_from` on are drawn.
    ShowSegment {
        epoch: u64,
        seg: usize,
        origin_ms: u64,
        /// Reveal offsets; all zero when the segment was skipped.
        reveal_ms: Arc<[u32]>,
    },
    ScheduleCues {
        epoch: u64,
        origin_ms: u64,
        cues: Arc<[Cue]>,
    },
    /// Fire these cues right now (skip kept expressions and sounds).
    FireNow {
        cues: Vec<Cue>,
    },
    WakeAt {
        epoch: u64,
        at_ms: u64,
        reason: WakeReason,
    },
    ContinueIndicator(bool),
    /// Start the exit animation (fade-out or skin presentation exit).
    StartExit {
        at_ms: u64,
        style: ExitStyle,
    },
    /// Every unit of the line is visible.
    LineRevealed(LineId),
    /// The line left the box (exit or clear finished).
    LineFinished(LineId),
    /// `auto_advance_ms` elapsed: start the next queued line if any.
    AutoAdvance(LineId),
}

/// What is on screen at a moment, for tests and for history sync.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RevealSnapshot {
    pub phase: Phase,
    pub visible_from: u32,
    /// Units `visible_from..revealed` are visible.
    pub revealed: u32,
    pub opacity: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Revealing {
        seg: usize,
        origin: u64,
        skipped: bool,
        last_unit_at: Option<u64>,
    },
    Settling {
        seg: usize,
        until: u64,
    },
    Gated {
        seg: usize,
    },
    Done {
        at: u64,
    },
    Exiting {
        at: u64,
        style: ExitStyle,
        next: ExitAction,
    },
    Finished,
}

pub struct Player {
    line: LineId,
    timeline: Arc<Timeline>,
    profile: PlaybackProfile,
    state: State,
    epoch: u64,
    /// `LineRevealed` went out (a line paged after it was done keeps it).
    revealed: bool,
    /// Skip jumped to the end: the last page stays.
    skipped_to_end: bool,
    /// Pages started by [`Self::paginate`]; their own layout is not paged
    /// again, so a re-wrap cannot add a sliver of a page.
    paged: Vec<u32>,
    /// Typing follows speech ([`Self::follow_speech`]).
    speech: Option<Speech>,
}

impl Player {
    pub fn start(
        line: LineId,
        timeline: Arc<Timeline>,
        profile: PlaybackProfile,
        now: u64,
    ) -> (Self, Vec<PlayerEffect>) {
        let mut player = Self::raw(line, timeline, profile);
        let mut out = Vec::new();
        player.enter_segment(0, now, &mut out);
        (player, out)
    }

    /// Start a line that is already on screen (live-mirror send): skips segment
    /// reveal and enters [`Phase::Done`] directly.
    pub fn start_completed(
        line: LineId,
        timeline: Arc<Timeline>,
        profile: PlaybackProfile,
        now: u64,
    ) -> (Self, Vec<PlayerEffect>) {
        let mut player = Self::raw(line, timeline, profile);
        let mut out = Vec::new();
        let pending: Vec<Cue> = player
            .timeline
            .segments
            .iter()
            .flat_map(|s| s.cues.iter())
            .filter(|c| c.kind.survives_skip())
            .cloned()
            .collect();
        if !pending.is_empty() {
            out.push(PlayerEffect::FireNow { cues: pending });
        }
        player.enter_done(now, &mut out);
        (player, out)
    }

    /// Start directly in [`Phase::Exiting`] (e.g. retiring an uncommitted mirror draft).
    pub fn start_exiting(
        line: LineId,
        timeline: Arc<Timeline>,
        profile: PlaybackProfile,
        now: u64,
    ) -> (Self, Vec<PlayerEffect>) {
        let mut player = Self::raw(line, timeline, profile);
        player.revealed = true;
        player.state = State::Done { at: now };
        let (_, out) = player.begin_exit(now);
        (player, out)
    }

    fn raw(line: LineId, timeline: Arc<Timeline>, profile: PlaybackProfile) -> Self {
        Self {
            line,
            timeline,
            profile,
            state: State::Finished,
            epoch: 0,
            revealed: false,
            skipped_to_end: false,
            paged: Vec::new(),
            speech: None,
        }
    }

    /// Update the profile mid-line (e.g. when reduced motion or skin effects change).
    pub fn set_profile(&mut self, profile: PlaybackProfile, now: u64) -> Vec<PlayerEffect> {
        self.profile = profile;
        let mut out = Vec::new();
        match self.state {
            State::Settling { seg, .. } if self.profile.intro_ms == 0 => {
                self.end_segment(seg, now, &mut out);
            }
            State::Exiting {
                style: ExitStyle::Presentation { .. },
                ..
            } if !matches!(self.profile.exit, ExitStyle::Presentation { ms } if ms > 0) => {
                out = self.finish_exit(now);
            }
            _ => {}
        }
        out
    }

    /// Passing this gate retires the current visible page.
    pub fn next_clears_page(&self) -> bool {
        if let State::Gated { seg } = self.state {
            self.timeline
                .segments
                .get(seg + 1)
                .is_some_and(|next| next.visible_from != self.timeline.segments[seg].visible_from)
        } else {
            false
        }
    }

    pub fn line(&self) -> LineId {
        self.line
    }

    pub fn timeline(&self) -> &Arc<Timeline> {
        &self.timeline
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn phase(&self) -> Phase {
        match self.state {
            State::Revealing { .. } => Phase::Revealing,
            State::Settling { .. } => Phase::Settling,
            State::Gated { .. } => Phase::Gated,
            State::Done { .. } => Phase::Done,
            State::Exiting { .. } => Phase::Exiting,
            State::Finished => Phase::Finished,
        }
    }

    /// Current segment index (the last one once done).
    pub fn segment(&self) -> usize {
        match self.state {
            State::Revealing { seg, .. } | State::Settling { seg, .. } | State::Gated { seg } => {
                seg
            }
            State::Exiting {
                next: ExitAction::PageNext(next_seg),
                ..
            } => next_seg.saturating_sub(1),
            _ => self.timeline.last_segment(),
        }
    }

    fn bump(&mut self) -> u64 {
        self.epoch += 1;
        self.epoch
    }

    fn enter_segment(&mut self, seg: usize, now: u64, out: &mut Vec<PlayerEffect>) {
        if let Some(mut speech) = self.speech.take() {
            // Words said while the box waited show as the segment starts.
            let units = self.timeline.segments[seg].units.clone();
            let said = units
                .clone()
                .take_while(|u| speech.release[*u as usize].is_some())
                .count() as u32;
            for u in units.clone() {
                speech.release[u as usize] = None;
            }
            speech.release(units.start + said, now);
            self.state = State::Revealing {
                seg,
                origin: now,
                skipped: false,
                last_unit_at: None,
            };
            out.push(PlayerEffect::ContinueIndicator(false));
            self.retime(&speech, seg, now, now, out);
            self.speech = Some(speech);
            return;
        }
        let epoch = self.bump();
        let segment = &self.timeline.segments[seg];
        let last_unit_at = segment
            .reveal_ms
            .iter()
            .max()
            .copied()
            .map(|off| now.saturating_add(u64::from(off)));
        self.state = State::Revealing {
            seg,
            origin: now,
            skipped: false,
            last_unit_at,
        };
        out.push(PlayerEffect::ContinueIndicator(false));
        out.push(PlayerEffect::ShowSegment {
            epoch,
            seg,
            origin_ms: now,
            reveal_ms: segment.reveal_ms.clone(),
        });
        if !segment.cues.is_empty() {
            out.push(PlayerEffect::ScheduleCues {
                epoch,
                origin_ms: now,
                cues: segment.cues.clone(),
            });
        }
        out.push(PlayerEffect::WakeAt {
            epoch,
            at_ms: now + u64::from(segment.duration_ms),
            reason: WakeReason::SegmentEnd,
        });
    }

    fn settle_or_end(
        &mut self,
        seg: usize,
        last_unit_at: Option<u64>,
        now: u64,
        out: &mut Vec<PlayerEffect>,
    ) {
        if self.profile.intro_ms > 0
            && let Some(until) = last_unit_at
                .map(|at| at.saturating_add(u64::from(self.profile.intro_ms)))
                .filter(|&until| now < until)
        {
            let epoch = self.bump();
            self.state = State::Settling { seg, until };
            out.push(PlayerEffect::WakeAt {
                epoch,
                at_ms: until,
                reason: WakeReason::Settled,
            });
            return;
        }
        self.end_segment(seg, now, out);
    }

    fn end_segment(&mut self, seg: usize, now: u64, out: &mut Vec<PlayerEffect>) {
        match self.timeline.segments[seg].end {
            SegmentEnd::LineEnd => self.enter_done(now, out),
            _ => self.gate(seg, now, out),
        }
    }

    /// Wait at the end of `seg` for continue, or for the gate's own time.
    fn gate(&mut self, seg: usize, now: u64, out: &mut Vec<PlayerEffect>) {
        let epoch = self.bump();
        self.state = State::Gated { seg };
        out.push(PlayerEffect::ContinueIndicator(true));
        if let Some(ms) = self.timeline.segments[seg].end.auto_ms() {
            out.push(PlayerEffect::WakeAt {
                epoch,
                at_ms: now + u64::from(ms),
                reason: WakeReason::AutoContinue,
            });
        }
    }

    fn enter_done(&mut self, now: u64, out: &mut Vec<PlayerEffect>) {
        let epoch = self.bump();
        self.state = State::Done { at: now };
        out.push(PlayerEffect::ContinueIndicator(false));
        if !std::mem::replace(&mut self.revealed, true) {
            out.push(PlayerEffect::LineRevealed(self.line));
        }
        let units = self.timeline.unit_count;
        match self.profile.after {
            AfterLine::Hold => {}
            AfterLine::Fade {
                delay_ms,
                per_unit_ms,
            } => out.push(PlayerEffect::WakeAt {
                epoch,
                at_ms: now + u64::from(delay_ms) + u64::from(per_unit_ms) * u64::from(units),
                reason: WakeReason::AfterLine,
            }),
            AfterLine::Clear { delay_ms } => out.push(PlayerEffect::WakeAt {
                epoch,
                at_ms: now + u64::from(delay_ms),
                reason: WakeReason::AfterLine,
            }),
        }
        if let Some(ms) = self.profile.auto_advance_ms {
            out.push(PlayerEffect::WakeAt {
                epoch,
                at_ms: now + u64::from(ms),
                reason: WakeReason::AutoAdvance,
            });
        }
    }

    fn start_exit(
        &mut self,
        at: u64,
        style: ExitStyle,
        next: ExitAction,
        out: &mut Vec<PlayerEffect>,
    ) {
        let dur_ms = match style {
            ExitStyle::Instant => {
                match next {
                    ExitAction::PageNext(seg) => self.enter_segment(seg, at, out),
                    ExitAction::FinishLine => self.finish(out),
                }
                return;
            }
            ExitStyle::Fade { ms } => u64::from(ms),
            ExitStyle::Presentation { ms } => u64::from(ms),
        };
        let epoch = self.bump();
        self.state = State::Exiting { at, style, next };
        out.push(PlayerEffect::ContinueIndicator(false));
        out.push(PlayerEffect::StartExit { at_ms: at, style });
        out.push(PlayerEffect::WakeAt {
            epoch,
            at_ms: at.saturating_add(dur_ms),
            reason: WakeReason::ExitDone,
        });
    }

    fn advance_from_gate(&mut self, seg: usize, now: u64, out: &mut Vec<PlayerEffect>) {
        let clears_page = self
            .timeline
            .segments
            .get(seg + 1)
            .is_some_and(|next| next.visible_from != self.timeline.segments[seg].visible_from);
        if clears_page
            && let ExitStyle::Presentation { ms } = self.profile.exit
            && ms > 0
            && self.timeline.segments[seg].units.end > self.timeline.segments[seg].visible_from
        {
            self.start_exit(
                now,
                ExitStyle::Presentation { ms },
                ExitAction::PageNext(seg + 1),
                out,
            );
            return;
        }
        self.enter_segment(seg + 1, now, out);
    }

    /// Begin a presentation exit to retire the current line (used by `ClearDisplay`
    /// and `Replace`). Returns `true` if a finite presentation exit started (the
    /// caller should wait for [`WakeReason::ExitDone`]), or `false` if the line
    /// finished immediately.
    pub fn begin_exit(&mut self, now: u64) -> (bool, Vec<PlayerEffect>) {
        let mut out = Vec::new();
        if let State::Exiting {
            at,
            style: ExitStyle::Presentation { ms },
            ..
        } = self.state
        {
            self.state = State::Exiting {
                at,
                style: ExitStyle::Presentation { ms },
                next: ExitAction::FinishLine,
            };
            return (true, out);
        }
        if let ExitStyle::Presentation { ms } = self.profile.exit
            && ms > 0
            && !matches!(self.state, State::Finished)
        {
            self.start_exit(
                now,
                ExitStyle::Presentation { ms },
                ExitAction::FinishLine,
                &mut out,
            );
            return (true, out);
        }
        self.finish(&mut out);
        (false, out)
    }

    /// Complete an active [`Phase::Exiting`] immediately (e.g. on user skip or
    /// settings change).
    pub fn finish_exit(&mut self, now: u64) -> Vec<PlayerEffect> {
        let mut out = Vec::new();
        if let State::Exiting { next, .. } = self.state {
            match next {
                ExitAction::PageNext(seg) => self.enter_segment(seg, now, &mut out),
                ExitAction::FinishLine => self.finish(&mut out),
            }
        }
        out
    }

    /// A scheduled wake arrived. Stale epochs are ignored.
    pub fn on_wake(&mut self, epoch: u64, reason: WakeReason, now: u64) -> Vec<PlayerEffect> {
        let mut out = Vec::new();
        if epoch != self.epoch {
            return out;
        }
        match (self.state, reason) {
            (
                State::Revealing {
                    seg,
                    skipped: false,
                    last_unit_at,
                    ..
                },
                WakeReason::SegmentEnd,
            ) => self.settle_or_end(seg, last_unit_at, now, &mut out),
            (State::Revealing { seg, .. }, WakeReason::SegmentEnd) => {
                self.end_segment(seg, now, &mut out)
            }
            (State::Settling { seg, .. }, WakeReason::Settled) => {
                self.end_segment(seg, now, &mut out)
            }
            (State::Gated { seg }, WakeReason::AutoContinue) => {
                self.advance_from_gate(seg, now, &mut out)
            }
            (State::Done { .. }, WakeReason::AfterLine) => match self.profile.after {
                AfterLine::Fade { .. } => {
                    let style = match self.profile.exit {
                        ExitStyle::Presentation { ms } if ms > 0 => ExitStyle::Presentation { ms },
                        _ => ExitStyle::Fade { ms: FADE_OUT_MS },
                    };
                    self.start_exit(now, style, ExitAction::FinishLine, &mut out);
                }
                AfterLine::Clear { .. } => match self.profile.exit {
                    ExitStyle::Presentation { ms } if ms > 0 => {
                        self.start_exit(
                            now,
                            ExitStyle::Presentation { ms },
                            ExitAction::FinishLine,
                            &mut out,
                        );
                    }
                    _ => self.finish(&mut out),
                },
                AfterLine::Hold => {}
            },
            (State::Done { .. }, WakeReason::AutoAdvance) => {
                out.push(PlayerEffect::AutoAdvance(self.line))
            }
            (State::Exiting { .. }, WakeReason::ExitDone) => {
                out = self.finish_exit(now);
            }
            (_, WakeReason::SpeechTimeout) if self.speech.as_ref().is_some_and(|s| !s.heard) => {
                out = self.speech_ended(now).1;
            }
            _ => {}
        }
        out
    }

    /// Pace the rest of the line to speech: from now on units wait for
    /// [`Self::speech_reached`]. Typing goes on by itself if no word comes
    /// within [`SPEECH_WAIT_MS`] (or after [`Self::speech_ended`]). Returns
    /// whether the current schedule was replaced (cancel its cues first).
    pub fn follow_speech(&mut self, now: u64) -> (bool, Vec<PlayerEffect>) {
        let mut out = Vec::new();
        let State::Revealing {
            seg,
            origin,
            skipped: false,
            ..
        } = self.state
        else {
            return (false, out);
        };
        if self.speech.is_some() {
            return (false, out);
        }
        // What is on screen already stays; the rest waits.
        let tl = &self.timeline;
        let mut release = vec![None; tl.unit_count as usize];
        let s = &tl.segments[seg];
        for u in 0..s.units.start {
            release[u as usize] = Some(0);
        }
        let elapsed = now.saturating_sub(origin);
        for (i, at) in s.reveal_ms.iter().enumerate() {
            if u64::from(*at) <= elapsed {
                release[s.units.start as usize + i] = Some(origin + u64::from(*at));
            }
        }
        let speech = Speech {
            release,
            heard: false,
        };
        self.retime(&speech, seg, origin, now, &mut out);
        self.speech = Some(speech);
        out.push(PlayerEffect::WakeAt {
            epoch: self.epoch,
            at_ms: now + u64::from(SPEECH_WAIT_MS),
            reason: WakeReason::SpeechTimeout,
        });
        (true, out)
    }

    /// Speech reached unit `through` of the line (exclusive): type up to it.
    pub fn speech_reached(&mut self, through: u32, now: u64) -> (bool, Vec<PlayerEffect>) {
        let mut out = Vec::new();
        let Some(mut speech) = self.speech.take() else {
            return (false, out);
        };
        speech.heard = true;
        speech.release(through, now);
        let res = match self.state {
            State::Revealing {
                seg,
                origin,
                skipped: false,
                ..
            } => {
                self.retime(&speech, seg, origin, now, &mut out);
                (true, out)
            }
            // Waiting at a gate: the next segment picks the words up.
            _ => (false, out),
        };
        self.speech = Some(speech);
        res
    }

    /// Speech is over (or never came): the rest types at its own pace.
    pub fn speech_ended(&mut self, now: u64) -> (bool, Vec<PlayerEffect>) {
        let mut out = Vec::new();
        let Some(mut speech) = self.speech.take() else {
            return (false, out);
        };
        let State::Revealing {
            seg,
            origin,
            skipped: false,
            ..
        } = self.state
        else {
            return (false, out);
        };
        // Resume the line's own rhythm from the first unit still waiting,
        // after what speech released has typed in.
        let s = &self.timeline.segments[seg];
        let start = s.units.start as usize;
        let first = (0..s.reveal_ms.len()).find(|i| speech.release[start + i].is_none());
        if let Some(first) = first {
            let typed = (start..start + first)
                .filter_map(|u| speech.release[u])
                .max()
                .unwrap_or(now)
                .max(now);
            let base = s.reveal_ms[first];
            for i in first..s.reveal_ms.len() {
                let natural = u64::from(s.reveal_ms[i] - base);
                speech.release[start + i] = Some(typed + natural);
            }
        }
        self.retime(&speech, seg, origin, now, &mut out);
        (true, out)
    }

    /// Re-issue segment `seg` (begun at `origin`) on speech's schedule: the
    /// units released so far and their cues; the end once all are out.
    fn retime(
        &mut self,
        speech: &Speech,
        seg: usize,
        origin: u64,
        now: u64,
        out: &mut Vec<PlayerEffect>,
    ) {
        let epoch = self.bump();
        let s = &self.timeline.segments[seg];
        let start = s.units.start as usize;
        // Released units are a prefix: the rest stays hidden.
        let offsets: Vec<u32> = (0..s.reveal_ms.len())
            .map_while(|i| speech.release[start + i])
            .map(|at| at.saturating_sub(origin).min(u64::from(u32::MAX)) as u32)
            .collect();
        let all = offsets.len() == s.reveal_ms.len();
        let last = s.reveal_ms.last().copied().unwrap_or(0);
        let tail = s.duration_ms.saturating_sub(last);
        let end = all.then(|| offsets.last().copied().unwrap_or(0).saturating_add(tail));
        let last_unit_at = all
            .then(|| {
                offsets
                    .iter()
                    .max()
                    .copied()
                    .map(|off| origin.saturating_add(u64::from(off)))
            })
            .flatten();
        self.state = State::Revealing {
            seg,
            origin,
            skipped: false,
            last_unit_at,
        };
        // A cue keeps its place in the text: the unit it sounds with (the
        // first one not before it), or the end.
        let elapsed = now.saturating_sub(origin);
        let cues: Vec<Cue> = s
            .cues
            .iter()
            .filter_map(|c| {
                let unit = match c.kind {
                    CueKind::TypeSound { unit, .. } => Some((unit as usize).saturating_sub(start)),
                    _ => s.reveal_ms.iter().position(|at| *at >= c.at_ms),
                };
                let at = match unit {
                    Some(i) if i < s.reveal_ms.len() => *offsets.get(i)?,
                    _ => end?,
                };
                (u64::from(at) > elapsed || (at == 0 && elapsed == 0)).then(|| Cue {
                    at_ms: at,
                    kind: c.kind.clone(),
                })
            })
            .collect();
        let offsets: Arc<[u32]> = offsets.into();
        out.push(PlayerEffect::ShowSegment {
            epoch,
            seg,
            origin_ms: origin,
            reveal_ms: offsets,
        });
        if !cues.is_empty() {
            out.push(PlayerEffect::ScheduleCues {
                epoch,
                origin_ms: origin,
                cues: cues.into(),
            });
        }
        if let Some(end) = end {
            out.push(PlayerEffect::WakeAt {
                epoch,
                at_ms: origin + u64::from(end),
                reason: WakeReason::SegmentEnd,
            });
        }
    }

    fn finish(&mut self, out: &mut Vec<PlayerEffect>) {
        self.bump();
        self.state = State::Finished;
        out.push(PlayerEffect::LineFinished(self.line));
    }

    /// The continue key. While revealing or settling it completes the segment;
    /// at a gate it starts the next segment; while exiting it completes the exit.
    /// Returns `true` when it did something.
    pub fn advance(&mut self, now: u64) -> (bool, Vec<PlayerEffect>) {
        let mut out = Vec::new();
        match self.state {
            State::Revealing { seg, origin, .. } => {
                self.skip_segment(seg, origin, now, &mut out);
                (true, out)
            }
            State::Settling { seg, .. } => {
                self.skip_segment(seg, now, now, &mut out);
                (true, out)
            }
            State::Gated { seg } => {
                self.advance_from_gate(seg, now, &mut out);
                (true, out)
            }
            State::Exiting { .. } => (true, self.finish_exit(now)),
            _ => (false, out),
        }
    }

    /// Reveal the rest of the current segment at once. Pending sounds and
    /// expressions fire now; typing clicks and shakes are dropped.
    fn skip_segment(&mut self, seg: usize, origin: u64, now: u64, out: &mut Vec<PlayerEffect>) {
        // Skipped by hand: the rest of the line types on its own.
        self.speech = None;
        let epoch = self.bump();
        let segment = &self.timeline.segments[seg];
        let elapsed = now.saturating_sub(origin);
        let pending: Vec<Cue> = segment
            .cues_after(elapsed)
            .filter(|c| c.kind.survives_skip())
            .cloned()
            .collect();
        let zeros = segment.instant();
        self.state = State::Revealing {
            seg,
            origin: now,
            skipped: true,
            last_unit_at: None,
        };
        out.push(PlayerEffect::ShowSegment {
            epoch,
            seg,
            origin_ms: now,
            reveal_ms: zeros,
        });
        if !pending.is_empty() {
            out.push(PlayerEffect::FireNow { cues: pending });
        }
        self.end_segment(seg, now, out);
    }

    /// Jump to the fully revealed end of the line (queue "finish then next").
    /// Sounds and expressions not reached yet, here and in later segments,
    /// fire now.
    pub fn finish_reveal(&mut self, now: u64) -> Vec<PlayerEffect> {
        let mut out = Vec::new();
        self.speech = None;
        let last = self.timeline.last_segment();
        let segments = &self.timeline.segments;
        let (seg, elapsed) = match self.state {
            State::Revealing {
                seg,
                origin,
                skipped: false,
                ..
            } => (seg, now.saturating_sub(origin)),
            State::Revealing { seg, .. } | State::Settling { seg, .. } | State::Gated { seg } => {
                (seg, u64::MAX)
            }
            _ => (last, u64::MAX),
        };
        let pending: Vec<Cue> = segments[seg]
            .cues_after(elapsed)
            .chain(segments[seg + 1..].iter().flat_map(|s| s.cues.iter()))
            .filter(|c| c.kind.survives_skip())
            .cloned()
            .collect();
        let epoch = self.bump();
        out.push(PlayerEffect::ShowSegment {
            epoch,
            seg: last,
            origin_ms: now,
            reveal_ms: self.timeline.segments[last].instant(),
        });
        if !pending.is_empty() {
            out.push(PlayerEffect::FireNow { cues: pending });
        }
        self.skipped_to_end = true;
        self.enter_done(now, &mut out);
        out
    }

    /// The box is full at each of `breaks` (units of the line, measured on
    /// the page now on screen): turn a page there. Breaks outside the page
    /// being typed are ignored. Returns whether the current segment's
    /// schedule was replaced (its pending cues must be cancelled first).
    pub fn paginate(
        &mut self,
        breaks: &[u32],
        auto_ms: Option<u32>,
        now: u64,
    ) -> (bool, Vec<PlayerEffect>) {
        let mut out = Vec::new();
        let segments = &self.timeline.segments;
        // Breaks may land from the start of the segment on (from the page's
        // start for a line already done).
        let (seg, from) = match self.state {
            State::Revealing { seg, .. } | State::Settling { seg, .. } | State::Gated { seg } => {
                (seg, segments[seg].units.start)
            }
            State::Done { .. } if !self.skipped_to_end => {
                let seg = self.timeline.last_segment();
                (seg, segments[seg].visible_from)
            }
            _ => return (false, out),
        };
        let page = segments[seg].visible_from;
        if self.paged.contains(&page) {
            return (false, out);
        }
        let page_end = segments[seg..]
            .iter()
            .find(|s| s.visible_from != page)
            .map_or(self.timeline.unit_count, |s| s.units.start);
        let mut breaks: Vec<u32> = breaks
            .iter()
            .copied()
            .filter(|b| *b > from && *b < page_end)
            .collect();
        breaks.sort_unstable();
        breaks.dedup();
        let tl = Arc::make_mut(&mut self.timeline);
        let mut changed = false;
        for b in &breaks {
            changed |= tl.split_page(*b, auto_ms);
        }
        self.paged.extend(&breaks);
        if !changed {
            return (false, out);
        }
        match self.state {
            State::Revealing {
                seg,
                origin,
                skipped,
                ..
            } => self.repage_current(seg, origin, skipped, now, out),
            State::Settling { seg, .. } => self.repage_current(seg, 0, false, now, out),
            State::Gated { seg } => {
                // A pause that became a page may now turn on its own.
                self.gate(seg, now, &mut out);
                (false, out)
            }
            State::Done { .. } => self.back_to_first_page(breaks[0], now, out),
            _ => (false, out),
        }
    }

    /// The segment being typed was cut short by a page break.
    fn repage_current(
        &mut self,
        seg: usize,
        origin: u64,
        skipped: bool,
        now: u64,
        mut out: Vec<PlayerEffect>,
    ) -> (bool, Vec<PlayerEffect>) {
        if !skipped && let Some(speech) = self.speech.take() {
            // The cut segment keeps following speech.
            self.retime(&speech, seg, origin, now, &mut out);
            self.speech = Some(speech);
            return (true, out);
        }
        let elapsed = now.saturating_sub(origin);
        let epoch = self.bump();
        let s = &self.timeline.segments[seg];
        let cut = u64::from(s.duration_ms);
        let last_unit_at = s
            .reveal_ms
            .iter()
            .max()
            .copied()
            .map(|off| origin.saturating_add(u64::from(off)));
        out.push(PlayerEffect::ShowSegment {
            epoch,
            seg,
            origin_ms: origin,
            reveal_ms: if skipped {
                s.instant()
            } else {
                s.reveal_ms.clone()
            },
        });
        if !skipped && elapsed < cut {
            self.state = State::Revealing {
                seg,
                origin,
                skipped: false,
                last_unit_at,
            };
            let cues: Vec<Cue> = s.cues_after(elapsed).cloned().collect();
            if !cues.is_empty() {
                out.push(PlayerEffect::ScheduleCues {
                    epoch,
                    origin_ms: origin,
                    cues: cues.into(),
                });
            }
            out.push(PlayerEffect::WakeAt {
                epoch,
                at_ms: origin + cut,
                reason: WakeReason::SegmentEnd,
            });
            return (true, out);
        }
        // Typed (or skipped) past the cut already: the next page must not
        // repeat what already went out.
        let fired = elapsed.saturating_sub(cut);
        let next = &mut Arc::make_mut(&mut self.timeline).segments[seg + 1];
        let cues: Vec<Cue> = next
            .cues
            .iter()
            .filter(|c| {
                matches!(c.kind, CueKind::SpeakingStart | CueKind::SpeakingStop)
                    || if skipped {
                        !c.kind.survives_skip()
                    } else {
                        u64::from(c.at_ms) > fired
                    }
            })
            .cloned()
            .collect();
        next.cues = cues.into();
        if !skipped {
            self.settle_or_end(seg, last_unit_at, now, &mut out);
        } else {
            self.end_segment(seg, now, &mut out);
        }
        (true, out)
    }

    /// The line was shown all at once before it was measured: go back to its
    /// first page. Its markers all fired when the line started.
    fn back_to_first_page(
        &mut self,
        first: u32,
        now: u64,
        mut out: Vec<PlayerEffect>,
    ) -> (bool, Vec<PlayerEffect>) {
        let Some(a) = self
            .timeline
            .segments
            .iter()
            .position(|s| s.units.end == first)
        else {
            return (false, out);
        };
        for s in Arc::make_mut(&mut self.timeline).segments[a + 1..].iter_mut() {
            let typing: Vec<Cue> = s
                .cues
                .iter()
                .filter(|c| !c.kind.survives_skip())
                .cloned()
                .collect();
            s.cues = typing.into();
        }
        out.push(PlayerEffect::ShowSegment {
            epoch: self.epoch,
            seg: a,
            origin_ms: now,
            reveal_ms: self.timeline.segments[a].instant(),
        });
        self.gate(a, now, &mut out);
        (true, out)
    }

    pub fn snapshot(&self, now: u64) -> RevealSnapshot {
        let tl = &self.timeline;
        let (seg, revealed) = match self.state {
            State::Revealing {
                seg,
                origin,
                skipped,
                ..
            } => {
                let s = &tl.segments[seg];
                let shown = if skipped {
                    s.reveal_ms.len()
                } else if let Some(speech) = &self.speech {
                    (0..s.reveal_ms.len())
                        .map_while(|i| speech.release[s.units.start as usize + i])
                        .filter(|&at| at <= now)
                        .count()
                } else {
                    let elapsed = now.saturating_sub(origin);
                    s.reveal_ms
                        .iter()
                        .filter(|t| u64::from(**t) <= elapsed)
                        .count()
                };
                (seg, s.units.start + shown as u32)
            }
            State::Settling { seg, .. } | State::Gated { seg } => (seg, tl.segments[seg].units.end),
            State::Exiting {
                next: ExitAction::PageNext(next_seg),
                ..
            } => {
                let seg = next_seg.saturating_sub(1);
                (seg, tl.segments[seg].units.end)
            }
            _ => (tl.last_segment(), tl.unit_count),
        };
        let opacity = match self.state {
            State::Exiting {
                at,
                style: ExitStyle::Fade { ms },
                ..
            } => 1.0 - (now.saturating_sub(at) as f32 / ms.max(1) as f32).clamp(0.0, 1.0),
            State::Finished => 0.0,
            _ => 1.0,
        };
        RevealSnapshot {
            phase: self.phase(),
            visible_from: tl.segments[seg].visible_from,
            revealed,
            opacity,
        }
    }
}
