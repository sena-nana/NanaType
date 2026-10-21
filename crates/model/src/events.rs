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
