//! The dialogue box.
//!
//! One builder draws the typewriter, the input editor's box and the history
//! bubbles, so they cannot drift apart. The text itself is a single NanaUI
//! rich text node (a `RichTextView`, or the `RichTextEditor` in the input
//! window) built from the document by [`super::rich`]; the typewriter reveal
//! and the per-character effects are glyph presentation evaluated on the GPU,
//! so nothing reshapes while a line plays.

mod chrome;
mod text_node;
mod track;

pub use chrome::{BoxCtx, Source, dialogue_box, preview_box};
pub(crate) use chrome::{canvas_size, decor, empty_placeholder, vertical_text};
pub use text_node::{Base, Timing, document, has_effects, resolve_entrance};
pub(crate) use track::fresh_preview_line;
pub use track::{
    DisplayPresence, DisplayTrackState, display_animating, display_compositor_needs_tick,
    frame_demand,
};

#[cfg(test)]
mod decor_tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;

    use nana_ui::runtime::Stack;
    use nana_ui::runtime::view::widget;
    use nana_ui_core::{LengthSpec, PositionSpec};
    use nanatype_core::Reveal;
    use nanatype_model::Overflow;
    use nanatype_theme::{DecorLayer, anchor_frac};

    use super::super::Stage;
    use super::super::overflow;
    use super::chrome::{box_surface_for, nameplate};

    #[test]
    fn artwork_keeps_its_static_frame_as_a_runtime_fallback() {
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        let skin = registry.load("nanatype.celestial").unwrap().skin;
        let resolved = skin.resolve_dialogue((920.0, 220.0));
        let surface = box_surface_for(
            &skin,
            &resolved,
            resolved
                .dialogue
                .box_
                .frame
                .as_ref()
                .map(|_| String::from("file:///still.png")),
            true,
        );
        assert!(
            surface.frame.is_some(),
            "an artwork shader must keep its static frame underneath"
        );
    }

    #[test]
    fn below_shader_decor_is_not_duplicated_across_decor_and_shader_art() {
        let mut skin = nanatype_theme::Skin::default();
        skin.dialogue.decor.push(nanatype_theme::DecorSpec {
            layer: DecorLayer::Below,
            shader: Some("glow".into()),
            ..Default::default()
        });
        skin.dialogue.decor.push(nanatype_theme::DecorSpec {
            layer: DecorLayer::Below,
            shader: None,
            ..Default::default()
        });
        let resolved = skin.resolve_dialogue((920.0, 220.0));
        let non_shader_below = resolved
            .dialogue
            .decor
            .iter()
            .filter(|d| d.layer == DecorLayer::Below && d.shader.is_none())
            .count();
        let shader_below = resolved
            .dialogue
            .decor
            .iter()
            .filter(|d| d.layer == DecorLayer::Below && d.shader.is_some())
            .count();
        assert_eq!(non_shader_below, 1);
        assert_eq!(shader_below, 1);
    }

    #[test]
    fn artwork_nameplates_shrink_the_text_before_the_ornament_strip() {
        use nana_ui::runtime::{DocumentId, RuntimeDocument};
        use nana_ui_devtools::agent::RuntimeAgentSession;

        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        for theme in [
            "gothic",
            "celestial",
            "forest",
            "tidal",
            "lantern",
            "citrus",
        ] {
            let skin = registry.load(&format!("nanatype.{theme}")).unwrap().skin;
            let ui = Stage::new(
                Arc::new(nanatype_settings::Settings::default()),
                skin.clone(),
                nanatype_i18n::Locale::ZhCn,
            );
            for name in ["奈奈", "奈奈与联机伙伴・A Very Long Speaker Name"] {
                let ctx = BoxCtx {
                    skin: skin.clone(),
                    urls: Arc::new(HashMap::new()),
                    source: Source::Preview,
                    preview_name: name.into(),
                };
                let resolved = skin.resolve_dialogue((528.0, 220.0));
                let right_pad = resolved.dialogue.padding[2];
                let plate_size = resolved.dialogue.nameplate.size;
                let id = DocumentId::new(1).unwrap();
                let mut doc = RuntimeDocument::new(id);
                doc.context_mut()
                    .mount_view_root(id, move || {
                        widget(
                            Stack::column(0.)
                                .width(LengthSpec::Px(528.))
                                .height(LengthSpec::Px(220.))
                                .with_layout(|l| l.position = PositionSpec::Relative),
                        )
                        .children(nameplate(ui, &ctx, &resolved))
                    })
                    .unwrap();
                let mut session = RuntimeAgentSession::new(doc, 528, 220).unwrap();
                session.screenshot_rgba().unwrap();
                let label = session
                    .accessibility_dump()
                    .into_iter()
                    .find(|n| n.label.as_deref() == Some(name))
                    .expect("nameplate text keeps its accessible name");
                assert!(label.bounds.width > 0., "{theme}: name is visible");
                assert!(
                    label.bounds.x + label.bounds.width <= 528. - right_pad + 2.,
                    "{theme}: name extends into the ornament strip: {:?}",
                    label.bounds
                );
                if name == "奈奈" {
                    assert!(
                        label.bounds.width < plate_size * 3.,
                        "{theme}: short names keep their natural width"
                    );
                }
            }
        }
    }

    /// Vertical-rl text starts at the top right: the name plate heads the
    /// first column there, whole and upright beside the body, and goes back
    /// over the first line when the switch is off.
    #[test]
    fn vertical_text_heads_its_first_column_with_the_name_plate() {
        use nana_ui::runtime::{DocumentId, RuntimeDocument};
        use nana_ui_devtools::agent::RuntimeAgentSession;
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        let mut failures = Vec::new();
        for entry in registry.entries() {
            let skin = registry.load(&entry.id).unwrap().skin;
            let [w, h] = skin
                .dialogue
                .region
                .map(|v| (v + 2.0 * skin.dialogue.overscan) as u32);
            let ui = Stage::new(
                Arc::new(nanatype_settings::Settings::default()),
                skin.clone(),
                nanatype_i18n::Locale::ZhCn,
            );
            let text = nanatype_markup::parse("晚上好！今天也谢谢你来～").doc;
            ui.display.update(|v| {
                let v = Arc::make_mut(v);
                v.empty = false;
                v.reveal = Reveal::full(text.unit_count() as u32);
                v.doc = Arc::new(text);
                v.speaker.name = "奈奈".into();
            });
            let id = DocumentId::new(1).unwrap();
            let mut doc = RuntimeDocument::new(id);
            doc.context_mut()
                .mount_view_root(id, move || dialogue_box(ui, Source::Display, None))
                .unwrap();
            let mut session = RuntimeAgentSession::new(doc, w, h).unwrap();
            for vertical in [true, false, true] {
                ui.settings
                    .update(|s| Arc::make_mut(s).overlays.display.vertical_text = vertical);
                session.flush().unwrap();
                let resolved = skin.resolve_dialogue((w as f32, h as f32)).dialogue;
                let p = &resolved.nameplate;
                if !p.visible {
                    continue;
                }
                let plates: Vec<_> = session
                    .accessibility_dump()
                    .into_iter()
                    .filter(|n| n.label.as_deref() == Some("奈奈"))
                    .collect();
                assert_eq!(plates.len(), 1, "{}: one name plate", entry.id);
                let name = plates[0].bounds.clone();
                let port = session
                    .document()
                    .context()
                    .world()
                    .layout_box(ui.text_port.get_untracked().unwrap())
                    .unwrap();
                let mut check = |ok: bool, what: &str| {
                    if !ok {
                        failures.push(format!(
                            "{} vertical={vertical}: {what}: name={name:?} body={port:?}",
                            entry.id
                        ));
                    }
                };
                if !vertical {
                    check(
                        name.y + name.height <= port.y + 2.,
                        "a horizontal plate stays over the first line",
                    );
                    continue;
                }
                if anchor_frac(p.anchor)[0] >= 1.0 {
                    continue;
                }
                check(
                    name.height >= p.size * 1.8 && name.width < p.size * 1.6,
                    "the whole name runs down the plate",
                );
                check(
                    name.x >= port.x + port.width - 1.,
                    "the plate heads the columns from the right",
                );
                check(
                    (name.y - port.y).abs() <= p.padding[0] + p.size * 0.5 + 2.,
                    "the name starts level with the columns",
                );
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn responsive_resize_keeps_the_text_node_schedule_and_track_independence() {
        use nana_ui::runtime::{DocumentId, RuntimeDocument};
        use nana_ui_devtools::agent::{AgentSession, RuntimeAgentSession};
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        let mut skin = (*registry.load("nanatype.halo").unwrap().skin).clone();
        skin.dialogue.overflow = Overflow::Scroll;
        let ui = Stage::new(
            Arc::new(nanatype_settings::Settings::default()),
            Arc::new(skin),
            nanatype_i18n::Locale::ZhCn,
        );
        let text = nanatype_markup::parse(&format!(
            "[at=42 name=奈奈] [sticker=nana/heart]{}",
            "中文竖排：你好，世界！Hello 2026。[b]字体[/b]与[ruby=かんじ]漢字[/ruby]。\n"
                .repeat(15)
        ))
        .doc;
        ui.display.update(|v| {
            let v = Arc::make_mut(v);
            v.empty = false;
            v.doc = Arc::new(text);
            v.reveal = Reveal::full(600);
            v.reply = Some(nanatype_model::ChatRef::new(
                nanatype_model::ChatUser {
                    name: "这是一位名字非常长的观众 Hello 2026".into(),
                    ..Default::default()
                },
                "这是一条回复引用：中文标点，Hello 2026！需要跟随正文方向变化。",
                nanatype_model::SourceKind::Chat,
            ));
        });
        let original = ui.display.get_untracked();
        let id = DocumentId::new(1).unwrap();
        let mut doc = RuntimeDocument::new(id);
        doc.context_mut()
            .mount_view_root(id, move || dialogue_box(ui, Source::Display, None))
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, 760, 220).unwrap();
        session.flush().unwrap();
        let node = ui.text_node.get_untracked().unwrap();
        let quote_ids: Vec<_> = session
            .accessibility_dump()
            .into_iter()
            .filter(|n| {
                n.agent_path.as_deref().is_some_and(|p| {
                    p.ends_with("reply-quote-name") || p.ends_with("reply-quote-text")
                })
            })
            .map(|n| n.id)
            .collect();
        assert_eq!(quote_ids.len(), 2, "quote name and text are mounted");
        let presentation = session
            .document()
            .context()
            .world()
            .glyph_presentation(node)
            .cloned();
        for (w, h, vertical) in [
            (300, 600, true),
            (400, 400, false),
            (400, 401, true),
            (1200, 120, false),
            (300, 1200, false),
            (760, 220, true),
            (760, 220, false),
        ] {
            ui.settings
                .update(|s| Arc::make_mut(s).overlays.display.vertical_text = vertical);
            session.set_viewport(w, h, 1.0).unwrap();
            session.flush().unwrap();
            assert_eq!(ui.text_node.get_untracked(), Some(node));
            let world = session.document().context().world();
            for &id in &quote_ids {
                let quote_node = nana_ui::runtime::StableNodeId::new(id).unwrap();
                let (_, quote_layout) = world.text_layout(quote_node).unwrap();
                assert_eq!(
                    quote_layout.is_vertical(),
                    vertical,
                    "reply quote follows its display's writing mode"
                );
                if vertical {
                    let q = world.layout_box(quote_node).unwrap();
                    let port = world
                        .layout_box(ui.text_port.get_untracked().unwrap())
                        .unwrap();
                    assert!(
                        q.x >= port.x + port.width - 1.,
                        "vertical reply must sit to the right of the body: quote={q:?}, body={port:?}"
                    );
                    assert!(
                        q.y >= port.y - 1. && q.y + q.height <= port.y + port.height + 1.,
                        "reply remains inside the text viewport"
                    );
                }
            }
            assert_eq!(world.glyph_presentation(node), presentation.as_ref());
            let style = &world.node_style(node).unwrap().layout;
            assert_eq!(
                style.writing_mode,
                Some(if vertical {
                    nana_ui_core::WritingModeSpec::VerticalRl
                } else {
                    nana_ui_core::WritingModeSpec::HorizontalTb
                })
            );
            let (_, layout) = world.text_layout(node).unwrap();
            assert!(layout.lines.len() > 1);
            if vertical {
                assert!(
                    world.layout_box(node).unwrap().width + 0.5 >= layout.physical_size().0,
                    "the full column stack must own the inline-object coordinate space: body={:?}, physical={:?}, viewport={w}x{h}",
                    world.layout_box(node),
                    layout.physical_size()
                );
                let port = world
                    .layout_box(ui.text_port.get_untracked().unwrap())
                    .unwrap();
                let body = world.layout_box(node).unwrap();
                assert!(
                    port.width > w as f32 * 0.3,
                    "reply must leave space for the body: {port:?}"
                );
                assert!(
                    (body.height - port.height).abs() < 1.0,
                    "columns must wrap at the actual text viewport height"
                );
                assert!(
                    (body.x + body.width - port.x - port.width).abs() < 1.0,
                    "column stack must anchor right for {w}x{h}: body={body:?}, port={port:?}, physical={:?}",
                    layout.physical_size()
                );
                let writing = nana_ui_core::WritingContext::new(
                    nana_ui_core::WritingModeSpec::VerticalRl,
                    nana_ui_core::DirSpec::Ltr,
                );
                let physical = |i: usize| {
                    let b = layout.lines[i].bounds;
                    writing.line_rect_to_page((b.x, b.y, b.width, b.height), port.width)
                };
                assert!(physical(0).0 > physical(1).0, "columns must advance left");
                assert!(physical(0).3 > physical(0).2);
            }
            assert!(Arc::ptr_eq(&ui.display.get_untracked(), &original));
            ui.settings
                .update(|s| Arc::make_mut(s).overlays.display.size = Some([w as f32, h as f32]));
            session.flush().unwrap();
            assert_eq!(ui.text_node.get_untracked(), Some(node));
        }
    }

    #[test]
    fn following_the_typing_scrolls_a_mention_label_fully_out_of_view() {
        use nana_ui::runtime::{DocumentId, RuntimeDocument};
        use nana_ui_devtools::agent::RuntimeAgentSession;
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        let ui = Stage::new(
            Arc::new(nanatype_settings::Settings::default()),
            registry.load("nanatype.bubble").unwrap().skin,
            nanatype_i18n::Locale::ZhCn,
        );
        let text = nanatype_markup::parse("[at=1001 name=星野ほたる] 晚上好！今天也谢谢你来～").doc;
        ui.display.update(|v| {
            let v = Arc::make_mut(v);
            v.empty = false;
            v.reveal = Reveal::full(text.unit_count() as u32);
            v.doc = Arc::new(text);
            v.reply = Some(nanatype_model::ChatRef::new(
                nanatype_model::ChatUser {
                    name: "星野ほたる".into(),
                    ..Default::default()
                },
                "主播晚上好！今天打什么字呀",
                nanatype_model::SourceKind::Chat,
            ));
        });
        let id = DocumentId::new(1).unwrap();
        let mut doc = RuntimeDocument::new(id);
        doc.context_mut()
            .mount_view_root(id, move || dialogue_box(ui, Source::Display, None))
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, 300, 240).unwrap();
        session.flush().unwrap();
        let world = session.document().context().world();
        let (_, layout) = world
            .text_layout(ui.text_node.get_untracked().unwrap())
            .unwrap();
        let port = world
            .layout_box(ui.text_port.get_untracked().unwrap())
            .unwrap();
        let label = layout.objects.first().expect("the mention is laid out");
        let lines = overflow::line_spans(layout);
        let last = lines.len() - 1;
        let scroll = overflow::scroll_target(&lines, last, port.height);
        assert!(
            scroll > 0.0,
            "the line overflows the box: {lines:?}, {port:?}"
        );
        assert!(
            label.rect.y + label.rect.height <= scroll,
            "the label's bottom edge is still in view: {label:?}, scrolled to {scroll}"
        );
        assert!(lines[last].bottom - scroll <= port.height + 0.5);
    }
}

#[cfg(test)]
mod sticker_reveal_tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;

    use nana_ui::runtime::{DocumentId, RuntimeDocument};
    use nana_ui_devtools::agent::RuntimeAgentSession;
    use nana_ui_devtools::offscreen::{OffscreenSnapshots, Size};
    use nanatype_core::Reveal;
    use nanatype_model::{Entrance, EntranceKind, Inline};

    use super::super::Stage;

    const SIZE: Size<u32> = Size {
        width: 760,
        height: 220,
    };

    fn red_sticker(painter: &OffscreenSnapshots, slot: &str) -> nana_ui::HostTextureRegistry {
        let texture = painter
            .gpu
            .create_texture(&nana_ui::GpuTextureDescriptor {
                label: Some("sticker"),
                width: 64,
                height: 64,
                format: nana_ui::GpuTextureFormat::RGBA8_UNORM,
                usage: nana_ui::GpuTextureUsages::SAMPLED | nana_ui::GpuTextureUsages::COPY_DST,
            })
            .unwrap();
        painter
            .gpu
            .write_texture(
                &texture,
                nana_ui::GpuTextureRegion {
                    x: 0,
                    y: 0,
                    width: 64,
                    height: 64,
                },
                &[255u8, 0, 0, 255].repeat(64 * 64),
                256,
            )
            .unwrap();
        let registry = nana_ui::HostTextureRegistry::new();
        registry.register(
            slot.to_owned(),
            nana_ui::HostTexture::new(&texture),
            64,
            64,
            nana_ui::HostTextureAlphaMode::Premultiplied,
        );
        registry
    }

    fn sticker_seen(skin: &str, at: &[u64]) -> Vec<(f32, usize)> {
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        let ui = Stage::new(
            Arc::new(nanatype_settings::Settings::default()),
            registry.load(skin).unwrap().skin,
            nanatype_i18n::Locale::ZhCn,
        );
        let text = nanatype_markup::parse("ab[sticker=cats/hello]cd").doc;
        let Inline::Atom(atom) = &text.paragraphs[0].inlines[1] else {
            panic!("not a sticker: {text:?}");
        };
        let url = "file:///stickers/hello.webp".to_owned();
        let slot = format!("sticker-test-{skin}");
        super::super::rich::publish_animated(
            [(
                url.clone(),
                super::super::rich::AnimatedPicture {
                    slot: slot.clone(),
                    width: 64,
                    height: 64,
                },
            )]
            .into_iter()
            .collect(),
        );
        ui.asset_urls.set(Arc::new(
            [(atom.asset.to_string(), url)].into_iter().collect(),
        ));
        ui.display.update(|v| {
            let v = Arc::make_mut(v);
            v.empty = false;
            v.line = Some(nanatype_model::LineId(1));
            v.doc = Arc::new(text);
            v.entrance = Entrance {
                kind: EntranceKind::Fade,
                ms: 100,
            };
            v.reveal = Reveal {
                visible_from: 0,
                shown: 0,
                segment: Some(nanatype_core::SegmentReveal {
                    origin_ms: 1000,
                    start: 0,
                    reveal_ms: Arc::from([0, 100, 200, 300, 400]),
                }),
                unit_times: None,
                hidden_from: 5,
            };
        });
        let id = DocumentId::new(1).unwrap();
        let mut doc = RuntimeDocument::new(id);
        let clock = |doc: &mut RuntimeDocument, ms: u64| {
            let t = std::time::Duration::from_millis(ms);
            doc.context_mut().advance_animations(t);
            doc.sync_presentation_clock(t);
        };
        clock(&mut doc, 1000);
        crate::clock::capture_at(1000, || {
            doc.context_mut()
                .mount_view_root(id, move || dialogue_box(ui, Source::Display, None))
                .unwrap()
        });
        let mut session = RuntimeAgentSession::new(doc, SIZE.width, SIZE.height).unwrap();
        crate::clock::capture_at(1000, || session.flush().unwrap());
        let mut painter = OffscreenSnapshots::new().unwrap();
        let renderers = super::super::sweep::renderers();
        let textures = red_sticker(&painter, &slot);
        let seen = at
            .iter()
            .map(|&ms| {
                clock(session.document_mut(), ms);
                crate::clock::capture_at(ms, || session.flush().unwrap());
                let node = ui.text_node.get_untracked().unwrap();
                let scene = session.document().scene();
                let opacity = scene
                    .primitives()
                    .find(|p| p.node == node && format!("{:?}", p.kind).starts_with("Custom"))
                    .map(|p| p.opacity)
                    .expect("the sticker is laid out");
                let pixels = painter
                    .paint(scene, SIZE, [0.0; 4], Some(&textures), Some(&renderers))
                    .unwrap();
                let red = pixels
                    .chunks(4)
                    .filter(|p| p[0] > 120 && p[1] < 60 && p[2] < 60 && p[3] > 100)
                    .count();
                (opacity, red)
            })
            .collect();
        super::super::rich::publish_animated(HashMap::new());
        seen
    }

    #[test]
    fn a_sticker_appears_when_typing_reaches_it() {
        let seen = sticker_seen("nanatype.halo", &[1050, 1150, 1250, 1450]);
        assert_eq!(seen[0], (0.0, 0), "not reached yet: {seen:?}");
        assert_eq!(seen[1], (0.0, 0), "not reached yet: {seen:?}");
        assert!(
            seen[2].0 > 0.0 && seen[2].0 < 1.0,
            "fading in with the text: {seen:?}"
        );
        assert!(seen[3].0 == 1.0 && seen[3].1 > 0, "in: {seen:?}");
        let seen = sticker_seen("nanatype.crayon", &[1150, 1250, 1450]);
        assert_eq!(seen[0], (0.0, 0), "not reached yet: {seen:?}");
        assert!(
            seen[1].0 > 0.0 && seen[1].0 < 1.0,
            "fading in with the material: {seen:?}"
        );
        assert!(seen[2].0 == 1.0 && seen[2].1 > 0, "in: {seen:?}");
    }
}

#[cfg(test)]
mod reveal_end_tests {
    use super::*;
    use std::sync::Arc;

    use nana_ui::runtime::{DocumentId, RuntimeDocument};
    use nana_ui_devtools::agent::RuntimeAgentSession;
    use nana_ui_devtools::offscreen::{OffscreenSnapshots, Size};
    use nanatype_core::Reveal;
    use nanatype_model::{Entrance, EntranceKind};

    use super::super::Stage;

    const SIZE: Size<u32> = Size {
        width: 760,
        height: 220,
    };

    fn left_on_screen(typed: bool, reduced: bool) -> Vec<u8> {
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        let ui = Stage::new(
            Arc::new(nanatype_settings::Settings::default()),
            registry.load("nanatype.subtitle").unwrap().skin,
            nanatype_i18n::Locale::ZhCn,
        );
        ui.reduced_motion.set(reduced);
        ui.display.update(|v| {
            let v = Arc::make_mut(v);
            v.empty = false;
            v.line = Some(nanatype_model::LineId(1));
            v.doc = Arc::new(nanatype_markup::parse("字幕皮肤").doc);
            v.entrance = Entrance {
                kind: EntranceKind::Fade,
                ms: 120,
            };
            v.reveal = if typed {
                Reveal {
                    visible_from: 0,
                    shown: 0,
                    segment: Some(nanatype_core::SegmentReveal {
                        origin_ms: 1000,
                        start: 0,
                        reveal_ms: Arc::from([0, 100, 200, 300]),
                    }),
                    unit_times: None,
                    hidden_from: 4,
                }
            } else {
                Reveal::full(4)
            };
        });
        let id = DocumentId::new(1).unwrap();
        let mut doc = RuntimeDocument::new(id);
        let clock = |doc: &mut RuntimeDocument, ms: u64| {
            let t = std::time::Duration::from_millis(ms);
            doc.context_mut().advance_animations(t);
            doc.sync_presentation_clock(t);
        };
        clock(&mut doc, 1000);
        crate::clock::capture_at(1000, || {
            doc.context_mut()
                .mount_view_root(id, move || dialogue_box(ui, Source::Display, None))
                .unwrap()
        });
        let mut session = RuntimeAgentSession::new(doc, SIZE.width, SIZE.height).unwrap();
        let mut painter = OffscreenSnapshots::new().unwrap();
        let renderers = super::super::sweep::renderers();
        let mut left = Vec::new();
        for ms in (1000..=1600).step_by(16) {
            session
                .document_mut()
                .sync_presentation_clock(std::time::Duration::from_millis(ms));
            crate::clock::capture_at(ms, || session.flush().unwrap());
            let scene = session.document().scene();
            left = painter
                .paint(scene, SIZE, [0.0; 4], None, Some(&renderers))
                .unwrap();
        }
        left
    }

    fn the_typed_line_ends_as_the_whole_line(reduced: bool) {
        let white = |pixels: &[u8]| {
            pixels
                .chunks(4)
                .filter(|p| p[0] > 235 && p[1] > 235 && p[2] > 235 && p[3] > 235)
                .count()
        };
        let whole = left_on_screen(false, reduced);
        let typed = left_on_screen(true, reduced);
        assert!(white(&whole) > 0, "the line is drawn in white");
        assert_eq!(
            white(&typed),
            white(&whole),
            "reduced motion {reduced}: the last character is not fully in"
        );
        assert!(typed == whole);
    }

    #[test]
    fn a_typed_line_s_last_character_is_left_fully_drawn() {
        the_typed_line_ends_as_the_whole_line(false);
    }

    #[test]
    fn with_reduced_motion_too() {
        the_typed_line_ends_as_the_whole_line(true);
    }
}

#[cfg(test)]
mod transition_override_tests {
    use super::*;
    use std::sync::Arc;

    use nana_ui::runtime::{DocumentId, RuntimeDocument};
    use nanatype_core::Reveal;
    use nanatype_model::{Entrance, EntranceKind};

    use super::super::Stage;

    #[test]
    fn explicit_entrance_overrides_the_skin_mask_in_history_too() {
        let skin = nanatype_theme::SkinRegistry::new(None, "1.0.0")
            .load("nanatype.crayon")
            .unwrap()
            .skin;
        let mut settings = nanatype_settings::Settings::default();
        settings.playback.entrance = Some(EntranceKind::Pop);
        let ui = Stage::new(
            Arc::new(settings),
            skin.clone(),
            nanatype_i18n::Locale::ZhCn,
        );
        let base = Base::history(
            &skin,
            Entrance {
                kind: EntranceKind::Fade,
                ms: 180,
            },
            false,
        );
        let text = nanatype_markup::parse("测试").doc;
        let id = DocumentId::new(1).unwrap();
        let mut doc = RuntimeDocument::new(id);
        let root = doc
            .context_mut()
            .mount_view_root(id, move || {
                document(ui, &text, &base, Timing::Reveal(Reveal::full(2)))
            })
            .unwrap();
        let p = doc
            .context()
            .world()
            .glyph_presentation(root.roots()[0])
            .unwrap();
        assert!(p.reveal.as_ref().unwrap().intro.pop);
        assert!(p.transitions.entrance.is_none());
    }

    fn intro_of(
        markup: &str,
        setting: Option<EntranceKind>,
        base_ms: u16,
    ) -> (nana_ui_core::GlyphIntro, bool) {
        let skin = nanatype_theme::SkinRegistry::new(None, "1.0.0")
            .load("nanatype.crayon")
            .unwrap()
            .skin;
        let mut settings = nanatype_settings::Settings::default();
        settings.playback.entrance = setting;
        let ui = Stage::new(
            Arc::new(settings),
            skin.clone(),
            nanatype_i18n::Locale::ZhCn,
        );
        let base = Base::dialogue(
            &skin,
            Entrance {
                kind: if base_ms == 0 {
                    EntranceKind::None
                } else {
                    EntranceKind::Fade
                },
                ms: base_ms,
            },
            false,
        );
        let text = nanatype_markup::parse(markup).doc;
        let units = text.unit_count() as u32;
        let id = DocumentId::new(1).unwrap();
        let mut doc = RuntimeDocument::new(id);
        let root = doc
            .context_mut()
            .mount_view_root(id, move || {
                document(ui, &text, &base, Timing::Reveal(Reveal::full(units)))
            })
            .unwrap();
        let p = doc
            .context()
            .world()
            .glyph_presentation(root.roots()[0])
            .unwrap()
            .clone();
        (p.reveal.unwrap().intro, p.transitions.entrance.is_some())
    }

    #[test]
    fn entrance_tags_set_the_line_s_intro() {
        let (intro, mask) = intro_of("测试", None, 180);
        assert!(intro.fade && !intro.pop, "untagged text keeps the skin's");
        assert!(mask, "and the skin's entrance mask");

        let (intro, mask) = intro_of("[pop=90]测试[/pop]", None, 180);
        assert!(intro.pop);
        assert!((intro.duration_s - 0.09).abs() < 1e-6);
        assert!(!mask, "a tag replaces the skin's entrance mask");

        let (intro, _) = intro_of("[slide]测[/slide]试", Some(EntranceKind::Pop), 180);
        assert!(intro.rise_px > 0.0, "a tag wins over the setting");

        let (intro, _) = intro_of("[noentrance]测试[/noentrance]", None, 180);
        assert_eq!(intro.duration_s, 0.0);

        let (intro, _) = intro_of("[pop]测试[/pop]", None, 0);
        assert_eq!(intro.duration_s, 0.0, "a skipped line still shows at once");
    }
}
