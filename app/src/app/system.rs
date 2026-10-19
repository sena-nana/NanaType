use super::App;
use crate::msg::GlobalAction;
use crate::services::sounds;
use nanatype_audio::AudioStatus;
use nanatype_core::Cmd;
use nanatype_settings::Hotkey;
use std::time::Duration;

impl App {
    pub(super) fn record_hotkey(&mut self, action: Option<GlobalAction>) {
        self.ui.recording.set(action);
        self.apply_services();
    }

    pub(super) fn hotkey_captured(&mut self, hotkey: Hotkey) {
        if let Some(action) = self.ui.recording.get_untracked() {
            self.ui.recording.set(None);
            self.edit(|s| {
                // A chord can belong to one action only.
                let taken: Vec<GlobalAction> = GlobalAction::all_in(s)
                    .filter(|o| *o != action && o.get(s) == Some(&hotkey))
                    .collect();
                for other in taken {
                    other.set(s, None);
                }
                action.set(s, Some(hotkey));
            });
            self.apply_services();
        }
    }

    pub(super) fn copy_text(&mut self, text: String) {
        let ok = {
            use nana_ui_platform::ClipboardHost;
            nana_ui_platform::OsClipboard::new().write_text(&text)
        };
        if ok {
            let t = self.ui.now_t("common-copied");
            self.notice(t, false);
        }
    }

    pub(super) fn play_sample(&mut self) {
        // Development aid: `NANATYPE_DEMO_MARKUP` plays that line instead.
        let line = std::env::var("NANATYPE_DEMO_MARKUP")
            .ok()
            .filter(|_| cfg!(debug_assertions))
            .unwrap_or_else(|| self.ui.now_t("output-sample-line"));
        self.run(Cmd::SendMarkup(line));
    }

    pub(super) fn vts_authorize(&mut self) {
        if !self.settings.integrations.vts.enabled {
            self.edit(|s| s.integrations.vts.enabled = true);
        }
        self.bridge.vts_request_token();
    }

    pub(super) fn tray_error_expired(&mut self, id: u64) {
        if self.tray_error.as_ref().is_some_and(|(e, _)| *e == id) {
            self.tray_error = None;
        }
    }

    pub(super) fn audio_status(&mut self, status: AudioStatus) {
        let text = match &status {
            AudioStatus::Running { device, rate } => format!("{device} · {rate} Hz"),
            AudioStatus::Lost(e) | AudioStatus::Failed(e) => {
                let t = self.ui.tr("err-audio", &[("message", e.as_str().into())]);
                self.notice(t.clone(), true);
                t
            }
        };
        if matches!(status, AudioStatus::Running { .. }) {
            // A fresh stream starts with an empty mixer.
            sounds::install(&self.audio, &Vec::new(), &self.bank);
            self.audio_levels();
        }
        let devices = nanatype_audio::list_devices();
        self.edit_status(|s| {
            s.audio = text;
            s.audio_devices = devices;
        });
    }

    pub(super) fn quit(&mut self) {
        self.quitting = true;
        self.save_geometry();
        self.persist.flush(Duration::from_secs(2));
    }
}
