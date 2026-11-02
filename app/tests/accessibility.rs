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
fn a_window_matrix_cell_is_named_by_its_line_and_column() {
    let ui = Ui::new(
        Arc::new(Settings::default()),
        common::skin("nanatype.halo"),
        Locale::EnUs,
    );
    ui.page.set(settings::Page::Windows);
    let mut session =
        common::session()
            .themed(false)
            .locale(Locale::EnUs)
            .mount(1000, 1200, move || settings::settings_window(ui));
    for _ in 0..4 {
        session.flush().unwrap();
    }
    let labels: Vec<_> = session
        .accessibility_dump()
        .into_iter()
        .filter_map(|n| n.label)
        .collect();
    for name in ["Always on top: History", "Interaction: Partner"] {
        assert!(
            labels.iter().any(|l| l == name),
            "no {name:?} in {labels:?}"
        );
    }
}

#[test]
fn the_line_editor_is_named_for_what_it_is_not_its_hint() {
    let ui = Ui::new(
        Arc::new(Settings::default()),
        common::skin("nanatype.halo"),
        Locale::EnUs,
    );
    let mut session =
        common::session()
            .themed(false)
            .locale(Locale::EnUs)
            .mount(1200, 640, move || input::input_window(ui));
    for _ in 0..4 {
        session.flush().unwrap();
    }
    let name = nanatype::ui::I18N.t_in(Locale::EnUs, "editor-label");
    assert!(
        session
            .accessibility_dump()
            .into_iter()
            .any(|n| n.label.as_deref() == Some(name.as_str())),
        "no control named {name:?}"
    );
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

/// The settings window at `percent` UI scale and in `high_contrast`, at the
/// minimum size the scale gives it, as the app installs both: the window
/// zooms its whole content, so the page is laid out in the window's size
/// over the zoom and painted that much larger.
fn scaled_settings(page: settings::Page, percent: u32, high_contrast: bool) -> RuntimeAgentSession {
    use nana_ui::runtime::{DocumentId, RuntimeDocument};
    use nanatype::ui::kit::theme::{Chrome, compile};
    use nanatype::windows::{WindowKind, minimum_size, zoom};
    let mut s = Settings::default();
    s.accessibility.ui_scale = percent;
    s.general.onboarding_done = true;
    let zoom = zoom(WindowKind::Settings, &s, None);
    let (w, h) = minimum_size(WindowKind::Settings, zoom).unwrap();
    let ui = Ui::new(Arc::new(s), common::skin("nanatype.halo"), Locale::EnUs);
    ui.page.set(page);
    ui.chrome_contrast.set(high_contrast);
    let id = DocumentId::new(1).unwrap();
    let mut doc = RuntimeDocument::new(id);
    let cx = doc.context_mut();
    cx.set_theme_tokens(compile(Chrome {
        dark: false,
        high_contrast,
    }))
    .unwrap();
    nanatype_frame::apply_locale(cx, Locale::EnUs);
    cx.mount_view_root(id, move || settings::settings_window(ui))
        .unwrap();
    let content = |v: f64| (v / f64::from(zoom)).round() as u32;
    let mut session = RuntimeAgentSession::new_scaled(doc, content(w), content(h), zoom).unwrap();
    for _ in 0..4 {
        session.flush().unwrap();
    }
    session
}

#[test]
fn at_a_large_ui_scale_settings_controls_stay_inside_the_window() {
    let mut found = Vec::new();
    for percent in [100, 200] {
        for page in settings::Page::ALL {
            let mut session = scaled_settings(page, percent, false);
            let width = session.accessibility_dump()[0].bounds.width;
            if let Ok(dir) = std::env::var("NANATYPE_A11Y_CAPTURE") {
                std::fs::create_dir_all(&dir).unwrap();
                session
                    .screenshot_png(
                        std::path::Path::new(&dir).join(format!("{page:?}-{percent}.png")),
                    )
                    .unwrap();
            }
            for n in session.accessibility_dump() {
                if !CONTROLS.contains(&n.role.as_str()) || n.bounds.width == 0.0 {
                    continue;
                }
                if n.bounds.x < -0.5 || n.bounds.x + n.bounds.width > width + 0.5 {
                    found.push(format!(
                        "{percent}% {page:?}: {} {:?} at x {}..{} of {width}",
                        n.role,
                        n.label,
                        n.bounds.x,
                        n.bounds.x + n.bounds.width
                    ));
                }
            }
        }
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
}

#[test]
fn high_contrast_settings_paint_on_an_opaque_white_page() {
    let mut session = scaled_settings(settings::Page::General, 100, true);
    if let Ok(dir) = std::env::var("NANATYPE_A11Y_CAPTURE") {
        std::fs::create_dir_all(&dir).unwrap();
        session
            .screenshot_png(std::path::Path::new(&dir).join("General-high-contrast.png"))
            .unwrap();
    }
    let (size, rgba) = session.screenshot_rgba().unwrap();
    // The page's far corner, away from every card and control.
    let corner = common::pixel(&rgba, size.width, size.width - 4, size.height - 4);
    assert_eq!(corner, [255, 255, 255, 255]);
}
