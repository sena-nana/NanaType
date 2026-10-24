//! OBS link: the obs-websocket connection that sizes each Spout/Syphon
//! output to the bounding box OBS gives its source in the program scene.
//! Without OBS (off, unreachable, no source, no box) an output keeps its
//! fixed size.

use std::path::Path;
use std::sync::Arc;

use nanatype_obs::{ObsConfig, ObsEvent, ObsItem, ObsScene, ObsStatus, ObsUpdate};
use nanatype_secrets::SecretStore;
use nanatype_settings::{Settings, SpoutOutput, fit_output_size};

use crate::app::App;
use crate::msg::Msg;
use crate::output::KINDS;
use crate::ui::LinkState;
use crate::windows::WindowKind;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub status: ObsStatus,
    /// The program scene, while connected.
    pub scene: Option<Arc<ObsScene>>,
    /// A password is kept in the secret store.
    pub password_saved: bool,
}

/// Where one output's size comes from, for its settings row.
#[derive(Debug, Clone, PartialEq)]
pub enum Sizing {
    /// OBS is not linked, or no source is chosen: the fixed size.
    Manual,
    /// OBS is not connected (yet): the fixed size meanwhile.
    Offline,
    /// The program scene does not hold the source.
    Missing { scene: String },
    /// The source has no bounding box; [`App::obs_fit_bounds`] gives it one.
    NoBounds { scene: String },
    /// Following the box; `size` is what is sent (`bounds` fitted to 1080p).
    Following { bounds: [f64; 2], size: [u32; 2] },
}

/// What `out` is sized by.
pub fn sizing(s: &Settings, state: &State, out: &SpoutOutput) -> Sizing {
    if !s.output.obs.enabled || out.obs_source.is_empty() {
        return Sizing::Manual;
    }
    let Some(scene) = state.scene.as_deref() else {
        return Sizing::Offline;
    };
    let Some(item) = scene.item(&out.obs_source) else {
        return Sizing::Missing {
            scene: scene.name.clone(),
        };
    };
    match item
        .transform
        .bounds
        .and_then(|b| Some((b, fit_output_size(b[0], b[1])?)))
    {
        Some((bounds, size)) => Sizing::Following { bounds, size },
        None => Sizing::NoBounds {
            scene: scene.name.clone(),
        },
    }
}

/// The sources a user can pick: capture sources first, then any other
/// input in the program scene (not groups or nested scenes).
pub fn candidates(scene: &ObsScene) -> Vec<&ObsItem> {
    let mut items: Vec<&ObsItem> = scene
        .items
        .iter()
        .filter(|i| !i.group && !i.kind.is_empty())
        .collect();
    items.sort_by_key(|i| !i.is_capture());
    items
}

/// The size a new bounding box takes: the source as OBS draws it now, else
/// what NanaType sends, else the window's size.
pub fn box_size(item: &ObsItem, out: &SpoutOutput, window: Option<[f32; 2]>) -> [u32; 2] {
    let [w, h] = item.transform.size;
    fit_output_size(w, h)
        .or_else(|| {
            out.size
                .and_then(|[w, h]| fit_output_size(w.into(), h.into()))
        })
        .or_else(|| window.and_then(|[w, h]| fit_output_size(w.into(), h.into())))
        .unwrap_or([1280, 720])
}

/// The obs-websocket password, read from the secret store the first time.
pub struct Password {
    store: SecretStore,
    value: Option<Option<String>>,
}

impl Password {
    pub fn new(config_dir: &Path) -> Self {
        Self {
            store: SecretStore::new(config_dir, "obs-websocket"),
            value: None,
        }
    }

    fn get(&mut self) -> Option<String> {
        self.value
            .get_or_insert_with(|| match self.store.load() {
                Ok(bytes) => bytes
                    .and_then(|b| String::from_utf8(b).ok())
                    .filter(|p| !p.is_empty()),
                Err(e) => {
                    nanatype_diag::report!(crate::diag::OBS_SECRET_FAILED, op = 0u64; "{e}");
                    None
                }
            })
            .clone()
    }

    /// Keep `password` (or forget it when empty). It stays for this run
    /// even when the system will not store it.
    fn set(&mut self, password: &str) -> std::io::Result<()> {
        self.value = Some((!password.is_empty()).then(|| password.to_owned()));
        if password.is_empty() {
            self.store.clear()
        } else {
            self.store.save(password.as_bytes())
        }
    }
}

impl App {
    pub(super) fn obs_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Obs(event) => self.obs_event(event),
            Msg::ObsPassword(password) => {
                if let Err(e) = self.obs_password.set(&password) {
                    nanatype_diag::report!(crate::diag::OBS_SECRET_FAILED, op = 1u64; "{e}");
                    let text = crate::ui::I18N
                        .t_in(self.ui.locale.get_untracked(), "obs-password-not-stored");
                    self.notice(text, true);
                }
                self.obs_configure();
                self.publish_obs_sizes();
            }
            Msg::ObsFitBounds(kind) => self.obs_fit_bounds(kind),
            _ => unreachable!("not an OBS message"),
        }
    }

    fn obs_event(&mut self, event: ObsEvent) {
        if event.epoch != self.obs.epoch() {
            return;
        }
        match event.update {
            ObsUpdate::Status(status) => {
                if let ObsStatus::Error(fault) = &status {
                    nanatype_diag::report!(crate::diag::OBS_ERROR; "{fault}");
                }
                let refused = matches!(status, ObsStatus::AuthFailed | ObsStatus::NeedsPassword);
                self.obs_state(|s| s.status = status);
                let link = self.obs_link_state();
                if refused && let LinkState::Error(text) = &link {
                    self.notice(text.clone(), true);
                }
                self.edit_status(|s| s.obs = link);
            }
            ObsUpdate::Scene(scene) => {
                self.obs_state(|s| s.scene = scene);
                let link = self.obs_link_state();
                self.edit_status(|s| s.obs = link);
                self.publish_obs_sizes();
            }
            ObsUpdate::Refused(comment) => {
                nanatype_diag::report!(crate::diag::OBS_ERROR; "refused: {comment}");
                let text = self.ui.tr("obs-refused", &[("reason", comment.into())]);
                self.notice(text, true);
            }
        }
    }

    fn obs_state(&self, f: impl FnOnce(&mut State)) {
        self.ui.obs.update(|s| f(Arc::make_mut(s)));
    }

    /// Connect, reconnect or stop after a settings change, and size the
    /// outputs from what is known. The caller publishes the settings.
    pub(super) fn obs_configure(&mut self) {
        let link = &self.settings.output.obs;
        let config = (link.enabled && !link.host.trim().is_empty()).then(|| ObsConfig {
            host: link.host.trim().to_owned(),
            port: link.port,
            password: None,
        });
        // Only read the secret store once OBS is wanted.
        let config = config.map(|c| ObsConfig {
            password: self.obs_password.get(),
            ..c
        });
        let epoch = self.obs.epoch();
        let connecting = config.is_some();
        self.obs.set(config);
        let renewed = self.obs.epoch() != epoch;
        let saved = self
            .obs_password
            .value
            .as_ref()
            .is_some_and(Option::is_some);
        self.obs_state(|s| {
            s.password_saved = saved;
            if renewed {
                s.scene = None;
                s.status = if connecting {
                    ObsStatus::Connecting
                } else {
                    ObsStatus::Off
                };
            }
        });
        let link = self.obs_link_state();
        self.edit_status(|s| s.obs = link);
        self.stamp_obs_sizes();
    }

    /// Write each output's OBS size into the live settings (never saved).
    /// Whether any changed.
    fn stamp_obs_sizes(&mut self) -> bool {
        let state = self.ui.obs.get_untracked();
        let mut changed = false;
        for kind in KINDS {
            let size =
                crate::output::settings_for(kind, &self.settings).and_then(|out| {
                    match sizing(&self.settings, &state, out) {
                        Sizing::Following { size, .. } => Some(size),
                        _ => None,
                    }
                });
            if let Some(out) = output_mut(kind, &mut self.settings)
                && out.obs_size != size
            {
                out.obs_size = size;
                changed = true;
            }
        }
        changed
    }

    /// [`App::stamp_obs_sizes`], then let the page and the senders know. A
    /// size is not configuration: no service restarts and nothing is saved.
    fn publish_obs_sizes(&mut self) {
        if self.stamp_obs_sizes() {
            self.ui.settings.set(Arc::new(self.settings.clone()));
            self.refresh_spout_status();
        }
    }

    /// Give the output's OBS source a bounding box at the size it is drawn.
    fn obs_fit_bounds(&mut self, kind: WindowKind) {
        let state = self.ui.obs.get_untracked();
        let Some(out) = crate::output::settings_for(kind, &self.settings) else {
            return;
        };
        let Some(item) = state.scene.as_deref().and_then(|s| s.item(&out.obs_source)) else {
            return;
        };
        let window = kind.overlay(&self.settings).and_then(|o| o.size);
        let size = box_size(item, out, window);
        self.obs.set_bounds(&out.obs_source, size);
    }

    fn obs_link_state(&self) -> LinkState {
        let locale = self.ui.locale.get_untracked();
        let state = self.ui.obs.get_untracked();
        let t = |key: &str| crate::ui::I18N.t_in(locale, key);
        match &state.status {
            ObsStatus::Off => LinkState::Off,
            ObsStatus::Connecting => LinkState::Connecting,
            ObsStatus::Connected => match &state.scene {
                Some(scene) => LinkState::Connected(crate::ui::I18N.tr_in(
                    locale,
                    "obs-status-connected",
                    &[("scene", scene.name.clone().into())],
                )),
                None => LinkState::Connecting,
            },
            ObsStatus::NeedsPassword => LinkState::Error(t("obs-status-needs-password")),
            ObsStatus::AuthFailed => LinkState::Error(t("obs-status-auth-failed")),
            ObsStatus::Error(fault) => LinkState::Error(crate::ui::link_text::fault(locale, fault)),
        }
    }
}

fn output_mut(kind: WindowKind, s: &mut Settings) -> Option<&mut SpoutOutput> {
    match kind {
        WindowKind::Display => Some(&mut s.output.display),
        WindowKind::PeerDisplay => Some(&mut s.output.peer_display),
        WindowKind::History => Some(&mut s.output.history),
        _ => None,
    }
}
