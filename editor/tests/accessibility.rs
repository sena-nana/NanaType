//! What assistive technology hears in the package editor: every control in
//! every mode has a name, and the launcher's too.

mod common;

use nana_ui_devtools::agent::RuntimeAgentSession;
use nanatype_editor::{
    session::{self as pe, Mode},
    ui::{self, Ui},
};
use nanatype_i18n::Locale;
use nanatype_settings::Settings;
use nanatype_theme::MANIFEST;
use nanatype_theme::editor::ProjectKind;
use std::sync::Arc;

/// Roles a user operates, and so must hear named.
const CONTROLS: &[&str] = &[
    "button",
    "switch",
    "slider",
    "checkbox",
    "combobox",
    "text-input",
    "radio-group",
    "tab",
    "spin-button",
];

fn unnamed(where_: &str, agent: &RuntimeAgentSession) -> Vec<String> {
    agent
        .accessibility_dump()
        .into_iter()
        .filter(|n| CONTROLS.contains(&n.role.as_str()))
        .filter(|n| n.label.as_deref().is_none_or(|l| l.trim().is_empty()))
        .map(|n| format!("{where_}: {} {:?} has no name", n.role, n.agent_path))
        .collect()
}

#[test]
fn every_editor_control_is_named() {
    let mut session = pe::create(ProjectKind::Mixed, "editor.a11y").unwrap();
    session.select(MANIFEST);
    let view = Arc::new(session.view(None, false));
    let mut found = Vec::new();
    for locale in [Locale::ZhCn, Locale::EnUs] {
        for mode in [
            Mode::Files,
            Mode::Package,
            Mode::Signing,
            Mode::Lua,
            Mode::Data,
        ] {
            let ui = Ui::new(
                Arc::new(Settings::default()),
                Arc::new(Default::default()),
                locale,
            );
            ui.package_editor.set(Some(view.clone()));
            ui.mode.set(mode);
            let mut agent = common::session().themed(false).mount(1280, 800, move || {
                ui::window::editor_window(ui, nana_ui::runtime::view::entity_ref())
            });
            agent.flush().unwrap();
            found.extend(unnamed(&format!("{locale:?} {mode:?}"), &agent));
        }
        let ui = Ui::new(
            Arc::new(Settings::default()),
            Arc::new(Default::default()),
            locale,
        );
        let mut agent = common::session()
            .themed(false)
            .mount(1280, 800, move || ui::views::launcher(ui));
        agent.flush().unwrap();
        found.extend(unnamed(&format!("{locale:?} launcher"), &agent));
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
}
