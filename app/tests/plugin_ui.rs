//! Actual GPU output and retained painter invalidation for third-party value trees.

mod common;

use nana_ui_devtools::agent::AgentSession;
use nanatype::ui::{self, Ui};
use nanatype_i18n::Locale;
use nanatype_plugins::{LuaSandbox, Mount};
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
    root.plugin_views.set(Arc::new(HashMap::new()));
    let (_, empty) = session.screenshot_rgba().unwrap();
    assert!(
        empty.as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 0),
        "disabled plugin left rendered content behind"
    );
}

/// Example archives install strictly, and the phrase example reads its own
/// declared data only through the reviewed snapshot.
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
    let mut vm = LuaSandbox::new(&reviewed.source).unwrap();
    let mut step = vm
        .dispatch(&serde_json::json!({"type": "init", "reads": ["phrases"]}))
        .unwrap();
    let mut texts = Vec::new();
    for _ in 0..7 {
        // Answer chained requests exactly as the supervisor does.
        while !step.requests.is_empty() {
            let replies: Vec<_> = step
                .requests
                .iter()
                .map(|r| match reviewed.data.answer(&r.id, &r.query) {
                    Ok(value) => nanatype_plugins::DataReply {
                        token: r.token,
                        value: Some(value),
                        error: None,
                    },
                    Err(e) => nanatype_plugins::DataReply::error(r.token, e),
                })
                .collect();
            step = vm.deliver(&replies).unwrap();
        }
        match &step.update.views[0].root {
            nanatype_plugins::Node::Text { text, .. } => texts.push(text.clone()),
            node => panic!("{node:?}"),
        }
        step = vm
            .dispatch(&serde_json::json!({"type": "line_start"}))
            .unwrap();
    }
    assert_eq!(texts[0], "今天也辛苦了");
    assert_eq!(texts[1], "Thank you for watching");
    assert_eq!(texts[5], "今天也辛苦了", "wraps after the last phrase");
}
