//! The editor's chrome fits its window in every mode, language and size: a
//! long derived-copy origin ran over the save status and out of the window,
//! and the matched-layout caption ran on under the inspector.
mod common;
use nanatype_editor::{
    session::{self, Mode},
    ui::{self, Ui},
};
use nanatype_i18n::Locale;
use nanatype_settings::Settings;
use nanatype_theme::SkinRegistry;
use std::collections::HashMap;
use std::sync::Arc;

#[test]
fn the_chrome_fits_the_window() {
    let dir = tempfile::tempdir().unwrap();
    let registry = SkinRegistry::new(Some(dir.path().join("packages")), "1.0.0");
    // A derived copy: the longest origin the status bar shows.
    let mut draft =
        session::copy_installed(&registry, "nanatype.parchment", "editor.room").unwrap();
    let (ticket, project) = draft.validation_due().unwrap();
    draft.validated(ticket, Arc::new(session::validate(&registry, &project)));
    let view = Arc::new(draft.view(None, false));
    let [min_w, min_h] = ui::shell::MIN_SIZE.map(|v| v as u32);
    let mut bad = Vec::new();
    for locale in Locale::ALL {
        for (w, h) in [(min_w, min_h), (1600, 1000)] {
            for mode in Mode::ALL {
                let ui = Ui::new(
                    Arc::new(Settings::default()),
                    Arc::new(Default::default()),
                    locale,
                );
                ui.package_editor.set(Some(view.clone()));
                ui.mode.set(mode);
                let mut agent = common::session().themed(false).mount(w, h, move || {
                    ui::window::editor_window(ui, nana_ui::runtime::view::entity_ref())
                });
                for _ in 0..4 {
                    agent.flush().unwrap();
                }
                let tag = format!("{locale:?} {w}x{h} {mode:?}");
                let nodes: Vec<_> = agent
                    .accessibility_dump()
                    .into_iter()
                    .filter(|n| n.bounds.width > 0.0 && n.bounds.height > 0.0)
                    .collect();
                for n in nodes.iter().filter(|n| n.role != "generic") {
                    let b = &n.bounds;
                    if b.x < -0.5 || b.x + b.width > w as f32 + 0.5 {
                        bad.push(format!("{tag}: {} {:?} at {b:?}", n.role, n.label));
                    }
                }
                // The status bar's texts side by side, none over another.
                let status = ui.now_t("pkgedit-status");
                let bar = nodes
                    .iter()
                    .find(|n| n.label.as_deref() == Some(status.as_str()))
                    .expect("the status bar")
                    .id;
                let parents: HashMap<u64, Option<u64>> =
                    nodes.iter().map(|n| (n.id, n.parent)).collect();
                let in_bar = |mut id: u64| loop {
                    match parents.get(&id).copied().flatten() {
                        Some(p) if p == bar => break true,
                        Some(p) => id = p,
                        None => break false,
                    }
                };
                let texts: Vec<_> = nodes
                    .iter()
                    .filter(|n| n.role == "text" && in_bar(n.id))
                    .collect();
                for (i, a) in texts.iter().enumerate() {
                    for c in &texts[i + 1..] {
                        let (p, q) = (&a.bounds, &c.bounds);
                        if p.x.max(q.x) < (p.x + p.width).min(q.x + q.width) - 0.5 {
                            bad.push(format!("{tag}: {:?} over {:?}", a.label, c.label));
                        }
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
