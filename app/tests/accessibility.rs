//! What assistive technology hears in the app's own windows: every control
//! has a name, and a switch or a slider says where it stands.
mod common;
use nana_ui_devtools::agent::{AccessibilityDumpNode, RuntimeAgentSession};
use nanatype::ui::{Ui, input, settings};
use nanatype_i18n::Locale;
use nanatype_settings::Settings;
use std::collections::HashMap;
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
    "radio",
    "tab",
    "menu-item",
    "spin-button",
];

/// Controls in `session` that are unnamed, or that hide their state.
fn problems(where_: &str, session: &mut RuntimeAgentSession) -> Vec<String> {
    let mut found = Vec::new();
    for n in session.accessibility_dump() {
        if !CONTROLS.contains(&n.role.as_str()) {
            continue;
        }
        let describe = |n: &AccessibilityDumpNode, what: &str| {
            format!("{where_}: {} {:?} {what}", n.role, n.agent_path)
        };
        if n.label.as_deref().is_none_or(|l| l.trim().is_empty()) {
            found.push(describe(&n, "has no name"));
        }
        if n.role == "switch" && n.checked.is_none() {
            found.push(describe(&n, "does not say whether it is on"));
        }
        if n.role == "slider" && n.value.is_none() {
            found.push(describe(&n, "does not say its value"));
        }
    }
    found
}

#[test]
fn every_settings_control_is_named_and_says_its_state() {
    let mut found = Vec::new();
    for locale in [Locale::ZhCn, Locale::EnUs] {
        for page in settings::Page::ALL {
            let ui = Ui::new(
                Arc::new(Settings::default()),
                common::skin("nanatype.halo"),
                locale,
            );
            ui.page.set(page);
            let mut session =
                common::session()
                    .themed(false)
                    .locale(locale)
                    .mount(1000, 2400, move || settings::settings_window(ui));
            for _ in 0..4 {
                session.flush().unwrap();
            }
            found.extend(problems(&format!("{locale:?} {page:?}"), &mut session));
        }
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
}

#[test]
fn a_settings_row_names_its_control_by_its_label() {
    let ui = Ui::new(
        Arc::new(Settings::default()),
        common::skin("nanatype.halo"),
        Locale::EnUs,
    );
    ui.page.set(settings::Page::Playback);
    let mut session =
        common::session()
            .themed(false)
            .locale(Locale::EnUs)
            .mount(1000, 2400, move || settings::settings_window(ui));
    for _ in 0..4 {
        session.flush().unwrap();
    }
    let reduced = nanatype::ui::I18N.t_in(Locale::EnUs, "playback-reduced");
    let switch = session
        .accessibility_dump()
        .into_iter()
        .find(|n| n.role == "switch" && n.label.as_deref() == Some(reduced.as_str()))
        .unwrap_or_else(|| panic!("no switch named {reduced:?}"));
    assert_eq!(switch.checked, Some(false));
}

#[test]
fn every_input_window_control_is_named() {
    let mut found = Vec::new();
    for locale in [Locale::ZhCn, Locale::EnUs] {
        let ui = Ui::new(
            Arc::new(Settings::default()),
            common::skin("nanatype.halo"),
            locale,
        );
        let mut session =
            common::session()
                .themed(false)
                .locale(locale)
                .mount(1200, 640, move || input::input_window(ui));
        for _ in 0..4 {
            session.flush().unwrap();
        }
        found.extend(problems(&format!("{locale:?} input"), &mut session));
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
}

#[test]
fn tab_reaches_every_settings_control() {
    let mut found = Vec::new();
    for page in settings::Page::ALL {
        let ui = Ui::new(
            Arc::new(Settings::default()),
            common::skin("nanatype.halo"),
            Locale::EnUs,
        );
        ui.page.set(page);
        let mut session =
            common::session()
                .themed(false)
                .locale(Locale::EnUs)
                .mount(1000, 2400, move || settings::settings_window(ui));
        for _ in 0..4 {
            session.flush().unwrap();
        }
        // Tab until focus comes back round to where it started.
        let mut seen = Vec::new();
        for _ in 0..400 {
            session.key_press("Tab", "Tab", Default::default()).unwrap();
            session.flush().unwrap();
            let focused = session.accessibility_dump().into_iter().find(|n| n.focused);
            match focused {
                Some(n) if seen.first() == Some(&n.id) => break,
                Some(n) => seen.push(n.id),
                None => {}
            }
        }
        let nodes = session.accessibility_dump();
        let parents: HashMap<u64, Option<u64>> = nodes.iter().map(|n| (n.id, n.parent)).collect();
        let inside = |mut id: u64, of: u64| loop {
            if id == of {
                break true;
            }
            match parents.get(&id).copied().flatten() {
                Some(p) => id = p,
                None => break false,
            }
        };
        // A radio group is one stop: arrows move between its radios.
        for n in &nodes {
            if !CONTROLS.contains(&n.role.as_str()) || n.role == "radio" || n.disabled {
                continue;
            }
            if !seen.iter().any(|&s| inside(s, n.id)) {
                found.push(format!(
                    "{page:?}: Tab never reaches {} {:?}",
                    n.role, n.label
                ));
            }
        }
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
}
