//! Expression and speaking events for other streaming tools.
//!
//! * **VTube Studio**: a plugin client on the public API that turns on
//!   expressions and presses hotkeys as `[expr=...]` and `[vts ...]` markers
//!   are reached, and moves the model's mouth while the speaker talks.
//! * **OSC**: UDP messages under a prefix (`/nanatype/expression "smile"`),
//!   for VNyan, Warudo, VSeeFace and anything else that speaks OSC.
//! * **WebSocket broadcast**: a localhost server that pushes JSON events to
//!   every connected client (browser sources, NanaLive, custom scripts).
//!
//! Each sink runs on its own thread; [`BridgeHandle::emit`] never blocks.

mod events;
mod osc;
mod vts;
mod ws;

pub use events::{BridgeEvent, BridgeStatus, Sink};
pub use osc::OscConfig;
pub use vts::{LipSync, VtsConfig, VtsHotkey, mapping_key};
pub use ws::WsConfig;

use std::sync::Arc;

type StatusFn = Arc<dyn Fn(BridgeStatus) + Send + Sync>;

/// Owns the sink threads. Dropping it stops them.
pub struct BridgeHandle {
    status: StatusFn,
    vts: Option<vts::VtsClient>,
    osc: Option<osc::OscSender>,
    ws: Option<ws::WsServer>,
}

impl BridgeHandle {
    pub fn new(status: impl Fn(BridgeStatus) + Send + Sync + 'static) -> Self {
        Self {
            status: Arc::new(status),
            vts: None,
            osc: None,
            ws: None,
        }
    }

    /// Start, update or stop VTube Studio. Only a new port reconnects.
    pub fn set_vts(&mut self, config: Option<VtsConfig>) {
        match (&mut self.vts, config) {
            (Some(c), Some(cfg)) if c.port() == cfg.port => c.update(cfg),
            (_, Some(cfg)) => self.vts = Some(vts::VtsClient::start(cfg, self.status.clone())),
            (_, None) => {
                if self.vts.take().is_some() {
                    (self.status)(BridgeStatus::Off(Sink::Vts));
                }
            }
        }
    }

    pub fn set_osc(&mut self, config: Option<OscConfig>) {
        match (&self.osc, config) {
            (Some(c), Some(cfg)) if c.config() == &cfg => {}
            (_, Some(cfg)) => self.osc = osc::OscSender::start(cfg, self.status.clone()),
            (_, None) => {
                if self.osc.take().is_some() {
                    (self.status)(BridgeStatus::Off(Sink::Osc));
                }
            }
        }
    }

    pub fn set_ws(&mut self, config: Option<WsConfig>) {
        match (&self.ws, config) {
            (Some(c), Some(cfg)) if c.config() == &cfg => {}
            (_, Some(cfg)) => self.ws = Some(ws::WsServer::start(cfg, self.status.clone())),
            (_, None) => {
                if self.ws.take().is_some() {
                    (self.status)(BridgeStatus::Off(Sink::WebSocket));
                }
            }
        }
    }

    /// Ask VTube Studio for a plugin token (the user confirms inside VTS).
    pub fn vts_request_token(&self) {
        if let Some(v) = &self.vts {
            v.request_token();
        }
    }

    pub fn vts_refresh_hotkeys(&self) {
        if let Some(v) = &self.vts {
            v.refresh_hotkeys();
        }
    }

    pub fn emit(&self, event: BridgeEvent) {
        if let Some(v) = &self.vts {
            v.emit(&event);
        }
        if let Some(o) = &self.osc {
            o.emit(&event);
        }
        if let Some(w) = &self.ws {
            w.emit(&event);
        }
    }
}
