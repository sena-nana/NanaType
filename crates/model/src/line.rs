use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::{ChatRef, Document, Rgba8};

/// Unique within a process run; ordered by creation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LineId(pub u64);

impl LineId {
    pub fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// Who said the line, copied when it was sent so later setting edits do not
/// rewrite history.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Speaker {
    pub name: String,
    /// Avatar captured at submission; independent from later account changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
    /// Portrait variant active when the line started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub portrait: Option<String>,
    /// Colour of the name on the name plate; `None` keeps the skin's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Rgba8>,
}

/// What the typewriter does after the last character.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AfterLine {
    /// Stay until the next line replaces it.
    #[default]
    Hold,
    /// Fade out after `delay_ms + per_unit_ms × units`.
    Fade { delay_ms: u32, per_unit_ms: u32 },
    /// Disappear without a fade after `delay_ms`.
    Clear { delay_ms: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LineMeta {
    /// Continue to the next queued line this long after the reveal ends.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_advance_ms: Option<u32>,
    /// Overrides the settings' after-line behaviour for this line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<AfterLine>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub id: LineId,
    pub doc: Document,
    pub speaker: Speaker,
    #[serde(default)]
    pub meta: LineMeta,
    /// Unix milliseconds.
    pub created_at: u64,
    /// The chat message this line answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply: Option<Box<ChatRef>>,
}

impl Line {
    pub fn new(doc: Document, speaker: Speaker) -> Self {
        Self {
            id: LineId::next(),
            doc,
            speaker,
            meta: LineMeta::default(),
            created_at: unix_millis(),
            reply: None,
        }
    }
}

pub fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
