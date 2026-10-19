//! `NANATYPE_PREVIEW_SCALE_AUDIT`: tall and maximum canvases in the preview.

use super::*;

pub fn audit(ui: Ui, out: &std::path::Path, theme: Arc<nana_ui::CompiledTheme>) {
    ui.page.set(settings::Page::Skins);
    let id = ui.settings.with_untracked(|s| s.skin.id.clone());
    for (name, [w, h], font) in [
        ("tall", [300, 1200], 48),
        ("very-tall", [800, 2400], 48),
        ("maximum-height", [160, 8192], 200),
        ("maximum-square", [8192, 8192], 200),
        ("maximum-width", [8192, 96], 48),
    ] {
        let mut draft = settings::preview::Draft::load(&ui.settings.get_untracked(), &id, ui);
        for pane in [&mut draft.display, &mut draft.history] {
            pane.width = w.to_string();
            pane.height = h.to_string();
            pane.size = font.to_string();
        }
        ui.skin_drafts.update(|drafts| {
            drafts.insert(id.clone(), draft);
        });
        for history in [false, true] {
            ui.skin_preview_history.set(history);
            let mode = if history { "history" } else { "display" };
            shot(
                &format!("preview-{mode}-{name}"),
                out,
                920,
                1000,
                theme.clone(),
                |doc, id| {
                    doc.context_mut()
                        .mount_view_root(id, move || settings::settings_window(ui))
                        .unwrap();
                },
            );
        }
    }
}
