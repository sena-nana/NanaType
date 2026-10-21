//! Message sources feeding the history list.
//!
//! The local typewriter is one source; live-platform chat (Bilibili danmaku
//! through the authenticated live connection) is another. Both arrive as [`HistoryItem`]s, so the
//! history view does not care where a row came from.

use serde::{Deserialize, Serialize};

use crate::{AssetRef, Document, LineId};

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SourceId(pub String);

impl SourceId {
    pub const LOCAL: &'static str = "local";

    pub fn local() -> Self {
        Self(Self::LOCAL.to_owned())
    }

    pub fn is_local(&self) -> bool {
        self.0 == Self::LOCAL
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// Lines the streamer typed.
    Typewriter,
    /// Viewer chat from a live platform.
    Chat,
    /// Gifts, memberships and other paid events.
    Event,
}

/// One piece of a live-platform message: text, or an inline platform
/// picture (emoji, emote) with its alt text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatRun {
    Text(String),
    Emote { alt: String, url: String },
}

/// A viewer on a live platform, as a line names or quotes them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChatUser {
    /// Platform user id; empty when the platform did not give one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub uid: String,
    pub name: String,
    /// Cached avatar picture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<AssetRef>,
}

/// A chat message a line answers: shown as a small quote above the line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatRef {
    pub user: ChatUser,
    /// The message as plain text, already shortened for display.
    pub text: String,
    #[serde(default = "chat_kind")]
    pub kind: SourceKind,
}

fn chat_kind() -> SourceKind {
    SourceKind::Chat
}

impl ChatRef {
    /// Longest quote kept, in characters; longer text ends in an ellipsis.
    pub const MAX_CHARS: usize = 60;

    pub fn new(user: ChatUser, text: &str, kind: SourceKind) -> Self {
        let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let text = if flat.chars().count() > Self::MAX_CHARS {
            let mut cut: String = flat.chars().take(Self::MAX_CHARS - 1).collect();
            cut.push('…');
            cut
        } else {
            flat
        };
        Self { user, text, kind }
    }
}

/// One row of the history list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryItem {
    pub source: SourceId,
    pub kind: SourceKind,
    /// Present for typewriter lines, so the row can be recalled into the editor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<LineId>,
    pub author: String,
    /// Avatar reference understood by the source (asset ref or URL).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
    pub doc: Document,
    /// Unix milliseconds.
    pub at: u64,
    /// The platform user id of a chat row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,
    /// The chat message this row answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply: Option<Box<ChatRef>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SourceEvent {
    Item(Box<HistoryItem>),
    /// The source connected, reconnected or failed; `detail` is for the UI.
    Status {
        connected: bool,
        detail: String,
    },
}

/// A producer of history rows. Implementations run on their own thread and
/// deliver through `emit`, which wakes the UI; they never touch UI state.
pub trait MessageSource: Send {
    fn id(&self) -> SourceId;
    fn kind(&self) -> SourceKind;
    /// Begin producing. `emit` may be called from any thread.
    fn start(&mut self, emit: Box<dyn Fn(SourceEvent) + Send + Sync>);
    fn stop(&mut self);
}
