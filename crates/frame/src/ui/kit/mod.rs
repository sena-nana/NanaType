//! NanaType's design system: the one theme every window installs
//! ([`theme`]), its typed tokens ([`Type`], [`gap`], [`inset`], [`field`],
//! [`Tone`]) and the parts application chrome is built from — cards, rows,
//! bars, tags, badges, dots and keycaps.
//!
//! Chrome views name these roles instead of numbers (a test holds them to
//! it), so changing how the app looks is a change here. Skin-drawn content
//! (the dialogue box, the history, quotes) is styled by the skin instead.

pub mod keycap;
mod layout;
mod parts;
pub mod theme;
mod tokens;

pub use layout::*;
pub use parts::*;
pub use tokens::*;
