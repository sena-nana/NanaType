//! `skin.toml`. Every table and field has a default, so a skin only states
//! what differs from its `base`. Inheritance is a deep merge of TOML tables
//! before this schema is applied (see [`crate::load`]).

use std::collections::BTreeMap;

use nanatype_model::{EntranceKind, Overflow, Rgba8};
use nanatype_playback::{PacingParams, PunctPauses, TypingSoundRule};
use serde::{Deserialize, Serialize};

pub const SKIN_SCHEMA: u32 = 3;

/// Localized display strings keyed by BCP-47 tag (`zh-CN`, `en-US`, `ja-JP`).
pub type Localized = BTreeMap<String, String>;

pub fn localized(map: &Localized, locale: &str, fallback: &str) -> String {
    map.get(locale)
        .or_else(|| {
            let lang = locale.split('-').next().unwrap_or(locale);
            map.iter()
                .find(|(k, _)| k.split('-').next() == Some(lang))
                .map(|(_, v)| v)
        })
        .or_else(|| map.get("en-US"))
        .or_else(|| map.values().next())
        .cloned()
        .unwrap_or_else(|| fallback.to_owned())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Appearance {
    #[default]
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    #[default]
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

/// A nine-slice image: `slice` is `[top, right, bottom, left]` in image pixels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NineSlice {
    pub image: String,
    pub slice: [u32; 4],
    /// Draw the center too; false leaves a native/material background beneath the edges.
    #[serde(default = "yes")]
    pub fill: bool,
    /// Drawn size of the slice borders as a multiple of `slice`.
    #[serde(default = "one")]
    pub scale: f32,
    #[serde(default)]
    pub repeat: bool,
    /// Reserve the painted slices in layout. False makes a complete plate
    /// preserve its own corner cuts without adding to the authored padding.
    #[serde(default = "yes")]
    pub layout_inset: bool,
}

fn yes() -> bool {
    true
}

fn one() -> f32 {
    1.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextShadowSpec {
    pub color: Rgba8,
    #[serde(default)]
    pub dx: f32,
    #[serde(default)]
    pub dy: f32,
    #[serde(default)]
    pub blur: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextStrokeSpec {
    pub color: Rgba8,
    pub width: f32,
}

/// A skin colour a newer field can follow instead of naming a hex value, so
/// decorations and tails track the box colour (and the user's overrides).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorRole {
    /// `dialogue.box.color`
    Box,
    /// `dialogue.box.border_color`
    Border,
    /// `chrome.accent`
    Accent,
    /// `dialogue.text.color`
    Text,
    /// `dialogue.nameplate.color`
    Name,
    /// `dialogue.nameplate.text_color`
    NameText,
}

impl ColorRole {
    const ALL: [(Self, &'static str); 6] = [
        (Self::Box, "box"),
        (Self::Border, "border"),
        (Self::Accent, "accent"),
        (Self::Text, "text"),
        (Self::Name, "name"),
        (Self::NameText, "name_text"),
    ];

    pub fn name(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(r, _)| *r == self)
            .map_or("", |(_, n)| n)
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.iter().find(|(_, n)| *n == name).map(|(r, _)| *r)
    }
}

/// `"#rrggbbaa"` or a [`ColorRole`] name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorRef {
    Rgba(Rgba8),
    Role(ColorRole),
}

impl From<Rgba8> for ColorRef {
    fn from(c: Rgba8) -> Self {
        Self::Rgba(c)
    }
}

impl Serialize for ColorRef {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Rgba(c) => c.serialize(serializer),
            Self::Role(r) => serializer.serialize_str(r.name()),
        }
    }
}

impl<'de> Deserialize<'de> for ColorRef {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        if let Some(role) = ColorRole::parse(text.trim()) {
            return Ok(Self::Role(role));
        }
        text.parse::<Rgba8>()
            .map(Self::Rgba)
            .map_err(|_| serde::de::Error::custom(format!("invalid colour `{text}`: use #rrggbb(aa) or one of box, border, accent, text, name, name_text")))
    }
}

/// A length in px (`24`) or a percentage of the box (`"50%"`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Len {
    Px(f32),
    Percent(f32),
}

impl Default for Len {
    fn default() -> Self {
        Self::Px(0.0)
    }
}

impl Serialize for Len {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Px(v) => serializer.serialize_f64(f64::from(*v)),
            Self::Percent(v) => serializer.serialize_str(&format!("{v}%")),
        }
    }
}

impl<'de> Deserialize<'de> for Len {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl serde::de::Visitor<'_> for V {
            type Value = Len;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a number of px or a percentage like \"50%\"")
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Len, E> {
                Ok(Len::Px(v as f32))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Len, E> {
                Ok(Len::Px(v as f32))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Len, E> {
                Ok(Len::Px(v as f32))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Len, E> {
                v.trim()
                    .strip_suffix('%')
                    .and_then(|n| n.trim().parse::<f32>().ok())
                    .filter(|n| n.is_finite())
                    .map(Len::Percent)
                    .ok_or_else(|| E::custom(format!("`{v}` is not a percentage like \"50%\"")))
            }
        }
        deserializer.deserialize_any(V)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GradientKind {
    #[default]
    Linear,
    Radial,
}

/// A CSS-like gradient painted over a fill colour.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GradientSpec {
    pub kind: GradientKind,
    /// Linear direction in degrees, CSS style: 0 = to top, 90 = to right.
    pub angle: f32,
    /// Radial centre in percent of the box.
    pub center: [f32; 2],
    /// Radial: a circle rather than an ellipse.
    pub circle: bool,
    /// `[[position 0–1, colour], …]`, 2–8 stops.
    pub stops: Vec<(f32, ColorRef)>,
}

impl Default for GradientSpec {
    fn default() -> Self {
        Self {
            kind: GradientKind::Linear,
            angle: 180.0,
            center: [50.0, 50.0],
            circle: false,
            stops: Vec::new(),
        }
    }
}

/// A box shadow layer (outer, or inset for inner glows and rules).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShadowSpec {
    pub color: ColorRef,
    #[serde(default)]
    pub dx: f32,
    #[serde(default)]
    pub dy: f32,
    #[serde(default)]
    pub blur: f32,
    #[serde(default)]
    pub spread: f32,
    #[serde(default)]
    pub inset: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BorderStyleSpec {
    #[default]
    Solid,
    Dashed,
    Dotted,
}

/// What every drawn part can add to its fill colour: a gradient, per-corner
/// radii, per-side borders and shadow layers. Flattened, so the keys sit
/// directly in the part's table.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ShapeSpec {
    pub material: Option<String>,
    pub border_material: Option<String>,
    /// Opt-in vector outline; absent keeps the original box or image frame.
    pub contour: Option<ContourPreset>,
    /// Painted over the fill colour.
    pub gradient: Option<GradientSpec>,
    /// `[top_left, top_right, bottom_right, bottom_left]`; replaces `radius`.
    pub radii: Option<[f32; 4]>,
    /// `[top, right, bottom, left]`; replaces `border_width`.
    pub border_widths: Option<[f32; 4]>,
    pub border_style: BorderStyleSpec,
    pub shadows: Vec<ShadowSpec>,
}

/// Bounded vector outlines. Paint changes without moving the text viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContourPreset {
    Rounded,
    Orbit,
    Hud,
    Parchment,
    Cinema,
    Rpg,
    Slant,
    Cloud,
    Chat,
    Comic,
    Terminal,
    Pixel,
    Sakura,
    Ink,
    Ribbon,
    Seal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotifPreset {
    Rose,
    Moon,
    Leaf,
    Jellyfish,
    Lantern,
    CitrusSlice,
    Orbit,
    Sparkle,
    Blossom,
    Petal,
    Flourish,
    Rays,
    Halftone,
    Fold,
    Circuit,
    Paper,
}

impl ShapeSpec {
    pub fn radii(&self, radius: f32) -> [f32; 4] {
        self.radii.unwrap_or([radius; 4])
    }

    pub fn border_widths(&self, width: f32) -> [f32; 4] {
        self.border_widths.unwrap_or([width; 4])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Edge {
    Top,
    Right,
    #[default]
    Bottom,
    Left,
}

impl Edge {
    /// Index into `[top, right, bottom, left]` arrays.
    pub fn index(self) -> usize {
        self as usize
    }
}

/// A speech-bubble tail: a triangle on one side of the box that shares its
/// fill and border.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TailSpec {
    pub side: Edge,
    /// Where the tail's base starts along the side, 0–1 from the left/top.
    pub at: f32,
    /// Base width in px.
    pub width: f32,
    /// How far the tip reaches past the box edge, px.
    pub length: f32,
    /// Shift of the tip along the side, px.
    pub lean: f32,
    /// Fill; the box colour when unset.
    pub color: Option<ColorRef>,
    /// Repeat the box's hard (unblurred, outer) shadows under the tail.
    pub shadow: bool,
}

impl Default for TailSpec {
    fn default() -> Self {
        Self {
            side: Edge::Bottom,
            at: 0.2,
            width: 32.0,
            length: 22.0,
            lean: 0.0,
            color: None,
            shadow: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecorLayer {
    /// Drawn before the box, so the box covers it.
    Below,
    #[default]
    Above,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecorWhen {
    #[default]
    Always,
    /// Only while the name plate shows.
    Speaker,
    /// Only on the display box (and the skin picker's preview), never in the
    /// input window: chrome that would look like real controls there.
    Display,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FontRole {
    Body,
    Name,
    #[default]
    Accent,
}

/// Small, paint-only loops. Only live dialogue boxes run these; editors,
/// history and previews use the decoration's original pose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecorMotionKind {
    Float,
    Breathe,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DecorMotionSpec {
    pub kind: DecorMotionKind,
    pub period_s: f32,
    /// Float: vertical pixels either side of rest. Breathe: opacity lost
    /// at the dimmest point, multiplied by the decoration's own opacity.
    pub amplitude: f32,
    /// Fraction of a cycle, 0–1.
    pub phase: f32,
}

impl Default for DecorMotionSpec {
    fn default() -> Self {
        Self {
            kind: DecorMotionKind::Float,
            period_s: 6.0,
            amplitude: 0.1,
            phase: 0.0,
        }
    }
}

/// A decorative shape placed relative to the dialogue box: accent bars,
/// inner rules, ornaments, seals, title bars.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DecorSpec {
    /// Keep this physical width/height ratio while resolving window constraints.
    pub aspect_ratio: Option<f32>,
    /// Maximum fraction of the window available to this ornament (not a scale-up).
    pub max_fraction: Option<[f32; 2]>,
    /// Hide secondary details below either logical window dimension.
    pub min_window: Option<[f32; 2]>,
    pub shader: Option<String>,
    /// Transparent packaged artwork, fitted without stretching.
    pub image: Option<String>,
    pub motif: Option<MotifPreset>,
    pub layer: DecorLayer,
    /// The decor's own point of this name lands on the box's.
    pub anchor: Anchor,
    pub offset: [f32; 2],
    pub size: [Len; 2],
    /// `[top, right, bottom, left]` from the box edges (negative reaches
    /// outside); stretches the decor and replaces `anchor` and `size`.
    pub inset: Option<[f32; 4]>,
    pub color: Option<ColorRef>,
    pub radius: f32,
    pub border_color: Option<ColorRef>,
    pub border_width: f32,
    #[serde(flatten)]
    pub shape: ShapeSpec,
    /// Clip to a polygon, points in percent of the decor (3–8 points).
    pub polygon: Vec<[f32; 2]>,
    /// Degrees, clockwise.
    pub rotate: f32,
    pub opacity: f32,
    /// A glyph or two drawn centred in the decor.
    pub text: String,
    pub text_color: ColorRef,
    pub text_size: f32,
    pub font: FontRole,
    pub when: DecorWhen,
    pub motion: Option<DecorMotionSpec>,
}

impl Default for DecorSpec {
    fn default() -> Self {
        Self {
            aspect_ratio: None,
            max_fraction: None,
            min_window: None,
            shader: None,
            image: None,
            motif: None,
            layer: DecorLayer::Above,
            anchor: Anchor::TopLeft,
            offset: [0.0, 0.0],
            size: [Len::Px(16.0), Len::Px(16.0)],
            inset: None,
            color: None,
            radius: 0.0,
            border_color: None,
            border_width: 0.0,
            shape: ShapeSpec::default(),
            polygon: Vec::new(),
            rotate: 0.0,
            opacity: 1.0,
            text: String::new(),
            text_color: ColorRef::Role(ColorRole::Text),
            text_size: 16.0,
            font: FontRole::Accent,
            when: DecorWhen::Always,
            motion: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContinueMotion {
    #[default]
    Bob,
    Blink,
    Pulse,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign {
    #[default]
    Start,
    Center,
    End,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Meta {
    pub schema: u32,
    pub id: String,
    pub version: String,
    /// Oldest NanaType version that understands this skin.
    pub min_app: String,
    /// Id of the skin this one extends.
    pub base: Option<String>,
    pub name: Localized,
    pub description: Localized,
    pub author: String,
    pub license: String,
    pub preview: Option<String>,
    /// Picker categories: classic, streaming, game, retro, cute, elegant, minimal.
    pub tags: Vec<String>,
}

impl Default for Meta {
    fn default() -> Self {
        Self {
            schema: SKIN_SCHEMA,
            id: String::new(),
            version: "1.0.0".into(),
            min_app: "0.1.0".into(),
            base: None,
            name: Localized::new(),
            description: Localized::new(),
            author: String::new(),
            license: String::new(),
            preview: None,
            tags: Vec::new(),
        }
    }
}

/// Content colours and the appearance used to classify skins in the picker.
/// Operator controls use an independent application theme.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Chrome {
    pub appearance: Appearance,
    pub accent: Rgba8,
    pub surface: Rgba8,
    pub text: Rgba8,
}

impl Default for Chrome {
    fn default() -> Self {
        Self {
            appearance: Appearance::Light,
            accent: Rgba8::rgb(0, 0, 0),
            surface: Rgba8::rgb(255, 255, 255),
            text: Rgba8::rgb(0, 0, 0),
        }
    }
}

/// A font role: a packaged file and/or system families, tried in order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FontSpec {
    pub file: Option<String>,
    pub families: Vec<String>,
    pub weight: u16,
}

impl FontSpec {
    /// The system sans stack for a BCP-47 language tag: Japanese text needs
    /// Japanese faces first or Han characters take Chinese glyph shapes.
    pub fn locale_families(tag: &str) -> Vec<String> {
        let names: &[&str] = if tag.get(..2).is_some_and(|l| l.eq_ignore_ascii_case("ja")) {
            &["Yu Gothic UI", "Meiryo UI", "Hiragino Sans", "Noto Sans JP"]
        } else {
            &["Microsoft YaHei UI", "PingFang SC", "Noto Sans SC"]
        };
        names.iter().map(|n| (*n).into()).collect()
    }

    /// Whether this role still has the unmodified default stack, that is,
    /// the skin names no fonts of its own for it.
    pub fn is_default_stack(&self) -> bool {
        self.file.is_none() && self.families == Self::locale_families("zh")
    }
}

impl Default for FontSpec {
    fn default() -> Self {
        Self {
            file: None,
            families: Self::locale_families("zh"),
            weight: 500,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Fonts {
    pub body: FontSpec,
    pub name: FontSpec,
    pub accent: FontSpec,
    /// Optional default Latin face; explicit authored fonts take precedence.
    pub latin: Option<FontSpec>,
}

impl Default for Fonts {
    /// Names are semibold unless a skin says otherwise; the name plate uses
    /// the weight as given, so a skin can ask for a regular name.
    fn default() -> Self {
        Self {
            body: FontSpec::default(),
            name: FontSpec {
                weight: 600,
                ..FontSpec::default()
            },
            accent: FontSpec::default(),
            latin: None,
        }
    }
}

impl Fonts {
    pub fn role(&self, role: FontRole) -> &FontSpec {
        match role {
            FontRole::Body => &self.body,
            FontRole::Name => &self.name,
            FontRole::Accent => &self.accent,
        }
    }

    /// Body, name and accent.
    pub fn roles_mut(&mut self) -> [&mut FontSpec; 3] {
        [&mut self.body, &mut self.name, &mut self.accent]
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BoxSpec {
    pub color: Rgba8,
    pub border_color: Rgba8,
    pub border_width: f32,
    /// A second border inside the first (RPG windows).
    pub inner_border_color: Option<Rgba8>,
    pub inner_border_width: f32,
    pub radius: f32,
    pub glow: Option<Rgba8>,
    pub glow_blur: f32,
    pub shadow: Option<TextShadowSpec>,
    /// Replaces the drawn box when present.
    pub frame: Option<NineSlice>,
    /// Box width as a fraction of the region (ADV boxes span the screen).
    pub width_fraction: f32,
    /// Its `shadows` paint after `inner_border_color`, `glow` and `shadow`.
    #[serde(flatten)]
    pub shape: ShapeSpec,
    /// Room kept free inside the region `[top, right, bottom, left]`, for tails
    /// and ornaments that reach past the box.
    pub margin: [f32; 4],
}

impl Default for BoxSpec {
    fn default() -> Self {
        Self {
            color: Rgba8::rgba(0, 0, 0, 0),
            border_color: Rgba8::rgba(0, 0, 0, 0),
            border_width: 0.0,
            inner_border_color: None,
            inner_border_width: 0.0,
            radius: 0.0,
            glow: None,
            glow_blur: 12.0,
            shadow: None,
            frame: None,
            width_fraction: 1.0,
            shape: ShapeSpec::default(),
            margin: [0.0; 4],
        }
    }
}

fn layer(color: ColorRef, dx: f32, dy: f32, blur: f32, spread: f32, inset: bool) -> ShadowSpec {
    ShadowSpec {
        color,
        dx,
        dy,
        blur,
        spread,
        inset,
    }
}

impl BoxSpec {
    /// Every shadow layer in paint order: the inner border, glow and shadow
    /// fields as layers, then `shadows`.
    pub fn shadow_layers(&self) -> Vec<ShadowSpec> {
        let mut out = Vec::new();
        if let Some(c) = self.inner_border_color {
            // A second border drawn just inside the first.
            let spread = self.inner_border_width.max(1.0);
            out.push(layer(c.into(), 0.0, 0.0, 0.0, spread, true));
        }
        if let Some(c) = self.glow {
            out.push(layer(c.into(), 0.0, 0.0, self.glow_blur, 0.0, false));
        }
        if let Some(s) = &self.shadow {
            out.push(layer(s.color.into(), s.dx, s.dy, s.blur, 0.0, false));
        }
        out.extend(self.shape.shadows.iter().cloned());
        out
    }

    /// `[top, right, bottom, left]` border widths; none under a frame image.
    pub fn border_widths(&self) -> [f32; 4] {
        if self.frame.is_some() {
            [0.0; 4]
        } else {
            self.shape.border_widths(self.border_width)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextSpec {
    /// Optional body-only font override; name and accent roles are unchanged.
    pub font_family: Option<String>,
    pub material: Option<String>,
    pub color: Rgba8,
    pub size: f32,
    pub line_height: f32,
    pub letter_spacing: f32,
    pub stroke: Option<TextStrokeSpec>,
    pub shadow: Option<TextShadowSpec>,
    pub align: TextAlign,
}

impl Default for TextSpec {
    fn default() -> Self {
        Self {
            font_family: None,
            material: None,
            color: Rgba8::rgb(0, 0, 0),
            size: 28.0,
            line_height: 1.4,
            letter_spacing: 0.0,
            stroke: None,
            shadow: None,
            align: TextAlign::Start,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NameplateSpec {
    pub visible: bool,
    pub color: Rgba8,
    pub text_color: Rgba8,
    pub size: f32,
    pub radius: f32,
    pub padding: [f32; 2],
    /// Offset of the plate from the box's top-left corner; negative `y`
    /// lifts it onto the border like most ADV skins.
    pub offset: [f32; 2],
    pub frame: Option<NineSlice>,
    pub stroke: Option<TextStrokeSpec>,
    pub border_color: Option<ColorRef>,
    pub border_width: f32,
    #[serde(flatten)]
    pub shape: ShapeSpec,
    pub text_shadow: Option<TextShadowSpec>,
    /// The plate's own point of this name is placed at the box's, then moved
    /// by `offset`.
    pub anchor: Anchor,
    /// Degrees, clockwise.
    pub rotate: f32,
}

impl Default for NameplateSpec {
    fn default() -> Self {
        Self {
            visible: true,
            color: Rgba8::rgba(0, 0, 0, 0),
            text_color: Rgba8::rgb(0, 0, 0),
            size: 18.0,
            radius: 0.0,
            padding: [10.0, 4.0],
            offset: [0.0, 0.0],
            frame: None,
            stroke: None,
            border_color: None,
            border_width: 0.0,
            shape: ShapeSpec::default(),
            text_shadow: None,
            anchor: Anchor::TopLeft,
            rotate: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    #[default]
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PortraitSpec {
    pub visible: bool,
    pub side: Side,
    pub size: [f32; 2],
    pub offset: [f32; 2],
    /// Image used when no variant is active, or when the active one is missing.
    pub image: Option<String>,
    /// Variant name → image. Expression markers switch to the variant of the
    /// same name.
    pub variants: BTreeMap<String, String>,
    pub radius: f32,
}

impl Default for PortraitSpec {
    fn default() -> Self {
        Self {
            visible: false,
            side: Side::Left,
            size: [160.0, 200.0],
            offset: [0.0, 0.0],
            image: None,
            variants: BTreeMap::new(),
            radius: 12.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ContinueSpec {
    /// A glyph drawn when no image is given.
    pub glyph: String,
    pub color: Rgba8,
    pub size: f32,
    /// Sprite sheet laid out horizontally.
    pub image: Option<String>,
    pub frames: u32,
    pub fps: f32,
    pub anchor: Anchor,
    pub bob_px: f32,
    /// From the anchor point of the box's border edge, px.
    pub offset: [f32; 2],
    pub motion: ContinueMotion,
    /// One bob, blink or pulse cycle, seconds.
    pub period_s: f32,
    /// Drawn size of `image` (one frame); `size × 1.4` square when unset.
    pub image_size: Option<[f32; 2]>,
}

impl Default for ContinueSpec {
    fn default() -> Self {
        Self {
            glyph: String::new(),
            color: Rgba8::rgb(0, 0, 0),
            size: 16.0,
            image: None,
            frames: 1,
            fps: 8.0,
            anchor: Anchor::BottomRight,
            bob_px: 0.0,
            offset: [-14.0, -8.0],
            motion: ContinueMotion::None,
            period_s: 0.83,
            image_size: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EntranceSpec {
    pub material: Option<String>,
    pub scope: TransitionScope,
    pub kind: EntranceKind,
    pub ms: u16,
}

impl Default for EntranceSpec {
    fn default() -> Self {
        Self {
            material: None,
            scope: TransitionScope::Glyph,
            kind: EntranceKind::None,
            ms: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionScope {
    #[default]
    Glyph,
    Text,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExitSpec {
    pub material: Option<String>,
    pub scope: TransitionScope,
    pub ms: u16,
}
impl Default for ExitSpec {
    fn default() -> Self {
        Self {
            material: None,
            scope: TransitionScope::Text,
            ms: 320,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RevealSpec {
    pub handwriting: Option<crate::HandwritingSpec>,
    pub base_ms: u16,
    pub cjk_factor: f32,
    pub space_ms: u16,
    pub punct: PunctPauses,
    pub entrance: EntranceSpec,
    pub exit: Option<ExitSpec>,
    pub light: Option<TextLightSpec>,
    /// Presentation-only character substitution; the source document stays intact.
    pub scramble: Option<ScrambleSpec>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScrambleSpec {
    pub alphabet: String,
    pub interval_ms: u16,
    pub entrance_ms: u16,
    pub exit_hold_ms: u16,
    pub exit_ms: u16,
}

impl Default for ScrambleSpec {
    fn default() -> Self {
        Self {
            alphabet: "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789+-=<>[]{}()/".into(),
            interval_ms: 25,
            entrance_ms: 140,
            exit_hold_ms: 80,
            exit_ms: 400,
        }
    }
}

/// Finite glyph-attached light, rendered separately from the coverage mask.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextLightSpec {
    pub color: Rgba8,
    pub intensity: f32,
    pub radius: f32,
}

impl Default for TextLightSpec {
    fn default() -> Self {
        Self {
            color: Rgba8::rgba(255, 213, 147, 255),
            intensity: 1.0,
            radius: 12.0,
        }
    }
}

impl Default for RevealSpec {
    fn default() -> Self {
        let p = PacingParams::default();
        Self {
            handwriting: None,
            base_ms: p.base_ms,
            cjk_factor: p.cjk_factor,
            space_ms: p.space_ms,
            punct: p.punct,
            entrance: EntranceSpec::default(),
            exit: None,
            light: None,
            scramble: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Dialogue {
    pub responsive: Option<crate::ResponsiveDialogue>,
    pub shader_effects: Vec<crate::ShaderEffect>,
    /// Logical size of the dialogue region; the display window and the input
    /// stage are this size (plus `overscan` on every side).
    pub region: [f32; 2],
    /// Transparent margin so jumping, shaking and glowing text is not clipped.
    pub overscan: f32,
    /// Text inset inside the box: `[left, top, right, bottom]`.
    pub padding: [f32; 4],
    #[serde(rename = "box")]
    pub box_: BoxSpec,
    pub text: TextSpec,
    pub nameplate: NameplateSpec,
    pub portrait: PortraitSpec,
    #[serde(rename = "continue")]
    pub continue_: ContinueSpec,
    pub reveal: RevealSpec,
    /// Text taller than the box: clip, scroll along, or page.
    pub overflow: Overflow,
    /// In page mode, turn a full page on its own after this long; `None`
    /// waits for the continue key like `[page]`.
    pub overflow_auto_ms: Option<u32>,
    /// Background drawn behind the box inside the region (rarely used).
    pub background: Option<String>,
    pub tail: Option<TailSpec>,
    pub decor: Vec<DecorSpec>,
    /// The quoted chat message above a line that answers one.
    pub reply: ReplySpec,
    pub sweep: SweepSpec,
}

/// Optional light passing across the dialogue surface, beneath its text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SweepSpec {
    pub enabled: bool,
    pub color: Rgba8,
    pub intensity: f32,
    pub period_s: f32,
}
impl Default for SweepSpec {
    fn default() -> Self {
        Self {
            enabled: false,
            color: Rgba8::WHITE,
            intensity: 0.18,
            period_s: 4.0,
        }
    }
}

/// A chat message a line answers, quoted in one small line above it:
/// avatar · name: text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReplySpec {
    pub visible: bool,
    /// Text size; `None` is 62% of the line's text.
    pub size: Option<f32>,
    /// Defaults to the text colour; the name takes the accent colour.
    pub color: Option<ColorRef>,
    /// How strongly the quote's accent rule and avatar are drawn; its name
    /// and text stay at full strength, so they keep their contrast.
    pub opacity: f32,
}

impl Default for ReplySpec {
    fn default() -> Self {
        Self {
            visible: true,
            size: None,
            color: None,
            opacity: 0.8,
        }
    }
}

/// A live-chat viewer named in a line (`[at=…]`): a pill with their avatar
/// and name, drawn the same in the editor, on the dialogue box and in
/// history.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MentionSpec {
    pub fill: ColorRef,
    pub text_color: ColorRef,
    /// Size relative to the text around it.
    pub scale: f32,
    pub avatar: bool,
}

impl Default for MentionSpec {
    fn default() -> Self {
        Self {
            fill: ColorRef::Role(ColorRole::Accent),
            text_color: ColorRef::Rgba(Rgba8::rgb(255, 255, 255)),
            scale: 0.82,
            avatar: true,
        }
    }
}

/// How history shows live-chat rows next to the streamer's own lines.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HistoryChatSpec {
    /// Chat text size relative to the history text.
    pub text_scale: f32,
    /// Opacity of the complete chat bubble artwork, independent of its text.
    pub bubble_opacity: f32,
}

impl Default for HistoryChatSpec {
    fn default() -> Self {
        Self {
            text_scale: 0.88,
            bubble_opacity: 0.7,
        }
    }
}

impl Default for Dialogue {
    fn default() -> Self {
        Self {
            responsive: None,
            shader_effects: Vec::new(),
            region: [760.0, 150.0],
            overscan: 18.0,
            padding: [20.0, 22.0, 20.0, 14.0],
            box_: BoxSpec::default(),
            text: TextSpec::default(),
            nameplate: NameplateSpec::default(),
            portrait: PortraitSpec::default(),
            continue_: ContinueSpec::default(),
            reveal: RevealSpec::default(),
            overflow: Overflow::default(),
            overflow_auto_ms: None,
            background: None,
            tail: None,
            decor: Vec::new(),
            reply: ReplySpec::default(),
            sweep: SweepSpec::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AvatarShape {
    #[default]
    Circle,
    Square,
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HistoryAvatar {
    pub shape: AvatarShape,
    pub size: f32,
    pub ring_color: Option<ColorRef>,
    pub ring_width: f32,
    /// A speaker's fallback avatar (no picture): a flat fill instead of
    /// the accent gradient.
    pub color: Option<ColorRef>,
    /// Corner radius of a square avatar; default 18% of the size.
    pub radius: Option<f32>,
}

impl Default for HistoryAvatar {
    fn default() -> Self {
        Self {
            shape: AvatarShape::Circle,
            size: 40.0,
            ring_color: None,
            ring_width: 0.0,
            color: None,
            radius: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryEntrance {
    None,
    #[default]
    SlideUp,
    Fade,
    SlideLeft,
}

/// A pill behind the speaker's name in the history list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HistoryNamePlate {
    pub color: ColorRef,
    pub text_color: ColorRef,
    pub radius: f32,
    pub padding: [f32; 2],
    pub border_color: Option<ColorRef>,
    pub border_width: f32,
    #[serde(flatten)]
    pub shape: ShapeSpec,
}

impl Default for HistoryNamePlate {
    fn default() -> Self {
        Self {
            color: ColorRef::Role(ColorRole::Name),
            text_color: ColorRef::Role(ColorRole::NameText),
            radius: 10.0,
            padding: [9.0, 2.0],
            border_color: None,
            border_width: 0.0,
            shape: ShapeSpec::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryLayout {
    /// Name and time above the bubble.
    #[default]
    Stack,
    /// Name, line and time side by side on one row, like a chat log.
    Inline,
}

/// A surface behind the whole history list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HistoryPanel {
    pub color: Rgba8,
    pub border_color: Option<ColorRef>,
    pub border_width: f32,
    pub radius: f32,
    #[serde(flatten)]
    pub shape: ShapeSpec,
    /// List inset inside the panel: `[left, top, right, bottom]`.
    pub padding: [f32; 4],
    /// Space around the panel inside the window: `[top, right, bottom, left]`.
    pub margin: [f32; 4],
}

impl Default for HistoryPanel {
    fn default() -> Self {
        Self {
            color: Rgba8::rgba(0, 0, 0, 0),
            border_color: None,
            border_width: 0.0,
            radius: 0.0,
            shape: ShapeSpec::default(),
            padding: [12.0; 4],
            margin: [0.0; 4],
        }
    }
}

/// A header bar across the top of the history panel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HistoryTitle {
    pub text: String,
    pub color: Rgba8,
    pub size: f32,
    pub weight: u16,
    pub background: Rgba8,
    pub height: f32,
    /// Horizontal inset of the text.
    pub padding: f32,
}

impl Default for HistoryTitle {
    fn default() -> Self {
        Self {
            text: "HISTORY".into(),
            color: Rgba8::rgb(96, 98, 102),
            size: 20.0,
            weight: 600,
            background: Rgba8::rgba(0, 0, 0, 0),
            height: 40.0,
            padding: 16.0,
        }
    }
}

/// The Bilibili-style list: avatar, name, then a bubble per line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct History {
    /// Whether consecutive messages from one author share their header.
    pub group_speakers: bool,
    /// Panel-less window padding: left, top, right, bottom.
    pub padding: [f32; 4],
    /// Panel-less window margin: top, right, bottom, left.
    pub margin: [f32; 4],
    /// Extra horizontal room around each row.
    pub row_padding: f32,
    /// Additional width reserved beside the text column (independent of row paint).
    pub column_inset: f32,
    pub width: f32,
    pub height: f32,
    pub avatar: HistoryAvatar,
    pub name_color: Rgba8,
    pub name_size: f32,
    pub name_stroke: Option<TextStrokeSpec>,
    pub bubble_color: Rgba8,
    pub bubble_border: Option<Rgba8>,
    pub bubble_radius: f32,
    pub bubble_padding: [f32; 2],
    pub bubble_frame: Option<NineSlice>,
    pub bubble_material: Option<String>,
    pub bubble_contour: Option<ContourPreset>,
    pub text: TextSpec,
    pub gap: f32,
    pub entrance: HistoryEntrance,
    pub time_color: Rgba8,
    pub bubble_gradient: Option<GradientSpec>,
    pub bubble_radii: Option<[f32; 4]>,
    pub bubble_border_width: f32,
    pub bubble_shadows: Vec<ShadowSpec>,
    pub name_plate: Option<HistoryNamePlate>,
    pub layout: HistoryLayout,
    /// Width of the name column in the inline layout.
    pub name_width: f32,
    pub name_weight: u16,
    /// Defaults to two points under the name.
    pub time_size: Option<f32>,
    /// Show `HH:MM:SS` instead of `HH:MM`.
    pub time_seconds: bool,
    pub panel: Option<HistoryPanel>,
    pub title: Option<HistoryTitle>,
    /// Decorations placed on the panel (or the window, without one).
    pub decor: Vec<DecorSpec>,
    /// Leave out the line still on the dialogue box until it clears; the
    /// user's setting can override this.
    pub hide_latest: bool,
    /// Live-chat rows.
    pub chat: HistoryChatSpec,
    /// The chat message a row answers, quoted above its bubble.
    pub reply: ReplySpec,
}

impl Default for History {
    fn default() -> Self {
        Self {
            group_speakers: true,
            padding: [4.0, 8.0, 4.0, 8.0],
            margin: [8.0; 4],
            row_padding: 0.0,
            column_inset: 0.0,
            width: 420.0,
            height: 560.0,
            avatar: HistoryAvatar::default(),
            name_color: Rgba8::rgb(0, 0, 0),
            name_size: 14.0,
            name_stroke: None,
            bubble_color: Rgba8::rgba(0, 0, 0, 0),
            bubble_border: None,
            bubble_radius: 0.0,
            bubble_padding: [12.0, 8.0],
            bubble_frame: None,
            bubble_material: None,
            bubble_contour: None,
            text: TextSpec {
                size: 18.0,
                line_height: 1.45,
                ..TextSpec::default()
            },
            gap: 10.0,
            entrance: HistoryEntrance::None,
            time_color: Rgba8::rgb(96, 96, 96),
            bubble_gradient: None,
            bubble_radii: None,
            bubble_border_width: 0.0,
            bubble_shadows: Vec::new(),
            name_plate: None,
            layout: HistoryLayout::Stack,
            name_width: 96.0,
            name_weight: 700,
            time_size: None,
            time_seconds: false,
            panel: None,
            title: None,
            decor: Vec::new(),
            hide_latest: false,
            chat: HistoryChatSpec::default(),
            reply: ReplySpec::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TypingSound {
    pub files: Vec<String>,
    pub volume: f32,
    pub pitch_jitter: f32,
    pub every_n: u8,
    pub min_interval_ms: u16,
    pub skip_punct: bool,
}

impl Default for TypingSound {
    fn default() -> Self {
        Self {
            files: Vec::new(),
            volume: 0.6,
            pitch_jitter: 0.05,
            every_n: 1,
            min_interval_ms: 30,
            skip_punct: true,
        }
    }
}

impl TypingSound {
    pub fn rule(&self, enabled: bool) -> TypingSoundRule {
        TypingSoundRule {
            enabled,
            every_n: self.every_n.max(1),
            skip_space: true,
            skip_punct: self.skip_punct,
            min_interval_ms: self.min_interval_ms,
            variants: self.files.len().clamp(1, 255) as u8,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Sounds {
    pub typing: TypingSound,
    /// Played when a line starts.
    pub open: Option<String>,
    /// Played on continue.
    pub advance: Option<String>,
    /// Named effects for `[se=name]`.
    pub se: BTreeMap<String, String>,
}

/// Skin defaults for per-character effects. Markup `amp`/`speed` multiply these.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EffectDefaults {
    pub shake_px: f32,
    pub shake_hz: f32,
    pub wave_px: f32,
    pub wave_period_s: f32,
    pub jump_px: f32,
    pub jump_period_s: f32,
    pub pulse_amount: f32,
    pub pulse_period_s: f32,
    pub flicker_amount: f32,
    pub flicker_hz: f32,
    pub rainbow_period_s: f32,
    pub screen_shake_px: f32,
    /// Smallest contrast ratio a coloured run may have against the box
    /// colour before it gets a 1.5px outline in the skin's darkest (or
    /// lightest) ink. `0` turns the guard off.
    pub contrast_guard: f32,
}

impl Default for EffectDefaults {
    fn default() -> Self {
        Self {
            shake_px: 1.6,
            shake_hz: 14.0,
            wave_px: 3.0,
            wave_period_s: 1.2,
            jump_px: 6.0,
            jump_period_s: 0.9,
            pulse_amount: 0.12,
            pulse_period_s: 1.0,
            flicker_amount: 1.0,
            flicker_hz: 8.0,
            rainbow_period_s: 2.4,
            screen_shake_px: 6.0,
            contrast_guard: 0.0,
        }
    }
}

/// A complete, resolved skin description.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Skin {
    #[serde(skip)]
    pub stroke_data: Option<std::sync::Arc<crate::StrokeData>>,
    pub materials: BTreeMap<String, crate::MaterialSpec>,
    #[serde(skip)]
    pub material_programs: BTreeMap<String, std::sync::Arc<crate::ShaderProgram>>,
    pub shaders: BTreeMap<String, crate::ShaderSpec>,
    #[serde(skip)]
    pub shader_programs: BTreeMap<String, std::sync::Arc<crate::ShaderProgram>>,
    pub meta: Meta,
    pub chrome: Chrome,
    pub fonts: Fonts,
    pub dialogue: Dialogue,
    pub history: History,
    pub sounds: Sounds,
    pub effects: EffectDefaults,
    pub mention: MentionSpec,
}

impl Skin {
    pub fn material_references(&self) -> Vec<(&'static str, &str)> {
        let mut refs = Vec::new();
        if let Some(name) = &self.dialogue.reveal.entrance.material {
            refs.push(("dialogue.reveal.entrance.material", name.as_str()));
        }
        if let Some(name) = self
            .dialogue
            .reveal
            .exit
            .as_ref()
            .and_then(|e| e.material.as_ref())
        {
            refs.push(("dialogue.reveal.exit.material", name.as_str()));
        }

        for (field, value) in [
            ("dialogue.text.material", &self.dialogue.text.material),
            ("history.text.material", &self.history.text.material),
            ("history.bubble_material", &self.history.bubble_material),
        ] {
            if let Some(value) = value {
                refs.push((field, value.as_str()));
            }
        }
        let shapes = std::iter::once((
            "dialogue.box.material",
            "dialogue.box.border_material",
            &self.dialogue.box_.shape,
        ))
        .chain(std::iter::once((
            "dialogue.nameplate.material",
            "dialogue.nameplate.border_material",
            &self.dialogue.nameplate.shape,
        )))
        .chain(self.dialogue.decor.iter().map(|d| {
            (
                "dialogue.decor.material",
                "dialogue.decor.border_material",
                &d.shape,
            )
        }))
        .chain(self.history.decor.iter().map(|d| {
            (
                "history.decor.material",
                "history.decor.border_material",
                &d.shape,
            )
        }))
        .chain(self.history.name_plate.iter().map(|p| {
            (
                "history.name_plate.material",
                "history.name_plate.border_material",
                &p.shape,
            )
        }))
        .chain(self.history.panel.iter().map(|p| {
            (
                "history.panel.material",
                "history.panel.border_material",
                &p.shape,
            )
        }));
        for (fill, border, shape) in shapes {
            if let Some(value) = &shape.material {
                refs.push((fill, value.as_str()));
            }
            if let Some(value) = &shape.border_material {
                refs.push((border, value.as_str()));
            }
        }
        if let Some(responsive) = &self.dialogue.responsive {
            for layout in responsive.layouts() {
                let shapes = layout
                    .box_
                    .iter()
                    .map(|v| &v.shape)
                    .chain(layout.nameplate.iter().map(|v| &v.shape))
                    .chain(layout.decor.iter().flatten().map(|v| &v.shape));
                for shape in shapes {
                    if let Some(v) = &shape.material {
                        refs.push(("dialogue.responsive.material", v.as_str()));
                    }
                    if let Some(v) = &shape.border_material {
                        refs.push(("dialogue.responsive.border_material", v.as_str()));
                    }
                }
            }
        }
        refs
    }

    /// Full authored dialogue artwork, either a static plate or a live effect.
    pub fn has_dialogue_artwork(&self) -> bool {
        self.dialogue
            .box_
            .frame
            .as_ref()
            .is_some_and(|f| !f.layout_inset)
            || self
                .dialogue
                .shader_effects
                .iter()
                .any(|e| e.target == crate::ShaderTarget::Artwork)
    }

    pub fn display_name(&self, locale: &str) -> String {
        localized(&self.meta.name, locale, &self.meta.id)
    }

    pub fn description(&self, locale: &str) -> String {
        localized(&self.meta.description, locale, "")
    }

    /// The concrete colour of `c` in this skin.
    pub fn color(&self, c: ColorRef) -> Rgba8 {
        let d = &self.dialogue;
        match c {
            ColorRef::Rgba(c) => c,
            ColorRef::Role(ColorRole::Box) => d.box_.color,
            ColorRef::Role(ColorRole::Border) => d.box_.border_color,
            ColorRef::Role(ColorRole::Accent) => self.chrome.accent,
            ColorRef::Role(ColorRole::Text) => d.text.color,
            ColorRef::Role(ColorRole::Name) => d.nameplate.color,
            ColorRef::Role(ColorRole::NameText) => d.nameplate.text_color,
        }
    }

    /// Pacing from the skin's reveal table and the user's speed settings.
    pub fn pacing(&self, speed_scale: f32, punctuation_pauses: bool, typing: bool) -> PacingParams {
        let r = &self.dialogue.reveal;
        PacingParams {
            base_ms: r.base_ms,
            cjk_factor: r.cjk_factor,
            space_ms: r.space_ms,
            punct: r.punct,
            punctuation_pauses,
            speed_scale,
            typing,
        }
    }

    /// Every packaged file the manifest refers to, with the field that names it.
    pub fn referenced_files(&self) -> Vec<(&'static str, &str, FileKind)> {
        let mut out: Vec<(&'static str, &str, FileKind)> = Vec::new();
        if let Some(spec) = &self.dialogue.reveal.handwriting {
            out.push((
                "dialogue.reveal.handwriting.data",
                &spec.data,
                FileKind::StrokeData,
            ));
        }
        for spec in self.materials.values().take(16) {
            out.push(("materials.source", &spec.source, FileKind::Shader));
            for path in spec.textures.iter().take(2) {
                out.push(("materials.textures", path, FileKind::Image));
            }
        }
        for spec in self.shaders.values().take(16) {
            out.push(("shaders.source", &spec.source, FileKind::Shader));
            for path in spec.textures.iter().take(2) {
                out.push(("shaders.textures", path, FileKind::Image));
            }
        }
        for (field, path) in [
            ("meta.preview", &self.meta.preview),
            ("dialogue.background", &self.dialogue.background),
            ("dialogue.portrait.image", &self.dialogue.portrait.image),
            ("dialogue.continue.image", &self.dialogue.continue_.image),
        ] {
            if let Some(p) = path {
                out.push((field, p.as_str(), FileKind::Image));
            }
        }
        for (field, slice) in [
            ("dialogue.box.frame", &self.dialogue.box_.frame),
            ("dialogue.nameplate.frame", &self.dialogue.nameplate.frame),
            ("history.bubble_frame", &self.history.bubble_frame),
        ] {
            if let Some(s) = slice {
                out.push((field, s.image.as_str(), FileKind::NineSlice(s.slice)));
            }
        }
        for (field, decor) in [
            ("dialogue.decor.image", &self.dialogue.decor),
            ("history.decor.image", &self.history.decor),
        ] {
            for d in decor {
                if let Some(image) = &d.image {
                    out.push((field, image.as_str(), FileKind::Image));
                }
            }
        }
        for path in self.dialogue.portrait.variants.values() {
            out.push(("dialogue.portrait.variants", path.as_str(), FileKind::Image));
        }
        if let Some(responsive) = &self.dialogue.responsive {
            for layout in responsive.layouts() {
                for p in layout.portrait.iter().flat_map(|p| p.variants.values()) {
                    out.push(("dialogue.responsive.portrait.variants", p, FileKind::Image));
                }
                for decor in layout.decor.iter().flatten() {
                    if let Some(image) = &decor.image {
                        out.push(("dialogue.responsive.decor.image", image, FileKind::Image));
                    }
                }
                for frame in [
                    layout.box_.as_ref().and_then(|v| v.frame.as_ref()),
                    layout.nameplate.as_ref().and_then(|v| v.frame.as_ref()),
                ]
                .into_iter()
                .flatten()
                {
                    out.push((
                        "dialogue.responsive.frame",
                        &frame.image,
                        FileKind::NineSlice(frame.slice),
                    ));
                }
                for image in [
                    layout.portrait.as_ref().and_then(|v| v.image.as_ref()),
                    layout.continue_.as_ref().and_then(|v| v.image.as_ref()),
                ]
                .into_iter()
                .flatten()
                {
                    out.push(("dialogue.responsive.image", image, FileKind::Image));
                }
            }
        }
        for (field, font) in [
            ("fonts.body.file", &self.fonts.body),
            ("fonts.name.file", &self.fonts.name),
            ("fonts.accent.file", &self.fonts.accent),
        ] {
            if let Some(f) = &font.file {
                out.push((field, f.as_str(), FileKind::Font));
            }
        }
        if let Some(f) = self.fonts.latin.as_ref().and_then(|f| f.file.as_deref()) {
            out.push(("fonts.latin.file", f, FileKind::Font));
        }
        for f in &self.sounds.typing.files {
            out.push(("sounds.typing.files", f.as_str(), FileKind::Sound));
        }
        for (field, s) in [
            ("sounds.open", &self.sounds.open),
            ("sounds.advance", &self.sounds.advance),
        ] {
            if let Some(s) = s {
                out.push((field, s.as_str(), FileKind::Sound));
            }
        }
        for s in self.sounds.se.values() {
            out.push(("sounds.se", s.as_str(), FileKind::Sound));
        }
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    StrokeData,
    Shader,
    Image,
    NineSlice([u32; 4]),
    Font,
    Sound,
}
