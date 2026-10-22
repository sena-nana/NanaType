use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Straight (not premultiplied) sRGB colour. Serialized as `#rrggbb` or `#rrggbbaa`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Rgba8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba8 {
    pub const WHITE: Self = Self::rgb(255, 255, 255);
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub fn with_alpha(self, a: u8) -> Self {
        Self { a, ..self }
    }

    /// `#rrggbb` when opaque, `#rrggbbaa` otherwise. Lowercase.
    pub fn to_hex(self) -> String {
        if self.a == 255 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        }
    }

    /// WCAG relative luminance of the colour, ignoring alpha.
    pub fn luminance(self) -> f32 {
        let lin = |v: u8| {
            let v = f32::from(v) / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(self.r) + 0.7152 * lin(self.g) + 0.0722 * lin(self.b)
    }

    /// WCAG contrast ratio against `other`, treating both as opaque.
    pub fn contrast(self, other: Self) -> f32 {
        let (a, b) = (self.luminance(), other.luminance());
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseColorError(pub String);

impl fmt::Display for ParseColorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid colour `{}`", self.0)
    }
}

impl std::error::Error for ParseColorError {}

impl FromStr for Rgba8 {
    type Err = ParseColorError;

    /// Accepts `#rgb`, `#rgba`, `#rrggbb` and `#rrggbbaa`; the `#` is optional.
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let error = || ParseColorError(input.to_owned());
        let hex = input.trim().strip_prefix('#').unwrap_or(input.trim());
        if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(error());
        }
        let nibble = |i: usize| u8::from_str_radix(&hex[i..i + 1], 16).map(|v| v * 17);
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16);
        let parsed = match hex.len() {
            3 => (nibble(0), nibble(1), nibble(2), Ok(255)),
            4 => (nibble(0), nibble(1), nibble(2), nibble(3)),
            6 => (byte(0), byte(2), byte(4), Ok(255)),
            8 => (byte(0), byte(2), byte(4), byte(6)),
            _ => return Err(error()),
        };
        match parsed {
            (Ok(r), Ok(g), Ok(b), Ok(a)) => Ok(Self { r, g, b, a }),
            _ => Err(error()),
        }
    }
}

impl fmt::Display for Rgba8 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl Serialize for Rgba8 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Rgba8 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}
