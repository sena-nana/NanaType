//! The events NanaType tells other tools about (VTube Studio, OSC,
//! WebSocket clients and plugins), in their wire shape.

use serde::Serialize;

use crate::VtsAction;

/// Something worth telling other tools.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BridgeEvent {
    LineStart {
        id: u64,
        text: String,
        markup: String,
        /// The character the line is said by.
        speaker: String,
        /// The line's translation, when it is already known (a replay or a
        /// repeated line); otherwise a `translation` event follows.
        #[serde(skip_serializing_if = "Option::is_none")]
        translation: Option<String>,
    },
    /// A sent line's translation arrived.
    Translation {
        /// The `line_start` id it belongs to.
        id: u64,
        text: String,
        /// The language code it is in (`en`, `zh-Hant`).
        target: String,
    },
    LineEnd {
        id: u64,
    },
    Expression {
        name: String,
        intensity: Option<f32>,
    },
    Portrait {
        variant: String,
    },
    Speaking {
        speaking: bool,
    },
    Shake {
        amp: f32,
        ms: u32,
    },
    /// A `[vts ...]` marker was reached.
    Vts {
        action: VtsAction,
    },
}
