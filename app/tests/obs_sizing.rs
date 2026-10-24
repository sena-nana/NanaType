//! Where an output's size comes from while OBS is linked.

use std::sync::Arc;

use nanatype::app::obs::{Sizing, State, box_size, candidates, sizing};
use nanatype_obs::{ObsItem, ObsScene, ObsStatus, Transform};
use nanatype_settings::{Settings, SpoutOutput};

fn item(source: &str, kind: &str, bounds: Option<[f64; 2]>, size: [f64; 2]) -> ObsItem {
    ObsItem {
        id: 1,
        source: source.into(),
        kind: kind.into(),
        group: false,
        transform: Transform { size, bounds },
    }
}

fn linked(source: &str) -> (Settings, SpoutOutput) {
    let mut s = Settings::default();
    s.output.obs.enabled = true;
    let out = SpoutOutput {
        obs_source: source.into(),
        size: Some([1280, 720]),
        ..SpoutOutput::default()
    };
    s.output.history = out.clone();
    (s, out)
}

fn connected(items: Vec<ObsItem>) -> State {
    State {
        status: ObsStatus::Connected,
        scene: Some(Arc::new(ObsScene {
            name: "Live".into(),
            items,
        })),
        password_saved: false,
    }
}

#[test]
fn sizing_falls_back_to_the_fixed_size_until_a_box_is_known() {
    let (mut s, out) = linked("NanaType History");
    assert_eq!(sizing(&s, &State::default(), &out), Sizing::Offline);
    let state = connected(vec![item("Camera", "dshow_input", None, [1.0, 1.0])]);
    assert_eq!(
        sizing(&s, &state, &out),
        Sizing::Missing {
            scene: "Live".into()
        }
    );
    let state = connected(vec![item(
        "NanaType History",
        "spout_capture",
        None,
        [400.0, 600.0],
    )]);
    assert_eq!(
        sizing(&s, &state, &out),
        Sizing::NoBounds {
            scene: "Live".into()
        }
    );
    let state = connected(vec![item(
        "NanaType History",
        "spout_capture",
        Some([2400.0, 3600.0]),
        [400.0, 600.0],
    )]);
    assert_eq!(
        sizing(&s, &state, &out),
        Sizing::Following {
            bounds: [2400.0, 3600.0],
            size: [1080, 1620]
        }
    );
    s.output.obs.enabled = false;
    assert_eq!(sizing(&s, &state, &out), Sizing::Manual);
    let (s, out) = linked("");
    assert_eq!(sizing(&s, &state, &out), Sizing::Manual);
}

#[test]
fn capture_sources_are_offered_first_and_groups_never() {
    let mut group = item("Group", "", None, [1.0, 1.0]);
    group.group = true;
    let scene = ObsScene {
        name: "Live".into(),
        items: vec![
            item("Camera", "dshow_input", None, [1.0, 1.0]),
            group,
            item("Nested", "", None, [1.0, 1.0]),
            item("NanaType", "spout_capture", None, [1.0, 1.0]),
        ],
    };
    let names: Vec<_> = candidates(&scene)
        .iter()
        .map(|i| i.source.as_str())
        .collect();
    assert_eq!(names, ["NanaType", "Camera"]);
}

#[test]
fn a_new_box_keeps_the_drawn_size_within_1080p() {
    let out = SpoutOutput {
        size: Some([1280, 720]),
        ..SpoutOutput::default()
    };
    let drawn = item("S", "spout_capture", None, [3840.0, 1280.0]);
    assert_eq!(box_size(&drawn, &out, None), [1920, 640]);
    // Not sending yet: OBS draws it at 0 × 0.
    let empty = item("S", "spout_capture", None, [0.0, 0.0]);
    assert_eq!(box_size(&empty, &out, None), [1280, 720]);
    let window = SpoutOutput::default();
    assert_eq!(box_size(&empty, &window, Some([400.0, 600.0])), [400, 600]);
    assert_eq!(box_size(&empty, &window, None), [1280, 720]);
}
