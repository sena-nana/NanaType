//! Motion tokens: the four durations the app's chrome moves on, and the
//! transitions built from them.
//!
//! - `hover` 120ms: a control's hover colour (the theme's hover cross-fade).
//! - `state` 160ms: a badge or chip changing state, a toast leaving.
//! - `enter` 200ms: something arriving — opacity 0 → 1 while it rises 8px
//!   into place on a small settle (`cubic-bezier(.34, 1.4, .64, 1)`).
//! - `press` 80ms: a call to action dips to 96% and back.
//!
//! Reduced motion (the setting or the system's) lands every view-layer
//! transition at once: the host hands the effective preference to each
//! window's runtime, and [`press`] checks it itself. The dialogue box does
//! not use these: it keeps its own eased fade, with no settle.

use std::time::Duration;

use nana_ui::runtime::view::{Implicit, Presence, Transition};
use nana_ui::runtime::{
    AnimatableProperty, AnimationDirection, AnimationFillMode, AnimationIteration,
    AnimationPlayback, Easing, Keyframe, MotionCurve, MotionGraph, MotionTargetId, MotionTiming,
    MotionTo, MotionTrack, MotionTrackId, MotionValue, PaintTransform, View, ViewContext,
};
use nana_ui_core::{DialogTransition, ToastMotion};

/// A control's hover colour.
pub const HOVER_MS: u64 = 120;
/// A state change: a chip, a toast going. (Link badges change in place: a
/// switching block would widen them, see `settings::controls`.)
pub const STATE_MS: u64 = 160;
/// An arrival: a step, a chip, a toast, the summoned input window.
pub const ENTER_MS: u64 = 200;
/// Each half of a press: down to [`PRESS_SCALE`], and back.
pub const PRESS_MS: u64 = 80;
/// How far below its place an arriving view starts (px).
pub const ENTER_RISE: f32 = 8.0;
/// How far a pressed call to action dips.
pub const PRESS_SCALE: f32 = 0.96;

/// The settle an arrival's move lands on: a little past its place and back.
/// Only for moves; opacity takes [`FADE`].
pub const SETTLE: Easing = Easing::CubicBezier([0.34, 1.4, 0.64, 1.0]);
/// Fades and colour changes.
pub const FADE: Easing = Easing::EaseOutCubic;

const fn ms(v: u64) -> Duration {
    Duration::from_millis(v)
}

fn scale(s: f32) -> PaintTransform {
    PaintTransform {
        a: s,
        b: 0.0,
        c: 0.0,
        d: s,
        e: 0.0,
        f: 0.0,
    }
}

fn rise(dy: f32) -> PaintTransform {
    PaintTransform {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: dy,
    }
}

/// `enter` for a branch or row that replaces another: it fades in and rises
/// into place; what it replaces goes at once, so the two never overlap.
pub fn enter() -> Transition {
    Transition::new().enter(
        Presence::new(ms(ENTER_MS))
            .opacity(0.0)
            .translate(0.0, ENTER_RISE)
            .ease(FADE)
            .ease_transform(SETTLE),
    )
}

/// `enter` as implicit transitions, for a node that stays mounted and is
/// shown by binding its opacity to 1 and its transform to [`rest`] (the
/// summoned input window).
pub fn enter_implicit() -> [Implicit; 2] {
    [
        Implicit::new(AnimatableProperty::Opacity, ms(ENTER_MS)).ease(FADE),
        Implicit::new(AnimatableProperty::Transform, ms(ENTER_MS)).ease(SETTLE),
    ]
}

/// Where a node shown with [`enter_implicit`] rests (`shown`) or waits.
pub fn rest(shown: bool) -> (f32, PaintTransform) {
    if shown {
        (1.0, rise(0.0))
    } else {
        (0.0, rise(ENTER_RISE))
    }
}

/// Toasts arrive on `enter` and leave on `state`.
pub fn toast() -> ToastMotion {
    ToastMotion {
        enter: DialogTransition::new(ENTER_MS as u16, SETTLE),
        exit: DialogTransition::new(STATE_MS as u16, FADE),
        enter_offset_y: ENTER_RISE,
        enter_scale: 1.0,
        ..ToastMotion::DEFAULT
    }
}

/// `press` on the node handling an activation: a dip to [`PRESS_SCALE`]
/// about its centre and back, [`PRESS_MS`] each way. Nothing under reduced
/// motion.
pub fn press<V: View>(cx: &mut ViewContext<'_, V>, reduced: bool) {
    if reduced {
        return;
    }
    let (Some(id), Some(target)) = (MotionTrackId::new(1), MotionTargetId::new(1)) else {
        return;
    };
    let from = MotionValue::Transform(scale(1.0));
    let track = MotionTrack {
        id,
        // The timeline builder points it at the handling node.
        target,
        property: AnimatableProperty::Transform,
        from,
        to: MotionTo::Keyframes(vec![
            Keyframe {
                offset: 0.5,
                value: MotionValue::Transform(scale(PRESS_SCALE)),
                easing: Some(FADE),
            },
            Keyframe {
                offset: 1.0,
                value: from,
                easing: Some(FADE),
            },
        ]),
        timing: MotionTiming::new(Duration::ZERO, ms(PRESS_MS * 2), ms(16)),
        curve: MotionCurve::Easing(Easing::Linear),
        playback: AnimationPlayback::running(
            AnimationIteration::ONCE,
            AnimationDirection::Normal,
            AnimationFillMode::None,
        ),
        velocity: from.zero_velocity(),
    };
    cx.node().timeline(MotionGraph::track(track)).start();
}

/// Retained samples of a periodic curve. Only the compositor clock advances
/// after mounting; samples never write back into a widget's logical style.
/// 64 linear segments keep sine error below 0.13% of its amplitude.
pub fn periodic(
    node: nana_ui::runtime::view::NodeRef,
    period_s: f32,
    sample: impl Fn(f64) -> (PaintTransform, f32) + 'static,
) {
    nana_ui::runtime::view::on_mount(move |cx| {
        let Some(node) = node.get_untracked() else {
            return;
        };
        let period = f64::from(period_s.max(0.1));
        let origin = crate::clock::now_ms() as f64 / 1000.0;
        let values: Vec<_> = (0..=64)
            .map(|i| sample(origin + period * i as f64 / 64.0))
            .collect();
        let mut tracks = Vec::new();
        for (number, property) in [
            (1, AnimatableProperty::Transform),
            (2, AnimatableProperty::Opacity),
        ] {
            let get = |i: usize| {
                if number == 1 {
                    MotionValue::Transform(values[i].0)
                } else {
                    MotionValue::Scalar(values[i].1)
                }
            };
            let from = get(0);
            if (1..=64).all(|i| get(i) == from) {
                continue;
            }
            tracks.push(MotionGraph::track(MotionTrack {
                id: MotionTrackId::new(number).unwrap(),
                target: MotionTargetId::new(1).unwrap(),
                property,
                from,
                to: MotionTo::Keyframes(
                    (1..=64)
                        .map(|i| Keyframe {
                            offset: i as f32 / 64.0,
                            value: get(i),
                            easing: None,
                        })
                        .collect(),
                ),
                timing: MotionTiming::new(Duration::ZERO, Duration::from_secs_f64(period), ms(16)),
                curve: MotionCurve::Easing(Easing::Linear),
                playback: AnimationPlayback::running(
                    AnimationIteration::INFINITE,
                    AnimationDirection::Normal,
                    AnimationFillMode::None,
                ),
                velocity: from.zero_velocity(),
            }));
        }
        let now = cx.animation_now();
        let mut queue = nana_ui::runtime::MutationQueue::new();
        queue
            .node(node, now)
            .timeline(MotionGraph::parallel(tracks))
            .start();
        let _ = cx.commit_mutations(queue);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    // Checked at compile time: the tokens are constants.
    const _: () = assert!(PRESS_MS < HOVER_MS && HOVER_MS < STATE_MS && STATE_MS < ENTER_MS);

    #[test]
    fn the_settle_overshoots_and_lands() {
        let peak = (1..100)
            .map(|i| SETTLE.sample(i as f32 / 100.0))
            .fold(0.0_f32, f32::max);
        assert!(peak > 1.0, "a settle passes its place: {peak}");
        assert!((SETTLE.sample(1.0) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn rest_is_in_place_when_shown() {
        let (o, t) = rest(true);
        assert_eq!((o, t.f), (1.0, 0.0));
        let (o, t) = rest(false);
        assert_eq!((o, t.f), (0.0, ENTER_RISE));
    }
}
