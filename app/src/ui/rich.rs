//! NanaType documents ⇄ NanaUI rich text.
//!
//! The dialogue box, the history bubbles and the editor are all NanaUI rich
//! text nodes. A [`Document`] becomes one `RichText`: runs become spans,
//! ruby groups become annotations, stickers become inline image objects,
//! mentions of chat viewers become styled labels with their avatar, and (in
//! the editor only) markers become labelled chips in the chrome's colours. Effects and pacing
//! have no rich-text field of their own, so each distinct combination is
//! interned in a table and the span carries its index as `effect`; the same
//! table yields the glyph effects the display plays. Converting back
//! ([`from_rich`]) restores the exact document the editor holds.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use nana_ui_core::{
    GlyphEffect, GlyphEffectChain, GlyphIntro, PaintColor, RevealSchedule, RichLabelStyle,
    RichObject, RichObjectContent, RichSpanStyle, RichText, RichTextShadow, RichTextStroke,
    TextDecorationLine, TextStrokePlacement,
};
use nanatype_core::Reveal;
use nanatype_i18n::Locale;
use nanatype_model::{
    ContinuousKind, Document, Effects, Entrance, EntranceKind, FontSlot, Inline, Marker, Pace,
    Paragraph, Rgba8, RubyGroup, Shadow, Size, Stroke, Style, TextRun,
};
use nanatype_theme::EffectDefaults;
use unicode_segmentation::UnicodeSegmentation;

/// How a document is drawn: the node's base text look.
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    pub size: f32,
    pub color: Rgba8,
    pub weight: u16,
    pub line_height: f32,
    pub body: Option<String>,
    pub name: Option<String>,
    pub accent: Option<String>,
    pub latin: Option<(String, u16)>,
    pub mention: MentionLook,
    /// The box the text is drawn on, for the display's skin-aware text
    /// effects. `None` in the editor, whose spans must convert back exactly.
    pub ink: Option<BoxInk>,
}

impl Look {
    /// Resolve the authored font identically for toolbar edits and rendering.
    pub fn font_style(&self, font: &FontSlot) -> RichSpanStyle {
        let (family, alias) = match font {
            FontSlot::Body => (self.body.as_ref(), Some("body")),
            FontSlot::Name => (self.name.as_ref(), Some("name")),
            FontSlot::Accent => (self.accent.as_ref(), Some("accent")),
            FontSlot::Family(f) => (Some(f), None),
        };
        let style = RichSpanStyle::new().family(family.cloned().unwrap_or_default());
        match alias {
            Some(alias) => style.family_alias(alias),
            None => style,
        }
    }
}

/// What a run's own colour, glow and the skin's text shadow are measured
/// against on the display: the box colour and the skin's inks.
#[derive(Debug, Clone, PartialEq)]
pub struct BoxInk {
    /// The box fill (opaque enough to judge contrast against).
    pub fill: Rgba8,
    /// `effects.contrast_guard`: runs under this ratio against `fill` get a
    /// 1.5px outline. `0` = off.
    pub guard: f32,
    /// The skin's darkest and lightest ink, for that outline.
    pub dark: Rgba8,
    pub light: Rgba8,
    /// The skin's base text shadow, which a run with its own colour takes in
    /// that colour instead when the shadow is a tinted glow.
    pub shadow: Option<nanatype_theme::TextShadowSpec>,
}

/// Width of the contrast guard's outline (logical px).
pub const GUARD_STROKE: f32 = 1.5;
/// A glow on a light box: at most this alpha (55%) …
const LIGHT_GLOW_ALPHA: u8 = 140;
/// … and at least this blur, so it reads as light, not a smudge.
const LIGHT_GLOW_BLUR: f32 = 10.0;

impl BoxInk {
    /// For text on a box filled `fill` in `skin`; `None` when the fill is too
    /// transparent to judge contrast against.
    pub fn new(
        skin: &nanatype_theme::Skin,
        fill: Rgba8,
        base: &nanatype_theme::TextSpec,
    ) -> Option<Self> {
        if fill.a < 128 {
            return None;
        }
        let inks = [
            base.color,
            skin.dialogue.text.color,
            skin.chrome.text,
            skin.dialogue.box_.border_color,
        ]
        .map(|c| c.with_alpha(255));
        let l = |c: &Rgba8| crate::skin_theme::lightness(*c);
        let dark = inks.iter().min_by(|a, b| l(a).total_cmp(&l(b)));
        let light = inks.iter().max_by(|a, b| l(a).total_cmp(&l(b)));
        Some(Self {
            fill: fill.with_alpha(255),
            guard: skin.effects.contrast_guard.max(0.0),
            dark: dark.copied().unwrap_or(Rgba8::BLACK),
            light: light.copied().unwrap_or(Rgba8::WHITE),
            shadow: base.shadow.clone(),
        })
    }

    fn light_box(&self) -> bool {
        crate::skin_theme::lightness(self.fill) > 60.0
    }

    /// The 1.5px outline a run in `color` needs to read on the box, if any:
    /// whichever ink stands further from the run.
    pub fn guard_stroke(&self, color: Rgba8) -> Option<Stroke> {
        use crate::skin_theme::contrast;
        if self.guard <= 0.0 || color.a < 64 {
            return None;
        }
        let color = color.with_alpha(255);
        if contrast(color, self.fill) >= self.guard {
            return None;
        }
        let ink = if contrast(self.dark, color) >= contrast(self.light, color) {
            self.dark
        } else {
            self.light
        };
        Some(Stroke {
            color: ink,
            width_px: GUARD_STROKE,
        })
    }

    /// A run's own shadow as drawn: a glow (no offset) on a light box keeps
    /// its hue but at most 55% alpha and at least 10px blur.
    pub fn shadow(&self, s: Shadow) -> Shadow {
        if s.dx == 0.0 && s.dy == 0.0 && s.blur > 0.0 && self.light_box() {
            return Shadow {
                color: s.color.with_alpha(s.color.a.min(LIGHT_GLOW_ALPHA)),
                blur: s.blur.max(LIGHT_GLOW_BLUR),
                ..s
            };
        }
        s
    }

    /// The skin's text shadow under a run of its own `color`: a tinted glow
    /// (terminal's green, night's blue) takes the run's colour at the same
    /// alpha; a neutral drop shadow stays as it is (`None`).
    pub fn base_shadow_for(&self, color: Rgba8) -> Option<Shadow> {
        let s = self.shadow.as_ref()?;
        if crate::skin_theme::chroma(s.color) < 0.15 {
            return None;
        }
        Some(Shadow {
            color: color.with_alpha(s.color.a),
            dx: s.dx,
            dy: s.dy,
            blur: s.blur,
        })
    }
}

/// How a mentioned chat viewer is drawn: the skin's `[mention]`. It also
/// carries the editor's marker chips, the other inline labels a skin colours.
#[derive(Debug, Clone, PartialEq)]
pub struct MentionLook {
    pub fill: [f32; 4],
    pub text: [f32; 4],
    pub scale: f32,
    pub avatar: bool,
    pub weight: u16,
    pub markers: MarkerLook,
}

/// How the editor draws markers: chips in the skin's chrome colours (an
/// accent wash with readable ink), each kind told apart by a glyph and a
/// coloured stripe rather than by its fill alone.
#[derive(Debug, Clone, PartialEq)]
pub struct MarkerLook {
    pub fill: Rgba8,
    pub text: Rgba8,
}

impl MarkerLook {
    pub fn from_skin(skin: &nanatype_theme::Skin) -> Self {
        let c = &skin.chrome;
        let fill = mix(c.surface.with_alpha(255), c.accent.with_alpha(255), 0.16);
        Self {
            fill,
            text: crate::skin_theme::on(fill),
        }
    }

    /// A marker (or a missing picture) as a chip: the label after its kind's
    /// glyph and stripe.
    pub fn object(&self, id: u64, label: impl Into<Arc<str>>, kind: ChipKind) -> RichObject {
        RichObject::label(
            id,
            label,
            RichLabelStyle {
                fill: crate::skin_theme::rgba(self.fill),
                text: crate::skin_theme::rgba(self.text),
                scale: 0.62,
                weight: Some(600),
                radius_px: Some(4.0),
                icon: Some(kind.icon(self.text)),
                icon_round: false,
            },
        )
    }
}

fn mix(a: Rgba8, b: Rgba8, t: f32) -> Rgba8 {
    let m = |x: u8, y: u8| (f32::from(x) * (1.0 - t) + f32::from(y) * t).round() as u8;
    Rgba8::rgba(m(a.r, b.r), m(a.g, b.g), m(a.b, b.b), m(a.a, b.a))
}

/// What a marker chip stands for: its glyph and the hue of its stripe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChipKind {
    /// `[wait]`: a clock.
    Wait,
    /// `[pause]`: wait for continue, a play mark.
    Continue,
    /// `[page]`: a page break.
    Page,
    /// `[se]`: a note.
    Sound,
    /// `[expr]`, portraits and VTube Studio: a face.
    Expression,
    /// `[screenshake]`: a zigzag.
    Shake,
    /// A sticker or picture whose file is missing.
    Picture,
    /// `[at]`: a mention.
    Mention,
}

impl ChipKind {
    pub fn of(m: &Marker) -> Self {
        match m {
            Marker::Wait { .. } => Self::Wait,
            Marker::Pause { .. } => Self::Continue,
            Marker::Page => Self::Page,
            Marker::Sound { .. } => Self::Sound,
            Marker::Expression { .. } | Marker::Portrait { .. } | Marker::Vts(_) => {
                Self::Expression
            }
            Marker::ScreenShake { .. } => Self::Shake,
        }
    }

    /// The stripe's hue: pacing orange, sounds green, faces purple, shake
    /// pink, pictures slate, mentions blue.
    pub fn hue(self) -> Rgba8 {
        match self {
            Self::Wait | Self::Continue | Self::Page => Rgba8::rgb(237, 140, 51),
            Self::Sound => Rgba8::rgb(56, 168, 140),
            Self::Expression => Rgba8::rgb(140, 102, 217),
            Self::Shake => Rgba8::rgb(224, 97, 140),
            Self::Picture => Rgba8::rgb(115, 128, 148),
            Self::Mention => Rgba8::rgb(61, 133, 235),
        }
    }

    /// The glyph, as 24-unit SVG strokes (Tabler's grid).
    fn glyph(self) -> &'static str {
        match self {
            Self::Wait => r#"<circle cx="12" cy="12" r="8.5"/><path d="M12 7.5V12l3 2"/>"#,
            Self::Continue => r#"<path d="M8 5.5v13l10-6.5z" fill="INK"/>"#,
            Self::Page => r#"<path d="M5 4.5h14M5 19.5h14M12 8v8M9 13l3 3 3-3"/>"#,
            Self::Sound => {
                r#"<circle cx="7.5" cy="17" r="2.5"/><circle cx="17" cy="15" r="2.5"/><path d="M10 17V6l9.5-2v11"/>"#
            }
            Self::Expression => {
                r#"<circle cx="12" cy="12" r="8.5"/><path d="M9 10h.01M15 10h.01M9 14.5a4 4 0 0 0 6 0"/>"#
            }
            Self::Shake => r#"<path d="M3 12l3-5 4 10 4-10 4 10 3-5"/>"#,
            Self::Picture => {
                r#"<rect x="4" y="4" width="16" height="16" rx="2"/><path d="M4 16l5-5 5 5M14 14l2-2 4 4"/>"#
            }
            Self::Mention => {
                r#"<circle cx="12" cy="12" r="3.5"/><path d="M15.5 12v1.5a2.5 2.5 0 0 0 5 0V12a8.5 8.5 0 1 0-3.5 6.9"/>"#
            }
        }
    }

    /// The chip's picture: the hue as a stripe down its left edge and the
    /// glyph in `ink` beside it, as an SVG data URL.
    pub fn icon(self, ink: Rgba8) -> Arc<str> {
        type IconCache = Mutex<HashMap<(ChipKind, Rgba8), Arc<str>>>;
        static CACHE: LazyLock<IconCache> = LazyLock::new(Default::default);
        let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        Arc::clone(cache.entry((self, ink)).or_insert_with(|| {
            let rgb = |c: Rgba8| format!("rgb({},{},{})", c.r, c.g, c.b);
            let (ink, hue) = (rgb(ink), rgb(self.hue()));
            let glyph = self.glyph().replace("INK", &ink);
            Arc::from(format!(
                "data:image/svg+xml,<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"48\" height=\"48\" viewBox=\"0 0 24 24\">\
                 <rect x=\"0\" y=\"0\" width=\"3\" height=\"24\" rx=\"1\" fill=\"{hue}\"/>\
                 <g transform=\"translate(6.5 3) scale(0.75)\" fill=\"none\" stroke=\"{ink}\" stroke-width=\"2.4\" stroke-linecap=\"round\" stroke-linejoin=\"round\">{glyph}</g></svg>"
            ))
        }))
    }
}

/// The smallest contrast between the `@` label's ink and its fill.
pub const MENTION_MIN: f32 = 4.5;

/// The `@` label's fill and ink. The skin's ink (white on the accent by
/// default) always stays: dark ink on a light accent turned into a smudge
/// under the line's drop shadow (rpg, night). When the ink does not reach
/// [`MENTION_MIN`] on the fill, the fill is deepened instead, toward the ink
/// that reads best on the label's own ink, just far enough that it does.
pub fn mention_colors(skin: &nanatype_theme::Skin) -> (Rgba8, Rgba8) {
    use crate::skin_theme::{accent_fg, contrast, on};
    let m = &skin.mention;
    let fill = skin.color(m.fill);
    let text = skin.color(m.text_color).with_alpha(255);
    let solid = fill.with_alpha(255);
    if contrast(text, solid) >= MENTION_MIN {
        return (fill, text);
    }
    let deeper = accent_fg(solid, text, on(text), MENTION_MIN);
    // Opaque: the ratio holds whatever the label is drawn over.
    (deeper.with_alpha(255), text)
}

impl Default for MentionLook {
    fn default() -> Self {
        Self::from_skin(&nanatype_theme::Skin::default())
    }
}

impl MentionLook {
    /// Inline labels in operator panels use the application palette.
    pub fn for_ui(dark: bool) -> Self {
        let p = crate::skin_theme::palette(dark);
        let fill = crate::skin_theme::rgba8(p.surface);
        let accent = crate::skin_theme::rgba8(p.accent);
        let fill = mix(fill, accent, 0.16);
        let text = crate::skin_theme::on(fill);
        Self {
            fill: crate::skin_theme::rgba(fill),
            text: crate::skin_theme::rgba(text),
            scale: 0.82,
            avatar: true,
            weight: 700,
            markers: MarkerLook { fill, text },
        }
    }

    pub fn from_skin(skin: &nanatype_theme::Skin) -> Self {
        let m = &skin.mention;
        let (fill, text) = mention_colors(skin);
        Self {
            fill: crate::skin_theme::rgba(fill),
            text: crate::skin_theme::rgba(text),
            scale: if m.scale.is_finite() {
                m.scale.clamp(0.3, 2.0)
            } else {
                0.82
            },
            avatar: m.avatar,
            // A bundled body font is a single face. Asked for bold, it is
            // emboldened by swash, which takes a glyph's winding from all of
            // its contours at once and pulls some outlines in instead (WenKai
            // た, Fusion Pixel ほ): one glyph of the label came out thin.
            // 500 is the heaviest weight that is never synthesized.
            weight: if skin.fonts.body.file.is_some() && skin.fonts.body.weight < 600 {
                500
            } else {
                700
            },
            markers: MarkerLook::from_skin(skin),
        }
    }

    /// The label for `mention`, with its avatar when one has a URL.
    pub fn object(
        &self,
        id: u64,
        mention: &nanatype_model::Mention,
        urls: &HashMap<String, String>,
    ) -> RichObject {
        let icon = self
            .avatar
            .then_some(mention.user.avatar.as_ref())
            .flatten()
            .and_then(|a| urls.get(&a.to_string()))
            .map(|u| Arc::<str>::from(u.as_str()));
        RichObject::label(
            id,
            mention.alt(),
            RichLabelStyle {
                fill: self.fill,
                text: self.text,
                scale: self.scale,
                weight: Some(self.weight),
                icon,
                ..RichLabelStyle::default()
            },
        )
    }
}

/// Outline badge text through the same registered faces and fallback as body text.
/// SVG image decoding deliberately does not load fonts or external font resources.
fn badge_glyph_paths(
    text: &str,
    look: &Look,
    font: f32,
    x: f32,
    baseline: f32,
    fill: &str,
    weight: u16,
) -> String {
    use nana_text::TextEngine;
    use nana_ui::runtime::TextShaper;
    use std::fmt::Write;
    struct Outline(String);
    impl ttf_parser::OutlineBuilder for Outline {
        fn move_to(&mut self, x: f32, y: f32) {
            let _ = write!(self.0, "M{x} {y}");
        }
        fn line_to(&mut self, x: f32, y: f32) {
            let _ = write!(self.0, "L{x} {y}");
        }
        fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
            let _ = write!(self.0, "Q{x1} {y1} {x} {y}");
        }
        fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
            let _ = write!(self.0, "C{x1} {y1} {x2} {y2} {x} {y}");
        }
        fn close(&mut self) {
            self.0.push('Z');
        }
    }
    let Some(engine) = nana_ui::NanaTextShaper::default().text_engine() else {
        return String::new();
    };
    let mut engine = engine.lock().unwrap_or_else(|e| e.into_inner());
    let source = nana_text::TextSource::new(text);
    let style = nana_text::TextStyle {
        font_family: look.body.as_deref().map(Arc::from),
        font_size_px: font,
        font_weight: weight,
        ..Default::default()
    };
    let layout = engine.layout(
        nana_text::TextKind::Label,
        &source,
        &style,
        &Default::default(),
        &mut Default::default(),
    );
    let advance = layout.runs.iter().map(|r| r.advance_px).sum::<f32>();
    let mut out = String::new();
    for run in &layout.runs {
        let Some(data) = engine.fonts().face_data(run.font) else {
            continue;
        };
        let Ok(face) = ttf_parser::Face::parse(data.bytes(), data.index()) else {
            continue;
        };
        let scale = font / f32::from(face.units_per_em());
        let mut pen = x - advance * 0.5 + run.origin_x_px;
        for glyph in &run.glyphs {
            let mut path = Outline(String::new());
            face.outline_glyph(ttf_parser::GlyphId(glyph.glyph_id as u16), &mut path);
            let gx = pen + glyph.offset_x_px;
            let gy = baseline - glyph.offset_y_px;
            let _ = write!(
                out,
                r#"<path fill="{fill}" transform="translate({gx} {gy}) scale({scale} -{scale})" d="{}"/>"#,
                path.0
            );
            pen += glyph.advance_px;
        }
    }
    out
}

fn badge_icon_paths(source: &str, width: f32, round: bool) -> Option<String> {
    use std::fmt::Write;
    let path = nana_ui::resolve_background_image_url(source)?;
    if std::fs::metadata(&path).ok()?.len() > 64 * 1024 {
        return None;
    }
    let image =
        crate::assets::decode_image(&std::fs::read(path).ok()?, 1024, 16 * 1024 * 1024).ok()?;
    let side = (width - 4.).round().clamp(8., 32.) as u32;
    let image = image
        .resize_to_fill(side, side, image::imageops::FilterType::Triangle)
        .to_rgba8();
    let radius = if round { width * 0.5 - 2. } else { width * 0.2 };
    let mut svg = format!(
        r#"<defs><clipPath id="avatar"><rect x="2" y="2" width="{}" height="{}" rx="{radius}"/></clipPath></defs><g clip-path="url(#avatar)">"#,
        width - 4.,
        width - 4.
    );
    let step = (width - 4.) / side as f32;
    for (x, y, pixel) in image.enumerate_pixels() {
        let [r, g, b, a] = pixel.0;
        if a == 0 {
            continue;
        }
        let _ = write!(
            svg,
            r#"<rect x="{}" y="{}" width="{}" height="{}" fill="rgb({r},{g},{b})" fill-opacity="{}"/>"#,
            2. + x as f32 * step,
            2. + y as f32 * step,
            step + 0.01,
            step + 0.01,
            a as f32 / 255.
        );
    }
    svg.push_str("</g>");
    Some(svg)
}

/// The engine's label glyph runs are horizontal-only. Preserve a label as
/// one reveal/object unit by drawing its vertical badge as a contained SVG.
/// Ordinary text still uses native vertical shaping and punctuation.
pub fn vertical_objects(rich: &mut RichText, look: &Look, max_inline: f32) {
    static CACHE: LazyLock<Mutex<HashMap<blake3::Hash, RichObject>>> =
        LazyLock::new(Default::default);
    let objects = rich.objects().to_vec();
    for (offset, mut object) in objects {
        match &object.content {
            RichObjectContent::Label { label, style } => {
                use nana_ui::runtime::TextShaper;
                let generation = nana_ui::NanaTextShaper::default().font_generation();
                let key = blake3::hash(
                    format!(
                        "{label:?}/{style:?}/{:?}/{}/{max_inline}/{generation}",
                        look.body, look.size
                    )
                    .as_bytes(),
                );
                if let Some(mut cached) = CACHE
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(&key)
                    .cloned()
                {
                    cached.id = object.id;
                    rich.set_object(offset, cached);
                    continue;
                }
                let font = look.size * style.scale;
                let width = (font * 1.5).ceil().max(8.);
                let icon_height = if style.icon.is_some() { width } else { 0. };
                let cap = (((max_inline.max(font * 3.) - icon_height - font) / (font * 1.15))
                    as usize)
                    .clamp(1, 24);
                let mut chars: Vec<_> =
                    label.graphemes(true).take(cap).map(str::to_owned).collect();
                if label.graphemes(true).count() > cap {
                    chars.pop();
                    chars.push("…".into());
                }
                let height = (font + icon_height + chars.len() as f32 * font * 1.15).ceil();
                let color = |c: [f32; 4]| {
                    format!(
                        "rgba({},{},{},{})",
                        (c[0] * 255.).round(),
                        (c[1] * 255.).round(),
                        (c[2] * 255.).round(),
                        c[3]
                    )
                };
                let mut svg = format!(
                    r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{width}" height="{height}"><rect width="{width}" height="{height}" rx="{}" fill="{}"/>"#,
                    width * 0.5,
                    color(style.fill)
                );
                if let Some(icon) = style
                    .icon
                    .as_ref()
                    .and_then(|url| badge_icon_paths(url, width, style.icon_round))
                {
                    svg.push_str(&icon);
                }
                for (i, ch) in chars.iter().enumerate() {
                    let y = icon_height + font + i as f32 * font * 1.15;
                    svg.push_str(&badge_glyph_paths(
                        ch,
                        look,
                        font,
                        width * 0.5,
                        y,
                        &color(style.text),
                        style.weight.unwrap_or(700),
                    ));
                }
                svg.push_str("</svg>");
                // Swap line-relative dimensions: the image's physical badge stays upright.
                object = RichObject::image(
                    object.id,
                    format!(
                        "data:image/svg+xml;base64,{}",
                        super::settings::controls::base64(svg.as_bytes())
                    ),
                    height,
                    width,
                )
                .descent(width * 0.5);
                let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
                if cache.len() >= 128 {
                    cache.clear();
                }
                cache.insert(key, object.clone());
            }
            RichObjectContent::Image { .. } | RichObjectContent::HostTexture { .. } => {
                std::mem::swap(&mut object.width_px, &mut object.height_px);
                object.descent_px = object.height_px * 0.5;
            }
            _ => {}
        }
        rich.set_object(offset, object);
    }
}

/// Measure the complete column stack with the same native engine that paints it.
/// The layout box must own this extent: native inline objects use the full stack
/// as their origin, whereas an auto-sized CSS box can be limited to its parent.
pub fn vertical_extent(
    rich: &RichText,
    node: &nana_ui::runtime::NodeStyle,
    height: f32,
    locale: Locale,
) -> f32 {
    use nana_text::{TextEngine, TextSource, TextStyle};
    use nana_ui::runtime::{TextShaper, nana_font_family};
    let l = &node.layout;
    let base = TextStyle {
        font_family: l.font_family.as_deref().map(nana_font_family),
        font_size_px: l.font_size.unwrap_or(16.),
        font_weight: l.font_weight.unwrap_or(400),
        line_height: l.line_height,
        letter_spacing_px: l.letter_spacing.unwrap_or(0.),
        language: nana_text::font::LanguageTag::new(locale.tag()),
        ..Default::default()
    };
    let mut source = TextSource::new(rich.text());
    source.set_spans(
        rich.spans()
            .iter()
            .filter(|(_, s)| !s.shape.is_empty())
            .map(|(range, s)| {
                let shape = &s.shape;
                let mut style = base.clone();
                if let Some(family) = &shape.family {
                    style.font_family = Some(nana_font_family(family));
                }
                if let Some(size) = shape
                    .size_em
                    .map(|em| em * base.font_size_px)
                    .or(shape.size_px)
                {
                    style.font_size_px = size;
                    if let Some(nana_ui_core::LineHeightSpec::Absolute(h)) = style.line_height {
                        style.line_height = Some(nana_ui_core::LineHeightSpec::Relative(
                            h / base.font_size_px,
                        ));
                    }
                }
                if let Some(weight) = shape.weight {
                    style.font_weight = weight;
                }
                if let Some(italic) = shape.italic {
                    style.italic = italic;
                }
                if let Some(spacing) = shape.letter_spacing_px {
                    style.letter_spacing_px = spacing;
                }
                if let Some(features) = &shape.features {
                    style.features = features.to_vec();
                }
                nana_text::TextSpan {
                    range,
                    style,
                    composition: None,
                }
            })
            .collect(),
    );
    source.set_objects(
        rich.objects()
            .iter()
            .map(|(offset, object)| {
                let [width_px, ascent_px, descent_px] = object.line_box();
                nana_text::InlineObject {
                    offset: *offset,
                    id: object.id,
                    metrics: nana_text::InlineObjectMetrics {
                        width_px,
                        ascent_px,
                        descent_px,
                        envelope: None,
                    },
                }
            })
            .collect(),
    );
    source.set_rubies(
        rich.rubies()
            .iter()
            .map(|(range, text)| nana_text::RubySpan {
                range: range.clone(),
                text: Arc::clone(text),
            })
            .collect(),
    );
    let mut offset = 0;
    if !rich.paragraph_alignments().is_empty() {
        source.set_paragraph_alignments(
            rich.text()
                .split('\n')
                .enumerate()
                .map(|(i, text)| {
                    let entry = (offset, rich.paragraph_alignment(i));
                    offset += text.len() + 1;
                    entry
                })
                .collect(),
        );
    }
    let mut constraints = nana_text::TextConstraints {
        max_height_px: Some(height.max(1.)),
        wrap: Some(l.text_wrap_break()),
        preserve_lines: true,
        writing_mode: nana_ui_core::WritingModeSpec::VerticalRl,
        text_orientation: nana_ui_core::TextOrientationSpec::Mixed,
        wrap_style: nana_ui_core::TextWrapStyleSpec::Pretty,
        align: match node.text_horizontal_alignment {
            nana_ui::runtime::TextHorizontalAlignment::Start => nana_ui_core::TextAlignSpec::Start,
            nana_ui::runtime::TextHorizontalAlignment::Center => {
                nana_ui_core::TextAlignSpec::Center
            }
            nana_ui::runtime::TextHorizontalAlignment::End => nana_ui_core::TextAlignSpec::End,
        },
        ..Default::default()
    };
    nana_ui::runtime::apply_used_typography(
        &mut constraints,
        l.text_typography
            .used(matches!(locale, Locale::ZhCn | Locale::JaJp)),
    );
    let engine = nana_ui::NanaTextShaper::default()
        .text_engine()
        .expect("native text engine");
    nana_text::lock_text_engine(&engine)
        .layout(
            nana_text::TextKind::Paragraph,
            &source,
            &base,
            &constraints,
            &mut Default::default(),
        )
        .physical_size()
        .0
        .ceil()
        .max(1.)
}

// ---- interned attribute combinations -----------------------------------

static ATTRS: LazyLock<Mutex<Vec<(Effects, Pace)>>> = LazyLock::new(|| Mutex::new(Vec::new()));

/// The span `effect` index for these effects and pacing (`None` = plain).
pub fn attr_index(effects: &Effects, pace: Pace) -> Option<u16> {
    if effects.is_none() && pace.is_inherit() {
        return None;
    }
    let mut table = ATTRS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(i) = table.iter().position(|(e, p)| e == effects && *p == pace) {
        return Some(i as u16);
    }
    table.push((effects.clone(), pace));
    Some((table.len() - 1) as u16)
}

/// Like [`attr_index`] but always an index, so a toolbar can switch a span
/// back to no effect (span fields can only be overlaid, not removed).
pub fn attr_index_any(effects: &Effects, pace: Pace) -> u16 {
    if let Some(i) = attr_index(effects, pace) {
        return i;
    }
    let mut table = ATTRS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(i) = table
        .iter()
        .position(|(e, p)| e.is_none() && p.is_inherit())
    {
        return i as u16;
    }
    table.push((Effects::default(), Pace::Inherit));
    (table.len() - 1) as u16
}

pub fn attr_of(index: u16) -> (Effects, Pace) {
    ATTRS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(usize::from(index))
        .cloned()
        .unwrap_or((Effects::default(), Pace::Inherit))
}

fn glyph_effect(e: &nanatype_model::Continuous, d: &EffectDefaults) -> GlyphEffect {
    match e.kind {
        ContinuousKind::Shake => {
            GlyphEffect::shake(d.shake_px * e.amp).frequency(d.shake_hz * e.speed)
        }
        ContinuousKind::Wave => {
            GlyphEffect::wave(d.wave_px * e.amp).frequency(e.speed / d.wave_period_s.max(0.05))
        }
        ContinuousKind::Jump => {
            GlyphEffect::jump(d.jump_px * e.amp).frequency(e.speed / d.jump_period_s.max(0.05))
        }
        ContinuousKind::Pulse => GlyphEffect::pulse(d.pulse_amount * e.amp)
            .frequency(e.speed / d.pulse_period_s.max(0.05)),
        ContinuousKind::Flicker => GlyphEffect::new(
            nana_ui_core::GlyphEffectKind::Flicker,
            d.flicker_amount * e.amp,
            d.flicker_hz * e.speed,
        ),
        ContinuousKind::Rainbow => GlyphEffect::new(
            nana_ui_core::GlyphEffectKind::Rainbow,
            e.amp,
            e.speed / d.rainbow_period_s.max(0.05),
        ),
    }
}

/// Monotonic version of the append-only attribute table.
pub fn attr_version() -> usize {
    ATTRS.lock().unwrap_or_else(|e| e.into_inner()).len()
}

struct CachedEffects {
    version: usize,
    defaults: EffectDefaults,
    reduced: bool,
    effects: Arc<[GlyphEffectChain]>,
}
static EFFECTS: Mutex<Vec<CachedEffects>> = Mutex::new(Vec::new());

/// Each attribute slot names all its continuous effects, or an empty chain.
/// Pacing and entrance metadata must never keep a window awake.
pub fn effect_table(defaults: &EffectDefaults, reduced: bool) -> Arc<[GlyphEffectChain]> {
    let table = ATTRS.lock().unwrap_or_else(|e| e.into_inner());
    let mut cache = EFFECTS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(found) = cache
        .iter()
        .find(|c| c.version == table.len() && c.defaults == *defaults && c.reduced == reduced)
    {
        return found.effects.clone();
    }
    let effects: Arc<[GlyphEffectChain]> = table
        .iter()
        .map(|(effects, _)| {
            GlyphEffectChain::new(if reduced {
                Vec::new()
            } else {
                effects
                    .continuous
                    .iter()
                    .map(|e| glyph_effect(e, defaults))
                    .filter(GlyphEffect::is_effective)
                    .collect()
            })
        })
        .collect();
    // Bound old revisions and hot-reloaded palettes, while keeping both motion preferences.
    if cache.len() >= 8 {
        cache.remove(0);
    }
    cache.push(CachedEffects {
        version: table.len(),
        defaults: defaults.clone(),
        reduced,
        effects: effects.clone(),
    });
    effects
}

// ---- inline objects ------------------------------------------------------

/// Ids for inlines that ride in rich text as objects. Equal inlines share an
/// id; only the newest [`MAX_OBJECTS`] are kept, which is far more than any
/// editor or visible list holds at once (a long stream interns every avatar,
/// emote and mention it shows).
#[derive(Default)]
struct Objects {
    next: u64,
    by_id: std::collections::BTreeMap<u64, (blake3::Hash, Inline)>,
    by_key: HashMap<blake3::Hash, u64>,
}

const MAX_OBJECTS: usize = 4096;

static OBJECTS: LazyLock<Mutex<Objects>> = LazyLock::new(Default::default);

fn object_key(inline: &Inline) -> blake3::Hash {
    blake3::hash(&serde_json::to_vec(inline).unwrap_or_default())
}

impl Objects {
    fn intern(&mut self, inline: &Inline) -> u64 {
        let key = object_key(inline);
        if let Some(id) = self.by_key.get(&key) {
            return *id;
        }
        self.next += 1;
        let id = self.next;
        self.by_key.insert(key, id);
        self.by_id.insert(id, (key, inline.clone()));
        while self.by_id.len() > MAX_OBJECTS {
            if let Some((_, (old, _))) = self.by_id.pop_first() {
                self.by_key.remove(&old);
            }
        }
        id
    }
}

/// An object id for an atom, mention or marker; equal inlines share one id.
pub fn object_id(inline: &Inline) -> u64 {
    OBJECTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .intern(inline)
}

fn object_of(id: u64) -> Option<Inline> {
    OBJECTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .by_id
        .get(&id)
        .map(|(_, inline)| inline.clone())
}

/// Width over height of an image file, cached.
fn aspect(url: &str) -> f32 {
    static CACHE: LazyLock<Mutex<HashMap<String, f32>>> = LazyLock::new(Default::default);
    if let Some(a) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).get(url) {
        return *a;
    }
    let a = crate::assets::file_path(url)
        .and_then(|p| imagesize::size(&p).ok())
        .map(|s| s.width as f32 / s.height.max(1) as f32)
        .filter(|a| a.is_finite() && *a > 0.0)
        .unwrap_or(1.0);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if cache.len() >= 4096 {
        cache.clear();
    }
    cache.insert(url.to_owned(), a);
    a
}

// ---- document → rich text -----------------------------------------------

fn color(c: Rgba8) -> PaintColor {
    PaintColor::srgb(crate::skin_theme::rgba(c))
}

fn span_style(style: &Style, effects: &Effects, pace: Pace, look: &Look) -> RichSpanStyle {
    let mut s = RichSpanStyle::new();
    if let Some(c) = style.color {
        s = s.color(color(c));
    }
    if let Some(size) = style.size {
        s = match size {
            Size::Em(em) => s.size_em(em),
            Size::Px(px) => s.size(px),
        };
    }
    match style.bold {
        Some(true) => s = s.weight(700.max(look.weight + 200).min(900)),
        Some(false) => s = s.weight(400.min(look.weight)),
        None => {}
    }
    if let Some(i) = style.italic {
        s = s.italic(i);
    }
    if let Some(font) = &style.font {
        s = s.overlay(&look.font_style(font));
    }

    if style.underline == Some(true) {
        s = s.underline();
    }
    if style.strike == Some(true) {
        s = s.line_through();
    }
    let ink = look.ink.as_ref();
    // On the display, a run of its own colour that would not read on the box
    // gets the skin's contrast guard outline (unless it has an outline).
    let stroke = style.stroke.or_else(|| {
        let (ink, c) = (ink?, style.color?);
        ink.guard_stroke(c)
    });
    if let Some(st) = stroke {
        s = s.stroke(
            RichTextStroke::new(st.width_px, color(st.color)).placement(TextStrokePlacement::Under),
        );
    }
    let shadow = match (style.shadow, ink) {
        (Some(sh), Some(ink)) => Some(Some(ink.shadow(sh))),
        (Some(sh), None) => Some(Some(sh)),
        // The skin's tinted text glow follows a run's own colour.
        (None, Some(ink)) => style.color.and_then(|c| {
            ink.shadow.as_ref()?;
            Some(ink.base_shadow_for(c))
        }),
        (None, None) => None,
    };
    // `None` leaves the node's shadow (a neutral drop shadow, or none).
    if let Some(Some(sh)) = shadow {
        s = s.shadow(RichTextShadow::new(
            [sh.dx, sh.dy],
            sh.blur,
            color(sh.color),
        ));
    }
    if let Some(i) = attr_index(effects, pace) {
        s = s.effect(i);
    }
    s
}

/// What converting a document needs to know about its surroundings.
pub struct Env<'a> {
    pub look: &'a Look,
    pub urls: &'a HashMap<String, String>,
    /// The editor's text: markers as labelled tags (which take room, so
    /// the editor may break a line where the display does not).
    pub editor: bool,
    pub locale: Locale,
}

pub fn to_rich(doc: &Document, env: &Env<'_>) -> RichText {
    let mut b = RichText::builder();
    // Byte length so far, to style objects after building.
    let mut len = 0usize;
    let mut object_spans: Vec<(usize, RichSpanStyle)> = Vec::new();
    let replacement = nana_ui_core::OBJECT_REPLACEMENT.len_utf8();
    for (pi, para) in doc.paragraphs.iter().enumerate() {
        if pi > 0 {
            b.plain("\n");
            len += 1;
        }
        for inline in &para.inlines {
            match inline {
                Inline::Text(run) => {
                    let s = span_style(&run.style, &run.effects, run.pace, env.look);
                    if let Some((family, weight)) = &env.look.latin
                        && run.style.font.is_none()
                    {
                        // Keep whole Latin words together for cursive shaping.
                        let mut start = 0;
                        let mut previous = None;
                        for (end, ch) in run
                            .text
                            .char_indices()
                            .chain(std::iter::once((run.text.len(), '\0')))
                        {
                            let latin = ch.is_ascii()
                                || matches!(ch as u32, 0x00c0..=0x024f | 0x0300..=0x036f | 0x1e00..=0x1eff);
                            if previous.is_some_and(|last| last != latin) || end == run.text.len() {
                                let mut part = s.clone();
                                if previous == Some(true) {
                                    part = part.family(family.clone()).family_alias(
                                        if run.style.bold.is_none() {
                                            "latin-default"
                                        } else {
                                            "latin-styled"
                                        },
                                    );
                                    if run.style.bold.is_none() && !env.editor {
                                        part = part.weight(*weight);
                                    }
                                }
                                b.push(&run.text[start..end], part);
                                start = end;
                            }
                            previous = Some(latin);
                        }
                    } else if s.is_empty() {
                        b.plain(&run.text);
                    } else {
                        b.push(&run.text, s);
                    }
                    len += run.text.len();
                }
                Inline::Ruby(r) => {
                    let s = span_style(&r.base.style, &r.base.effects, r.base.pace, env.look);
                    b.styled_ruby(&r.base.text, r.annotation.clone(), s);
                    len += r.base.text.len();
                }
                Inline::Atom(a) => {
                    let id = object_id(inline);
                    let h = env.look.size * a.height_em;
                    let object = match env.urls.get(&a.asset.to_string()) {
                        Some(url) => {
                            if let Some(cached) =
                                crate::stickers::cache::lookup(url).filter(|c| c.animated())
                            {
                                RichObject::texture(
                                    id,
                                    cached.slot(),
                                    h * cached.width as f32 / cached.height.max(1) as f32,
                                    h,
                                )
                                .descent(h * 0.15)
                            } else {
                                RichObject::image(id, url.clone(), h * aspect(url), h)
                                    .descent(h * 0.15)
                            }
                        }
                        // Missing picture: keep its place, labelled in the
                        // editor.
                        None if env.editor => {
                            let alt = if a.alt.is_empty() {
                                a.asset.to_string()
                            } else {
                                a.alt.clone()
                            };
                            env.look.mention.markers.object(id, alt, ChipKind::Picture)
                        }
                        None => RichObject::chip(id, a.alt.clone(), 5),
                    };
                    let s = span_style(&Style::default(), &a.effects, Pace::Inherit, env.look);
                    if !s.is_empty() {
                        object_spans.push((len, s));
                    }
                    b.object(object);
                    len += replacement;
                }
                Inline::Mention(m) => {
                    let id = object_id(inline);
                    let mut s = span_style(&Style::default(), &m.effects, Pace::Inherit, env.look);
                    // The label is a solid pill: the line's drop shadow
                    // under its glyphs only blurs them (rpg). A clear
                    // shadow of its own instead; not in the editor, whose
                    // spans convert back exactly.
                    if !env.editor {
                        s = s.shadow(RichTextShadow::new(
                            [0.0, 0.0],
                            0.0,
                            color(Rgba8::rgba(0, 0, 0, 0)),
                        ));
                    }
                    if !s.is_empty() {
                        object_spans.push((len, s));
                    }
                    b.object(env.look.mention.object(id, m, env.urls));
                    len += replacement;
                }
                Inline::Marker(m) => {
                    if env.editor {
                        let id = object_id(inline);
                        let label = super::input::marker_label(m, env.locale);
                        b.object(env.look.mention.markers.object(id, label, ChipKind::of(m)));
                        len += replacement;
                    }
                }
            }
        }
    }
    let mut rich = b.build();
    for (i, para) in doc.paragraphs.iter().enumerate() {
        if let Some(align) = para.align {
            rich.set_paragraph_alignment(i, Some(align_to_rich(align)));
        }
    }
    for (start, style) in object_spans {
        rich.set_span(start..start + replacement, style);
    }
    rich
}

// ---- rich text → document -------------------------------------------------

fn style_back(s: Option<&RichSpanStyle>, _look: &Look) -> (Style, Effects, Pace) {
    let Some(s) = s else {
        return (Style::default(), Effects::default(), Pace::Inherit);
    };
    let mut style = Style::default();
    if let Some(c) = s.paint.color {
        let [r, g, b, a] = c.to_srgb();
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        style.color = Some(Rgba8::rgba(q(r), q(g), q(b), q(a)));
    }
    style.size = s
        .shape
        .size_em
        .map(Size::Em)
        .or_else(|| s.shape.size_px.map(Size::Px));
    if let Some(w) = s.shape.weight {
        style.bold = Some(w >= 600);
    }
    style.italic = s.shape.italic;
    if let Some(f) = &s.shape.family {
        let f = f.to_string();
        style.font = Some(match s.shape.family_alias.as_deref() {
            Some("body") => FontSlot::Body,
            Some("name") => FontSlot::Name,
            Some("accent") => FontSlot::Accent,
            _ => FontSlot::Family(f),
        });
    }
    if matches!(
        s.shape.family_alias.as_deref(),
        Some("latin-default" | "latin-styled")
    ) {
        style.font = None;
    }
    if let Some(TextDecorationLine {
        underline,
        line_through,
    }) = s.paint.decoration
    {
        style.underline = underline.then_some(true);
        style.strike = line_through.then_some(true);
    }
    let q = |c: PaintColor| {
        let [r, g, b, a] = c.to_srgb();
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        Rgba8::rgba(q(r), q(g), q(b), q(a))
    };
    if let Some(st) = s.paint.stroke.as_ref().filter(|st| st.width_px > 0.0) {
        style.stroke = Some(Stroke {
            color: q(st.color),
            width_px: st.width_px,
        });
    }
    if let Some(sh) = s.paint.shadows.as_ref().and_then(|l| l.first()) {
        style.shadow = Some(Shadow {
            color: q(sh.color),
            dx: sh.offset[0],
            dy: sh.offset[1],
            blur: sh.blur_px,
        });
    }
    let (effects, pace) = s.effect.map(attr_of).unwrap_or_default();
    (style, effects, pace)
}

/// The document an editor holds. Exact inverse of [`to_rich`] for the editor.
pub fn from_rich(rich: &RichText, look: &Look) -> Document {
    let text = rich.text();
    let objects: HashMap<usize, &RichObject> =
        rich.objects().iter().map(|(o, ob)| (*o, ob)).collect();
    let rubies: Vec<(std::ops::Range<usize>, Arc<str>)> = rich.rubies().to_vec();
    let mut edges: Vec<usize> = vec![0, text.len()];
    for (range, _) in rich.spans().iter() {
        edges.push(range.start);
        edges.push(range.end);
    }
    for (offset, _) in rich.objects() {
        edges.push(*offset);
        edges.push(offset + nana_ui_core::OBJECT_REPLACEMENT.len_utf8());
    }
    for (range, _) in &rubies {
        edges.push(range.start);
        edges.push(range.end);
    }
    for (i, c) in text.char_indices() {
        if c == '\n' {
            edges.push(i);
            edges.push(i + 1);
        }
    }
    edges.sort_unstable();
    edges.dedup();

    let mut paragraphs = vec![Paragraph::default()];
    let mut pos = 0;
    while pos < text.len() {
        let current = paragraphs.last_mut().expect("one paragraph");
        if text[pos..].starts_with('\n') {
            paragraphs.push(Paragraph::default());
            pos += 1;
            continue;
        }
        if let Some(object) = objects.get(&pos) {
            let inline = object_of(object.id).or_else(|| match &object.content {
                RichObjectContent::Image { source } => Some(Inline::Atom(nanatype_model::Atom {
                    kind: nanatype_model::AtomKind::Image,
                    asset: nanatype_model::AssetRef::File(
                        crate::assets::file_path(source).map_or_else(
                            || source.to_string(),
                            |p| p.to_string_lossy().into_owned(),
                        ),
                    ),
                    height_em: object.height_px / look.size.max(1.0),
                    alt: String::new(),
                    effects: Effects::default(),
                })),
                _ => None,
            });
            if let Some(mut inline) = inline {
                match &mut inline {
                    Inline::Atom(a) => a.effects = style_back(rich.style_at(pos), look).1,
                    Inline::Mention(m) => m.effects = style_back(rich.style_at(pos), look).1,
                    _ => {}
                }
                current.inlines.push(inline);
            }
            pos += nana_ui_core::OBJECT_REPLACEMENT.len_utf8();
            continue;
        }
        if let Some((range, annotation)) = rubies.iter().find(|(r, _)| r.start == pos) {
            let (style, effects, pace) = style_back(rich.style_at(pos), look);
            current.inlines.push(Inline::Ruby(RubyGroup {
                base: TextRun {
                    text: text[range.clone()].to_owned(),
                    style,
                    effects,
                    pace,
                },
                annotation: annotation.to_string(),
            }));
            pos = range.end;
            continue;
        }
        let end = edges
            .iter()
            .copied()
            .find(|e| *e > pos)
            .unwrap_or(text.len());
        let (style, effects, pace) = style_back(rich.style_at(pos), look);
        current.inlines.push(Inline::Text(TextRun {
            text: text[pos..end].to_owned(),
            style,
            effects,
            pace,
        }));
        pos = end;
    }
    for (i, para) in paragraphs.iter_mut().enumerate() {
        para.align = rich.paragraph_alignment(i).and_then(align_from_rich);
    }
    Document { paragraphs }.normalized()
}

// ---- presentation ---------------------------------------------------------

/// The byte offset of each grapheme of a node's text and the unit it shows;
/// the breaks between paragraphs are not units.
pub fn unit_graphemes(text: &str) -> impl Iterator<Item = (usize, Option<u32>)> + '_ {
    let mut unit = 0;
    text.grapheme_indices(true).map(move |(byte, g)| {
        let this = (g != "\n").then(|| {
            unit += 1;
            unit - 1
        });
        (byte, this)
    })
}

/// Reveal times per grapheme of a node's text, on the session clock:
/// `Some(0)` already shown, `None` hidden.
#[derive(Debug, Clone)]
pub struct RevealPlan {
    pub at_ms: Vec<Option<u64>>,
    pub intro: Entrance,
}

impl RevealPlan {
    /// Map a [`Reveal`] over units onto the graphemes of `rich` (which was
    /// built from the same document for a display).
    pub fn new(rich: &RichText, reveal: &Reveal, intro: Entrance) -> Self {
        let mut last = Some(0);
        let at_ms = unit_graphemes(rich.text())
            .map(|(_, unit)| {
                if let Some(unit) = unit {
                    last = reveal.unit_at(unit);
                }
                last
            })
            .collect();
        Self { at_ms, intro }
    }

    /// The schedule relative to a document clock reading `anim_now`, taken
    /// at session time `now_ms`.
    pub fn schedule(&self, anim_now: Duration, now_ms: u64, reduced: bool) -> RevealSchedule {
        let at_s: Vec<f32> = self
            .at_ms
            .iter()
            .map(|at| match at {
                None => 1.0e6,
                Some(0) => -60.0,
                Some(t) => (*t as f64 - now_ms as f64) as f32 / 1000.0,
            })
            .collect();
        let secs = f32::from(self.intro.ms) / 1000.0;
        let intro = match self.intro.kind {
            _ if reduced => GlyphIntro::fade(0.0),
            EntranceKind::None => GlyphIntro::fade(0.0),
            EntranceKind::Fade => GlyphIntro::fade(secs),
            EntranceKind::Pop => GlyphIntro::pop(secs),
            EntranceKind::SlideUp => GlyphIntro::rise(10.0, secs),
        };
        let schedule = RevealSchedule::new(anim_now, at_s).intro(intro);
        // A pause/page boundary hides the unscheduled suffix until Core
        // supplies another segment. It is not an animation lasting 11 days.
        match self.at_ms.iter().position(Option::is_none) {
            Some(limit) => schedule.limit(limit as u32),
            None => schedule,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_ids_dedupe_and_stay_bounded() {
        let mut objects = Objects::default();
        let a = objects.intern(&Inline::text("a"));
        assert_eq!(objects.intern(&Inline::text("a")), a);
        for n in 0..MAX_OBJECTS + 8 {
            objects.intern(&Inline::text(format!("object-{n}")));
        }
        assert_eq!(objects.by_id.len(), MAX_OBJECTS);
        assert_eq!(objects.by_key.len(), MAX_OBJECTS);
        assert!(!objects.by_id.contains_key(&a));
    }

    #[test]
    fn latin_defaults_preserve_words_explicit_fonts_and_editor_roundtrip() {
        let mut look = look();
        look.latin = Some(("Tangerine".into(), 700));
        let urls = HashMap::new();
        let doc =
            nanatype_markup::parse("中文 Sonnet café [b]Bold[/b] [font=Consolas]Code[/font]").doc;
        let rich = to_rich(
            &doc,
            &Env {
                look: &look,
                urls: &urls,
                editor: true,
                locale: Locale::ZhCn,
            },
        );
        assert_eq!(from_rich(&rich, &look), doc);
        let sonnet = rich.text().find("Sonnet").unwrap();
        assert_eq!(
            rich.style_at(sonnet).unwrap().shape.family.as_deref(),
            Some("Tangerine")
        );
        assert_eq!(
            rich.style_at(sonnet).unwrap().shape.weight,
            None,
            "default script must not become an authored bold attribute"
        );
        let code = rich.text().find("Code").unwrap();
        assert_eq!(
            rich.style_at(code).unwrap().shape.family.as_deref(),
            Some("Consolas")
        );
        assert!(
            rich.spans()
                .iter()
                .any(|(range, _)| range.contains(&sonnet) && range.end >= sonnet + 6),
            "cursive words must stay in one span"
        );
    }

    #[test]
    fn personalized_body_font_disables_default_latin_face_per_surface() {
        let mut skin = (*nanatype_theme::SkinRegistry::new(None, "1.0.0")
            .load("nanatype.sonnet")
            .unwrap()
            .skin)
            .clone();
        skin.dialogue.text.font_family = Some("Consolas".into());
        let entrance = Entrance {
            kind: EntranceKind::None,
            ms: 0,
        };
        assert!(
            super::super::dialogue::Base::dialogue(&skin, entrance, false)
                .look()
                .latin
                .is_none()
        );
        assert!(
            super::super::dialogue::Base::history(&skin, entrance, false)
                .look()
                .latin
                .is_some()
        );
        skin.history.text.font_family = Some("Consolas".into());
        assert!(
            super::super::dialogue::Base::history(&skin, entrance, false)
                .look()
                .latin
                .is_none()
        );
    }

    fn look() -> Look {
        Look {
            size: 28.0,
            color: Rgba8::BLACK,
            weight: 500,
            line_height: 1.4,
            body: None,
            name: Some("Name Font".into()),
            accent: Some("Accent Font".into()),
            latin: None,
            mention: MentionLook::default(),
            ink: None,
        }
    }

    fn round_trip(markup: &str) {
        let doc = nanatype_markup::parse(markup).doc;
        let urls = HashMap::new();
        let look = look();
        let env = Env {
            look: &look,
            urls: &urls,
            editor: true,
            locale: Locale::ZhCn,
        };
        let rich = to_rich(&doc, &env);
        let back = from_rich(&rich, &look);
        assert_eq!(
            nanatype_markup::serialize(&back),
            nanatype_markup::serialize(&doc),
            "{markup}"
        );
    }

    #[test]
    fn changing_skins_preserves_fixed_sizes_and_authored_font_roles() {
        let doc = nanatype_markup::parse("[size=32px][font=name]固定[/font][/size][size=1.5][font=\"Name Font\"]相对[/font][/size]").doc;
        let urls = HashMap::new();
        let mut next = look();
        next.size = 40.0;
        next.name = Some("New Name Font".into());
        for look in [look(), next] {
            let rich = to_rich(
                &doc,
                &Env {
                    look: &look,
                    urls: &urls,
                    editor: true,
                    locale: Locale::ZhCn,
                },
            );
            assert_eq!(from_rich(&rich, &look), doc);
            assert_eq!(
                rich.style_at(0).unwrap().shape.family.as_deref(),
                look.name.as_deref()
            );
            let last = rich.spans().iter().last().unwrap().1;
            assert_eq!(last.shape.family.as_deref(), Some("Name Font"));
            assert_eq!(last.shape.size_em, Some(1.5));
        }
    }

    #[test]
    fn documents_survive_the_editor() {
        round_trip("你好[b]粗[/b][i]斜[/i][u]下[/u][s]删[/s]");
        round_trip(
            "[align=center][size=32px][font=body]固定[/font][/size][/align]\n[align=end][/align]",
        );
        round_trip("[size=1]倍率[/size][font=name]槽[/font][font=\"Name Font\"]同名字体[/font]");
        round_trip("[color=#e86a92]红[/color][size=1.5]大[/size][font=accent]装[/font]");
        round_trip("[stroke=#ffffff,2]描[/stroke][shadow=#000000aa,2,2,4]影[/shadow]");
        round_trip(
            "[shake]抖[/shake][wave amp=2]波[/wave][speed=0.5]慢[/speed][instant]快[/instant]",
        );
        round_trip("[ruby=かんじ]漢字[/ruby]と[b][ruby=よ]読[/ruby][/b]");
        round_trip("一[wait=500]二[pause]三[page]四[se=coin][expr=smile][screenshake=4,200]");
        round_trip("第一段\n第二段\n\n第四段");
        round_trip(
            "你好[at=42 name=观众 avatar=blob:bili-face-42]！[wave][at name=\"无 uid\"][/wave]",
        );
    }

    #[test]
    fn mention_labels_read_on_every_builtin_skin() {
        let reg = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        for e in reg.entries() {
            let skin = reg.load(&e.id).unwrap().skin;
            let (fill, text) = mention_colors(&skin);
            let r = crate::skin_theme::contrast(text.with_alpha(255), fill.with_alpha(255));
            assert!(r >= MENTION_MIN, "{}: @ label {r:.2}:1", e.id);
        }
    }

    #[test]
    fn a_mention_keeps_the_skins_ink_and_deepens_a_light_fill() {
        let reg = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        for id in ["nanatype.rpg", "nanatype.night", "nanatype.pixel"] {
            let skin = reg.load(id).unwrap().skin;
            let (fill, text) = mention_colors(&skin);
            // White on the accent, as the skin says; never dark ink, which
            // the line's drop shadow smears.
            assert_eq!(
                text,
                skin.color(skin.mention.text_color).with_alpha(255),
                "{id}"
            );
            assert!(crate::skin_theme::contrast(text, fill.with_alpha(255)) >= MENTION_MIN);
        }
        // rpg's light periwinkle accent is deepened, pixel's dark green kept.
        let rpg = reg.load("nanatype.rpg").unwrap().skin;
        assert_ne!(mention_colors(&rpg).0.with_alpha(255), rpg.chrome.accent);
        let pixel = reg.load("nanatype.pixel").unwrap().skin;
        assert_eq!(mention_colors(&pixel).0, pixel.chrome.accent);
    }

    #[test]
    fn vertical_inline_badges_keep_one_playback_unit_and_images_keep_physical_aspect() {
        let mut builder = RichText::builder();
        builder.object(RichObject::image(10, "file:///wide.png", 80., 40.));
        builder.object(RichObject::label(
            20,
            "@奈奈 & <friend>",
            RichLabelStyle::default(),
        ));
        let mut rich = builder.build();
        let before = rich.text().to_owned();
        let offsets: Vec<_> = rich.objects().iter().map(|(at, o)| (*at, o.id)).collect();
        vertical_objects(&mut rich, &look(), 400.);
        assert_eq!(rich.text(), before);
        assert_eq!(
            rich.objects()
                .iter()
                .map(|(at, o)| (*at, o.id))
                .collect::<Vec<_>>(),
            offsets
        );
        assert_eq!(rich.objects()[0].1.width_px, 40.);
        assert_eq!(rich.objects()[0].1.height_px, 80.);
        let object = &rich.objects()[1].1;
        assert!(object.width_px > object.height_px);
        let RichObjectContent::Image { source } = &object.content else {
            panic!("vertical label image");
        };
        assert!(source.starts_with("data:image/svg+xml;base64,"));
        let mut again = RichText::builder();
        again.object(RichObject::label(
            50,
            "@奈奈 & <friend>",
            RichLabelStyle::default(),
        ));
        let mut again = again.build();
        vertical_objects(&mut again, &look(), 400.);
        assert_eq!(
            again.objects()[0].1.id,
            50,
            "cached pictures must not replace object identities"
        );
    }

    #[test]
    fn a_mention_is_one_label_object_with_its_avatar() {
        let doc = nanatype_markup::parse("[at=42 name=观众 avatar=blob:bili-face-42]").doc;
        let mut urls = HashMap::new();
        urls.insert(
            "blob:bili-face-42".to_owned(),
            "file:///f/42.png".to_owned(),
        );
        let look = look();
        let env = Env {
            look: &look,
            urls: &urls,
            editor: false,
            locale: Locale::ZhCn,
        };
        let rich = to_rich(&doc, &env);
        let (_, object) = &rich.objects()[0];
        assert_eq!(
            object.tag_label().map(|l| l.to_string()),
            Some("@观众".into())
        );
        let style = object.label_style().expect("a styled label");
        assert_eq!(style.icon.as_deref(), Some("file:///f/42.png"));
    }

    #[test]
    fn a_mention_label_is_bold_unless_the_skin_bundles_a_single_face() {
        let reg = nanatype_theme::SkinRegistry::new(None, "test");
        for e in reg.entries() {
            let skin = reg.load(&e.id).expect("skin loads").skin;
            let bundled = skin.fonts.body.file.is_some();
            assert_eq!(
                MentionLook::from_skin(&skin).weight,
                if bundled { 500 } else { 700 },
                "{}",
                e.id
            );
        }
    }

    #[test]
    fn editor_markers_are_readable_chips_with_a_glyph() {
        let reg = nanatype_theme::SkinRegistry::new(None, "test");
        for e in reg.entries() {
            let skin = reg.load(&e.id).expect("skin loads").skin;
            let m = MarkerLook::from_skin(&skin);
            let r = crate::skin_theme::contrast(m.text, m.fill);
            assert!(r >= 4.5, "{}: marker chip ink {r:.2}:1", e.id);
        }
        let doc = nanatype_markup::parse("[wait=500][se=coin]").doc;
        let urls = HashMap::new();
        let look = look();
        let env = Env {
            look: &look,
            urls: &urls,
            editor: true,
            locale: Locale::ZhCn,
        };
        let rich = to_rich(&doc, &env);
        let icons: Vec<String> = rich
            .objects()
            .iter()
            .map(|(_, o)| {
                let icon = o.label_style().and_then(|s| s.icon.clone());
                icon.expect("a glyph").to_string()
            })
            .collect();
        assert_eq!(icons.len(), 2);
        assert!(icons.iter().all(|i| i.starts_with("data:image/svg+xml,")));
        assert_ne!(icons[0], icons[1], "each kind has its own glyph");
    }

    #[test]
    fn reveal_plans_follow_units_across_paragraphs() {
        let doc = Document::plain("ab\ncd");
        let urls = HashMap::new();
        let look = look();
        let env = Env {
            look: &look,
            urls: &urls,
            editor: false,
            locale: Locale::ZhCn,
        };
        let rich = to_rich(&doc, &env);
        let reveal = Reveal {
            visible_from: 0,
            shown: 1,
            segment: Some(nanatype_core::SegmentReveal {
                origin_ms: 1000,
                start: 1,
                reveal_ms: vec![0, 100, 200].into(),
            }),
            hidden_from: 4,
        };
        let plan = RevealPlan::new(
            &rich,
            &reveal,
            Entrance {
                kind: EntranceKind::Fade,
                ms: 100,
            },
        );
        assert_eq!(
            plan.at_ms,
            vec![Some(0), Some(1000), Some(1000), Some(1100), Some(1200)]
        );
    }

    fn builtin(id: &str) -> nanatype_theme::Skin {
        let reg = nanatype_theme::SkinRegistry::new(None, "test");
        (*reg
            .load(&format!("nanatype.{id}"))
            .expect("builtin skin loads")
            .skin)
            .clone()
    }

    fn display_spans(skin: &nanatype_theme::Skin, markup: &str) -> Vec<RichSpanStyle> {
        let base = super::super::dialogue::Base::dialogue(
            skin,
            Entrance {
                kind: EntranceKind::None,
                ms: 0,
            },
            false,
        );
        let look = base.look();
        let doc = nanatype_markup::parse(markup).doc;
        let urls = HashMap::new();
        let env = Env {
            look: &look,
            urls: &urls,
            editor: false,
            locale: Locale::ZhCn,
        };
        to_rich(&doc, &env)
            .spans()
            .iter()
            .map(|(_, s)| s.clone())
            .collect()
    }

    #[test]
    fn low_contrast_runs_get_the_guard_outline_in_the_darkest_ink() {
        let pixel = builtin("pixel");
        assert_eq!(pixel.effects.contrast_guard, 3.0);
        let spans = display_spans(&pixel, "[color=#e8f0a0]淡[/color][color=#0f380f]深[/color]");
        let pale = spans[0].paint.stroke.expect("pale run is outlined");
        assert_eq!(pale.width_px, GUARD_STROKE);
        assert_eq!(pale.color, color(Rgba8::rgb(15, 56, 15)));
        assert!(spans.iter().skip(1).all(|s| s.paint.stroke.is_none()));
        // Skins without the guard leave runs alone.
        let night = builtin("night");
        let spans = display_spans(&night, "[color=#101830]暗[/color]");
        assert!(spans[0].paint.stroke.is_none());
    }

    #[test]
    fn the_editor_never_gets_display_only_effects() {
        let pixel = builtin("pixel");
        let look = super::super::dialogue::Base::editor(&pixel).look();
        assert!(look.ink.is_none());
    }

    #[test]
    fn a_tinted_skin_glow_follows_the_runs_own_colour() {
        let terminal = builtin("terminal");
        let spans = display_spans(&terminal, "[color=#ff5050]红[/color]");
        let shadows = spans[0].paint.shadows.as_deref().expect("own shadow");
        assert_eq!(shadows[0].color, color(Rgba8::rgba(255, 80, 80, 0x80)));
    }

    #[test]
    fn a_glow_on_a_light_box_is_soft() {
        let halo = builtin("halo");
        let spans = display_spans(&halo, "[shadow=#0876d0,0,0,8]光[/shadow]");
        let sh = spans[0].paint.shadows.as_deref().unwrap()[0];
        assert_eq!(sh.blur_px, 10.0);
        assert_eq!(sh.color, color(Rgba8::rgba(8, 118, 208, 140)));
    }
}

#[cfg(test)]
mod effect_cache_tests {
    use super::*;
    #[test]
    fn paused_and_already_shown_reveals_do_not_schedule_hidden_glyphs() {
        let plan = RevealPlan {
            at_ms: vec![Some(100), None, None],
            intro: Entrance {
                kind: EntranceKind::Fade,
                ms: 200,
            },
        };
        let schedule = plan.schedule(Duration::ZERO, 0, false);
        assert_eq!(schedule.limit, Some(1));
        assert!(schedule.end() < Duration::from_secs(1));
        let plan = RevealPlan {
            at_ms: vec![None],
            ..plan
        };
        assert_eq!(
            plan.schedule(Duration::ZERO, 0, false).end(),
            Duration::ZERO
        );
    }
    #[test]
    fn empty_pacing_and_reduced_effects_are_static_and_tables_are_cached() {
        let pacing = attr_index(&Effects::default(), Pace::FixedMs(33)).unwrap() as usize;
        let defaults = EffectDefaults::default();
        let table = effect_table(&defaults, false);
        assert!(table[pacing].0.is_empty());
        let version = attr_version();
        let cached = effect_table(&defaults, false);
        if version == attr_version() && table.len() == version {
            assert!(Arc::ptr_eq(&table, &cached));
        }
        let effects = Effects {
            continuous: vec![
                nanatype_model::Continuous::new(ContinuousKind::Wave),
                nanatype_model::Continuous::new(ContinuousKind::Rainbow),
            ],
            entrance: None,
        };
        let index = attr_index(&effects, Pace::Inherit).unwrap() as usize;
        let table = effect_table(&defaults, false);
        assert_eq!(table[index].0.len(), 2);
        assert!(effect_table(&defaults, true)[index].0.is_empty());
    }
}

pub fn align_to_rich(align: nanatype_model::ParagraphAlign) -> nana_ui_core::TextAlignSpec {
    match align {
        nanatype_model::ParagraphAlign::Start => nana_ui_core::TextAlignSpec::Start,
        nanatype_model::ParagraphAlign::Center => nana_ui_core::TextAlignSpec::Center,
        nanatype_model::ParagraphAlign::End => nana_ui_core::TextAlignSpec::End,
    }
}

pub fn align_from_rich(
    align: nana_ui_core::TextAlignSpec,
) -> Option<nanatype_model::ParagraphAlign> {
    match align {
        nana_ui_core::TextAlignSpec::Start | nana_ui_core::TextAlignSpec::Left => {
            Some(nanatype_model::ParagraphAlign::Start)
        }
        nana_ui_core::TextAlignSpec::Center => Some(nanatype_model::ParagraphAlign::Center),
        nana_ui_core::TextAlignSpec::End | nana_ui_core::TextAlignSpec::Right => {
            Some(nanatype_model::ParagraphAlign::End)
        }
        _ => None,
    }
}
