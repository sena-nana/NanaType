//! Actual GPU output and retained painter invalidation for third-party value trees.

mod common;

use nana_ui_devtools::agent::{AgentSession, RuntimeAgentSession};
use nanatype::ui::{self, Ui};
use nanatype_i18n::Locale;
use nanatype_plugins::{DrawCommand, LuaSandbox, Mount, Node, PluginUpdate, PluginView};
use std::{collections::HashMap, sync::Arc};

#[test]
fn lua_canvas_is_host_rendered_updates_on_events_and_unmounts_cleanly() {
    let mut vm =
        LuaSandbox::new(include_str!("../../docs/examples/counter-plugin/main.lua")).unwrap();
    let root = Ui::new(
        Arc::new(Default::default()),
        Arc::new(Default::default()),
        Locale::ZhCn,
    );
    let first = vm.event(&serde_json::json!({"type":"init"})).unwrap();
    root.plugin_views
        .set(Arc::new(HashMap::from([("example.counter".into(), first)])));
    let mut session = common::session()
        .themed(false)
        .mount(320, 180, move || ui::plugins::mounted(root, Mount::Panel));
    session.set_clear(Some([0.; 4]));
    let (_, before) = session.screenshot_rgba().unwrap();
    for _ in 0..10 {
        let update = vm.event(&serde_json::json!({"type":"line_start"})).unwrap();
        root.plugin_views.set(Arc::new(HashMap::from([(
            "example.counter".into(),
            update,
        )])));
    }
    let (_, after) = session.screenshot_rgba().unwrap();
    assert_ne!(
        before, after,
        "host retained canvas/text did not react to the Lua event"
    );
    let (_, idle) = session.screenshot_rgba().unwrap();
    assert_eq!(after, idle, "idle plugin should retain identical paint");
    // Exercise rectangle paint, stroked paths and filled paths on the retained canvas.
    let mut update = root.plugin_views.get_untracked()["example.counter"].clone();
    let mut previous = idle;
    for command in [
        DrawCommand::Rect {
            rect: [0., 0., 120., 20.],
            color: [240, 100, 0, 255],
            radius: 4.,
        },
        DrawCommand::Path {
            points: vec![[160., 3.], [180., 20.], [220., 3.]],
            color: [255, 0, 0, 255],
            closed: false,
            width: 2.,
        },
        DrawCommand::Path {
            points: vec![[160., 3.], [195., 20.], [220., 3.]],
            color: [0, 200, 70, 255],
            closed: true,
            width: 0.,
        },
    ] {
        let Node::Column { children, .. } = &mut update.views[0].root else {
            unreachable!()
        };
        let Node::Canvas { draw, .. } = &mut children[1] else {
            unreachable!()
        };
        *draw = vec![command];
        root.plugin_views.set(Arc::new(HashMap::from([(
            "example.counter".into(),
            update.clone(),
        )])));
        let (_, changed) = session.screenshot_rgba().unwrap();
        assert_ne!(previous, changed, "canvas edit did not repaint");
        previous = changed;
    }
    root.plugin_views.set(Arc::new(HashMap::new()));
    let (_, empty) = session.screenshot_rgba().unwrap();
    assert!(
        empty.as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 0),
        "disabled plugin left rendered content behind"
    );
}

/// A hand-built panel: a caption, a bar `filled` px wide, a button, and
/// optionally one more line.
fn panel(caption: &str, filled: f32, extra: Option<&str>) -> PluginUpdate {
    let mut children = vec![
        Node::Text {
            id: "caption".into(),
            text: caption.into(),
        },
        Node::Canvas {
            id: "bar".into(),
            width: 200.,
            height: 20.,
            draw: vec![DrawCommand::Rect {
                rect: [0., 0., filled, 20.],
                color: [60, 150, 210, 255],
                radius: 4.,
            }],
        },
        Node::Button {
            id: "go".into(),
            text: format!("Go {caption}"),
            command: "go".into(),
        },
    ];
    if let Some(text) = extra {
        children.push(Node::Text {
            id: "extra".into(),
            text: text.into(),
        });
    }
    PluginUpdate {
        views: vec![PluginView {
            slot: Mount::Panel,
            root: Node::Column {
                id: "root".into(),
                children,
            },
        }],
        commands: Vec::new(),
    }
}

fn show(root: Ui, update: PluginUpdate) -> RuntimeAgentSession {
    root.plugin_views
        .set(Arc::new(HashMap::from([("example.panel".into(), update)])));
    let mut session = common::session()
        .themed(false)
        .mount(320, 180, move || ui::plugins::mounted(root, Mount::Panel));
    session.set_clear(Some([0.; 4]));
    session
}

/// What `update` looks like mounted on its own.
fn fresh(update: PluginUpdate) -> Vec<u8> {
    let root = Ui::new(
        Arc::new(Default::default()),
        Arc::new(Default::default()),
        Locale::ZhCn,
    );
    show(root, update).screenshot_rgba().unwrap().1
}

fn node_id(session: &mut RuntimeAgentSession, label: &str) -> Option<u64> {
    session
        .accessibility_dump()
        .iter()
        .find(|n| n.label.as_deref() == Some(label))
        .map(|n| n.id)
}

#[test]
fn same_shape_updates_keep_the_widgets_and_new_shapes_rebuild() {
    let root = Ui::new(
        Arc::new(Default::default()),
        Arc::new(Default::default()),
        Locale::ZhCn,
    );
    let mut session = show(root, panel("one", 40., None));
    let (_, before) = session.screenshot_rgba().unwrap();
    let caption = node_id(&mut session, "one").expect("caption");
    let button = node_id(&mut session, "Go one").expect("button");
    for (step, filled) in [("two", 160.), ("three", 100.)] {
        let update = panel(step, filled, None);
        root.plugin_views.set(Arc::new(HashMap::from([(
            "example.panel".into(),
            update.clone(),
        )])));
        let (_, after) = session.screenshot_rgba().unwrap();
        assert_ne!(before, after, "the update did not repaint");
        assert_eq!(after, fresh(update), "{step}: drawn unlike a fresh mount");
        assert_eq!(
            node_id(&mut session, step),
            Some(caption),
            "caption rebuilt"
        );
        assert_eq!(
            node_id(&mut session, &format!("Go {step}")),
            Some(button),
            "button rebuilt"
        );
    }
    // A child more or less is another shape: built anew, as a mount would.
    for update in [panel("four", 80., Some("more")), panel("five", 120., None)] {
        root.plugin_views.set(Arc::new(HashMap::from([(
            "example.panel".into(),
            update.clone(),
        )])));
        let (_, shown) = session.screenshot_rgba().unwrap();
        assert_eq!(
            shown,
            fresh(update),
            "a reshaped view is drawn unlike a mount"
        );
    }
    assert!(node_id(&mut session, "five").is_some());
    assert!(
        node_id(&mut session, "more").is_none(),
        "removed child stayed"
    );
}

/// The production worker reads example data by value from the reviewed snapshot.
#[test]
fn example_archives_install_and_declared_data_flows_by_value() {
    let examples = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/examples");
    let dir = tempfile::tempdir().unwrap();
    let mut registry = nanatype_theme::SkinRegistry::new(Some(dir.path().into()), "1.0.0");
    for name in ["counter-plugin", "phrase-plugin"] {
        registry
            .install(&examples.join(format!("{name}.ntpkg")))
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
    }
    let reviewed = nanatype::plugins::review(&registry, "example.phrases").unwrap();
    let reads: Vec<_> = reviewed.data.disclosures().collect();
    assert_eq!(reads.len(), 1);
    assert_eq!(reads[0].format, nanatype_theme::DataFormat::Json);
    // Change the installed file before starting: only the reviewed bytes may reach Lua.
    std::fs::write(
        dir.path().join("example.phrases/data/phrases.json"),
        r#"{"phrases":["unreviewed"]}"#,
    )
    .unwrap();
    let (sender, responses) = std::sync::mpsc::channel();
    let worker = nanatype_plugins::PluginProcess::spawn(
        std::path::Path::new(env!("CARGO_BIN_EXE_nanatype")),
        reviewed.source,
        serde_json::json!({"type": "init", "reads": ["phrases"]}),
        nanatype::plugins::data_answer(reviewed.data),
        move |response| {
            let _ = sender.send(response);
        },
    )
    .unwrap();
    let mut texts = Vec::new();
    for index in 0..7 {
        if index > 0 {
            worker
                .event(serde_json::json!({"type": "line_start"}))
                .unwrap();
        }
        let response = responses
            .recv_timeout(std::time::Duration::from_secs(20))
            .unwrap();
        let nanatype_plugins::WorkerResponse::Update { update, .. } = response else {
            panic!("{response:?}");
        };
        match &update.views[0].root {
            nanatype_plugins::Node::Text { text, .. } => texts.push(text.clone()),
            node => panic!("{node:?}"),
        }
    }
    assert_eq!(texts[0], "今天也辛苦了");
    assert_eq!(texts[1], "Thank you for watching");
    assert_eq!(texts[5], "今天也辛苦了", "wraps after the last phrase");
}

/// The shipped configuration: the application binary re-runs itself as the
/// confined worker and serves events from the counter example.
#[test]
fn application_binary_runs_confined_plugin_workers() {
    let (sender, responses) = std::sync::mpsc::channel();
    let worker = nanatype_plugins::PluginProcess::spawn(
        std::path::Path::new(env!("CARGO_BIN_EXE_nanatype")),
        include_str!("../../docs/examples/counter-plugin/main.lua").into(),
        serde_json::json!({"type": "init"}),
        Arc::new(|r| nanatype_plugins::DataReply::error(r.token, "no data")),
        move |response| {
            let _ = sender.send(response);
        },
    )
    .unwrap();
    let first = responses
        .recv_timeout(std::time::Duration::from_secs(30))
        .unwrap();
    assert!(
        matches!(first, nanatype_plugins::WorkerResponse::Update { .. }),
        "{first:?}"
    );
    worker
        .event(serde_json::json!({"type": "line_start"}))
        .unwrap();
    assert!(matches!(
        responses
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap(),
        nanatype_plugins::WorkerResponse::Update { .. }
    ));
}
