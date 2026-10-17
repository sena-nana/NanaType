//! Default native move and scale gestures for unlocked broadcast windows.
use nana_ui_platform::{InputPayload, PointerPhase, WheelUnit, host::WindowCommand};
use nanatype_settings::{Lock, Placement};

use crate::windows::{OVERLAY_MIN, WindowKind};

fn interactive(kind: WindowKind, lock: Lock, handled: bool) -> bool {
    lock != Lock::Locked
        && !handled
        && matches!(
            kind,
            WindowKind::Display | WindowKind::PeerDisplay | WindowKind::History
        )
}

/// Only unhandled right-button releases open a menu; a press must never drag.
pub(super) fn menu_position(
    kind: WindowKind,
    lock: Lock,
    payload: &InputPayload,
    handled: bool,
) -> Option<(f32, f32)> {
    if !interactive(kind, lock, handled) {
        return None;
    }
    match payload {
        InputPayload::Pointer(p)
            if p.phase == PointerPhase::Up
                && p.button == 2
                && p.x.is_finite()
                && p.y.is_finite() =>
        {
            Some((p.x, p.y))
        }
        _ => None,
    }
}

pub(super) fn command(
    kind: WindowKind,
    lock: Lock,
    payload: &InputPayload,
    handled: bool,
    frame: Option<Placement>,
) -> Option<WindowCommand> {
    if !interactive(kind, lock, handled) {
        return None;
    }
    match payload {
        InputPayload::Pointer(p) if p.phase == PointerPhase::Down && p.button == 0 => {
            Some(WindowCommand::Drag(kind.id()))
        }
        InputPayload::Wheel(wheel) => {
            let frame = frame?;
            if ![
                frame.x,
                frame.y,
                wheel.x.into(),
                wheel.y.into(),
                wheel.delta_y.into(),
            ]
            .into_iter()
            .all(f64::is_finite)
                || !(OVERLAY_MIN.0..=8192.0).contains(&frame.w)
                || !(OVERLAY_MIN.1..=8192.0).contains(&frame.h)
                || wheel.delta_y == 0.0
            {
                return None;
            }
            // One wheel notch grows by 10%; trackpads retain fractional motion.
            let steps = f64::from(wheel.delta_y)
                / match wheel.unit {
                    WheelUnit::Lines => 1.0,
                    WheelUnit::Pixels => 40.0,
                };
            let min = (OVERLAY_MIN.0 / frame.w).max(OVERLAY_MIN.1 / frame.h);
            let max = (8192.0 / frame.w).min(8192.0 / frame.h);
            let scale = 1.1_f64.powf(steps.clamp(-20.0, 20.0)).clamp(min, max);
            if (scale - 1.0).abs() < f64::EPSILON {
                return None;
            }
            // Keep the point under the pointer in the same desktop position.
            Some(WindowCommand::SetBounds {
                id: kind.id(),
                position: (
                    (frame.x + f64::from(wheel.x).clamp(0.0, frame.w) * (1.0 - scale)) as f32,
                    (frame.y + f64::from(wheel.y).clamp(0.0, frame.h) * (1.0 - scale)) as f32,
                ),
                size: ((frame.w * scale) as f32, (frame.h * scale) as f32),
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nana_ui_platform::{PointerId, PointerInput, WheelInput};

    const FRAME: Placement = Placement {
        x: 100.0,
        y: 200.0,
        w: 800.0,
        h: 400.0,
    };

    fn wheel(delta_y: f32, unit: WheelUnit) -> InputPayload {
        InputPayload::Wheel(WheelInput {
            pointer_id: PointerId(1),
            x: 400.0,
            y: 200.0,
            delta_x: 0.0,
            delta_y,
            unit,
            modifiers: Default::default(),
        })
    }

    #[test]
    fn unlocked_overlays_accept_default_gestures() {
        let press = InputPayload::Pointer(PointerInput::mouse(PointerPhase::Down, 10.0, 10.0));
        let wheel = wheel(1.0, WheelUnit::Lines);
        for kind in WindowKind::ALL {
            for lock in [Lock::Locked, Lock::Interactive] {
                for payload in [&press, &wheel] {
                    let allowed = lock != Lock::Locked
                        && matches!(
                            kind,
                            WindowKind::Display | WindowKind::PeerDisplay | WindowKind::History
                        );
                    assert_eq!(
                        command(kind, lock, payload, false, Some(FRAME)).is_some(),
                        allowed
                    );
                    assert!(command(kind, lock, payload, true, Some(FRAME)).is_none());
                }
            }
        }
        assert!(matches!(
            command(WindowKind::Display, Lock::Interactive, &press, false, None),
            Some(WindowCommand::Drag(_))
        ));
    }

    #[test]
    fn right_release_opens_only_unhandled_unlocked_broadcast_menus() {
        let mut pointer = PointerInput::mouse(PointerPhase::Up, 40.0, 30.0);
        pointer.button = 2;
        for kind in WindowKind::ALL {
            for lock in [Lock::Locked, Lock::Interactive] {
                let payload = InputPayload::Pointer(pointer);
                let allowed = lock == Lock::Interactive
                    && matches!(
                        kind,
                        WindowKind::Display | WindowKind::PeerDisplay | WindowKind::History
                    );
                assert_eq!(
                    menu_position(kind, lock, &payload, false),
                    allowed.then_some((40.0, 30.0))
                );
                assert_eq!(menu_position(kind, lock, &payload, true), None);
                assert!(command(kind, lock, &payload, false, Some(FRAME)).is_none());
            }
        }
        for phase in [PointerPhase::Down, PointerPhase::Up] {
            for button in [0, 1, 2] {
                pointer.phase = phase;
                pointer.button = button;
                let payload = InputPayload::Pointer(pointer);
                assert_eq!(
                    menu_position(WindowKind::Display, Lock::Interactive, &payload, false)
                        .is_some(),
                    phase == PointerPhase::Up && button == 2
                );
                assert_eq!(
                    command(
                        WindowKind::Display,
                        Lock::Interactive,
                        &payload,
                        false,
                        Some(FRAME)
                    )
                    .is_some(),
                    phase == PointerPhase::Down && button == 0
                );
            }
        }
        pointer.x = f32::NAN;
        assert!(
            menu_position(
                WindowKind::Display,
                Lock::Interactive,
                &InputPayload::Pointer(pointer),
                false
            )
            .is_none()
        );
    }

    fn zoom(frame: Placement, delta: f32, unit: WheelUnit) -> ((f32, f32), (f32, f32)) {
        match command(
            WindowKind::History,
            Lock::Interactive,
            &wheel(delta, unit),
            false,
            Some(frame),
        )
        .unwrap()
        {
            WindowCommand::SetBounds { position, size, .. } => (position, size),
            _ => panic!("expected resize"),
        }
    }

    #[test]
    fn wheel_scales_proportionally_around_the_pointer_and_reverses() {
        let (pos, size) = zoom(FRAME, 1.0, WheelUnit::Lines);
        assert_eq!(pos, (60.0, 180.0));
        assert_eq!(size, (880.0, 440.0));
        assert_eq!(zoom(FRAME, 40.0, WheelUnit::Pixels), (pos, size));
        let frame = Placement {
            x: pos.0.into(),
            y: pos.1.into(),
            w: size.0.into(),
            h: size.1.into(),
        };
        let (_, restored) = zoom(frame, -1.0, WheelUnit::Lines);
        assert_eq!(restored, (800.0, 400.0));
        assert!(zoom(FRAME, 1.0, WheelUnit::Pixels).1.0 < size.0);
    }

    #[test]
    fn zoom_respects_both_size_limits_and_ignores_invalid_motion() {
        let small = Placement {
            w: 200.0,
            h: 100.0,
            ..FRAME
        };
        assert_eq!(zoom(small, -100.0, WheelUnit::Lines).1, (192.0, 96.0));
        let large = Placement {
            w: 8000.0,
            h: 4000.0,
            ..FRAME
        };
        assert_eq!(zoom(large, 100.0, WheelUnit::Lines).1, (8192.0, 4096.0));
        for delta in [0.0, f32::NAN, f32::INFINITY] {
            assert!(
                command(
                    WindowKind::History,
                    Lock::Interactive,
                    &wheel(delta, WheelUnit::Lines),
                    false,
                    Some(FRAME)
                )
                .is_none()
            );
        }
    }

    #[test]
    fn production_overlay_views_leave_default_gestures_for_the_host() {
        use crate::ui::{Ui, dialogue, history};
        use nana_ui::runtime::view::{AnyView, IntoView};
        use nana_ui::runtime::{DocumentId, HeadlessInput, RuntimeDocument};
        use nana_ui_devtools::agent::RuntimeAgentSession;
        use std::sync::Arc;

        for kind in [
            WindowKind::Display,
            WindowKind::PeerDisplay,
            WindowKind::History,
        ] {
            let mut settings = nanatype_settings::Settings::default();
            let lock = kind.overlay(&settings).unwrap().lock;
            assert_eq!(lock, Lock::Interactive);
            settings.history.hide_latest = nanatype_settings::HideLatest::Never;
            let skin = nanatype_theme::SkinRegistry::new(None, "0.1.0")
                .load(nanatype_theme::DEFAULT_SKIN)
                .unwrap()
                .skin;
            let mut core = nanatype_core::Core::new(settings.clone(), skin.clone());
            for _ in 0..20 {
                core.handle(
                    nanatype_core::Cmd::SendMarkup("测试台词 Test line".into()),
                    0,
                );
                core.handle(nanatype_core::Cmd::Skip, 1);
            }
            let ui = Ui::new(Arc::new(settings), skin, nanatype_i18n::Locale::ZhCn);
            ui.display.set(Arc::new(core.display().clone()));
            ui.peer_display.set(ui.display.get_untracked());
            ui.history
                .set(Arc::new(core.history().iter().cloned().collect()));
            let id = DocumentId::new(1).unwrap();
            let mut doc = RuntimeDocument::new(id);
            doc.context_mut()
                .mount_view_root(id, move || -> AnyView {
                    match kind {
                        WindowKind::Display => dialogue::display_window(ui).into_any(),
                        WindowKind::PeerDisplay => dialogue::display_window(ui.remote()).into_any(),
                        _ => history::history_window(ui).into_any(),
                    }
                })
                .unwrap();
            let mut session = RuntimeAgentSession::new(doc, 800, 400).unwrap();
            let mut input = HeadlessInput::bind(session.document_mut().context_mut(), id);
            for payload in [
                wheel(1.0, WheelUnit::Lines),
                InputPayload::Pointer(PointerInput::mouse(PointerPhase::Down, 400.0, 200.0)),
            ] {
                let outcome = input
                    .route(session.document_mut().context_mut(), payload.clone())
                    .unwrap();
                assert!(
                    command(kind, lock, &payload, outcome.handled, Some(FRAME)).is_some(),
                    "{kind:?}: {outcome:?}"
                );
            }
        }
    }
}
