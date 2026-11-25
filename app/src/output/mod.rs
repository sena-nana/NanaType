//! Spout2 (Windows) and Syphon (macOS) output of the dialogue and history windows.
//!
//! NanaUI paints each window's own scene a second time into an offscreen
//! texture. On Windows that texture is exported as a DX12 shared handle and
//! handed to a Spout sender. On macOS the same-device texture is blitted into
//! a Syphon server on the Metal queue. Nothing is read back to the CPU, and
//! output keeps flowing while the window is hidden.

pub mod config;
#[cfg(all(windows, debug_assertions))]
pub mod probe;
#[cfg(any(windows, test))]
mod spout;
#[cfg(target_os = "macos")]
mod syphon;

use std::collections::HashMap;
use std::sync::Arc;

use nana_ui::runtime::view::Signal;
use nana_ui::{GpuContext, WindowOutputConfig, WindowOutputFrame, WindowOutputStatus};
use nanatype_i18n::Locale;
use nanatype_settings::{Settings, SpoutOutput};

pub use config::{
    KINDS, OutputStatuses, RuntimeOutputSizes, SPOUT_ERROR_KEYS, SYPHON_ERROR_KEYS, config,
    name_conflicts, settings_for, settings_for_mut,
};
pub(crate) use config::{ResolvedOutput, resolve};

use crate::ui::{I18N, LinkState};
use crate::windows::WindowKind;

#[derive(Debug, Default)]
pub struct PublishOutcome {
    pub error: Option<(&'static str, String)>,
    #[allow(dead_code)]
    pub perf: Option<(bool, f64)>,
}

/// Platform-specific offscreen output backend (Spout2 on Windows, Syphon on macOS).
#[allow(unused_variables)]
pub trait OutputTransport {
    fn sync(
        &mut self,
        s: &Settings,
        runtime_sizes: &RuntimeOutputSizes,
        live: &[(WindowKind, &SpoutOutput)],
    );

    fn paused_output(&self, kind: WindowKind, now: u64) -> Option<&SpoutOutput> {
        None
    }

    fn paused_config(
        &self,
        kind: WindowKind,
        s: &Settings,
        runtime_sizes: &RuntimeOutputSizes,
    ) -> Option<WindowOutputConfig> {
        None
    }

    fn cleanup_deadline(&self) -> Option<u64> {
        None
    }

    fn prepare(
        &mut self,
        s: &Settings,
        runtime_sizes: &RuntimeOutputSizes,
        gpu: &GpuContext,
        locale: Locale,
    ) -> Vec<(WindowKind, LinkState)> {
        Vec::new()
    }

    fn publish(
        &mut self,
        kind: WindowKind,
        out: &SpoutOutput,
        runtime_size: Option<[u32; 2]>,
        frame: &WindowOutputFrame,
        gpu: &GpuContext,
    ) -> PublishOutcome;

    fn status_override(
        &self,
        kind: WindowKind,
        base: &LinkState,
        locale: Locale,
    ) -> Option<LinkState> {
        None
    }

    fn take_recovered(&mut self) -> Vec<WindowKind> {
        Vec::new()
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
#[derive(Default)]
struct NullTransport;

#[cfg(not(any(windows, target_os = "macos")))]
impl OutputTransport for NullTransport {
    fn sync(
        &mut self,
        _s: &Settings,
        _runtime_sizes: &RuntimeOutputSizes,
        _live: &[(WindowKind, &SpoutOutput)],
    ) {
    }

    fn publish(
        &mut self,
        _kind: WindowKind,
        _out: &SpoutOutput,
        _runtime_size: Option<[u32; 2]>,
        _frame: &WindowOutputFrame,
        _gpu: &GpuContext,
    ) -> PublishOutcome {
        PublishOutcome::default()
    }
}

#[cfg(windows)]
type PlatformTransport = spout::SpoutTransport;
#[cfg(target_os = "macos")]
type PlatformTransport = syphon::Outputs;
#[cfg(not(any(windows, target_os = "macos")))]
type PlatformTransport = NullTransport;

#[derive(Default)]
pub struct OutputManager {
    names: HashMap<WindowKind, String>,
    /// Last status reported by NanaUI per window, kept as the single source of truth
    /// so recovering senders restore without a duplicate shadow map.
    base_statuses: HashMap<WindowKind, LinkState>,
    cleanup_scheduled: Option<u64>,
    pub status: OutputStatuses,
    transport: PlatformTransport,
}

fn t(locale: Locale, key: &str) -> String {
    I18N.t_in(locale, key)
}

impl OutputManager {
    pub fn sync(
        &mut self,
        s: &Settings,
        runtime_sizes: &RuntimeOutputSizes,
        locale: Locale,
        output_status: Option<Signal<Arc<OutputStatuses>>>,
    ) {
        let mut live = Vec::new();
        for kind in KINDS {
            if let Some(out) = resolve(kind, s).output() {
                live.push((kind, out));
            }
        }
        OutputTransport::sync(&mut self.transport, s, runtime_sizes, &live);
        for kind in KINDS {
            let resolved = resolve(kind, s);
            let wanted = resolved.output();
            let renamed = wanted.is_some_and(|o| self.names.get(&kind) != Some(&o.name));
            let state = match resolved {
                ResolvedOutput::Off => LinkState::Off,
                ResolvedOutput::Invalid(key) => LinkState::Error(t(locale, key)),
                ResolvedOutput::Ready(_) if renamed => LinkState::Connecting,
                ResolvedOutput::Ready(_) => match self.status.get(kind) {
                    LinkState::Off => LinkState::Connecting,
                    state => state.clone(),
                },
            };
            if let Some(out) = wanted {
                if renamed {
                    self.names.insert(kind, out.name.clone());
                    self.base_statuses.insert(kind, LinkState::Connecting);
                }
            } else {
                self.names.remove(&kind);
                self.base_statuses.remove(&kind);
            }
            self.status.set(kind, state);
        }
        if let Some(sig) = output_status {
            self.publish_status(sig);
        }
    }

    pub fn take_cleanup_schedule(&mut self) -> Option<Option<u64>> {
        let next = self.transport.cleanup_deadline();
        if next != self.cleanup_scheduled {
            self.cleanup_scheduled = next;
            Some(next)
        } else {
            None
        }
    }

    pub(crate) fn paused_output(&self, kind: WindowKind, now: u64) -> Option<&SpoutOutput> {
        self.transport.paused_output(kind, now)
    }

    pub(crate) fn config(
        &self,
        kind: WindowKind,
        s: &Settings,
        runtime_sizes: &RuntimeOutputSizes,
    ) -> Option<WindowOutputConfig> {
        let resolved = resolve(kind, s);
        if let Some(cfg) = resolved.config(runtime_sizes.get(kind)) {
            return Some(cfg);
        }
        if matches!(resolved, ResolvedOutput::Off) {
            return self.transport.paused_config(kind, s, runtime_sizes);
        }
        None
    }

    pub fn status_changed(
        &mut self,
        kind: WindowKind,
        status: &WindowOutputStatus,
        locale: Locale,
        output_status: Option<Signal<Arc<OutputStatuses>>>,
    ) {
        if let Some(base) = crate::ui::link_text::spout(locale, status) {
            self.base_statuses.insert(kind, base.clone());
            let effective = self
                .transport
                .status_override(kind, &base, locale)
                .unwrap_or(base);
            if self.status.set(kind, effective)
                && let Some(sig) = output_status
            {
                self.publish_status(sig);
            }
        }
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn prepare(
        &mut self,
        s: &Settings,
        runtime_sizes: &RuntimeOutputSizes,
        gpu: &GpuContext,
        locale: Locale,
        output_status: Signal<Arc<OutputStatuses>>,
    ) {
        let failures = self.transport.prepare(s, runtime_sizes, gpu, locale);
        let mut changed = false;
        for (kind, state) in failures {
            changed |= self.status.set(kind, state);
        }
        for kind in self.transport.take_recovered() {
            let base = self
                .base_statuses
                .get(&kind)
                .cloned()
                .unwrap_or(LinkState::Connecting);
            changed |= self.status.set(kind, base);
        }
        if changed {
            self.publish_status(output_status);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn frame(
        &mut self,
        kind: WindowKind,
        out: &SpoutOutput,
        runtime_size: Option<[u32; 2]>,
        frame: &WindowOutputFrame,
        gpu: &GpuContext,
        locale: Locale,
        output_status: Signal<Arc<OutputStatuses>>,
    ) -> PublishOutcome {
        let outcome =
            OutputTransport::publish(&mut self.transport, kind, out, runtime_size, frame, gpu);
        let mut changed = false;
        if let Some((key, _)) = &outcome.error {
            changed |= self
                .status
                .set(kind, crate::ui::link_text::spout_send(locale, key));
        }
        for recovered in self.transport.take_recovered() {
            let base = self
                .base_statuses
                .get(&recovered)
                .cloned()
                .unwrap_or(LinkState::Connecting);
            changed |= self.status.set(recovered, base);
        }
        if changed {
            self.publish_status(output_status);
        }
        outcome
    }

    fn publish_status(&self, output_status: Signal<Arc<OutputStatuses>>) {
        if output_status.with_untracked(|cur| **cur != self.status) {
            output_status.set(Arc::new(self.status.clone()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nana_ui::{WindowOutputAlpha, WindowOutputExtent};

    #[test]
    fn invalid_names_and_renames_share_configuration_and_sender_lifetime() {
        if !(cfg!(windows) || cfg!(target_os = "macos")) {
            return;
        }
        let mut settings = Settings::default();
        let sizes = RuntimeOutputSizes::default();
        settings.output.display.enabled = true;
        settings.output.display.name = "   ".into();
        let mut outputs = OutputManager::default();
        outputs.sync(&settings, &sizes, Locale::ZhCn, None);
        assert!(config(WindowKind::Display, &settings).is_none());
        assert!(outputs.names.is_empty());
        assert_eq!(
            outputs.status.get(WindowKind::Display),
            &LinkState::Error(t(Locale::ZhCn, "spout-error-name"))
        );
        settings.output.display.name = "Renamed".into();
        outputs.sync(&settings, &sizes, Locale::ZhCn, None);
        assert!(config(WindowKind::Display, &settings).unwrap().while_hidden);
        assert_eq!(
            outputs.status.get(WindowKind::Display),
            &LinkState::Connecting
        );
        outputs
            .status
            .set(WindowKind::Display, LinkState::Connected("ready".into()));
        outputs.sync(&settings, &sizes, Locale::ZhCn, None);
        assert!(matches!(
            outputs.status.get(WindowKind::Display),
            LinkState::Connected(_)
        ));
        settings.output.display.name = "Renamed again".into();
        outputs.sync(&settings, &sizes, Locale::ZhCn, None);
        assert_eq!(
            outputs.status.get(WindowKind::Display),
            &LinkState::Connecting
        );
        assert_eq!(outputs.names[&WindowKind::Display], "Renamed again");
    }

    #[test]
    fn independent_outputs_keep_their_size_and_alpha() {
        if !(cfg!(windows) || cfg!(target_os = "macos")) {
            return;
        }
        let mut s = Settings::default();
        s.output.display.enabled = true;
        s.output.display.size = Some([1280, 720]);
        s.output.history.enabled = true;
        s.output.history.size = Some([640, 360]);
        s.output.history.straight_alpha = true;
        let own = config(WindowKind::Display, &s).unwrap();
        let hist = config(WindowKind::History, &s).unwrap();
        assert_eq!(
            own.extent,
            WindowOutputExtent::Fixed {
                width: 1280,
                height: 720
            }
        );
        assert_eq!(
            hist.extent,
            WindowOutputExtent::Fixed {
                width: 640,
                height: 360
            }
        );
        assert_eq!(own.alpha, WindowOutputAlpha::Premultiplied);
        assert_eq!(hist.alpha, WindowOutputAlpha::Straight);
        assert!(own.while_hidden && hist.while_hidden);
        s.output.history.enabled = false;
        assert!(config(WindowKind::History, &s).is_none());
        assert_eq!(config(WindowKind::Display, &s), Some(own));
    }

    #[test]
    fn duplicate_enabled_names_are_rejected() {
        let mut s = Settings::default();
        let sizes = RuntimeOutputSizes::default();
        s.output.display.enabled = true;
        s.output.history.enabled = true;
        s.output.history.name = s.output.display.name.clone();
        assert!(name_conflicts(WindowKind::Display, &s));
        assert!(name_conflicts(WindowKind::History, &s));
        assert!(!name_conflicts(WindowKind::Danmaku, &s));
        assert!(config(WindowKind::Display, &s).is_none());
        assert!(config(WindowKind::History, &s).is_none());
        let mut outputs = OutputManager::default();
        outputs.sync(&s, &sizes, Locale::ZhCn, None);
        assert!(matches!(
            outputs.status.get(WindowKind::History),
            LinkState::Error(_)
        ));
        s.output.history.enabled = false;
        assert!(!name_conflicts(WindowKind::Display, &s));
    }

    #[test]
    fn separate_danmaku_output_follows_separate_mode() {
        if !(cfg!(windows) || cfg!(target_os = "macos")) {
            return;
        }
        let mut s = Settings::default();
        let sizes = RuntimeOutputSizes::default();
        s.output.danmaku.enabled = true;
        let mut outputs = OutputManager::default();
        outputs.sync(&s, &sizes, Locale::ZhCn, None);
        assert!(matches!(
            outputs.status.get(WindowKind::Danmaku),
            LinkState::Off
        ));
        assert!(config(WindowKind::Danmaku, &s).is_none());

        s.history.chat.separate = true;
        outputs.sync(&s, &sizes, Locale::ZhCn, None);
        assert!(matches!(
            outputs.status.get(WindowKind::Danmaku),
            LinkState::Connecting
        ));
        assert!(config(WindowKind::Danmaku, &s).is_some());

        let mut prev_s = s.clone();
        prev_s.history.chat.separate = false;
        prev_s.output.danmaku.enabled = false;
        outputs.sync(&prev_s, &sizes, Locale::ZhCn, None);
        assert!(matches!(
            outputs.status.get(WindowKind::Danmaku),
            LinkState::Off
        ));
    }
}
