//! External bridges, chat sources, OBS, plugins, and updates.

use nanatype_bridge::{BridgeHandle, LipSync, OscConfig, VtsConfig, WsConfig};
use nanatype_settings::Settings;

use super::blive_feeds::BliveFeedManager;
use super::secret::LazySecret;
use crate::services::chat_assets::ChatAssets;
use crate::ui::{EditShared, LinkState, Ui};

pub struct IntegrationsHub {
    pub bridge: BridgeHandle,
    pub twitch: nanatype_twitch::TwitchHandle,
    pub blive: BliveFeedManager,
    pub obs: nanatype_obs::ObsHandle,
    pub obs_password: LazySecret,
    #[cfg(feature = "updates")]
    pub updater: crate::services::updater::Updater,
    #[cfg(feature = "updates")]
    pub pending_update: super::update::Pending,
    pub chat_assets: ChatAssets,
    pub plugins: crate::plugins::Manager,
    pub plugin_activation: crate::plugins::Activation,
}

impl IntegrationsHub {
    pub fn apply_bridge_settings(&mut self, settings: &Settings, ui: Ui) {
        let i = &settings.integrations;
        self.bridge.set_vts(i.vts.enabled.then(|| VtsConfig {
            port: i.vts.port,
            token: i.vts.token.clone(),
            expressions: i.vts.expressions.clone(),
            lip_sync: (i.vts.lip_sync && !i.vts.mouth_param.trim().is_empty()).then(|| LipSync {
                param: i.vts.mouth_param.trim().to_owned(),
                amp: i.vts.mouth_amp,
            }),
            reset: i.vts.reset_expressions,
        }));
        self.bridge.set_osc(i.osc.enabled.then(|| OscConfig {
            host: i.osc.host.clone(),
            port: i.osc.port,
            prefix: i.osc.prefix.clone(),
        }));
        self.bridge.set_ws(i.websocket.enabled.then_some(WsConfig {
            port: i.websocket.port,
        }));
        ui.links.edit(|links| {
            if !i.vts.enabled {
                links.vts = LinkState::Off;
            }
            if !i.osc.enabled {
                links.osc = LinkState::Off;
            }
            if !i.websocket.enabled {
                links.ws = LinkState::Off;
            }
        });
    }
}

impl super::App {
    pub(super) fn bridge_event(&mut self, event: &nanatype_core::Event) {
        use nanatype_bridge::BridgeEvent;
        use nanatype_core::Event;
        let out = match event {
            Event::LineStarted(line) => BridgeEvent::LineStart {
                id: line.id.0,
                text: line.doc.plain_text(),
                markup: nanatype_markup::serialize(&line.doc),
                speaker: line.speaker.name.clone(),
            },
            Event::LineFinished(id) => BridgeEvent::LineEnd { id: id.0 },
            Event::Expression { name, intensity } => BridgeEvent::Expression {
                name: name.clone(),
                intensity: *intensity,
            },
            Event::Portrait(v) => BridgeEvent::Portrait { variant: v.clone() },
            // Read aloud, the voice moves the mouth instead (`bridge_speaking`).
            Event::Speaking(_) if self.settings.tts.enabled => return,
            Event::Speaking(s) => BridgeEvent::Speaking { speaking: *s },
            Event::Shake { amp, ms } => BridgeEvent::Shake { amp: *amp, ms: *ms },
            Event::Vts(action) => BridgeEvent::Vts {
                action: action.clone(),
            },
            Event::LineSubmitted(_) | Event::LineRevealed(_) | Event::Queued { .. } => return,
        };
        if self.integrations.plugins.has_running() {
            // Plugins can draw into the streamed display slot: never hand
            // them the user's absolute file paths.
            let plugin_out = match (&out, event) {
                (
                    BridgeEvent::LineStart {
                        id, text, speaker, ..
                    },
                    Event::LineStarted(line),
                ) => BridgeEvent::LineStart {
                    id: *id,
                    text: text.clone(),
                    markup: nanatype_markup::serialize(&super::without_local_paths(&line.doc)),
                    speaker: speaker.clone(),
                },
                _ => out.clone(),
            };
            if let Ok(event) = serde_json::to_value(&plugin_out) {
                self.integrations.plugins.broadcast(event, self.ui);
            }
        }
        self.integrations.bridge.emit(out);
    }

    /// A voice started or stopped talking.
    pub(super) fn bridge_speaking(&mut self, speaking: bool) {
        let out = nanatype_bridge::BridgeEvent::Speaking { speaking };
        if self.integrations.plugins.has_running()
            && let Ok(event) = serde_json::to_value(&out)
        {
            self.integrations.plugins.broadcast(event, self.ui);
        }
        self.integrations.bridge.emit(out);
    }

    pub(super) fn bridge_status(&mut self, status: nanatype_bridge::BridgeStatus) {
        use nanatype_bridge::{BridgeStatus, Sink};
        if let BridgeStatus::Error(sink, fault) = &status {
            let code = match sink {
                Sink::Vts => 0u64,
                Sink::Osc => 1,
                Sink::WebSocket => 2,
            };
            nanatype_diag::report!(crate::diag::BRIDGE_ERROR, sink = code; "{fault}");
        }
        let locale = self.ui.locale.get_untracked();
        if let Some((sink, state)) = crate::ui::link_text::bridge(locale, &status) {
            self.ui.links.edit(|links| match sink {
                Sink::Vts => links.vts = state,
                Sink::Osc => links.osc = state,
                Sink::WebSocket => links.ws = state,
            });
        }
        match status {
            BridgeStatus::VtsToken(token) => self.edit(|s| s.integrations.vts.token = Some(token)),
            BridgeStatus::VtsTokenRejected => self.edit(|s| s.integrations.vts.token = None),
            BridgeStatus::VtsHotkeys(keys) => {
                let list = keys.into_iter().map(|k| (k.id, k.name)).collect();
                self.ui.links.edit(|links| links.vts_hotkeys = list);
            }
            _ => {}
        }
    }
}
