use super::*;
use nanatype_markup::parse;
use nanatype_playback::WakeReason;
use nanatype_settings::{InputMode, Settings};
use nanatype_theme::SkinRegistry;
use std::sync::Arc;

#[test]
fn mirror_retains_unedited_units_and_inflight_timestamps() {
    let skin = SkinRegistry::new(None, "1.0.0")
        .load("nanatype.machine_learning")
        .unwrap()
        .skin;
    let mut settings = Settings::default();
    settings.general.input_mode = InputMode::Mirror;
    let mut core = Core::new(settings, skin);
    core.handle(Cmd::DraftChanged(parse("AB中文CD").doc), 1000);
    let before = core.display().mirror_reveal_ms.clone().unwrap();
    core.handle(Cmd::DraftChanged(parse("AB学文CD").doc), 1020);
    let after = core.display().mirror_reveal_ms.clone().unwrap();
    for i in [0, 1, 3, 4, 5] {
        assert_eq!(after[i], before[i]);
    }
    assert_eq!(after[2], 1020);
    core.handle(Cmd::DraftChanged(parse("AB学文CDD").doc), 1040);
    let next = core.display().mirror_reveal_ms.as_ref().unwrap();
    assert_eq!(&next[..6], after.as_ref());
    assert_eq!(next[6], 1040);
    assert_eq!(core.display().doc.plain_text(), "AB学文CDD");
    core.handle(Cmd::DraftChanged(parse("ABD").doc), 1060);
    assert_eq!(core.display().doc.plain_text(), "ABD");
}

#[test]
fn retirement_is_finite_and_respects_motion_settings() {
    let skin = SkinRegistry::new(None, "1.0.0")
        .load("nanatype.machine_learning")
        .unwrap()
        .skin;
    for (advanced, reduced) in [(true, false), (false, false), (true, true)] {
        let mut settings = Settings::default();
        settings.skin.advanced_effects = advanced;
        settings.playback.reduced_motion = reduced;
        let mut core = Core::new(settings, skin.clone());
        core.handle(Cmd::SendMarkup("中文 ML".into()), 1000);
        core.handle(Cmd::Skip, 2000);
        let fx = core.handle(Cmd::ClearDisplay, 3000);
        let wake = fx
            .wakes
            .iter()
            .find(|w| w.reason == WakeReason::PresentationDone);
        assert_eq!(wake.is_some(), advanced && !reduced);
        if let Some(w) = wake {
            assert_eq!(w.at_ms, 3400);
            core.handle(
                Cmd::Wake {
                    epoch: w.epoch,
                    reason: w.reason,
                },
                w.at_ms,
            );
        }
        assert!(core.display().empty);
    }
}

#[test]
fn mirror_times_survive_transport_and_are_optional_for_older_peers() {
    let mut view = DisplayView::empty(nanatype_model::Entrance {
        kind: nanatype_model::EntranceKind::None,
        ms: 0,
    });
    view.mirror_reveal_ms = Some(Arc::from([1000u64, 1020, 1040]));
    let value = serde_json::to_value(&view).unwrap();
    let restored: DisplayView = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(view, restored);
    let mut old = value;
    old.as_object_mut().unwrap().remove("mirror_reveal_ms");
    let restored: DisplayView = serde_json::from_value(old).unwrap();
    assert!(restored.mirror_reveal_ms.is_none());
}
