//! `NANATYPE_RESIZE_AUDIT`: native layouts at varied client sizes.

use super::*;

/// All-skin native layout captures at independently varied client sizes.
pub fn audit(
    ui: Ui,
    base: Arc<nanatype_theme::Skin>,
    out: &std::path::Path,
    theme: Arc<nana_ui::CompiledTheme>,
) {
    use nanatype_settings::{SkinPersonalization, SurfacePersonalization};
    let default_display = [
        base.dialogue.region[0] + 2.0 * base.dialogue.overscan,
        base.dialogue.region[1] + 2.0 * base.dialogue.overscan,
    ];
    let default_history = [base.history.width, base.history.height];
    for (name, display, history_size, font) in [
        ("default", default_display, default_history, None),
        ("narrow", [300.0, 240.0], [300.0, 560.0], None),
        ("minimum", [160.0, 96.0], [160.0, 96.0], None),
        ("wide-short", [1200.0, 120.0], [1200.0, 120.0], None),
        ("large-font", [300.0, 240.0], [300.0, 560.0], Some(48.0)),
    ] {
        let mut settings = (*ui.settings.get_untracked()).clone();
        let p = SkinPersonalization {
            display: SurfacePersonalization {
                size: Some(display),
                text_size: font,
                line_height: font.map(|_| 1.8),
                ..Default::default()
            },
            history: SurfacePersonalization {
                size: Some(history_size),
                text_size: font,
                line_height: font.map(|_| 1.8),
                ..Default::default()
            },
            ..Default::default()
        };
        settings
            .skin
            .personalizations
            .insert(settings.skin.id.clone(), p);
        let skin = Arc::new(nanatype::skin_theme::personalized(
            &base,
            &settings,
            &settings.skin.id,
        ));
        ui.settings.set(Arc::new(settings));
        ui.skin.set(skin);
        shot(
            &format!("display-{name}"),
            out,
            display[0] as u32,
            display[1] as u32,
            theme.clone(),
            |doc, id| {
                doc.context_mut()
                    .mount_view_root(id, move || dialogue::display_window(ui))
                    .unwrap();
            },
        );
        shot(
            &format!("history-{name}"),
            out,
            history_size[0] as u32,
            history_size[1] as u32,
            theme.clone(),
            |doc, id| {
                doc.context_mut()
                    .mount_view_root(id, move || history::history_window(ui))
                    .unwrap();
            },
        );
    }
    ui.page.set(settings::Page::Skins);
    shot("preview", out, 920, 1700, theme.clone(), |doc, id| {
        doc.context_mut()
            .mount_view_root(id, move || settings::settings_window(ui))
            .unwrap();
    });
}
