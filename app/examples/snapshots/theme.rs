//! `NANATYPE_THEME_SNAPSHOTS_ONLY` (and `NANATYPE_THEME_AUDIT`): a focused theme review.

use super::*;

/// Dialogue, input, history and the skin library only, plus the painter,
/// material and theme audits when asked for.
pub fn capture(scene: &Scene, out: &std::path::Path, ih: u32) {
    let (ui, w, h) = (scene.ui, scene.w, scene.h);
    let (skin, theme) = (&scene.skin, &scene.theme);
    let d = &skin.dialogue;
    if std::env::var_os("NANATYPE_PAINTER_AUDIT").is_some() {
        let surface = nanatype::ui::surface::Surface::dialogue_box(skin, None);
        let [mt, mr, mb, ml] = d.box_.margin;
        let overscan = d.overscan;
        let fraction = d.box_.width_fraction;
        shot("shape-only", out, w, h, theme.clone(), |doc, id| {
            doc.context_mut()
                .mount_view_root(id, move || {
                    use nana_ui::runtime::Stack;
                    use nana_ui::runtime::view::widget;
                    use nana_ui_core::{AlignSpec, LengthSpec};
                    widget(Stack::fill_column(0.0).with_layout(move |l| {
                        l.padding_left = Some(LengthSpec::Px(overscan + ml));
                        l.padding_right = Some(LengthSpec::Px(overscan + mr));
                        l.padding_top = Some(LengthSpec::Px(overscan + mt));
                        l.padding_bottom = Some(LengthSpec::Px(overscan + mb));
                        l.align_items = AlignSpec::Center;
                    }))
                    .children(widget(surface.stack(
                        Stack::fill_column(0.0).width(LengthSpec::Percent(fraction * 100.0)),
                    )))
                })
                .unwrap();
        });
    }
    shot(
        "history",
        out,
        skin.history.width as u32,
        skin.history.height as u32,
        theme.clone(),
        |doc, id| {
            doc.context_mut()
                .mount_view_root(id, move || history::history_window(ui))
                .unwrap();
        },
    );
    ui.page.set(settings::Page::Skins);
    shot(
        "settings-Skins",
        out,
        920,
        1700,
        theme.clone(),
        |doc, id| {
            doc.context_mut()
                .mount_view_root(id, move || settings::settings_window(ui))
                .unwrap();
        },
    );
    if std::env::var_os("NANATYPE_MATERIAL_AUDIT").is_some() && !skin.materials.is_empty() {
        material::audit(ui, out, w, h, theme.clone());
    }
    if std::env::var_os("NANATYPE_THEME_AUDIT").is_some() {
        audit(ui, out, w, h, ih, theme.clone());
    }
}

pub fn audit(
    ui: Ui,
    out: &std::path::Path,
    w: u32,
    h: u32,
    ih: u32,
    theme: Arc<nana_ui::CompiledTheme>,
) {
    let display_shot = |name: &str, ui: Ui, w, h| {
        nanatype::clock::capture_at(ui.now.get_untracked(), || {
            shot(name, out, w, h, theme.clone(), |doc, id| {
                doc.context_mut()
                    .mount_view_root(id, move || {
                        dialogue::dialogue_box(ui, dialogue::Source::Display, None)
                    })
                    .unwrap();
            })
        });
    };
    let input_shot = |name: &str, w, h| {
        shot(name, out, w, h, theme.clone(), |doc, id| {
            doc.context_mut()
                .mount_view_root(id, move || input::input_window(ui))
                .unwrap();
        });
    };
    let skin = ui.skin.get_untracked();
    let period_ms = match skin.meta.id.as_str() {
        "nanatype.night" => 6000,
        "nanatype.bubble" => 5000,
        "nanatype.sakura" => 24000, // both the 6s and 8s petals return to rest
        _ => 0,
    };
    if period_ms > 0 {
        let frames = if period_ms == 24000 { 16 } else { 8 };
        ui.peer_display.set(ui.display.get_untracked());
        for i in 0..=frames {
            ui.now.set(120_000 + i * period_ms / frames);
            ui.peer_now.set(120_000 + i * period_ms / frames);
            display_shot(&format!("motion-{i:02}"), ui, w, h);
            display_shot(&format!("motion-peer-{i:02}"), ui.remote(), w, h);
        }
        for i in 0..2 {
            ui.reduced_motion.set(true);
            ui.now.set(120_000 + i * 1750);
            display_shot(&format!("reduced-{i}"), ui, w, h);
            ui.reduced_motion.set(false);
            input_shot(&format!("input-still-{i}"), w, ih);
        }
        ui.reduced_motion.set(false);
    }
    // Long names plus quote/@, ruby and line breaks use the real composer
    // and display builders, including the text viewport's clipping.
    // Enlarge the skin region as well as the window: the editor deliberately
    // retains the configured region height rather than stretching with chrome.
    let mut stress_skin = (*skin).clone();
    stress_skin.dialogue.region[1] += 140.0;
    ui.skin.set(Arc::new(stress_skin));
    let mut settings = (*ui.settings.get_untracked()).clone();
    settings.general.speaker_name = "奈奈与联机伙伴・A Very Long Speaker Name".into();
    ui.settings.set(Arc::new(settings));
    let text = parse("[ruby=ものがたり]故事[/ruby]继续，今天也请多关照。\n第二行是长台词，用来检查装饰、换行与边界。\n第三行仍然清晰。[at=1001 name=星野ほたる]").doc;
    ui.draft.set(Arc::new(text.clone()));
    ui.editor_value.set(nanatype::ui::rich::to_rich(
        &text,
        &nanatype::ui::rich::Env {
            look: &dialogue::Base::editor(&skin).look(),
            urls: &ui.asset_urls.get_untracked(),
            editor: true,
            locale: ui.locale.get_untracked(),
        },
    ));
    let mut view = (*ui.display.get_untracked()).clone();
    view.doc = Arc::new(text.clone());
    view.speaker.name = "奈奈与联机伙伴・A Very Long Speaker Name".into();
    view.reveal = nanatype_core::Reveal::full(text.unit_count() as u32);
    view.revision += 1;
    ui.display.set(Arc::new(view));
    ui.now.set(150_000);
    // Wide visual-novel skins retain their original columns and menu row;
    // review them at 960px, compact dialogue skins at 560px.
    let narrow_width = w.min(if skin.meta.id == "nanatype.crayon" {
        300
    } else if skin.dialogue.region[0] >= 1100.0 {
        960
    } else {
        560
    });
    display_shot("display-long", ui, w, h + 140);
    display_shot("display-narrow", ui, narrow_width, h + 140);
    input_shot("input-narrow", narrow_width, ih + 140);
}
