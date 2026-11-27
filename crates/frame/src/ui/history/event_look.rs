use std::sync::Arc;

use nana_ui::runtime::view::{AnyView, IntoView, widget};
use nana_ui::runtime::{NodeStyle, Stack, Text};
use nanatype_model::{Document, EventKind, HistoryItem, Inline, Rgba8, SourceKind};
use nanatype_theme::{EventPriceColor, HistoryEventLayout, Skin};

use super::super::c;
use super::super::surface::{self, Surface};

/// A gift, paid message or membership as the skin dresses it, every
/// fallback to the message row already applied.
#[derive(Clone)]
pub(super) struct EventLook {
    pub layout: HistoryEventLayout,
    pub style: nanatype_theme::HistoryEventStyle,
    /// The colour under the text; the bubble's when the row has no own fill.
    pub fill: Rgba8,
    pub text: Rgba8,
    pub name: Rgba8,
    pub icon: Rgba8,
    pub opacity: f32,
    pub scale: f32,
    pub pad: [f32; 2],
    /// The platform's colour for the amount, where the skin wants it.
    pub price: Option<(EventPriceColor, Rgba8)>,
    /// The amount in its pill: text, fill, lettering.
    pub badge: Option<(String, Rgba8, Rgba8)>,
}

impl EventLook {
    pub(super) fn of(skin: &Skin, item: &HistoryItem) -> Option<Self> {
        let h = &skin.history;
        if item.kind != SourceKind::Event {
            return None;
        }
        let info = item.event.as_ref();
        let style = h
            .event
            .as_ref()?
            .resolve(info.map_or(EventKind::Notice, |e| e.kind));
        let placement = style.price_color.unwrap_or_default();
        let price = info
            .and_then(|e| e.color)
            .filter(|_| placement != EventPriceColor::None)
            .map(|rgb| Rgba8::rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8));
        let filled = price.filter(|_| placement == EventPriceColor::Fill);
        let fill = filled.unwrap_or(style.color.map_or(h.bubble_color, |col| skin.color(col)));
        // Only a fill that hides what is behind it can be measured against.
        let opaque = fill.a >= 128;
        let readable = |wanted: Rgba8, least: f32, otherwise: Rgba8| {
            if opaque && crate::skin_theme::contrast(wanted, fill) < least {
                otherwise
            } else {
                wanted
            }
        };
        let pick = |col: Option<nanatype_theme::ColorRef>, default: Rgba8| {
            col.map_or(default, |col| skin.color(col))
        };
        let text = readable(
            pick(style.text_color, h.text.color),
            4.5,
            crate::skin_theme::on(fill),
        );
        // The skin chose its name and icon colours for its own fill, not
        // for whichever colour the platform hands over.
        let on_fill = |wanted: Rgba8| match filled {
            Some(_) => text,
            None => readable(wanted, 3.0, text),
        };
        let name = on_fill(pick(style.name_color, h.name_color));
        let icon = on_fill(pick(style.icon_color, name));
        let badge = info
            .and_then(|e| e.amount.clone())
            .filter(|_| style.badge.is_some() || placement == EventPriceColor::Badge)
            .map(|amount| {
                let spec = style.badge.clone().unwrap_or_default();
                let (plate, lettering) = match price {
                    Some(price) if placement == EventPriceColor::Badge => {
                        (price, crate::skin_theme::on(price))
                    }
                    // On the platform's colour the pill takes the text's.
                    Some(_) if filled.is_some() => (text, fill),
                    _ => {
                        let plate = skin.color(spec.color);
                        let lettering = skin.color(spec.text_color);
                        if crate::skin_theme::contrast(lettering, plate) < 4.5 {
                            (plate, crate::skin_theme::on(plate))
                        } else {
                            (plate, lettering)
                        }
                    }
                };
                (amount, plate, lettering)
            });
        Some(Self {
            layout: style.layout.unwrap_or_default(),
            fill,
            text,
            name,
            icon,
            opacity: style
                .opacity
                .unwrap_or(if style.has_surface() || filled.is_some() {
                    1.0
                } else {
                    h.chat.bubble_opacity
                })
                .clamp(0.0, 1.0),
            scale: style.text_scale.unwrap_or(h.chat.text_scale),
            pad: style.padding.unwrap_or(h.bubble_padding),
            price: price.map(|price| (placement, price)),
            badge,
            style,
        })
    }

    /// The row's surface, the platform's colour laid where the skin put it.
    pub(super) fn surface(&self, skin: &Skin, frame_url: Option<String>) -> Surface {
        let mut surface = Surface::event(skin, &self.style, frame_url);
        match self.price {
            Some((EventPriceColor::Fill, price)) => {
                if surface.frame.take().is_some() {
                    surface.radii = [skin.history.bubble_radius; 4];
                }
                surface.fill = Some(c(price));
                surface.gradient = None;
                surface.material = None;
            }
            Some((EventPriceColor::Border, price)) => {
                surface.frame = None;
                surface.border_material = None;
                match &mut surface.border {
                    Some(border) => border.color = c(price),
                    None => {
                        surface.border = Some(surface::Border {
                            widths: [2.0; 4],
                            color: c(price),
                            style: Default::default(),
                        });
                    }
                }
            }
            _ => {}
        }
        surface
    }

    /// The row's text as this look shows it: without the amount the badge
    /// already carries.
    pub(super) fn dress(&self, doc: &Document) -> Document {
        let mut inlines: Vec<Inline> = doc
            .paragraphs
            .iter()
            .flat_map(|p| p.inlines.iter().cloned())
            .collect();
        if let Some((amount, ..)) = &self.badge
            && matches!(inlines.first(), Some(Inline::Text(run)) if run.text.trim_end() == amount)
        {
            inlines.remove(0);
        }
        if inlines.is_empty() {
            Document::new()
        } else {
            Document::from_inlines(inlines)
        }
    }

    pub(super) fn icon_view(&self, skin: &Skin, size: f32) -> Option<AnyView> {
        let glyph = self.style.icon.clone().filter(|g| !g.is_empty())?;
        let mut style = NodeStyle::default();
        let l = Arc::make_mut(&mut style.layout);
        l.font_size = Some(size);
        l.text_ext.font_family = skin.fonts.accent.families.first().cloned();
        l.paint_colors.color = Some(nana_ui_core::PaintColor::srgb(c(self.icon)));
        l.white_space = nana_ui_core::WhiteSpaceSpec::Nowrap;
        l.flex_shrink = Some(0.0);
        Some(widget(Text::new(glyph).decorative().style(style)).into_any())
    }

    pub(super) fn badge_view(&self, skin: &Skin, size: f32) -> Option<AnyView> {
        let (amount, plate, lettering) = self.badge.clone()?;
        let spec = self.style.badge.clone().unwrap_or_default();
        let mut style = NodeStyle::default();
        let l = Arc::make_mut(&mut style.layout);
        l.font_size = Some(size);
        l.font_weight = Some(700);
        l.text_ext.font_family = skin.fonts.name.families.first().cloned();
        l.paint_colors.color = Some(nana_ui_core::PaintColor::srgb(c(lettering)));
        l.white_space = nana_ui_core::WhiteSpaceSpec::Nowrap;
        super::super::tnum(l);
        let border = spec
            .border_color
            .filter(|_| spec.border_width > 0.0 && self.price.is_none())
            .map(|col| (spec.border_width, c(skin.color(col))));
        Some(
            widget(
                Stack::row(0.0)
                    .padding_xy((size * 0.6).round(), 1.0)
                    .with_layout(move |l| {
                        l.paint_colors.background = Some(nana_ui_core::PaintColor::srgb(c(plate)));
                        l.border_radius = Some(spec.radius);
                        l.flex_shrink = Some(0.0);
                        if let Some((width, color)) = border {
                            l.border_width = Some(width);
                            l.paint_colors.border = Some(nana_ui_core::PaintColor::srgb(color));
                            l.border_style = Some(nana_ui_core::BorderStyle::Solid);
                        }
                    }),
            )
            .children(widget(Text::new(amount).style(style)))
            .into_any(),
        )
    }
}
