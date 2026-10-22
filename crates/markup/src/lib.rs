//! A BBCode-style text form of [`Document`].
//!
//! ```text
//! [@auto=1500 after=fade]
//! [b][color=#e86a92]你好[/color][/b]，[wait=400][shake]我是[ruby=なな]奈奈[/ruby][/shake]！
//! [pause][se=coin][sticker=lib:cats/hello][at=1234 name="观众"]
//! ```
//!
//! Quick phrases, templates, the rich clipboard and the editor's source view
//! all use it. Parsing never fails: unknown or malformed tags stay in the text
//! as typed and are reported as [`Diagnostic`]s, so a round trip never loses
//! what the user wrote. `[[` is a literal `[`.

#![forbid(unsafe_code)]

mod parse;
mod serialize;
mod template;
mod value;

#[cfg(test)]
mod tests;

pub use parse::{Diagnostic, DiagnosticKind, Parsed, parse};
pub use serialize::{serialize, serialize_line};
pub use template::expand_template;

/// Tag names the parser understands, for editor autocompletion.
pub const SPAN_TAGS: &[&str] = &[
    "align", "b", "i", "u", "s", "color", "size", "stroke", "shadow", "font", "ruby", "shake",
    "wave", "jump", "rainbow", "pulse", "flicker", "fade", "pop", "slide", "em", "speed",
    "instant",
];
pub const POINT_TAGS: &[&str] = &[
    "wait",
    "pause",
    "page",
    "se",
    "expr",
    "portrait",
    "screenshake",
    "vts",
    "sticker",
    "emote",
    "img",
    "at",
];
