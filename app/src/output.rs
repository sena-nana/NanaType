//! Spout2 (Windows) and Syphon (macOS) output of the dialogue and history windows.
//!
//! NanaUI paints each window's own scene a second time into an offscreen
//! texture. On Windows that texture is exported as a DX12 shared handle and
//! handed to a Spout sender. On macOS the same-device texture is blitted into
//! a Syphon server on the Metal queue. Nothing is read back to the CPU, and
//! output keeps flowing while the window is hidden.

#[cfg(target_os = "macos")]
mod syphon;

#[cfg(all(windows, debug_assertions))]
pub mod probe;

use std::collections::HashMap;

use nana_ui::{
    WindowOutputAlpha, WindowOutputConfig, WindowOutputExport, WindowOutputExtent,
    WindowOutputFrame, WindowOutputStatus,
};
use nanatype_i18n::Locale;
use nanatype_settings::{Settings, SpoutOutput};

use crate::ui::{I18N, LinkState};
use crate::windows::WindowKind;

/// What a Spout sender's failure is called, one key per kind of failure.
pub const SPOUT_ERROR_KEYS: [&str; 6] = [
    "output-spout-unsupported",
    "spout-error-backend",
    "spout-error-interop",
    "spout-error-format",
    "spout-error-publish",
    "spout-error-name",
];

/// Syphon failures. Kept beside the Spout keys so every locale is checked.
pub const SYPHON_ERROR_KEYS: [&str; 2] = ["output-syphon-missing", "output-syphon-publish"];

#[cfg(windows)]
fn spout_error_key(e: &nanalive_spout::SpoutOutputError) -> &'static str {
    use nanalive_spout::SpoutOutputError as E;
    match e {
        E::UnsupportedPlatform => SPOUT_ERROR_KEYS[0],
        E::BackendUnavailable => SPOUT_ERROR_KEYS[1],
        E::DeviceInteropUnavailable => SPOUT_ERROR_KEYS[2],
        E::SurfaceFormatUnsupported
        | E::InvalidFrameDimensions { .. }
        | E::BufferTooSmall { .. } => SPOUT_ERROR_KEYS[3],
        E::PublishFailed => SPOUT_ERROR_KEYS[4],
        E::InvalidSenderName => SPOUT_ERROR_KEYS[5],
    }
}

pub fn settings_for(kind: WindowKind, s: &Settings) -> Option<&SpoutOutput> {
    match kind {
        WindowKind::Display => Some(&s.output.display),
        WindowKind::PeerDisplay => Some(&s.output.peer_display),
        WindowKind::History => Some(&s.output.history),
        _ => None,
    }
}

pub const KINDS: [WindowKind; 3] = [
    WindowKind::Display,
    WindowKind::PeerDisplay,
    WindowKind::History,
];

pub fn name_conflicts(kind: WindowKind, s: &Settings) -> bool {
    let Some(output) = settings_for(kind, s) else {
        return false;
    };
    KINDS
        .into_iter()
        .filter(|other| *other != kind)
        .filter_map(|other| settings_for(other, s))
        .any(|other| {
            output.enabled
                && other.enabled
                && !output.name.trim().is_empty()
                && output.name.trim() == other.name.trim()
        })
}

pub fn config(kind: WindowKind, s: &Settings) -> Option<WindowOutputConfig> {
    resolve(kind, s, true).config()
}

#[derive(Clone, Copy)]
pub(crate) enum ResolvedOutput<'a> {
    Off,
    Invalid(&'static str),
    Ready(&'a SpoutOutput),
}

/// Configuration, sender lifetime and frame delivery share this decision.
/// Enabled names remain reserved even while the partner is disconnected.
pub(crate) fn resolve(kind: WindowKind, s: &Settings, peer_active: bool) -> ResolvedOutput<'_> {
    let Some(out) = settings_for(kind, s)
        .filter(|o| o.enabled && (kind != WindowKind::PeerDisplay || peer_active))
    else {
        return ResolvedOutput::Off;
    };
    if !(cfg!(windows) || cfg!(target_os = "macos")) {
        ResolvedOutput::Invalid("output-spout-unsupported")
    } else if out.name.trim().is_empty() {
        ResolvedOutput::Invalid("spout-error-name")
    } else if name_conflicts(kind, s) {
        ResolvedOutput::Invalid("output-name-conflict")
    } else {
        ResolvedOutput::Ready(out)
    }
}

impl<'a> ResolvedOutput<'a> {
    pub(crate) fn output(self) -> Option<&'a SpoutOutput> {
        match self {
            Self::Ready(out) => Some(out),
            _ => None,
        }
    }

    pub(crate) fn config(self) -> Option<WindowOutputConfig> {
        let out = self.output()?;
        let extent = match out.size {
            Some([width, height]) => WindowOutputExtent::Fixed { width, height },
            None => WindowOutputExtent::MatchWindow,
        };
        // Windows shares a DX12 texture. macOS blits the same-device texture
        // into Syphon; NanaUI has no Metal export token.
        let export = if cfg!(windows) {
            WindowOutputExport::Native
        } else {
            WindowOutputExport::SameDevice
        };
        Some(
            WindowOutputConfig::default()
                .with_extent(extent)
                .with_alpha(if out.straight_alpha {
                    WindowOutputAlpha::Straight
                } else {
                    WindowOutputAlpha::Premultiplied
                })
                .with_export(export)
                .while_hidden(true),
        )
    }
}

#[cfg(windows)]
struct Sender {
    lease: SenderLease,
    device: nana_ui::DeviceGeneration,
    sender: nanalive_spout::GpuDx11SharedSender,
}

#[cfg(windows)]
impl Sender {
    fn matches(&self, out: &SpoutOutput, generation: nana_ui::DeviceGeneration) -> bool {
        self.device == generation && self.lease.output.name == out.name
    }
}

#[cfg(any(windows, test))]
const WARM_MS: u64 = 5_000;

// Only the device/context survive the image-resource grace period. One cache
// for all outputs; the deadline starts when the last valid output is disabled.
#[cfg(any(windows, test))]
const BRIDGE_IDLE_MS: u64 = 30_000;

#[cfg(windows)]
struct Bridge {
    device: nanalive_spout::GpuDx11SharedDevice,
    generation: nana_ui::DeviceGeneration,
    idle: IdleLease,
}

#[cfg(any(windows, test))]
#[derive(Default)]
struct IdleLease(Option<u64>);
#[cfg(any(windows, test))]
impl IdleLease {
    fn expired(&self, now: u64) -> bool {
        self.0.is_some_and(|at| now >= at)
    }
    fn reconcile(&mut self, active: bool, now: u64) {
        if active {
            self.0 = None;
        } else {
            self.0.get_or_insert(now.saturating_add(BRIDGE_IDLE_MS));
        }
    }
}

/// One bounded warm slot per output. Repeated edits never extend suspension.
#[cfg(any(windows, test))]
struct SenderLease {
    output: SpoutOutput,
    expires: Option<u64>,
}
#[cfg(any(windows, test))]
impl SenderLease {
    fn paused_config(
        &self,
        kind: WindowKind,
        s: &Settings,
        peer: bool,
    ) -> Option<WindowOutputConfig> {
        // Only sync mutates the lease. A live clock query here could cross the
        // deadline halfway through NanaUI's window loop: the first retirement
        // callback would cancel cleanup while another window still had its pool.
        if self.expires.is_none() || !can_pause(kind, &self.output, s, peer) {
            return None;
        }
        ResolvedOutput::Ready(&self.output)
            .config()
            .map(|config| config.with_suspended(true))
    }

    fn reconcile(&mut self, wanted: Option<&SpoutOutput>, pause: bool, now: u64) -> bool {
        if self.expires.is_some_and(|at| now >= at) {
            return false;
        }
        if let Some(out) = wanted {
            if out.name != self.output.name {
                return false;
            }
            if out != &self.output {
                self.output = out.clone();
            }
            self.expires = None;
            true
        } else if pause {
            self.expires.get_or_insert(now.saturating_add(WARM_MS));
            true
        } else {
            false
        }
    }
}

#[cfg(any(windows, test))]
fn can_pause(kind: WindowKind, previous: &SpoutOutput, s: &Settings, peer_active: bool) -> bool {
    let Some(out) = settings_for(kind, s) else {
        return false;
    };
    !out.enabled
        && (kind != WindowKind::PeerDisplay || peer_active)
        && out.name == previous.name
        && out.size == previous.size
        && out.straight_alpha == previous.straight_alpha
        && !KINDS
            .into_iter()
            .filter(|k| *k != kind)
            .filter_map(|k| settings_for(k, s))
            .any(|other| other.enabled && other.name.trim() == previous.name.trim())
}

#[derive(Default)]
pub struct Spout {
    /// Qualified sender identities, also used to reset status after a rename
    /// or after correcting an invalid configuration.
    names: HashMap<WindowKind, String>,
    #[cfg(windows)]
    senders: HashMap<WindowKind, Sender>,
    #[cfg(windows)]
    cleanup_scheduled: Option<u64>,
    #[cfg(windows)]
    bridge: Option<Bridge>,
    #[cfg(target_os = "macos")]
    syphon: syphon::Outputs,
    /// Last link state per window, for the settings page.
    pub status: HashMap<WindowKind, LinkState>,
}

fn t(locale: Locale, key: &str) -> String {
    I18N.t_in(locale, key)
}

impl Spout {
    /// Keep a disabled Windows sender briefly for a quick restart. Renaming,
    /// incompatible settings and peer disconnect release it immediately.
    pub fn sync(&mut self, s: &Settings, locale: Locale, peer_active: bool) {
        #[cfg(windows)]
        {
            let now = crate::clock::now_ms();
            let active = KINDS
                .into_iter()
                .any(|kind| resolve(kind, s, peer_active).output().is_some());
            if self
                .bridge
                .as_ref()
                .is_some_and(|bridge| bridge.idle.expired(now))
            {
                self.bridge = None;
                #[cfg(debug_assertions)]
                probe::lifecycle("bridge-expired", None, 0.0);
            }
            if let Some(bridge) = &mut self.bridge {
                bridge.idle.reconcile(active, now);
            }
            self.senders.retain(|kind, sender| {
                let pause = can_pause(*kind, &sender.lease.output, s, peer_active);
                sender
                    .lease
                    .reconcile(resolve(*kind, s, peer_active).output(), pause, now)
            });
        }
        #[cfg(target_os = "macos")]
        let mut live = Vec::new();
        for kind in KINDS {
            let resolved = resolve(kind, s, peer_active);
            let wanted = resolved.output();
            let renamed = wanted.is_some_and(|o| self.names.get(&kind) != Some(&o.name));
            let state = match resolved {
                ResolvedOutput::Off => LinkState::Off,
                ResolvedOutput::Invalid(key) => LinkState::Error(t(locale, key)),
                ResolvedOutput::Ready(_) if renamed => LinkState::Connecting,
                ResolvedOutput::Ready(_) => match self.status.get(&kind) {
                    // Just switched on: no frame has gone out yet.
                    None | Some(LinkState::Off) => LinkState::Connecting,
                    Some(state) => state.clone(),
                },
            };
            if let Some(out) = wanted {
                if renamed {
                    self.names.insert(kind, out.name.clone());
                }
                #[cfg(target_os = "macos")]
                live.push((kind, out.name.clone()));
            } else {
                self.names.remove(&kind);
            }
            self.status.insert(kind, state);
        }
        #[cfg(target_os = "macos")]
        self.syphon.sync(&live);
    }

    /// `Some(None)` cancels the previous timer. There is only one cleanup job.
    pub fn take_cleanup_schedule(&mut self) -> Option<Option<u64>> {
        #[cfg(windows)]
        {
            let next = self
                .senders
                .values()
                .filter_map(|s| s.lease.expires)
                .chain(self.bridge.as_ref().and_then(|b| b.idle.0))
                .min();
            if next != self.cleanup_scheduled {
                self.cleanup_scheduled = next;
                return Some(next);
            }
        }
        None
    }

    pub(crate) fn paused_output(&self, kind: WindowKind, now: u64) -> Option<&SpoutOutput> {
        #[cfg(windows)]
        return self
            .senders
            .get(&kind)
            .filter(|s| s.lease.expires.is_some_and(|at| now < at))
            .map(|s| &s.lease.output);
        #[cfg(not(windows))]
        {
            let _ = (kind, now);
            None
        }
    }

    pub(crate) fn config(
        &self,
        kind: WindowKind,
        s: &Settings,
        peer_active: bool,
    ) -> Option<WindowOutputConfig> {
        let resolved = resolve(kind, s, peer_active);
        if let Some(config) = resolved.config() {
            return Some(config);
        }
        #[cfg(windows)]
        if matches!(resolved, ResolvedOutput::Off) {
            return self
                .senders
                .get(&kind)
                .and_then(|sender| sender.lease.paused_config(kind, s, peer_active));
        }
        None
    }

    pub fn status_changed(
        &mut self,
        kind: WindowKind,
        status: &WindowOutputStatus,
        locale: Locale,
    ) {
        // A retired pool needs nothing here: the sender reopens its handles
        // on the next frame.
        if let Some(state) = crate::ui::link_text::spout(locale, status) {
            // NanaUI reports Active for the texture itself. A failed Syphon
            // publish has to stay on the badge.
            #[cfg(target_os = "macos")]
            let state = if let LinkState::Connected(_) = &state
                && let Some(key) = self.syphon.failure(kind)
            {
                crate::ui::link_text::spout_send(locale, key)
            } else {
                state
            };
            self.status.insert(kind, state);
        }
    }

    /// Prepare names before rasterization so OBS discovery overlaps the first
    /// native export. Match-window extents register on the first actual frame.
    #[cfg(windows)]
    pub(crate) fn prepare(&mut self, s: &Settings, peer: bool, gpu: &nana_ui::GpuContext) {
        // Invalidate even when all outputs are paused/disabled. An old device
        // must not survive restoration just because no frame is requested.
        if self
            .bridge
            .as_ref()
            .is_some_and(|b| b.generation != gpu.generation())
        {
            self.senders.clear();
            self.bridge = None;
        }
        let generation = gpu.generation();
        let pending = KINDS.map(|kind| {
            resolve(kind, s, peer)
                .output()
                .filter(|out| {
                    self.senders
                        .get(&kind)
                        .is_none_or(|sender| !sender.matches(out, generation))
                })
                .map(|out| (kind, out))
        });
        if pending.iter().all(Option::is_none) {
            return;
        }
        let Some(hal) = (unsafe {
            gpu.wgpu()
                .device()
                .as_hal::<nana_ui::wgpu::hal::api::Dx12>()
        }) else {
            return;
        };
        let luid = unsafe { hal.raw_device().GetAdapterLuid() };
        let luid = (i64::from(luid.HighPart) << 32) | i64::from(luid.LowPart);
        for (kind, out) in pending.into_iter().flatten() {
            // The frame path retains the normal readable publish error/fallback.
            if let Ok(sender) = self.ensure_sender(kind, out, generation, luid)
                && let Some([width, height]) = out.size
            {
                #[cfg(debug_assertions)]
                let start = std::time::Instant::now();
                let prepared = sender.sender.prepare(width, height);
                #[cfg(debug_assertions)]
                probe::lifecycle(
                    "surface-prepared",
                    Some(kind),
                    start.elapsed().as_secs_f64() * 1000.,
                );
                if !matches!(
                    prepared,
                    Ok(nanalive_spout::GpuDx11PublishReport {
                        status: nanalive_spout::SpoutPublishStatus::Sent,
                        ..
                    })
                ) {
                    self.senders.remove(&kind);
                }
            }
        }
    }

    #[cfg(windows)]
    fn ensure_sender(
        &mut self,
        kind: WindowKind,
        out: &SpoutOutput,
        generation: nana_ui::DeviceGeneration,
        luid: i64,
    ) -> Result<&mut Sender, nanalive_spout::SpoutOutputError> {
        if self.senders.get(&kind).is_some_and(|sender| {
            sender.matches(out, generation) && sender.sender.adapter_luid() == luid
        }) {
            return Ok(self.senders.get_mut(&kind).expect("matched sender"));
        }
        self.senders.remove(&kind);
        if self.bridge.as_ref().is_none_or(|b| {
            b.generation != generation
                || b.device.adapter_luid() != luid
                || b.idle.expired(crate::clock::now_ms())
        }) {
            // Retire old registrations and imports before changing devices.
            self.senders.clear();
            self.bridge = None;
            #[cfg(debug_assertions)]
            let start = std::time::Instant::now();
            let device = nanalive_spout::GpuDx11SharedDevice::new(luid)?;
            self.bridge = Some(Bridge {
                device,
                generation,
                idle: IdleLease::default(),
            });
            #[cfg(debug_assertions)]
            probe::lifecycle(
                "bridge-created",
                None,
                start.elapsed().as_secs_f64() * 1000.,
            );
        }
        let bridge = self.bridge.as_mut().expect("bridge created");
        bridge.idle.reconcile(true, crate::clock::now_ms());
        #[cfg(debug_assertions)]
        let start = std::time::Instant::now();
        let sender = nanalive_spout::GpuDx11SharedSender::with_device(&out.name, &bridge.device)?;
        self.senders.insert(
            kind,
            Sender {
                lease: SenderLease {
                    output: out.clone(),
                    expires: None,
                },
                device: generation,
                sender,
            },
        );
        #[cfg(debug_assertions)]
        probe::lifecycle(
            "sender-created",
            Some(kind),
            start.elapsed().as_secs_f64() * 1000.,
        );
        Ok(self.senders.get_mut(&kind).expect("sender inserted"))
    }

    /// Hand a frame to the window's Spout sender. When sending failed,
    /// returns what to say ([`SPOUT_ERROR_KEYS`]) and the sender's own
    /// (English) words for the log.
    #[allow(unused_variables)]
    pub fn frame(
        &mut self,
        kind: WindowKind,
        out: &SpoutOutput,
        frame: &WindowOutputFrame,
        gpu: &nana_ui::GpuContext,
    ) -> Option<(&'static str, String)> {
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            if self
                .senders
                .get(&kind)
                .is_some_and(|s| s.lease.expires.is_some() && s.device != gpu.generation())
            {
                self.senders.remove(&kind);
                return None;
            }
            let Some(mut token) = frame.take_native() else {
                // A deferred/unsupported native copy cannot clear the receiver.
                // End the grace period so an old image cannot remain parked.
                if self
                    .senders
                    .get(&kind)
                    .is_some_and(|s| s.lease.expires.is_some())
                {
                    self.senders.remove(&kind);
                }
                return None;
            };
            let luid = token.adapter_luid();
            let sender = match self.ensure_sender(kind, out, gpu.generation(), luid) {
                Ok(sender) => sender,
                Err(e) => return Some((spout_error_key(&e), e.to_string())),
            };
            let mut textures = [std::ptr::null_mut(); nana_ui::NATIVE_EXPORT_SLOTS];
            for (slot, handle) in textures.iter_mut().enumerate() {
                *handle = token.texture_handle(slot)?.as_raw_handle();
            }
            let [width, height] = token.extent();
            let shared = nanalive_spout::SharedTextureFrame {
                pool_generation: token.pool_generation(),
                adapter_luid: luid,
                width,
                height,
                textures: &textures,
                fence: token.fence_handle().as_raw_handle(),
                slot: token.slot(),
                ready_value: token.ready_value(),
                release_value: token.release_value(),
            };
            token.accept_release();
            // SAFETY: the handles stay open while `token`'s pool lives, which
            // outlasts this call; the sender signals release itself.
            let result = unsafe {
                sender
                    .sender
                    .send(&shared, nanalive_spout::GpuDx11SharedOptions::default())
            };
            #[cfg(all(windows, debug_assertions))]
            probe::observe(
                kind,
                gpu,
                &shared,
                &sender.sender,
                &result,
                sender.lease.expires.is_some(),
            );
            // If transparency could not be sent, withdraw instead of leaving
            // the old dialogue visible during the grace period.
            let paused_failed = sender.lease.expires.is_some()
                && !matches!(
                    &result, Ok(r) if r.status == nanalive_spout::SpoutPublishStatus::Sent
                );
            if paused_failed {
                self.senders.remove(&kind);
            }
            result.err().map(|e| (spout_error_key(&e), e.to_string()))
        }
        #[cfg(target_os = "macos")]
        return self.syphon.publish(kind, &out.name, gpu, frame);
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            let _ = (frame, gpu);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paused_configuration_is_stable_across_deadline_until_atomic_sync() {
        let s = Settings::default();
        let mut lease = SenderLease {
            output: s.output.display.clone(),
            expires: Some(5_000),
        };
        let before = crate::clock::capture_at(4_999, || {
            lease.paused_config(WindowKind::Display, &s, false)
        });
        let after = crate::clock::capture_at(5_001, || {
            lease.paused_config(WindowKind::Display, &s, false)
        });
        assert_eq!(before, after);
        assert!(after.unwrap().suspended);
        // The single cleanup message reconciles/removes every expired lease,
        // before NanaUI queries any window. It cannot extend the old deadline.
        assert!(!lease.reconcile(None, true, 5_001));
    }

    #[test]
    fn bridge_idle_deadline_never_slides_and_resume_starts_a_new_idle_period() {
        let mut idle = IdleLease::default();
        idle.reconcile(false, 100);
        idle.reconcile(false, 5_100); // sender expiry cleanup
        idle.reconcile(false, 20_100); // hot reload/other updates
        assert_eq!(idle.0, Some(30_100));
        assert!(!idle.expired(30_099));
        assert!(idle.expired(30_100)); // checked before accepting a resume
        idle.reconcile(true, 30_101);
        assert_eq!(idle.0, None);
        idle.reconcile(false, 40_000);
        assert_eq!(idle.0, Some(70_000));
    }

    #[test]
    fn warm_lease_deadline_is_fixed_and_resume_cancels_it() {
        let out = Settings::default().output.display;
        let mut lease = SenderLease {
            output: out.clone(),
            expires: None,
        };
        assert!(lease.reconcile(None, true, 100));
        assert_eq!(lease.expires, Some(5100));
        assert!(lease.reconcile(None, true, 4000));
        assert_eq!(lease.expires, Some(5100));
        assert!(lease.reconcile(Some(&out), false, 5099));
        assert_eq!(lease.expires, None);
        assert!(lease.reconcile(None, true, 6000));
        assert!(!lease.reconcile(Some(&out), false, 11000));
    }

    #[test]
    fn paused_sender_rejects_rename_resize_alpha_collision_and_peer_departure() {
        let mut s = Settings::default();
        let previous = s.output.display.clone();
        assert!(can_pause(WindowKind::Display, &previous, &s, false));
        s.output.display.name.push('x');
        assert!(!can_pause(WindowKind::Display, &previous, &s, false));
        s.output.display = previous.clone();
        s.output.display.size = Some([100, 100]);
        assert!(!can_pause(WindowKind::Display, &previous, &s, false));
        s.output.display = previous.clone();
        s.output.display.straight_alpha = !previous.straight_alpha;
        assert!(!can_pause(WindowKind::Display, &previous, &s, false));
        s.output.display = previous.clone();
        s.output.history.enabled = true;
        s.output.history.name = previous.name.clone();
        assert!(!can_pause(WindowKind::Display, &previous, &s, false));
        let peer = s.output.peer_display.clone();
        assert!(!can_pause(WindowKind::PeerDisplay, &peer, &s, false));
        assert!(can_pause(WindowKind::PeerDisplay, &peer, &s, true));
        let mut lease = SenderLease {
            output: previous,
            expires: None,
        };
        assert!(!lease.reconcile(None, false, 0));
    }

    #[test]
    fn invalid_names_and_renames_share_configuration_and_sender_lifetime() {
        if !(cfg!(windows) || cfg!(target_os = "macos")) {
            return;
        }
        let mut settings = Settings::default();
        settings.output.display.enabled = true;
        settings.output.display.name = "   ".into();
        let mut outputs = Spout::default();
        outputs.sync(&settings, Locale::ZhCn, false);
        assert!(config(WindowKind::Display, &settings).is_none());
        assert!(outputs.names.is_empty());
        assert_eq!(
            outputs.status[&WindowKind::Display],
            LinkState::Error(t(Locale::ZhCn, "spout-error-name"))
        );
        settings.output.display.name = "Renamed".into();
        outputs.sync(&settings, Locale::ZhCn, false);
        assert!(config(WindowKind::Display, &settings).unwrap().while_hidden);
        assert_eq!(outputs.status[&WindowKind::Display], LinkState::Connecting);
        outputs
            .status
            .insert(WindowKind::Display, LinkState::Connected("ready".into()));
        outputs.sync(&settings, Locale::ZhCn, false);
        assert!(matches!(
            outputs.status[&WindowKind::Display],
            LinkState::Connected(_)
        ));
        settings.output.display.name = "Renamed again".into();
        outputs.sync(&settings, Locale::ZhCn, false);
        assert_eq!(outputs.status[&WindowKind::Display], LinkState::Connecting);
        assert_eq!(outputs.names[&WindowKind::Display], "Renamed again");
        settings.output.peer_display.enabled = true;
        assert!(
            resolve(WindowKind::PeerDisplay, &settings, false)
                .config()
                .is_none()
        );
        assert!(
            resolve(WindowKind::PeerDisplay, &settings, true)
                .config()
                .is_some()
        );
        settings.output.peer_display.name = settings.output.display.name.clone();
        outputs.sync(&settings, Locale::ZhCn, true);
        assert!(outputs.names.is_empty());
        assert!(
            resolve(WindowKind::Display, &settings, true)
                .output()
                .is_none()
        );
        settings.output.peer_display.enabled = false;
        outputs.sync(&settings, Locale::ZhCn, false);
        assert_eq!(outputs.status[&WindowKind::Display], LinkState::Connecting);
    }

    #[test]
    fn independent_outputs_keep_their_size_and_alpha() {
        if !(cfg!(windows) || cfg!(target_os = "macos")) {
            return;
        }
        let mut s = Settings::default();
        s.output.display.enabled = true;
        s.output.display.size = Some([1280, 720]);
        s.output.peer_display.enabled = true;
        s.output.peer_display.size = Some([640, 360]);
        s.output.peer_display.straight_alpha = true;
        let own = config(WindowKind::Display, &s).unwrap();
        let peer = config(WindowKind::PeerDisplay, &s).unwrap();
        assert_eq!(
            own.extent,
            WindowOutputExtent::Fixed {
                width: 1280,
                height: 720
            }
        );
        assert_eq!(
            peer.extent,
            WindowOutputExtent::Fixed {
                width: 640,
                height: 360
            }
        );
        assert_eq!(own.alpha, WindowOutputAlpha::Premultiplied);
        assert_eq!(peer.alpha, WindowOutputAlpha::Straight);
        assert!(own.while_hidden && peer.while_hidden);
        s.output.peer_display.enabled = false;
        assert!(config(WindowKind::PeerDisplay, &s).is_none());
        assert_eq!(config(WindowKind::Display, &s), Some(own));
        assert!(config(WindowKind::History, &s).is_none());
    }

    #[test]
    fn partner_output_stops_on_leave_and_can_start_again() {
        if !(cfg!(windows) || cfg!(target_os = "macos")) {
            return;
        }
        let mut s = Settings::default();
        s.output.display.enabled = true;
        s.output.peer_display.enabled = true;
        let mut outputs = Spout::default();
        let locale = Locale::ZhCn;
        outputs.sync(&s, locale, false);
        assert!(matches!(
            outputs.status[&WindowKind::PeerDisplay],
            LinkState::Off
        ));
        outputs.sync(&s, locale, true);
        assert!(matches!(
            outputs.status[&WindowKind::PeerDisplay],
            LinkState::Connecting
        ));
        outputs
            .status
            .insert(WindowKind::Display, LinkState::Connected("self".into()));
        outputs.status.insert(
            WindowKind::PeerDisplay,
            LinkState::Connected("partner".into()),
        );
        outputs.sync(&s, locale, false);
        assert!(matches!(
            outputs.status[&WindowKind::PeerDisplay],
            LinkState::Off
        ));
        assert!(matches!(
            outputs.status[&WindowKind::Display],
            LinkState::Connected(_)
        ));
        outputs.sync(&s, locale, true);
        assert!(matches!(
            outputs.status[&WindowKind::PeerDisplay],
            LinkState::Connecting
        ));
    }

    #[test]
    fn duplicate_enabled_names_are_rejected() {
        let mut s = Settings::default();
        s.output.display.enabled = true;
        s.output.peer_display.enabled = true;
        s.output.peer_display.name = s.output.display.name.clone();
        assert!(name_conflicts(WindowKind::Display, &s));
        assert!(name_conflicts(WindowKind::PeerDisplay, &s));
        assert!(!name_conflicts(WindowKind::History, &s));
        assert!(config(WindowKind::Display, &s).is_none());
        assert!(config(WindowKind::PeerDisplay, &s).is_none());
        let mut outputs = Spout::default();
        outputs.sync(&s, Locale::ZhCn, true);
        assert!(matches!(
            outputs.status[&WindowKind::PeerDisplay],
            LinkState::Error(_)
        ));
        s.output.peer_display.enabled = false;
        assert!(!name_conflicts(WindowKind::Display, &s));
    }
}
