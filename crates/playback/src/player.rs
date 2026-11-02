use std::sync::Arc;

use nanatype_model::{AfterLine, LineId};

use crate::{Cue, CueKind, SegmentEnd, Timeline};

/// Length of the fade-out once a fading line's hold time is over.
pub const FADE_OUT_MS: u32 = 450;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeReason {
    SegmentEnd,
    AutoContinue,
    AfterLine,
    FadeDone,
    AutoAdvance,
    /// Consumer-owned presentation deadline; the player does not schedule it.
    PresentationDone,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Revealing,
    /// Waiting at a pause or page marker for the continue key.
    Gated,
    /// Fully revealed; holding or counting down to a fade.
    Done,
    Fading,
    Finished,
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
    /// Start the fade-out animation.
    FadeOut {
        at_ms: u64,
        ms: u32,
    },
    /// Every unit of the line is visible.
    LineRevealed(LineId),
    /// The line left the box (fade or clear finished).
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

#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    Revealing {
        seg: usize,
        origin: u64,
        skipped: bool,
    },
    Gated {
        seg: usize,
    },
    Done {
        at: u64,
    },
    Fading {
        at: u64,
        ms: u32,
    },
    Finished,
}

pub struct Player {
    line: LineId,
    timeline: Arc<Timeline>,
    after: AfterLine,
    auto_advance_ms: Option<u32>,
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
    /// The current segment's reveal offsets while speech paces it.
    paced: Option<Arc<[u32]>>,
}

impl Player {
    pub fn start(
        line: LineId,
        timeline: Arc<Timeline>,
        after: AfterLine,
        auto_advance_ms: Option<u32>,
        now: u64,
    ) -> (Self, Vec<PlayerEffect>) {
        let mut player = Self {
            line,
            timeline,
            after,
            auto_advance_ms,
            state: State::Finished,
            epoch: 0,
            revealed: false,
            skipped_to_end: false,
            paged: Vec::new(),
            speech: None,
            paced: None,
        };
        let mut out = Vec::new();
        player.enter_segment(0, now, &mut out);
        (player, out)
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
            State::Gated { .. } => Phase::Gated,
            State::Done { .. } => Phase::Done,
            State::Fading { .. } => Phase::Fading,
            State::Finished => Phase::Finished,
        }
    }

    /// Current segment index (the last one once done).
    pub fn segment(&self) -> usize {
        match self.state {
            State::Revealing { seg, .. } | State::Gated { seg } => seg,
            _ => self.timeline.last_segment(),
        }
    }

    fn bump(&mut self) -> u64 {
        self.epoch += 1;
        self.epoch
    }

    fn enter_segment(&mut self, seg: usize, now: u64, out: &mut Vec<PlayerEffect>) {
        self.paced = None;
        if let Some(speech) = &mut self.speech {
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
            };
            out.push(PlayerEffect::ContinueIndicator(false));
            self.retime(seg, now, now, out);
            return;
        }
        let epoch = self.bump();
        let segment = &self.timeline.segments[seg];
        self.state = State::Revealing {
            seg,
            origin: now,
            skipped: false,
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
        match self.after {
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
        if let Some(ms) = self.auto_advance_ms {
            out.push(PlayerEffect::WakeAt {
                epoch,
                at_ms: now + u64::from(ms),
                reason: WakeReason::AutoAdvance,
            });
        }
    }

    /// A scheduled wake arrived. Stale epochs are ignored.
    pub fn on_wake(&mut self, epoch: u64, reason: WakeReason, now: u64) -> Vec<PlayerEffect> {
        let mut out = Vec::new();
        if epoch != self.epoch {
            return out;
        }
        match (self.state, reason) {
            (State::Revealing { seg, .. }, WakeReason::SegmentEnd) => {
                self.end_segment(seg, now, &mut out)
            }
            (State::Gated { seg }, WakeReason::AutoContinue) => {
                self.enter_segment(seg + 1, now, &mut out)
            }
            (State::Done { .. }, WakeReason::AfterLine) => match self.after {
                AfterLine::Fade { .. } => {
                    let epoch = self.bump();
                    self.state = State::Fading {
                        at: now,
                        ms: FADE_OUT_MS,
                    };
                    out.push(PlayerEffect::FadeOut {
                        at_ms: now,
                        ms: FADE_OUT_MS,
                    });
                    out.push(PlayerEffect::WakeAt {
                        epoch,
                        at_ms: now + u64::from(FADE_OUT_MS),
                        reason: WakeReason::FadeDone,
                    });
                }
                _ => self.finish(&mut out),
            },
            (State::Done { .. }, WakeReason::AutoAdvance) => {
                out.push(PlayerEffect::AutoAdvance(self.line))
            }
            (State::Fading { .. }, WakeReason::FadeDone) => self.finish(&mut out),
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
        self.speech = Some(Speech {
            release,
            heard: false,
        });
        self.retime(seg, origin, now, &mut out);
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
        let Some(speech) = &mut self.speech else {
            return (false, out);
        };
        speech.heard = true;
        speech.release(through, now);
        match self.state {
            State::Revealing {
                seg,
                origin,
                skipped: false,
            } => {
                self.retime(seg, origin, now, &mut out);
                (true, out)
            }
            // Waiting at a gate: the next segment picks the words up.
            _ => (false, out),
        }
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
        } = self.state
        else {
            self.paced = None;
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
        self.speech = Some(speech);
        self.retime(seg, origin, now, &mut out);
        self.speech = None;
        (true, out)
    }

    /// Re-issue segment `seg` (begun at `origin`) on speech's schedule: the
    /// units released so far and their cues; the end once all are out.
    fn retime(&mut self, seg: usize, origin: u64, now: u64, out: &mut Vec<PlayerEffect>) {
        let epoch = self.bump();
        let Some(speech) = &self.speech else {
            return;
        };
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
        self.paced = Some(offsets.clone());
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

    /// The continue key. While revealing it completes the segment; at a gate
    /// it starts the next segment. Returns `true` when it did something.
    pub fn advance(&mut self, now: u64) -> (bool, Vec<PlayerEffect>) {
        let mut out = Vec::new();
        match self.state {
            State::Revealing { seg, origin, .. } => {
                self.skip_segment(seg, origin, now, &mut out);
                (true, out)
            }
            State::Gated { seg } => {
                self.enter_segment(seg + 1, now, &mut out);
                (true, out)
            }
            _ => (false, out),
        }
    }

    /// Reveal the rest of the current segment at once. Pending sounds and
    /// expressions fire now; typing clicks and shakes are dropped.
    fn skip_segment(&mut self, seg: usize, origin: u64, now: u64, out: &mut Vec<PlayerEffect>) {
        // Skipped by hand: the rest of the line types on its own.
        self.speech = None;
        self.paced = None;
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
        self.paced = None;
        let last = self.timeline.last_segment();
        let segments = &self.timeline.segments;
        let (seg, elapsed) = match self.state {
            State::Revealing {
                seg,
                origin,
                skipped: false,
            } => (seg, now.saturating_sub(origin)),
            State::Revealing { seg, .. } | State::Gated { seg } => (seg, u64::MAX),
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
            State::Revealing { seg, .. } | State::Gated { seg } => (seg, segments[seg].units.start),
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
            } => self.repage_current(seg, origin, skipped, now, out),
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
        if self.speech.is_some() && !skipped {
            // The cut segment keeps following speech.
            self.retime(seg, origin, now, &mut out);
            return (true, out);
        }
        let elapsed = now.saturating_sub(origin);
        let epoch = self.bump();
        let s = &self.timeline.segments[seg];
        let cut = u64::from(s.duration_ms);
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
        self.end_segment(seg, now, &mut out);
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
            } => {
                let s = &tl.segments[seg];
                let elapsed = now.saturating_sub(origin);
                let reveal = self.paced.as_ref().unwrap_or(&s.reveal_ms);
                let shown = if skipped {
                    s.reveal_ms.len()
                } else {
                    reveal.iter().filter(|t| u64::from(**t) <= elapsed).count()
                };
                (seg, s.units.start + shown as u32)
            }
            State::Gated { seg } => (seg, tl.segments[seg].units.end),
            _ => (tl.last_segment(), tl.unit_count),
        };
        let opacity = match self.state {
            State::Fading { at, ms } => {
                1.0 - (now.saturating_sub(at) as f32 / ms.max(1) as f32).clamp(0.0, 1.0)
            }
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
