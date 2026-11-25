//! Per-track dialogue presence state machine and frame-demand scheduling.
//!
//! Replaces the process-global `Tracks` mutex array and `Box::leak` test TLS
//! with an owned [`DisplayTrackState`] handle on each [`Stage`] / [`PeerLane`],
//! keeping reactive view getters (`drawn`, `drawn_untracked`) 100% pure.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use nana_ui::runtime::view::{Signal, signal};
use nanatype_core::DisplayView;
use nanatype_model::{EntranceKind, LineId};

use super::super::{SceneFrameDemand, Stage};
use super::chrome::Source;

/// The box fades in and rises into place when a line appears on an empty
/// box, and fades out when it clears. No overshoot: a box that bounces
/// reads as jitter on stream.
const ENTER_MS: u64 = 180;
const EXIT_MS: u64 = 160;
const ENTER_RISE: f32 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DisplayPresence {
    #[default]
    Hidden,
    Entering {
        at_ms: u64,
    },
    Shown,
    Leaving {
        at_ms: u64,
    },
}

#[derive(Clone, Debug, PartialEq)]
struct TrackSnapshot {
    shown: Arc<DisplayView>,
    empty: bool,
    presence: DisplayPresence,
    observed: bool,
    moving_until: u64,
}

impl TrackSnapshot {
    fn new(initial: Arc<DisplayView>) -> Self {
        let empty = initial.empty;
        Self {
            shown: initial,
            empty,
            presence: DisplayPresence::Hidden,
            observed: false,
            moving_until: 0,
        }
    }

    fn see(&mut self, view: Arc<DisplayView>, now: u64) {
        let first = !std::mem::replace(&mut self.observed, true);
        if !view.empty {
            if !first && (self.empty || matches!(self.presence, DisplayPresence::Leaving { .. })) {
                self.presence = DisplayPresence::Entering { at_ms: now };
                self.moving_until = now.saturating_add(ENTER_MS);
            } else if first || matches!(self.presence, DisplayPresence::Hidden) {
                self.presence = DisplayPresence::Shown;
            }
            self.empty = false;
            self.shown = view;
            return;
        }
        if !self.empty {
            if !matches!(self.presence, DisplayPresence::Leaving { .. }) {
                self.presence = DisplayPresence::Leaving { at_ms: now };
                self.moving_until = now.saturating_add(EXIT_MS);
            }
            // Keep `self.shown` so text, name and portrait fade out together.
        } else {
            self.presence = DisplayPresence::Hidden;
            self.shown = view;
        }
    }

    fn presence(&self, now: u64, reduced: bool) -> (f32, f32) {
        match self.presence {
            DisplayPresence::Hidden => (1.0, 0.0),
            DisplayPresence::Shown => (1.0, 0.0),
            DisplayPresence::Leaving { at_ms } => {
                let t = (now.saturating_sub(at_ms) as f32 / EXIT_MS as f32).clamp(0.0, 1.0);
                (1.0 - t * t * t, 0.0)
            }
            DisplayPresence::Entering { at_ms } => {
                let t = (now.saturating_sub(at_ms) as f32 / ENTER_MS as f32).clamp(0.0, 1.0);
                let e = 1.0 - (1.0 - t).powi(3);
                (e, if reduced { 0.0 } else { ENTER_RISE * (1.0 - e) })
            }
        }
    }
}

/// Owned reactive presence/exit state for one dialogue track (local or peer).
#[derive(Clone, Copy)]
pub struct DisplayTrackState {
    state: Signal<TrackSnapshot>,
}

impl DisplayTrackState {
    pub fn new(initial: Arc<DisplayView>) -> Self {
        Self {
            state: signal(TrackSnapshot::new(initial)),
        }
    }

    /// Seed state for a fresh `dialogue_box` mount: a box mounted with a line
    /// already present starts opaque at once.
    pub(super) fn reset(&self, initial: Arc<DisplayView>) {
        let empty = initial.empty;
        self.state.set(TrackSnapshot {
            shown: initial,
            empty,
            presence: if empty {
                DisplayPresence::Hidden
            } else {
                DisplayPresence::Shown
            },
            observed: true,
            moving_until: 0,
        });
    }

    /// Advance the presence state machine for a new `view` at `now`.
    pub fn see(&self, view: Arc<DisplayView>, now: u64) {
        self.state.update(|s| s.see(view, now));
    }

    /// Pure tracked read of the view currently drawn (keeping the leaving view
    /// until its exit completes).
    pub fn drawn(&self) -> Arc<DisplayView> {
        self.state.with(|s| s.shown.clone())
    }

    /// Pure untracked read of the view currently drawn.
    pub fn drawn_untracked(&self) -> Arc<DisplayView> {
        self.state.with_untracked(|s| s.shown.clone())
    }

    /// Pure tracked read of the box's presence opacity and entrance rise.
    pub fn presence(&self, now: u64, reduced: bool) -> (f32, f32) {
        self.state.with(|s| s.presence(now, reduced))
    }

    /// Pure untracked read of the box's transition deadline (`moving_until`).
    pub fn moving_until(&self) -> u64 {
        self.state.with_untracked(|s| s.moving_until)
    }

    fn needs_sync(&self, view: &DisplayView) -> bool {
        self.state
            .with_untracked(|s| s.observed && s.empty != view.empty)
    }
}

/// The typewriter's view as drawn, pure tracked read.
pub(super) fn drawn(ui: Stage) -> Arc<DisplayView> {
    drawn_for(ui, Source::Display)
}

pub(super) fn drawn_for(ui: Stage, source: Source) -> Arc<DisplayView> {
    match source {
        Source::Preview => ui.preview_display.get(),
        Source::Display | Source::Draft => ui.track.drawn(),
    }
}

pub(super) fn drawn_untracked_for(ui: Stage, source: Source) -> Arc<DisplayView> {
    match source {
        Source::Preview => ui.preview_display.get_untracked(),
        Source::Display | Source::Draft => ui.track.drawn_untracked(),
    }
}

/// The display box's opacity and entrance rise at `now`: hidden while empty,
/// then its entrance, exit and `[fade]`.
pub(super) fn box_presence(ui: Stage, view: &DisplayView, now: u64, reduced: bool) -> (f32, f32) {
    let fade = view
        .exit
        .and_then(|e| e.fade())
        .map(|(at, ms)| 1.0 - (now.saturating_sub(at) as f32 / ms.max(1) as f32).clamp(0.0, 1.0))
        .unwrap_or(1.0);
    let (shown, rise) = ui.track.presence(now, reduced);
    (if view.empty { 0.0 } else { fade * shown }, rise)
}

/// One id per preview pass, so shader line-age restarts with the sample.
pub(crate) fn fresh_preview_line() -> LineId {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    LineId(NEXT.fetch_add(1, Ordering::Relaxed))
}

/// Whether the display still needs frames for a glyph reveal's compositor
/// transition. Pacing itself is owned by Core, whose last reveal wake is when
/// the final glyph *starts* to enter; this keeps frames until it has entered.
/// Scramble requests its own frames (`text_scramble::animating`).
pub fn display_compositor_needs_tick(ui: Stage, now: u64) -> bool {
    let view = ui.display.get_untracked();
    if view.empty
        || ui.reduced_motion.get_untracked()
        || view.entrance.kind == EntranceKind::None
        || view.entrance.ms == 0
    {
        return false;
    }
    let has_scramble = ui
        .skin
        .with_untracked(|skin| skin.dialogue.reveal.scramble.is_some());
    let scramble_disabled = has_scramble && !super::super::materials::enabled(ui);
    if scramble_disabled {
        return false;
    }
    view.reveal
        .settles_at(u64::from(view.entrance.ms))
        .is_some_and(|end| now < end)
}

/// Whether the display needs frames for the box itself (fade, shake, the
/// continue mark). Text reveal and effects request their own frames.
pub fn display_animating(ui: Stage, now: u64) -> bool {
    let moving_until = ui.track.moving_until();
    if moving_until > 0 && now <= moving_until.saturating_add(50) {
        return true;
    }
    let view = ui.display.get_untracked();
    if ui.track.needs_sync(&view) {
        return true;
    }
    if view.empty {
        return false;
    }
    if let Some((at_ms, ms)) = view.exit.and_then(|e| e.presentation())
        && now < at_ms + u64::from(ms)
    {
        return true;
    }
    if let Some((at, ms)) = view.exit.and_then(|e| e.fade())
        && now <= at + u64::from(ms) + 50
    {
        return true;
    }
    if let Some(sh) = view.shake
        && now <= sh.at_ms + u64::from(sh.ms) + 50
    {
        return true;
    }
    let moves = ui.skin.with_untracked(|s| {
        let c = &s.dialogue.continue_;
        c.image.is_some() && c.frames > 1
    });
    view.continue_indicator && moves && !ui.reduced_motion.get_untracked()
}

/// Complete frame demand for a dialogue track (`ui` or `ui.lane(seat)`),
/// unifying box transitions, glyph reveal compositor ticks, shaders, sweep,
/// text light, and text scramble for the host.
pub fn frame_demand(ui: Stage, now: u64) -> SceneFrameDemand {
    let tick_clock = display_animating(ui, now) || display_compositor_needs_tick(ui, now);
    let shaders = super::super::shader_effects::wake(ui);
    let render_only = shaders == super::super::shader_effects::Wake::Continuous
        || super::super::sweep::animating(ui)
        || super::super::text_light::animating(ui, now)
        || super::super::text_scramble::animating(ui, now);
    let next_at = match shaders {
        super::super::shader_effects::Wake::At(at) => Some(at),
        _ => None,
    };
    SceneFrameDemand {
        tick_clock,
        render_only,
        next_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nanatype_model::Entrance;

    #[test]
    fn a_populated_initial_mount_is_opaque_even_at_clock_zero() {
        let mut stage = TrackSnapshot::new(Arc::new(DisplayView::empty(Entrance {
            kind: EntranceKind::None,
            ms: 0,
        })));
        let mut view = DisplayView::empty(Entrance {
            kind: EntranceKind::None,
            ms: 0,
        });
        view.empty = false;
        stage.see(Arc::new(view), 0);
        assert_eq!(stage.presence(0, false), (1.0, 0.0));
        assert_eq!(stage.presence(48, false), (1.0, 0.0));
    }
}
