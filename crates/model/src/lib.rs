//! The dialogue document every NanaType surface agrees on.
//!
//! A [`Document`] is pure data: styled text runs, ruby groups, inline atoms
//! (stickers, emotes, images) and zero-width performance markers. It holds no
//! pixels and no window, GPU or audio types, so it serializes, diffs and tests
//! without a host. The editor, the typewriter and the history list all render
//! the same document; only the reveal state differs.

#![forbid(unsafe_code)]

mod chars;
mod color;
mod document;
mod envelope;
mod line;
mod link;
pub mod outgoing;
mod source;
mod style;

#[cfg(test)]
mod tests;

pub use chars::{CharClass, PunctKind, classify, is_cjk_grapheme};
pub use color::{ParseColorError, Rgba8};
pub use document::{
    AssetRef, Atom, AtomKind, Document, Inline, Item, MAX_UNITS, Marker, Mention, Paragraph,
    ParagraphAlign, RubyGroup, TextRun, Unit, UnitKind, VtsAction, VtsReset,
};
pub use envelope::{DOCUMENT_FORMAT, DOCUMENT_VERSION, DocumentEnvelope, EnvelopeError};
pub use line::{AfterLine, Line, LineId, LineMeta, Speaker, unix_millis};
pub use link::LinkFault;
pub use source::{
    ChatRef, ChatRun, ChatUser, HistoryItem, MessageSource, SourceEvent, SourceId, SourceKind,
};
pub use style::{
    Continuous, ContinuousKind, Effects, Entrance, EntranceKind, FontSlot, Overflow, Pace, Shadow,
    Size, Stroke, Style,
};
