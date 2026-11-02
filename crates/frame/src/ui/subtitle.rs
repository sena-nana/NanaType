//! A line's translation, drawn under it on the display and in history
//! bubbles: the text's own fonts, outline, shadow and alignment, smaller
//! and quieter, kept legible on the box. The skin's `dialogue.subtitle`
//! tunes it; a skin without one gets these defaults.

use std::sync::Arc;

use nana_ui::runtime::view::{AnyView, IntoView, widget};
use nana_ui::runtime::{NodeStyle, Stack, Text, TextHorizontalAlignment};
use nana_ui_core::LengthSpec;
use nanatype_model::Rgba8;
use nanatype_theme::{Skin, SubtitleSpec, TextAlign, TextSpec, TextStrokeSpec};

use super::quote::{QUOTE_MIN, legible};
use super::{Stage, c};
use crate::skin_theme::on;

/// The room a one-line subtitle takes under text drawn with `text`: what
/// the display keeps free for it while subtitles can show.
pub fn room(spec: &SubtitleSpec, text: &TextSpec) -> f32 {
    let size = spec.size_for(text.size);
    gap(spec.gap) + size * text.line_height.clamp(1.0, 2.0)
}

fn gap(gap: f32) -> f32 {
    gap.clamp(0.0, 64.0)
}

/// How long a subtitle takes to fade in.
pub const FADE_MS: u64 = 220;

/// The subtitle's colour: the spec's (or the text's) moved `muted` of the
/// way toward the backdrop, then back toward legibility. Over an unknown
/// backdrop (a see-through box) it stays the text colour, which the skin
/// already made readable there.
pub fn color(skin: &Skin, spec: &SubtitleSpec, text: Rgba8, backdrop: Option<Rgba8>) -> Rgba8 {
    let base = spec
        .color
        .map_or(text, |col| skin.color(col))
        .with_alpha(255);
    match backdrop {
        Some(bg) => {
            let muted = base.mix(bg, spec.muted.clamp(0.0, 1.0)).with_alpha(255);
            legible(muted, bg, QUOTE_MIN)
        }
        None => base,
    }
}

/// The subtitle's look under text drawn with `text` over `backdrop`.
pub fn style(
    ui: Stage,
    skin: &Skin,
    spec: &SubtitleSpec,
    text: &TextSpec,
    backdrop: Option<Rgba8>,
) -> NodeStyle {
    let size = spec.size_for(text.size);
    let ink = color(skin, spec, text.color, backdrop);
    let mut st = NodeStyle::default();
    let l = Arc::make_mut(&mut st.layout);
    l.paint.text_material = super::materials::enabled(ui)
        .then(|| super::materials::resolve(skin, text.material.as_deref()))
        .flatten();
    l.font_size = Some(size);
    l.font_weight = Some(skin.fonts.body.weight);
    l.font_family = text
        .font_family
        .clone()
        .or_else(|| skin.fonts.body.families.first().cloned());
    l.color = Some(c(ink));
    l.line_height = Some(nana_ui_core::LineHeightSpec::Absolute(
        size * text.line_height.clamp(1.0, 2.0),
    ));
    l.letter_spacing = Some(text.letter_spacing * size / text.size.max(1.0));
    l.overflow_wrap = Some(nana_ui_core::OverflowWrapSpec::Anywhere);
    l.width = Some(LengthSpec::Fill);
    l.min_width = Some(LengthSpec::Px(0.0));
    // The outline scales with the text; a see-through box without one gets
    // a thin contrasting edge, as the reply quote does.
    let stroke = text
        .stroke
        .as_ref()
        .map(|s| TextStrokeSpec {
            color: s.color,
            width: s.width * (size / text.size.max(1.0)).clamp(0.5, 1.0),
        })
        .or_else(|| {
            backdrop.is_none().then(|| TextStrokeSpec {
                color: on(ink),
                width: 1.5,
            })
        });
    if let Some(stroke) = stroke {
        super::surface::text_stroke(l, &stroke);
    }
    if let Some(shadow) = &text.shadow {
        super::surface::text_shadow(l, shadow);
    }
    st.text_horizontal_alignment = match text.align {
        TextAlign::Start => TextHorizontalAlignment::Start,
        TextAlign::Center => TextHorizontalAlignment::Center,
        TextAlign::End => TextHorizontalAlignment::End,
    };
    st
}

/// How far a subtitle that arrived at `at_ms` has faded in at `now`.
pub fn presence(at_ms: u64, now: u64, reduced: bool) -> f32 {
    if reduced || at_ms == 0 {
        return 1.0;
    }
    let t = (now.saturating_sub(at_ms) as f32 / FADE_MS as f32).clamp(0.0, 1.0);
    1.0 - (1.0 - t) * (1.0 - t)
}

/// `text` as a subtitle block, `gap` below what it follows.
pub fn view(text: &str, style: NodeStyle, gap: f32) -> AnyView {
    widget(
        Stack::column(0.0)
            .width(LengthSpec::Fill)
            .with_layout(move |l| {
                l.margin_top = Some(LengthSpec::Px(self::gap(gap)));
                l.flex_shrink = Some(0.0);
                l.min_width = Some(LengthSpec::Px(0.0));
            }),
    )
    .key("subtitle")
    .children(widget(Text::new(text.to_owned()).style(style)))
    .into_any()
}

/// `style` set for a subtitle column `height` tall in vertical-rl text: CJK
/// reads down the column, Latin runs sideways along it, as in the line.
pub fn column_style(mut style: NodeStyle, height: f32) -> NodeStyle {
    let l = Arc::make_mut(&mut style.layout);
    l.writing_mode = Some(nana_ui_core::WritingModeSpec::VerticalRl);
    l.text_orientation = Some(nana_ui_core::TextOrientationSpec::Mixed);
    l.height = Some(LengthSpec::Px(height));
    style
}

/// How wide `text` runs as a subtitle column, `gap` included.
pub fn column_width(
    text: &str,
    style: &NodeStyle,
    gap: f32,
    height: f32,
    locale: nanatype_i18n::Locale,
) -> f32 {
    let rich = nana_ui_core::RichText::new(text);
    super::rich::vertical_extent(&rich, style, height, locale).ceil() + self::gap(gap)
}

/// `text` as a subtitle column `width` wide (`gap` included) in a column
/// [`column_style`] set, `gap` after the columns it follows.
pub fn column_view(text: &str, mut style: NodeStyle, gap: f32, width: f32) -> AnyView {
    let gap = self::gap(gap);
    Arc::make_mut(&mut style.layout).width = Some(LengthSpec::Px((width - gap).max(1.0)));
    widget(Stack::column(0.0).with_layout(move |l| {
        l.margin_right = Some(LengthSpec::Px(gap));
        l.flex_shrink = Some(0.0);
        l.width = Some(LengthSpec::Px((width - gap).max(1.0)));
        l.height = Some(LengthSpec::Fill);
    }))
    .key("subtitle")
    .children(widget(Text::new(text.to_owned()).style(style)))
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skin_theme::contrast;

    #[test]
    fn every_built_in_skin_gets_a_smaller_legible_subtitle() {
        let registry = nanatype_theme::SkinRegistry::new(None, "1.0.0");
        let ids: Vec<String> = registry.entries().iter().map(|e| e.id.clone()).collect();
        assert!(ids.len() >= 20, "{ids:?}");
        for id in ids {
            let skin = registry.load(&id).unwrap().skin;
            let d = &skin.dialogue;
            let spec = &d.subtitle;
            assert!(spec.visible, "{id}");
            let size = spec.size_for(d.text.size);
            assert!(size < d.text.size && size >= 11.0, "{id}: {size}");
            let backdrop = super::super::quote::backdrop(d.box_.color, d.box_.frame.is_some());
            let ink = color(&skin, spec, d.text.color, backdrop);
            match backdrop {
                Some(bg) => {
                    assert!(contrast(ink, bg) >= QUOTE_MIN, "{id}: {ink:?} on {bg:?}");
                    // Muted: no stronger than the line itself.
                    assert!(
                        contrast(ink, bg) <= contrast(d.text.color.with_alpha(255), bg) + 0.01,
                        "{id}"
                    );
                }
                None => assert_eq!(ink, d.text.color.with_alpha(255), "{id}"),
            }
        }
    }

    #[test]
    fn a_skin_can_size_and_colour_its_subtitle() {
        let spec = SubtitleSpec {
            size: Some(15.0),
            muted: 0.0,
            ..SubtitleSpec::default()
        };
        assert_eq!(spec.size_for(28.0), 15.0);
        // Never larger than the line, never unreadably small.
        assert_eq!(spec.size_for(12.0), 12.0);
        assert_eq!(SubtitleSpec::default().size_for(10.0), 11.0);
        let skin = Skin::default();
        let black = Rgba8::rgb(0, 0, 0);
        let white = Rgba8::rgb(255, 255, 255);
        assert_eq!(color(&skin, &spec, black, Some(white)), black);
        let muted = color(&skin, &SubtitleSpec::default(), black, Some(white));
        assert_ne!(muted, black);
        assert!(contrast(muted, white) >= QUOTE_MIN);
    }

    #[test]
    fn it_fades_in_unless_motion_is_reduced() {
        assert_eq!(presence(1000, 1000, false), 0.0);
        assert!(presence(1000, 1000 + FADE_MS / 2, false) > 0.5);
        assert_eq!(presence(1000, 1000 + FADE_MS, false), 1.0);
        assert_eq!(presence(1000, 1000, true), 1.0);
    }
}
