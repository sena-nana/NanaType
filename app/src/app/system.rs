use super::App;
use crate::msg::{Msg, Shortcut};
use crate::services::sounds;
use crate::ui::{Performance, problem_text};
use nanatype_audio::AudioStatus;
use nanatype_core::Cmd;
use nanatype_settings::{Hotkey, PerformanceSettings};
use std::sync::Arc;
use std::time::Duration;

impl App {
    pub(super) fn record_hotkey(&mut self, action: Option<Shortcut>) {
        self.ui.recording.set(action);
        self.apply_services();
    }

    pub(super) fn hotkey_captured(&mut self, hotkey: Hotkey) {
        if let Some(action) = self.ui.recording.get_untracked() {
            self.ui.recording.set(None);
            // A chord can belong to one action only.
            self.edit(move |s| action.bind(s, Some(hotkey)));
            self.apply_services();
        }
    }

    /// Applies the scheduling preferences and keeps what they achieved for
    /// the Output page. A preference that changed from `prev` and that the
    /// system refused is also reported as a notice.
    pub(super) fn apply_performance(&mut self, prev: Option<&PerformanceSettings>) {
        let wanted = self.settings.performance.clone();
        let mut status = (*self.ui.status.get_untracked()).clone();
        let performance = Performance {
            gpu_high: nanatype_scheduling::apply_gpu(wanted.gpu_high),
            cpu: nanatype_scheduling::apply(wanted.cpu_high, wanted.cpu_performance_cores),
            gpu: nanatype_scheduling::gpu_choice(),
            gpu_running: status.performance.gpu_running.take(),
        };
        status.performance = performance.clone();
        self.ui.status.set(Arc::new(status));
        let Some(prev) = prev else {
            return;
        };
        let mut problems = Vec::new();
        if wanted.gpu_high && !prev.gpu_high && !performance.gpu_high {
            problems.push(self.ui.now_t("perf-error-gpu"));
        }
        if wanted.cpu_high != prev.cpu_high
            && let Some(problem) = &performance.cpu.priority_error
        {
            problems.push(problem_text(self.ui, problem));
        }
        if wanted.cpu_performance_cores != prev.cpu_performance_cores
            && let Some(problem) = &performance.cpu.affinity_error
            && !problem.not_needed()
        {
            problems.push(problem_text(self.ui, problem));
        }
        for text in problems {
            self.notice(text, true);
        }
    }

    /// Names the renderer's GPU for the Output page, once. Returns whether
    /// it was new.
    #[cfg(windows)]
    pub(crate) fn note_gpu(&self, gpu: &nana_ui::GpuContext) -> bool {
        if self
            .ui
            .status
            .with_untracked(|s| s.performance.gpu_running.is_some())
        {
            return false;
        }
        let name = gpu.wgpu().adapter_info().name.clone();
        self.edit_status(|s| s.performance.gpu_running = Some(name));
        true
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

    /// Package the logs off the UI thread; the result comes back as
    /// [`Msg::DiagnosticsExported`].
    pub(super) fn export_diagnostics(&mut self) {
        let paths = self.paths.clone();
        let locale = self.ui.locale.get_untracked();
        let summary = crate::services::diagnostics::summary(&self.settings, locale.tag());
        let ctx = self.ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("nanatype-diagnostics".into())
            .spawn(move || {
                let result = crate::services::diagnostics::export(&paths, &summary);
                ctx.dispatch(Msg::DiagnosticsExported(result));
            });
        match spawned {
            Ok(_) => {
                let text = self.ui.now_t("diagnostics-exporting");
                self.notice(text, false);
            }
            Err(e) => self.diagnostics_exported(Err(e.to_string())),
        }
    }

    pub(super) fn diagnostics_exported(&mut self, result: Result<std::path::PathBuf, String>) {
        match result {
            Ok(archive) => {
                let name = archive
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let text = self.ui.tr("diagnostics-exported", &[("file", name.into())]);
                self.notice(text, false);
                crate::services::reveal(&self.paths.logs);
            }
            Err(e) => {
                let text = self
                    .ui
                    .tr("diagnostics-export-failed", &[("message", e.into())]);
                self.notice(text, true);
            }
        }
    }

    /// Point out, once, that the last run crashed and a package can be sent.
    pub(super) fn notice_unseen_crash(&mut self) {
        if let Some(at) = crate::services::diagnostics::unseen_crash(&self.settings) {
            let text = self.ui.now_t("diagnostics-crash-notice");
            self.notice(text, true);
            self.edit(move |s| s.diagnostics.crash_seen = at);
        }
    }

    pub(super) fn audio_status(&mut self, status: AudioStatus) {
        match &status {
            AudioStatus::Running { rate, .. } => {
                nanatype_diag::event!(crate::diag::AUDIO_RUNNING, rate = *rate)
            }
            AudioStatus::Lost(e) => {
                nanatype_diag::report!(crate::diag::AUDIO_FAILED, lost = true; "{e}")
            }
            AudioStatus::Failed(e) => {
                nanatype_diag::report!(crate::diag::AUDIO_FAILED, lost = false; "{e}")
            }
        }
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
        self.speech.service.stop();
        self.quitting = true;
        crate::ui::shader_effects::close_guard();
        self.save_geometry();
        self.persist.flush(Duration::from_secs(2));
        nanatype_diag::marker("quit");
        nanatype_diag::flush_for_exit();
    }
}
