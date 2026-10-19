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
