//! A quoted chat message: above horizontal text, beside vertical display text.
//! History always keeps its horizontal quote. Both use the skin's `reply` spec.

use std::sync::Arc;

use nana_ui::runtime::view::{AnyView, IntoView, widget};
use nana_ui::runtime::{NodeStyle, Stack, Text};
use nana_ui_core::{AlignSpec, LengthSpec};
use nanatype_model::{ChatRef, Rgba8};
use nanatype_theme::{ReplySpec, Skin, TextSpec, TextStrokeSpec};

use super::{Ui, c};
use crate::skin_theme::{contrast, on};

/// What the quote is drawn over, when that is one known colour: the
/// dialogue box's fill on the display, the history panel's in history.
/// Fills under half opacity, or painted by an image, are not known (the
/// stream shows through), and leave the skin's colours as they are.
pub fn backdrop(fill: Rgba8, framed: bool) -> Option<Rgba8> {
    (!framed && fill.a >= 128).then_some(fill.with_alpha(255))
}

fn mix(a: Rgba8, b: Rgba8, t: f32) -> Rgba8 {
    let m = |x: u8, y: u8| (f32::from(x) * (1.0 - t) + f32::from(y) * t).round() as u8;
    Rgba8::rgb(m(a.r, b.r), m(a.g, b.g), m(a.b, b.b))
}

/// The smallest contrast a quote's name or text keeps on its backdrop.
pub const QUOTE_MIN: f32 = 4.5;

/// The contrast a quote's colours are picked for against the backdrop's
/// one known colour. Higher than [`QUOTE_MIN`]: boxes with a gradient or a
/// glow are lighter or darker than their fill where the quote sits (rpg's
/// box renders a name picked for 7:1 at 5.7:1), and the quote's text is
/// small (62% of the line's) and thin.
pub const QUOTE_PICK: f32 = 7.0;

/// `fg` over `bg`, moved toward the ink that reads best on `bg` just far
/// enough to reach `target`:1.
pub fn legible(fg: Rgba8, bg: Rgba8, target: f32) -> Rgba8 {
    let ink = on(bg);
    (0..=20)
        .map(|i| mix(fg.with_alpha(255), ink, i as f32 / 20.0))
        .find(|c| contrast(*c, bg) >= target)
        .unwrap_or(ink)
}

/// The quote's name and text colours: the accent and `text_color` (or the
/// spec's colour), kept legible on `backdrop` when it is known. Over an
/// unknown backdrop the name takes the text colour too, which the skin
/// already made readable where its lines are drawn; the accent stays on the
/// quote's bar. Both are drawn at full opacity: the spec's `opacity` quiets
/// only the bar and the avatar, since fading the text over the backdrop is
/// what left it pale.
pub fn colors(
    skin: &Skin,
    spec: &ReplySpec,
    text_color: Rgba8,
    backdrop: Option<Rgba8>,
) -> (Rgba8, Rgba8) {
    let text = spec.color.map_or(text_color, |col| skin.color(col));
    let name = skin.chrome.accent;
    match backdrop {
        Some(bg) => (legible(name, bg, QUOTE_PICK), legible(text, bg, QUOTE_PICK)),
        _ => (text, text),
    }
}

/// `reply` as a quote line, retaining the text's fonts, outline and shadow.
/// Transparent overlays need a contrasting outline when no skin outline exists.
pub fn quote_view(
    ui: Ui,
    skin: &Skin,
    spec: &ReplySpec,
    reply: &ChatRef,
    text: &TextSpec,
    backdrop: Option<Rgba8>,
) -> AnyView {
    quote_view_on(ui, skin, spec, reply, text, backdrop, false)
}

/// Display quotes follow their own window; history quotes always stay horizontal.
pub fn display_quote_view(
    ui: Ui,
    skin: &Skin,
    spec: &ReplySpec,
    reply: &ChatRef,
    text: &TextSpec,
    backdrop: Option<Rgba8>,
) -> AnyView {
    quote_view_on(ui, skin, spec, reply, text, backdrop, true)
}

fn quote_size(spec: &ReplySpec, text: &TextSpec) -> f32 {
    spec.size
        .filter(|s| s.is_finite() && *s > 0.)
        .unwrap_or((text.size * 0.62).max(10.))
}

/// Includes the separator and the gap to the body, but never stretches it.
pub fn column_width(spec: &ReplySpec, text: &TextSpec) -> f32 {
    let size = quote_size(spec, text);
    (size * 1.4).round() + (size * 0.5).round().max(6.) + 9.
}

fn quote_view_on(
    ui: Ui,
    skin: &Skin,
    spec: &ReplySpec,
    reply: &ChatRef,
    text: &TextSpec,
    backdrop: Option<Rgba8>,
    responsive: bool,
) -> AnyView {
    let vertical = move || {
        responsive
            && ui
                .skin
                .with(|s| nanatype_theme::dialogue_is_vertical(s, ui.display_size.get()))
    };
    let size = quote_size(spec, text);
    let name_height = (reply.user.name.chars().count() + 1) as f32 * size;
    let (name_color, color) = colors(skin, spec, text.color, backdrop);
    let style = |color: Rgba8, weight: u16, name: bool| {
        let mut st = NodeStyle::default();
        let l = Arc::make_mut(&mut st.layout);
        l.paint.text_material = super::materials::enabled(ui)
            .then(|| super::materials::resolve(skin, text.material.as_deref()))
            .flatten();
        l.font_size = Some(size);
        l.font_weight = Some(weight);
        l.font_family = if name {
            &skin.fonts.name
        } else {
            &skin.fonts.body
        }
        .families
        .first()
        .cloned();
        l.color = Some(c(color));
        l.white_space_nowrap = true;
        let stroke = text
            .stroke
            .as_ref()
            .map(|s| TextStrokeSpec {
                color: s.color,
                width: s.width * (size / text.size.max(1.0)).clamp(0.5, 1.0),
            })
            .or_else(|| {
                backdrop.is_none().then(|| TextStrokeSpec {
                    color: on(color),
                    width: 1.5,
                })
            });
        if let Some(stroke) = stroke {
            super::surface::text_stroke(l, &stroke);
        }
        if let Some(shadow) = &text.shadow {
            super::surface::text_shadow(l, shadow);
        }
        st
    };
    let opacity = spec.opacity.clamp(0.0, 1.0);
    let avatar = widget(Stack::row(0.0).with_layout(move |l| {
        l.opacity = Some(opacity);
        l.flex_shrink = Some(0.0);
    }))
    .children(super::danmaku::asset_avatar(
        ui,
        reply.user.avatar.as_ref().map(ToString::to_string),
        &reply.user.name,
        (size * 1.4).round(),
        c(skin.chrome.accent),
    ));
    let mut bar = c(skin.chrome.accent);
    bar[3] *= opacity;
    widget(
        Stack::row((size * 0.4).round())
            .align(AlignSpec::Center)
            .width(LengthSpec::Fill)
            .with_layout(move |l| {
                l.margin_bottom = Some(LengthSpec::Px(4.0));
                l.flex_shrink = Some(0.0);
                l.min_width = Some(LengthSpec::Px(0.0));
                // A 3px accent rule down the left marks the line as a quote.
                l.border_top_width = Some(0.0);
                l.border_right_width = Some(0.0);
                l.border_bottom_width = Some(0.0);
                l.border_left_width = Some(3.0);
                l.border_color = Some(bar);
                l.border_style = Some(nana_ui_core::BorderStyle::Solid);
                l.padding_left = Some(LengthSpec::Px((size * 0.5).round().max(6.0)));
            }),
    )
    .key("reply-quote")
    .bind(move |s: &mut Stack| {
        let v = vertical();
        super::restyle(s, |l| {
            l.direction = Some(if v {
                nana_ui_core::FlexDirection::Column
            } else {
                nana_ui_core::FlexDirection::Row
            });
            l.width = Some(if v {
                LengthSpec::Px((size * 1.4).round() + (size * 0.5).round().max(6.) + 3.)
            } else {
                LengthSpec::Fill
            });
            l.height = v.then_some(LengthSpec::Fill);
            l.min_height = Some(LengthSpec::Px(0.));
            l.margin_bottom = Some(LengthSpec::Px(if v { 0. } else { 4. }));
            l.margin_left = Some(LengthSpec::Px(if v { 6. } else { 0. }));
            l.border_left_width = Some(if v { 0. } else { 3. });
            l.border_right_width = Some(if v { 3. } else { 0. });
            l.padding_left = Some(LengthSpec::Px(if v {
                0.
            } else {
                (size * 0.5).round().max(6.)
            }));
            l.padding_right = Some(LengthSpec::Px(if v {
                (size * 0.5).round().max(6.)
            } else {
                0.
            }));
            l.overflow_x = if v {
                nana_ui_core::OverflowSpec::Hidden
            } else {
                nana_ui_core::OverflowSpec::Visible
            };
            l.overflow_y = if v {
                nana_ui_core::OverflowSpec::Hidden
            } else {
                nana_ui_core::OverflowSpec::Visible
            };
        });
    })
    .children((
        avatar,
        widget(
            Text::new(format!("{}：", reply.user.name))
                .style({
                    let mut st = style(name_color, 700, true);
                    let l = Arc::make_mut(&mut st.layout);
                    l.flex_shrink = Some(1.0);
                    l.min_width = Some(LengthSpec::Px(0.0));
                    l.max_width = Some(LengthSpec::Percent(40.0));
                    st
                })
                .truncating(),
        )
        .key("reply-quote-name")
        .bind(move |t: &mut Text| {
            let v = vertical();
            let l = Arc::make_mut(&mut t.style.layout);
            l.writing_mode = Some(if v {
                nana_ui_core::WritingModeSpec::VerticalRl
            } else {
                nana_ui_core::WritingModeSpec::HorizontalTb
            });
            l.text_orientation = Some(nana_ui_core::TextOrientationSpec::Mixed);
            l.width = v.then_some(LengthSpec::Px((size * 1.4).round()));
            l.height = v.then_some(LengthSpec::Min2(
                nana_ui_core::LengthAtom::Px(name_height),
                nana_ui_core::LengthAtom::Percent(30.),
            ));
            l.max_width = (!v).then_some(LengthSpec::Percent(40.));
            l.max_height = v.then_some(LengthSpec::Px((ui.display_size.get()[1] * 0.25).max(size)));
        }),
        widget(Stack::row(0.0).width(LengthSpec::Fill).with_layout(|l| {
            l.flex_shrink = Some(1.0);
            l.min_width = Some(LengthSpec::Px(0.0));
            l.overflow_x = nana_ui_core::OverflowSpec::Hidden;
        }))
        .bind(move |s: &mut Stack| {
            let v = vertical();
            super::restyle(s, |l| {
                l.width = Some(if v {
                    LengthSpec::Px((size * 1.4).round())
                } else {
                    LengthSpec::Fill
                });
                l.height = v.then_some(LengthSpec::Fill);
                l.flex_grow = Some(if v { 1. } else { 0. });
                l.flex_basis = v.then_some(LengthSpec::Px(0.));
                l.min_height = Some(LengthSpec::Px(0.));
                l.overflow_y = if v {
                    nana_ui_core::OverflowSpec::Hidden
                } else {
                    nana_ui_core::OverflowSpec::Visible
                };
            });
        })
        .children(
            widget(
                Text::new(reply.text.clone())
                    .style({
                        let mut st = style(color, 400, false);
                        let l = Arc::make_mut(&mut st.layout);
                        l.width = Some(LengthSpec::Fill);
                        l.min_width = Some(LengthSpec::Px(0.0));
                        l.max_width = Some(LengthSpec::Percent(100.0));
                        st
                    })
                    .truncating(),
            )
            .key("reply-quote-text")
            .bind(move |t: &mut Text| {
                let v = vertical();
                let l = Arc::make_mut(&mut t.style.layout);
                l.writing_mode = Some(if v {
                    nana_ui_core::WritingModeSpec::VerticalRl
                } else {
                    nana_ui_core::WritingModeSpec::HorizontalTb
                });
                l.text_orientation = Some(nana_ui_core::TextOrientationSpec::Mixed);
                l.height = v.then_some(LengthSpec::Fill);
                l.min_height = Some(LengthSpec::Px(0.));
            }),
        ),
    ))
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_peer_and_history_quotes_keep_independent_directions() {
        use nana_ui::runtime::{DocumentId, RuntimeDocument, StableNodeId};
        use nana_ui_devtools::agent::RuntimeAgentSession;
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        let skin = registry.load("nanatype.halo").unwrap().skin;
        let ui = Ui::new(
            Arc::new(Default::default()),
            skin,
            nanatype_i18n::Locale::ZhCn,
        );
        ui.display_size.set([300., 1200.]);
        ui.remote().display_size.set([1200., 300.]);
        let id = DocumentId::new(1).unwrap();
        let mut document = RuntimeDocument::new(id);
        document
            .context_mut()
            .mount_view_root(id, move || {
                let skin = ui.skin.get_untracked();
                let quote = |label: &str| {
                    ChatRef::new(
                        nanatype_model::ChatUser {
                            name: "很长的观众名字 Hello 2026".into(),
                            ..Default::default()
                        },
                        label,
                        nanatype_model::SourceKind::Chat,
                    )
                };
                widget(
                    Stack::row(8.)
                        .width(LengthSpec::Fill)
                        .height(LengthSpec::Fill),
                )
                .children((
                    widget(Stack::fill_column(0.))
                        .key("local")
                        .children(display_quote_view(
                            ui,
                            &skin,
                            &skin.dialogue.reply,
                            &quote("本地回复"),
                            &skin.dialogue.text,
                            None,
                        )),
                    widget(Stack::fill_column(0.))
                        .key("peer")
                        .children(display_quote_view(
                            ui.remote(),
                            &skin,
                            &skin.dialogue.reply,
                            &quote("对方回复"),
                            &skin.dialogue.text,
                            None,
                        )),
                    widget(Stack::fill_column(0.))
                        .key("history")
                        .children(quote_view(
                            ui,
                            &skin,
                            &skin.history.reply,
                            &quote("历史引用"),
                            &skin.history.text,
                            None,
                        )),
                ))
            })
            .unwrap();
        let mut session = RuntimeAgentSession::new(document, 900, 500).unwrap();
        session.flush().unwrap();
        let nodes: Vec<_> = ["本地回复", "对方回复", "历史引用"]
            .iter()
            .map(|label| {
                let node = session
                    .accessibility_dump()
                    .into_iter()
                    .find(|n| n.label.as_deref() == Some(*label))
                    .unwrap();
                StableNodeId::new(node.id).unwrap()
            })
            .collect();
        for local_vertical in [true, false, true, false] {
            ui.display_size.set(if local_vertical {
                [300., 1200.]
            } else {
                [1200., 300.]
            });
            ui.remote().display_size.set(if local_vertical {
                [1200., 300.]
            } else {
                [300., 1200.]
            });
            session.flush().unwrap();
            let world = session.document().context().world();
            for (&node, expected) in nodes.iter().zip([local_vertical, !local_vertical, false]) {
                assert_eq!(world.text_layout(node).unwrap().1.is_vertical(), expected);
            }
        }
    }

    #[test]
    fn quotes_read_on_every_builtin_box() {
        let reg = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        let mut bad = Vec::new();
        for e in reg.entries() {
            let skin = reg.load(&e.id).unwrap().skin;
            let (d, h) = (&skin.dialogue, &skin.history);
            let places = [
                (
                    "display",
                    backdrop(d.box_.color, d.box_.frame.is_some()),
                    &d.reply,
                    d.text.color,
                ),
                (
                    "history",
                    h.panel.as_ref().and_then(|p| backdrop(p.color, false)),
                    &h.reply,
                    h.text.color,
                ),
            ];
            for (place, bg, spec, text_color) in places {
                let Some(bg) = bg else { continue };
                let (name, text) = colors(&skin, spec, text_color, Some(bg));
                for (what, col) in [("name", name), ("text", text)] {
                    let r = contrast(col, bg);
                    if r < QUOTE_MIN {
                        bad.push(format!("{} {place} {what} {r:.2}", e.id));
                    }
                }
            }
        }
        assert!(bad.is_empty(), "{bad:?}");
    }
}
