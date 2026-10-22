//! Run attributes. Every field is optional: `None` inherits from the skin, so
//! switching skins restyles old lines that never chose a colour.

use serde::{Deserialize, Serialize};

use crate::Rgba8;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Size {
    /// Multiple of the skin body size.
    Em(f32),
    Px(f32),
}

impl Size {
    pub fn resolve(self, body_px: f32) -> f32 {
        match self {
            Self::Em(em) => body_px * em,
            Self::Px(px) => px,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub color: Rgba8,
    pub width_px: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Shadow {
    pub color: Rgba8,
    pub dx: f32,
    pub dy: f32,
    pub blur: f32,
}

/// Which skin font a run uses. A family name pins a system font.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FontSlot {
    Body,
    Name,
    Accent,
    Family(String),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Style {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<Rgba8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<Size>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub underline: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strike: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke: Option<Stroke>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shadow: Option<Shadow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font: Option<FontSlot>,
}

impl Style {
    pub fn is_plain(&self) -> bool {
        *self == Self::default()
    }

    /// `self` with every `Some` field of `over` replacing it.
    pub fn merged(&self, over: &Style) -> Style {
        Style {
            color: over.color.or(self.color),
            size: over.size.or(self.size),
            bold: over.bold.or(self.bold),
            italic: over.italic.or(self.italic),
            underline: over.underline.or(self.underline),
            strike: over.strike.or(self.strike),
            stroke: over.stroke.or(self.stroke),
            shadow: over.shadow.or(self.shadow),
            font: over.font.clone().or_else(|| self.font.clone()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContinuousKind {
    Shake,
    Wave,
    Jump,
    Rainbow,
    Pulse,
    Flicker,
}

impl ContinuousKind {
    pub const ALL: [Self; 6] = [
        Self::Shake,
        Self::Wave,
        Self::Jump,
        Self::Rainbow,
        Self::Pulse,
        Self::Flicker,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::Shake => "shake",
            Self::Wave => "wave",
            Self::Jump => "jump",
            Self::Rainbow => "rainbow",
            Self::Pulse => "pulse",
            Self::Flicker => "flicker",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.key() == key)
    }
}

/// A looping per-character effect. `amp` and `speed` are multiples of the
/// skin's defaults for that effect, so `1.0` always means "the skin's look".
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Continuous {
    pub kind: ContinuousKind,
    #[serde(default = "one")]
    pub amp: f32,
    #[serde(default = "one")]
    pub speed: f32,
}

fn one() -> f32 {
    1.0
}

impl Continuous {
    pub fn new(kind: ContinuousKind) -> Self {
        Self {
            kind,
            amp: 1.0,
            speed: 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntranceKind {
    /// Characters appear at full opacity.
    None,
    Fade,
    Pop,
    SlideUp,
}

impl EntranceKind {
    pub const ALL: [Self; 4] = [Self::None, Self::Fade, Self::Pop, Self::SlideUp];

    pub fn key(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Fade => "fade",
            Self::Pop => "pop",
            Self::SlideUp => "slide",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.key() == key)
    }
}

/// What the dialogue box does with text taller than itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Overflow {
    /// Lay the text out in full and cut it off at the box's bottom edge.
    Clip,
    /// Scroll up a line at a time as typing reaches the bottom edge.
    #[default]
    Scroll,
    /// Stop when the box is full, then clear it and go on (an automatic
    /// page break).
    Page,
}

impl Overflow {
    pub const ALL: [Self; 3] = [Self::Clip, Self::Scroll, Self::Page];

    pub fn key(self) -> &'static str {
        match self {
            Self::Clip => "clip",
            Self::Scroll => "scroll",
            Self::Page => "page",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|o| o.key() == key)
    }
}

/// How each character arrives during the reveal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entrance {
    pub kind: EntranceKind,
    pub ms: u16,
}

/// Bold, letter-spaced text read out one character at a time: `ms` of wait
/// before and after every character.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Emphasis {
    pub ms: u16,
}

impl Emphasis {
    pub const DEFAULT_MS: u16 = 100;
}

impl Default for Emphasis {
    fn default() -> Self {
        Self {
            ms: Self::DEFAULT_MS,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Effects {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub continuous: Vec<Continuous>,
    /// `None` uses the skin's entrance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entrance: Option<Entrance>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emphasis: Option<Emphasis>,
}

impl Effects {
    pub fn is_none(&self) -> bool {
        self.continuous.is_empty() && self.entrance.is_none() && self.emphasis.is_none()
    }

    pub fn has(&self, kind: ContinuousKind) -> bool {
        self.continuous.iter().any(|effect| effect.kind == kind)
    }

    /// Add `kind` if missing, remove it if present.
    pub fn toggle(&mut self, kind: ContinuousKind) {
        if let Some(index) = self.continuous.iter().position(|e| e.kind == kind) {
            self.continuous.remove(index);
        } else {
            self.continuous.push(Continuous::new(kind));
            self.continuous.sort_by_key(|e| e.kind as u8);
        }
    }
}

/// Reveal speed of a span relative to the line's pacing.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pace {
    #[default]
    Inherit,
    /// Multiplies the per-character delay; `0.5` is twice as fast.
    Scale(f32),
    /// Fixed delay per character, ignoring CJK and punctuation rules.
    FixedMs(u16),
    /// The whole span appears at once.
    Instant,
}

impl Pace {
    pub fn is_inherit(&self) -> bool {
        matches!(self, Self::Inherit)
    }
}
