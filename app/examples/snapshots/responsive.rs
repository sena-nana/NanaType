//! `NANATYPE_RESPONSIVE_AUDIT`: both writing modes and their previews.

use super::*;

pub fn audit(
    ui: Ui,
    skin: &nanatype_theme::Skin,
    out: &std::path::Path,
    theme: Arc<nana_ui::CompiledTheme>,
) {
    if std::env::var_os("NANATYPE_RESPONSIVE_LONG_TEXT").is_some() {
        let text = format!("[at=1001 name=星野ほたる avatar=blob:blive-face-1001] [sticker=nana/heart]{}", "中文竖排：你好，世界！Hello 2026。[b]字体[/b]与[color=#E8505B]颜色[/color]。[ruby=かんじ]漢字[/ruby]。\n".repeat(12));
        let doc = Arc::new(parse(&text).doc);
        ui.display.update(|view| {
            let view = Arc::make_mut(view);
            view.reveal = nanatype_core::Reveal::full(doc.unit_count() as u32);
            view.doc = doc;
            view.reply = None;
            view.revision += 1;
        });
    }
    let requested = |name: &str| {
        !std::env::var("NANATYPE_SNAPSHOT_VIEWS")
            .is_ok_and(|views| !views.split(',').any(|v| v.trim() == name))
    };
    let (mut windows, mut previews) = (0, 0);
    let default = [
        (skin.dialogue.region[0] + 2. * skin.dialogue.overscan) as u32,
        (skin.dialogue.region[1] + 2. * skin.dialogue.overscan) as u32,
    ];
    let id = DocumentId::new(1).unwrap();
    let mut doc = crate::document(id);
    doc.context_mut().set_theme_tokens(theme.clone()).unwrap();
    doc.context_mut()
        .mount_view_root(id, move || dialogue::display_window(ui))
        .unwrap();
    let mut session = RuntimeAgentSession::new(doc, default[0], default[1]).unwrap();
    session.set_gpu_renderers(nanatype::ui::sweep::renderers());
    session.set_clear(Some([0.; 4]));
    // Paging rewrites the reveal; every size starts from the whole line.
    let page = ui.display.get_untracked();
    for (state, enabled, reduced) in [
        ("static", false, false),
        ("effects", true, false),
        ("reduced", true, true),
    ] {
        ui.settings
            .update(|s| Arc::make_mut(s).skin.advanced_effects = enabled);
        ui.reduced_motion.set(reduced);
        for (case, [w, h]) in [
            ("default", default),
            ("narrow", [300, 240]),
            ("portrait", [300, 1200]),
            ("tall", [800, 2400]),
            ("minimum", [160, 96]),
            ("wide-short", [1200, 120]),
            ("max-height", [160, 8192]),
            ("max-width", [8192, 96]),
            ("square", [400, 400]),
            ("portrait-boundary", [400, 401]),
            ("default-vertical", default),
            ("portrait-vertical", [300, 1200]),
        ] {
            let name = format!("responsive-{state}-{case}");
            if !requested(&name) {
                continue;
            }
            ui.settings.update(|s| {
                Arc::make_mut(s).overlays.display.vertical_text = case.ends_with("-vertical")
            });
            let scale = (1080. / w.max(h) as f32).min(1.);
            if !Arc::ptr_eq(&ui.display.get_untracked(), &page) {
                ui.display.set(page.clone());
            }
            ui.text_scroll.set(0.);
            session.set_viewport(w, h, scale).unwrap();
            session.flush().unwrap();
            if std::env::var_os("NANATYPE_RESPONSIVE_LONG_TEXT").is_some()
                && let Some(node) = ui.text_node.get_untracked()
            {
                let world = session.document().context().world();
                if let Some((_, layout)) = world.text_layout(node)
                    && layout.is_vertical()
                {
                    let body = world.layout_box(node).unwrap();
                    let port = world
                        .layout_box(ui.text_port.get_untracked().unwrap())
                        .unwrap();
                    assert!(
                        body.width + 0.5 >= layout.physical_size().0,
                        "{name}: column stack is truncated"
                    );
                    assert!(
                        (body.height - port.height).abs() < 1.,
                        "{name}: wrapping height differs from viewport"
                    );
                    assert!(
                        (body.x + body.width - port.x - port.width).abs() < 1.,
                        "{name}: first column is not right aligned"
                    );
                    if let Some(first) = layout.objects.first() {
                        let rect = layout.page_rect(first.rect, body.width);
                        assert!(
                            body.x + rect.x >= port.x
                                && body.x + rect.x + rect.width <= port.x + port.width + 1.,
                            "{name}: leading inline object was clipped"
                        );
                    }
                }
            }
            follow_typing(ui, skin, &session);
            session.flush().unwrap();
            // Static authored backgrounds also prepare their GPU pipeline lazily.
            let _ = session.screenshot_rgba().unwrap();
            let started = std::time::Instant::now();
            while nanatype::ui::shader_effects::cache_stats().pending > 0
                && started.elapsed().as_secs() < 10
            {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            if !enabled || reduced {
                let first = nanatype::clock::capture_at(0, || session.screenshot_rgba().unwrap().1);
                let later =
                    nanatype::clock::capture_at(123_456, || session.screenshot_rgba().unwrap().1);
                // Native gradient avatars can differ by two byte levels at
                // antialiased edges between GPU submissions. Actual movement
                // or a changed print must still fail, without logging whole frames.
                let largest_delta = first
                    .iter()
                    .zip(&later)
                    .map(|(a, b)| a.abs_diff(*b))
                    .max()
                    .unwrap_or(0);
                assert!(
                    largest_delta <= 2,
                    "{name}: frozen artwork changed with time (maximum channel delta {largest_delta})"
                );
                if case == "portrait" && skin.meta.id == "nanatype.citrus" {
                    let pixel_width = (w as f32 * scale).round() as usize;
                    let sample = |x: f32, y: f32| {
                        let index = ((y * scale).round() as usize * pixel_width
                            + (x * scale).round() as usize)
                            * 4;
                        &first[index..index + 3]
                    };
                    let left = sample(skin.dialogue.overscan + 12., h as f32 * 0.6);
                    let middle = sample(w as f32 * 0.37, h as f32 * 0.6);
                    assert!(
                        left.iter().zip(middle).all(|(a, b)| a.abs_diff(*b) < 12),
                        "{name}: the portrait still has the extra left wrapper stripe"
                    );
                }
                if case == "portrait" && skin.meta.id == "nanatype.parchment" {
                    let pixel_width = (w as f32 * scale).round() as usize;
                    let mut difference = 0u64;
                    let mut samples = 0u64;
                    for y in 400..600usize {
                        for x in 45..80usize {
                            let index = (y * pixel_width + x) * 4;
                            for channel in 0..3 {
                                difference += u64::from(
                                    first[index + channel].abs_diff(first[index + 4 + channel]),
                                );
                                samples += 1;
                            }
                        }
                    }
                    assert!(
                        difference as f64 / samples as f64 > 0.45,
                        "{name}: the original paper fibres were replaced by a flat gradient"
                    );
                }
                assert!(
                    !nanatype::ui::shader_effects::animating(*ui),
                    "{name}: static artwork requests continuous frames"
                );
            }
            session
                .screenshot_png(out.join(format!("{name}.png")))
                .unwrap();
            windows += 1;
        }
    }
    ui.page.set(settings::Page::Skins);
    ui.skins.update(|rows| {
        if let Some(row) = Arc::make_mut(rows)
            .iter_mut()
            .find(|r| r.id == skin.meta.id)
        {
            row.skin = Some(Arc::new(skin.clone()));
        }
    });
    ui.reduced_motion.set(false);
    for (case, [w, h]) in [
        ("landscape", [920, 220]),
        ("portrait", [300, 1200]),
        ("maximum", [8192, 8192]),
    ] {
        let skin_id = ui.settings.with_untracked(|s| s.skin.id.clone());
        let mut draft = settings::preview::load_draft(&ui.settings.get_untracked(), &skin_id, ui);
        draft.display.width = w.to_string();
        draft.display.height = h.to_string();
        if case == "maximum" {
            draft.display.size = "200".into();
        }
        ui.skin_drafts.update(|d| {
            d.insert(skin_id.clone(), draft);
        });
        for (mode, play) in [("static", false), ("effects", true)] {
            let name = format!("responsive-preview-{mode}-{case}");
            if !requested(&name) {
                continue;
            }
            ui.shader_preview.set(play);
            shot(&name, out, 920, 1000, theme.clone(), |doc, id| {
                doc.context_mut()
                    .mount_view_root(id, move || settings::settings_window(ui))
                    .unwrap();
            });
            previews += 1;
        }
    }
    println!(
        "responsive: {} passed {windows} window and {previews} settings captures",
        skin.meta.id
    );
}

/// What the host does once a page taller than the box is laid out: scroll
/// the last typed line into view, or stop at the first full box and wait to
/// turn the page. Without it a capture shows the start of the text cut off
/// at the bottom edge, which the app never draws.
fn follow_typing(ui: Ui, skin: &nanatype_theme::Skin, session: &RuntimeAgentSession) {
    use nanatype::ui::overflow::{self, Follow, Measure, PageKey};
    let view = ui.display.get_untracked();
    let key = PageKey::of(&view);
    let Some(page) = overflow::published_on(ui.track_slot).filter(|p| p.key == key) else {
        return;
    };
    let world = session.document().context().world();
    let (Some(node), Some(port)) = (ui.text_node.get_untracked(), ui.text_port.get_untracked())
    else {
        return;
    };
    let (Some((_, layout)), Some(port)) = (world.text_layout(node), world.layout_box(port)) else {
        return;
    };
    let vertical = layout.is_vertical();
    let measure = Measure {
        vertical,
        lines: overflow::line_spans(layout),
        viewport: if vertical { port.width } else { port.height },
        glyphs: &page.glyphs,
    };
    match skin.dialogue.overflow {
        nanatype_model::Overflow::Clip => {}
        nanatype_model::Overflow::Scroll => {
            ui.text_scroll
                .set(Follow::default().frame(Some(key), Some(&measure), u64::MAX, true));
        }
        nanatype_model::Overflow::Page => {
            let Some(&first) =
                overflow::page_breaks(&measure.lines, measure.glyphs, measure.viewport).first()
            else {
                return;
            };
            // The core holds the first page until it is continued.
            let end = view.reveal.visible_from + first;
            ui.display.update(|view| {
                let view = Arc::make_mut(view);
                view.reveal.hidden_from = view.reveal.hidden_from.min(end);
                view.reveal.shown = view.reveal.shown.min(end);
                view.continue_indicator = true;
                view.revision += 1;
            });
        }
    }
}
