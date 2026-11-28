//! Expression and speaking events for other streaming tools.
//!
//! * **VTube Studio**: a plugin client on the public API that turns on
//!   expressions and presses hotkeys as `[expr=...]` and `[vts ...]` markers
//!   are reached, and moves the model's mouth while the speaker talks.
//! * **NanaLive**: the same, as a plugin on NanaLive's control API.
//! * **OSC**: UDP messages under a prefix (`/nanatype/expression "smile"`),
//!   for VNyan, Warudo, VSeeFace and anything else that speaks OSC.
//! * **WebSocket broadcast**: a localhost server that pushes JSON events to
//!   every connected client (browser sources, custom scripts).
//!
//! Each sink runs on its own thread; [`BridgeHandle::emit`] never blocks.

mod avatar;
mod events;
mod nanalive;
mod osc;
mod vts;
mod ws;

pub use avatar::{AvatarConfig, AvatarHotkey, LipSync, mapping_key};
pub use events::{BridgeEvent, BridgeStatus, Sink};
pub use nanalive::DEFAULT_PORT as NANALIVE_PORT;
pub use osc::OscConfig;
pub use ws::WsConfig;

use std::sync::Arc;

use avatar::{AvatarClient, Protocol};

type StatusFn = Arc<dyn Fn(BridgeStatus) + Send + Sync>;

/// Owns the sink threads. Dropping it stops them.
pub struct BridgeHandle {
    status: StatusFn,
    vts: Option<AvatarClient>,
    nanalive: Option<AvatarClient>,
    osc: Option<osc::OscSender>,
    ws: Option<ws::WsServer>,
}

/// Start, update or stop an avatar client. Only a new port reconnects.
fn set_avatar<P: Protocol>(
    slot: &mut Option<AvatarClient>,
    config: Option<AvatarConfig>,
    status: &StatusFn,
) {
    match (slot.as_mut(), config) {
        (Some(c), Some(cfg)) if c.port() == cfg.port => c.update(cfg),
        (_, Some(cfg)) => *slot = Some(AvatarClient::start::<P>(cfg, status.clone())),
        (_, None) => {
            if slot.take().is_some() {
                status(BridgeStatus::Off(P::SINK));
            }
        }
    }
}

impl BridgeHandle {
    pub fn new(status: impl Fn(BridgeStatus) + Send + Sync + 'static) -> Self {
        Self {
            status: Arc::new(status),
            vts: None,
            nanalive: None,
            osc: None,
            ws: None,
        }
    }

    fn avatar(&self, sink: Sink) -> Option<&AvatarClient> {
        match sink {
            Sink::Vts => self.vts.as_ref(),
            Sink::NanaLive => self.nanalive.as_ref(),
            Sink::Osc | Sink::WebSocket => None,
        }
    }

    /// Start, update or stop VTube Studio. Only a new port reconnects.
    pub fn set_vts(&mut self, config: Option<AvatarConfig>) {
        set_avatar::<vts::Vts>(&mut self.vts, config, &self.status);
    }

    /// Start, update or stop NanaLive. Only a new port reconnects.
    pub fn set_nanalive(&mut self, config: Option<AvatarConfig>) {
        set_avatar::<nanalive::NanaLive>(&mut self.nanalive, config, &self.status);
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

    /// Ask the avatar app (VTube Studio or NanaLive) for a plugin token; the
    /// user confirms inside the app.
    pub fn request_token(&self, sink: Sink) {
        if let Some(c) = self.avatar(sink) {
            c.request_token();
        }
    }

    pub fn refresh_hotkeys(&self, sink: Sink) {
        if let Some(c) = self.avatar(sink) {
            c.refresh_hotkeys();
        }
    }

    /// The voice reading the line began a word of `chars` characters: the
    /// avatar's mouth opens with it instead of flapping on its own.
    pub fn word(&self, chars: usize) {
        for c in [&self.vts, &self.nanalive].into_iter().flatten() {
            c.word(chars);
        }
    }

    pub fn emit(&self, event: BridgeEvent) {
        for c in [&self.vts, &self.nanalive].into_iter().flatten() {
            c.emit(&event);
        }
        if let Some(o) = &self.osc {
            o.emit(&event);
        }
        if let Some(w) = &self.ws {
            w.emit(&event);
        }
    }
}
