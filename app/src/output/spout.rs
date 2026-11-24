#[cfg(windows)]
use std::collections::HashMap;

use nana_ui::WindowOutputConfig;
#[cfg(windows)]
use nana_ui::{GpuContext, WindowOutputFrame};
#[cfg(windows)]
use nanatype_i18n::Locale;
use nanatype_settings::{Settings, SpoutOutput};

use super::config::{
    KINDS, ResolvedOutput, RuntimeOutputSizes, SPOUT_ERROR_KEYS, active_window, settings_for,
};
#[cfg(windows)]
use super::{OutputTransport, PublishOutcome};
#[cfg(windows)]
use crate::ui::LinkState;
use crate::windows::WindowKind;

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

const WARM_MS: u64 = 5_000;

// Only the device/context survive the image-resource grace period. One cache
// for all outputs; the deadline starts when the last valid output is disabled.
const BRIDGE_IDLE_MS: u64 = 30_000;

#[cfg(windows)]
struct Bridge {
    device: nanalive_spout::GpuDx11SharedDevice,
    generation: nana_ui::DeviceGeneration,
    idle: IdleLease,
}

#[derive(Default)]
struct IdleLease(Option<u64>);

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
struct SenderLease {
    output: SpoutOutput,
    runtime_size: Option<[u32; 2]>,
    expires: Option<u64>,
}

impl SenderLease {
    fn paused_config(
        &self,
        kind: WindowKind,
        s: &Settings,
        runtime_sizes: &RuntimeOutputSizes,
        partners: usize,
    ) -> Option<WindowOutputConfig> {
        // Only sync mutates the lease. A live clock query here could cross the
        // deadline halfway through NanaUI's window loop: the first retirement
        // callback would cancel cleanup while another window still had its pool.
        if self.expires.is_none()
            || !can_pause(
                kind,
                &self.output,
                self.runtime_size,
                s,
                runtime_sizes,
                partners,
            )
        {
            return None;
        }
        ResolvedOutput::Ready(&self.output)
            .config(self.runtime_size)
            .map(|config| config.with_suspended(true))
    }

    fn reconcile(
        &mut self,
        wanted: Option<&SpoutOutput>,
        runtime_size: Option<[u32; 2]>,
        pause: bool,
        now: u64,
    ) -> bool {
        if self.expires.is_some_and(|at| now >= at) {
            return false;
        }
        if let Some(out) = wanted {
            if out.name != self.output.name {
                return false;
            }
            if out != &self.output || self.runtime_size != runtime_size {
                self.output = out.clone();
                self.runtime_size = runtime_size;
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

// A sender that failed to open or prepare waits before the next attempt:
// prepare runs on every update and frame, and device creation is not free.
const RETRY_FIRST_MS: u64 = 1_000;
const RETRY_MAX_MS: u64 = 30_000;

/// The last failure of one output's sender. The wait doubles while the same
/// settings fail on the same device; anything else starts again at once.
struct Retry<G> {
    output: SpoutOutput,
    runtime_size: Option<[u32; 2]>,
    device: G,
    /// What the failure is called ([`SPOUT_ERROR_KEYS`]).
    key: &'static str,
    wait: u64,
    at: u64,
}

impl<G: Copy + PartialEq> Retry<G> {
    fn same(&self, out: &SpoutOutput, runtime_size: Option<[u32; 2]>, device: G) -> bool {
        self.device == device && self.runtime_size == runtime_size && &self.output == out
    }

    fn waiting(
        &self,
        out: &SpoutOutput,
        runtime_size: Option<[u32; 2]>,
        device: G,
        now: u64,
    ) -> bool {
        self.same(out, runtime_size, device) && now < self.at
    }

    fn after(
        previous: Option<Self>,
        out: &SpoutOutput,
        runtime_size: Option<[u32; 2]>,
        device: G,
        key: &'static str,
        now: u64,
    ) -> Self {
        let wait = previous
            .filter(|r| r.same(out, runtime_size, device))
            .map_or(RETRY_FIRST_MS, |r| {
                r.wait.saturating_mul(2).min(RETRY_MAX_MS)
            });
        Self {
            output: out.clone(),
            runtime_size,
            device,
            key,
            wait,
            at: now.saturating_add(wait),
        }
    }
}

fn can_pause(
    kind: WindowKind,
    previous: &SpoutOutput,
    previous_runtime_size: Option<[u32; 2]>,
    s: &Settings,
    runtime_sizes: &RuntimeOutputSizes,
    partners: usize,
) -> bool {
    let Some(out) = settings_for(kind, s) else {
        return false;
    };
    !out.enabled
        && active_window(kind, s, partners)
        && out.name == previous.name
        && runtime_sizes.extent_for(kind, out) == previous_runtime_size.or(previous.size)
        && out.straight_alpha == previous.straight_alpha
        && !KINDS
            .into_iter()
            .filter(|k| *k != kind && (*k != WindowKind::Danmaku || s.history.chat.separate))
            .filter_map(|k| settings_for(k, s))
            .any(|other| other.enabled && other.name.trim() == previous.name.trim())
}

#[cfg(windows)]
#[derive(Default)]
pub struct SpoutTransport {
    senders: HashMap<WindowKind, Sender>,
    bridge: Option<Bridge>,
    retries: HashMap<WindowKind, Retry<nana_ui::DeviceGeneration>>,
    recovered: Vec<WindowKind>,
}

#[cfg(windows)]
impl SpoutTransport {
    fn retry_waiting(
        &self,
        kind: WindowKind,
        out: &SpoutOutput,
        runtime_size: Option<[u32; 2]>,
        generation: nana_ui::DeviceGeneration,
        now: u64,
    ) -> bool {
        self.retries
            .get(&kind)
            .is_some_and(|retry| retry.waiting(out, runtime_size, generation, now))
    }

    fn sender_failed(
        &mut self,
        kind: WindowKind,
        out: &SpoutOutput,
        runtime_size: Option<[u32; 2]>,
        generation: nana_ui::DeviceGeneration,
        key: &'static str,
        now: u64,
    ) {
        self.senders.remove(&kind);
        let previous = self.retries.remove(&kind);
        self.retries.insert(
            kind,
            Retry::after(previous, out, runtime_size, generation, key, now),
        );
    }

    fn sender_recovered(&mut self, kind: WindowKind) {
        if self.retries.remove(&kind).is_some() && !self.recovered.contains(&kind) {
            self.recovered.push(kind);
        }
    }

    fn ensure_sender(
        &mut self,
        kind: WindowKind,
        out: &SpoutOutput,
        runtime_size: Option<[u32; 2]>,
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
            super::probe::lifecycle(
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
                    runtime_size,
                    expires: None,
                },
                device: generation,
                sender,
            },
        );
        #[cfg(debug_assertions)]
        super::probe::lifecycle(
            "sender-created",
            Some(kind),
            start.elapsed().as_secs_f64() * 1000.,
        );
        Ok(self.senders.get_mut(&kind).expect("sender inserted"))
    }
}

#[cfg(windows)]
impl OutputTransport for SpoutTransport {
    fn sync(
        &mut self,
        s: &Settings,
        runtime_sizes: &RuntimeOutputSizes,
        partners: usize,
        _live: &[(WindowKind, &SpoutOutput)],
    ) {
        let now = crate::clock::now_ms();
        let active = KINDS
            .into_iter()
            .any(|kind| super::config::resolve(kind, s, partners).output().is_some());
        if self
            .bridge
            .as_ref()
            .is_some_and(|bridge| bridge.idle.expired(now))
        {
            self.bridge = None;
            #[cfg(debug_assertions)]
            super::probe::lifecycle("bridge-expired", None, 0.0);
        }
        if let Some(bridge) = &mut self.bridge {
            bridge.idle.reconcile(active, now);
        }
        self.senders.retain(|kind, sender| {
            let pause = can_pause(
                *kind,
                &sender.lease.output,
                sender.lease.runtime_size,
                s,
                runtime_sizes,
                partners,
            );
            sender.lease.reconcile(
                super::config::resolve(*kind, s, partners).output(),
                runtime_sizes.get(*kind),
                pause,
                now,
            )
        });
    }

    fn paused_output(&self, kind: WindowKind, now: u64) -> Option<&SpoutOutput> {
        self.senders
            .get(&kind)
            .filter(|s| s.lease.expires.is_some_and(|at| now < at))
            .map(|s| &s.lease.output)
    }

    fn paused_config(
        &self,
        kind: WindowKind,
        s: &Settings,
        runtime_sizes: &RuntimeOutputSizes,
        partners: usize,
    ) -> Option<WindowOutputConfig> {
        self.senders
            .get(&kind)
            .and_then(|sender| sender.lease.paused_config(kind, s, runtime_sizes, partners))
    }

    fn cleanup_deadline(&self) -> Option<u64> {
        self.senders
            .values()
            .filter_map(|s| s.lease.expires)
            .chain(self.bridge.as_ref().and_then(|b| b.idle.0))
            .min()
    }

    fn prepare(
        &mut self,
        s: &Settings,
        runtime_sizes: &RuntimeOutputSizes,
        partners: usize,
        gpu: &GpuContext,
        locale: Locale,
    ) -> Vec<(WindowKind, LinkState)> {
        let mut updates = Vec::new();
        if self
            .bridge
            .as_ref()
            .is_some_and(|b| b.generation != gpu.generation())
        {
            self.senders.clear();
            self.bridge = None;
        }
        let generation = gpu.generation();
        let now = crate::clock::now_ms();
        let pending = KINDS.map(|kind| {
            super::config::resolve(kind, s, partners)
                .output()
                .filter(|out| {
                    let rsize = runtime_sizes.get(kind);
                    self.senders
                        .get(&kind)
                        .is_none_or(|sender| !sender.matches(out, generation))
                        && !self.retry_waiting(kind, out, rsize, generation, now)
                })
                .map(|out| (kind, out))
        });
        if pending.iter().all(Option::is_none) {
            return updates;
        }
        let Some(hal) = (unsafe {
            gpu.wgpu()
                .device()
                .as_hal::<nana_ui::wgpu::hal::api::Dx12>()
        }) else {
            return updates;
        };
        let luid = unsafe { hal.raw_device().GetAdapterLuid() };
        let luid = (i64::from(luid.HighPart) << 32) | i64::from(luid.LowPart);
        for (kind, out) in pending.into_iter().flatten() {
            let rsize = runtime_sizes.get(kind);
            let failure = match self.ensure_sender(kind, out, rsize, generation, luid) {
                Err(e) => Some((spout_error_key(&e), e.to_string())),
                Ok(sender) => rsize.or(out.size).and_then(|[width, height]| {
                    #[cfg(debug_assertions)]
                    let start = std::time::Instant::now();
                    let prepared = sender.sender.prepare(width, height);
                    #[cfg(debug_assertions)]
                    super::probe::lifecycle(
                        "surface-prepared",
                        Some(kind),
                        start.elapsed().as_secs_f64() * 1000.,
                    );
                    match prepared {
                        Ok(r) if r.status == nanalive_spout::SpoutPublishStatus::Sent => None,
                        Ok(r) => Some((SPOUT_ERROR_KEYS[4], format!("prepare: {:?}", r.status))),
                        Err(e) => Some((spout_error_key(&e), e.to_string())),
                    }
                }),
            };
            let Some((key, detail)) = failure else {
                self.sender_recovered(kind);
                continue;
            };
            nanatype_diag::report!(crate::diag::SPOUT_FAILED, window = crate::diag::window(kind); "{detail}");
            self.sender_failed(kind, out, rsize, generation, key, now);
            updates.push((kind, crate::ui::link_text::spout_send(locale, key)));
        }
        updates
    }

    fn publish(
        &mut self,
        kind: WindowKind,
        out: &SpoutOutput,
        runtime_size: Option<[u32; 2]>,
        frame: &WindowOutputFrame,
        gpu: &GpuContext,
    ) -> PublishOutcome {
        use std::os::windows::io::AsRawHandle;
        if self
            .senders
            .get(&kind)
            .is_some_and(|s| s.lease.expires.is_some() && s.device != gpu.generation())
        {
            self.senders.remove(&kind);
            return PublishOutcome::default();
        }
        let Some(mut token) = frame.take_native() else {
            if self
                .senders
                .get(&kind)
                .is_some_and(|s| s.lease.expires.is_some())
            {
                self.senders.remove(&kind);
            }
            return PublishOutcome::default();
        };
        let luid = token.adapter_luid();
        let generation = gpu.generation();
        let now = crate::clock::now_ms();
        if self.retry_waiting(kind, out, runtime_size, generation, now) {
            return PublishOutcome::default();
        }
        let sender = match self.ensure_sender(kind, out, runtime_size, generation, luid) {
            Ok(sender) => sender,
            Err(e) => {
                let key = spout_error_key(&e);
                self.sender_failed(kind, out, runtime_size, generation, key, now);
                return PublishOutcome {
                    error: Some((key, e.to_string())),
                    ..Default::default()
                };
            }
        };
        let mut textures = [std::ptr::null_mut(); nana_ui::NATIVE_EXPORT_SLOTS];
        for (slot, handle) in textures.iter_mut().enumerate() {
            let Some(texture) = token.texture_handle(slot) else {
                return PublishOutcome {
                    error: Some((
                        SPOUT_ERROR_KEYS[2],
                        format!("native export slot {slot} has no shared texture"),
                    )),
                    ..Default::default()
                };
            };
            *handle = texture.as_raw_handle();
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
        let start = std::time::Instant::now();
        // SAFETY: the handles stay open while `token`'s pool lives, which
        // outlasts this call; the sender signals release itself.
        let result = unsafe {
            sender
                .sender
                .send(&shared, nanalive_spout::GpuDx11SharedOptions::default())
        };
        #[cfg(all(windows, debug_assertions))]
        super::probe::observe(
            kind,
            gpu,
            &shared,
            &sender.sender,
            &result,
            sender.lease.expires.is_some(),
        );
        let paused_failed = sender.lease.expires.is_some()
            && !matches!(
                &result, Ok(r) if r.status == nanalive_spout::SpoutPublishStatus::Sent
            );
        if paused_failed {
            self.senders.remove(&kind);
        }
        let mut outcome = PublishOutcome::default();
        if let Ok(report) = &result {
            let sent = report.status == nanalive_spout::SpoutPublishStatus::Sent;
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            outcome.perf = Some((sent, ms));
            if !sent {
                frame.request_resend();
            }
            self.sender_recovered(kind);
        }
        outcome.error = result.err().map(|e| (spout_error_key(&e), e.to_string()));
        outcome
    }

    fn status_override(
        &self,
        kind: WindowKind,
        base: &LinkState,
        locale: Locale,
    ) -> Option<LinkState> {
        if let LinkState::Connected(_) = base
            && let Some(retry) = self.retries.get(&kind)
        {
            return Some(crate::ui::link_text::spout_send(locale, retry.key));
        }
        None
    }

    fn take_recovered(&mut self) -> Vec<WindowKind> {
        std::mem::take(&mut self.recovered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paused_configuration_is_stable_across_deadline_until_atomic_sync() {
        let s = Settings::default();
        let sizes = RuntimeOutputSizes::default();
        let mut lease = SenderLease {
            output: s.output.display.clone(),
            runtime_size: None,
            expires: Some(5_000),
        };
        let before = crate::clock::capture_at(4_999, || {
            lease.paused_config(WindowKind::Display, &s, &sizes, 0)
        });
        let after = crate::clock::capture_at(5_001, || {
            lease.paused_config(WindowKind::Display, &s, &sizes, 0)
        });
        assert_eq!(before, after);
        assert!(after.unwrap().suspended);
        // The single cleanup message reconciles/removes every expired lease,
        // before NanaUI queries any window. It cannot extend the old deadline.
        assert!(!lease.reconcile(None, None, true, 5_001));
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
    fn failed_sender_waits_longer_each_time_until_settings_or_device_change() {
        let out = Settings::default().output.display;
        let key = SPOUT_ERROR_KEYS[1];
        let retry = Retry::after(None, &out, None, 1u8, key, 100);
        assert_eq!((retry.key, retry.at), (key, 1_100));
        assert!(retry.waiting(&out, None, 1, 1_099));
        assert!(!retry.waiting(&out, None, 1, 1_100));
        let mut retry = retry;
        let mut at = 1_100;
        for wait in [2_000, 4_000, 8_000, 16_000, 30_000, 30_000] {
            retry = Retry::after(Some(retry), &out, None, 1, key, at);
            assert_eq!((retry.wait, retry.at), (wait, at + wait));
            at = retry.at;
        }
        // Another device or other settings neither wait nor inherit the wait.
        assert!(!retry.waiting(&out, None, 2, at - 1));
        let mut resized = out.clone();
        resized.size = Some([640, 360]);
        assert!(!retry.waiting(&resized, None, 1, at - 1));
        assert_eq!(
            Retry::after(Some(retry), &resized, None, 1, key, at).wait,
            1_000
        );
        let again = Retry::after(None, &out, None, 1, key, at);
        assert_eq!(
            Retry::after(Some(again), &out, None, 2, key, at).wait,
            1_000
        );
    }

    #[test]
    fn warm_lease_deadline_is_fixed_and_resume_cancels_it() {
        let out = Settings::default().output.display;
        let mut lease = SenderLease {
            output: out.clone(),
            runtime_size: None,
            expires: None,
        };
        assert!(lease.reconcile(None, None, true, 100));
        assert_eq!(lease.expires, Some(5100));
        assert!(lease.reconcile(None, None, true, 4000));
        assert_eq!(lease.expires, Some(5100));
        assert!(lease.reconcile(Some(&out), None, false, 5099));
        assert_eq!(lease.expires, None);
        assert!(lease.reconcile(None, None, true, 6000));
        assert!(!lease.reconcile(Some(&out), None, false, 11000));
    }

    #[test]
    fn paused_sender_rejects_rename_resize_alpha_collision_and_peer_departure() {
        let mut s = Settings::default();
        let sizes = RuntimeOutputSizes::default();
        let previous = s.output.display.clone();
        assert!(can_pause(
            WindowKind::Display,
            &previous,
            None,
            &s,
            &sizes,
            0
        ));
        s.output.display.name.push('x');
        assert!(!can_pause(
            WindowKind::Display,
            &previous,
            None,
            &s,
            &sizes,
            0
        ));
        s.output.display = previous.clone();
        s.output.display.size = Some([100, 100]);
        assert!(!can_pause(
            WindowKind::Display,
            &previous,
            None,
            &s,
            &sizes,
            0
        ));
        s.output.display = previous.clone();
        s.output.display.straight_alpha = !previous.straight_alpha;
        assert!(!can_pause(
            WindowKind::Display,
            &previous,
            None,
            &s,
            &sizes,
            0
        ));
        s.output.display = previous.clone();
        s.output.history.enabled = true;
        s.output.history.name = previous.name.clone();
        assert!(!can_pause(
            WindowKind::Display,
            &previous,
            None,
            &s,
            &sizes,
            0
        ));
        let peer = s.output.partners[0].clone();
        assert!(!can_pause(WindowKind::PARTNER, &peer, None, &s, &sizes, 0));
        assert!(can_pause(WindowKind::PARTNER, &peer, None, &s, &sizes, 1));
        let mut lease = SenderLease {
            output: previous,
            runtime_size: None,
            expires: None,
        };
        assert!(!lease.reconcile(None, None, false, 0));
    }
}
