//! Unified shader & material policy state machine (budget, device-loss strikes,
//! and debounced crash-guard persistence).
use nana_ui::{DeviceGeneration, GpuContext};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use super::super::effects::DeviceGenerations;
use super::super::shader_guard::Guard;

/// What all skin programs together may cost in one second: per-pixel cost
/// (`ShaderProgram::cost`) times covered pixels, summed over every draw.
/// About 1.6× the heaviest built-in (3608) over a 3840×2160 canvas at 60 fps.
pub(super) const DEFAULT_COST_BUDGET: u64 = 3_000_000_000_000;
static COST_BUDGET: AtomicU64 = AtomicU64::new(DEFAULT_COST_BUDGET);
const BUDGET_WINDOW: Duration = Duration::from_secs(1);
/// Programs drawn this close to a device's last draw share its loss.
const LOSS_WINDOW: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DisableReason {
    OverBudget,
    OverBudgetWhenFrozen,
    DeviceLostTwice,
    CompileFailed,
    CrashGuard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ShaderPolicyState {
    Active,
    Throttled,
    Disabled(DisableReason),
}

#[derive(Default, Clone, Copy)]
pub(super) struct Spend {
    pub cost: u64,
    pub animated: bool,
}

#[derive(Debug, PartialEq)]
pub(super) enum Verdict {
    Freeze,
    Disable,
}

/// When one window's draws together went over `budget`: the program that
/// spent most, frozen if it was playing and disabled if it already was not.
pub(super) fn budget_verdict(
    spent: &HashMap<String, Spend>,
    budget: u64,
) -> Option<(&str, Verdict)> {
    let total = spent.values().fold(0u64, |t, s| t.saturating_add(s.cost));
    if total <= budget {
        return None;
    }
    let (hash, s) = spent.iter().max_by_key(|(hash, s)| (s.cost, *hash))?;
    Some((
        hash,
        if s.animated {
            Verdict::Freeze
        } else {
            Verdict::Disable
        },
    ))
}

pub(super) fn short(hash: &str) -> &str {
    hash.get(..12).unwrap_or(hash)
}

#[derive(Default)]
pub(super) struct PolicyStore {
    states: HashMap<String, ShaderPolicyState>,
    device_losses: HashMap<String, u8>,
    guard: Guard,
    generations: DeviceGenerations,
    /// Aligned with `generations` (max 4): `None` when only materials drew on it.
    devices: Vec<(DeviceGeneration, Option<GpuContext>)>,
    last_drawn: HashMap<(DeviceGeneration, String), Instant>,
    window: Option<Instant>,
    spent: HashMap<String, Spend>,
    notices: Vec<String>,
    material_programs: HashMap<u64, (String, u32)>,
}

static POLICY: LazyLock<Mutex<PolicyStore>> = LazyLock::new(Default::default);

pub(super) fn store() -> MutexGuard<'static, PolicyStore> {
    POLICY.lock().unwrap_or_else(|e| e.into_inner())
}

impl PolicyStore {
    pub fn state(&self, hash: &str) -> ShaderPolicyState {
        self.states
            .get(hash)
            .copied()
            .unwrap_or(ShaderPolicyState::Active)
    }

    pub fn is_stopped(&self, hash: &str) -> bool {
        !matches!(self.state(hash), ShaderPolicyState::Active)
    }

    pub fn is_throttled(&self, hash: &str) -> bool {
        matches!(self.state(hash), ShaderPolicyState::Throttled)
    }

    pub fn is_disabled(&self, hash: &str) -> bool {
        matches!(self.state(hash), ShaderPolicyState::Disabled(_))
    }

    pub fn has_notices(&self) -> bool {
        !self.notices.is_empty()
    }

    pub fn take_notices(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notices)
    }

    pub fn disable(&mut self, hash: &str, reason: DisableReason, why: Option<&str>) {
        let losses = self.device_losses.entry(hash.to_owned()).or_default();
        *losses = (*losses).max(2);
        let was_disabled = self.is_disabled(hash);
        self.states
            .insert(hash.to_owned(), ShaderPolicyState::Disabled(reason));
        if !was_disabled && let Some(why) = why {
            nanatype_diag::report!(crate::diag::SHADER_DISABLED; "{}: {why}", short(hash));
            self.notices
                .push(format!("Skin shader {} disabled: {why}", short(hash)));
        }
    }

    pub fn record_compile_error(&mut self, hash: &str, err: &str) {
        let losses = self.device_losses.entry(hash.to_owned()).or_default();
        *losses += 1;
        let was_disabled = self.is_disabled(hash);
        self.states.insert(
            hash.to_owned(),
            ShaderPolicyState::Disabled(DisableReason::CompileFailed),
        );
        if !was_disabled {
            self.notices
                .push(format!("Skin shader {} disabled: {err}", short(hash)));
        }
    }

    pub fn init_guard(&mut self, path: PathBuf) {
        let (guard, crashed) = Guard::open(path);
        let crashed_set: HashSet<&String> = crashed.iter().collect();
        let blocked: Vec<String> = guard.blocked().cloned().collect();
        for hash in blocked {
            let why = crashed_set
                .contains(&hash)
                .then_some("it was running when NanaType or the GPU driver stopped");
            self.disable(&hash, DisableReason::CrashGuard, why);
        }
        self.guard = guard;
    }

    pub fn close_guard(&mut self) {
        self.guard.close();
    }

    pub fn flush_guard(&self) {
        self.guard.flush();
    }

    pub fn retry_disabled(&mut self) {
        self.guard.reset();
        self.states.clear();
        self.device_losses.clear();
    }

    pub fn observe(
        &mut self,
        generation: DeviceGeneration,
        gpu: Option<&GpuContext>,
    ) -> Option<DeviceGeneration> {
        if let Some((_, slot)) = self.devices.iter_mut().find(|(g, _)| *g == generation) {
            if slot.is_none() {
                *slot = gpu.cloned();
            }
            return None;
        }
        let lost: Vec<DeviceGeneration> = self
            .devices
            .iter()
            .filter(|(_, d)| d.as_ref().is_some_and(GpuContext::is_lost))
            .map(|(g, _)| *g)
            .collect();
        for g in lost {
            self.attribute_loss(g);
        }
        let retired = self.generations.touch(generation);
        self.devices.push((generation, gpu.cloned()));
        if let Some(retired) = retired {
            self.devices.retain(|(g, _)| *g != retired);
            self.last_drawn.retain(|(g, _), _| *g != retired);
        }
        self.states.retain(|hash, state| {
            !matches!(state, ShaderPolicyState::Disabled(_))
                || self.device_losses.get(hash).copied().unwrap_or(0) >= 2
        });
        retired
    }

    fn attribute_loss(&mut self, generation: DeviceGeneration) {
        let drawn: Vec<(String, Instant)> = self
            .last_drawn
            .iter()
            .filter(|((g, _), _)| *g == generation)
            .map(|((_, h), at)| (h.clone(), *at))
            .collect();
        self.last_drawn.retain(|(g, _), _| *g != generation);
        self.strike(drawn);
    }

    pub(super) fn strike(&mut self, drawn: Vec<(String, Instant)>) {
        let Some(latest) = drawn.iter().map(|(_, at)| *at).max() else {
            return;
        };
        for (hash, at) in drawn {
            if latest.duration_since(at) > LOSS_WINDOW {
                continue;
            }
            let failed = self.device_losses.entry(hash.clone()).or_default();
            *failed += 1;
            if *failed >= 2 {
                self.disable(
                    &hash,
                    DisableReason::DeviceLostTwice,
                    Some("the GPU device was lost twice while it ran"),
                );
            }
        }
    }

    pub fn spend(&mut self, hash: &str, cost: u64, animated: bool, now: Instant) {
        if self
            .window
            .is_none_or(|start| now.duration_since(start) >= BUDGET_WINDOW)
        {
            let verdict = budget_verdict(&self.spent, COST_BUDGET.load(Ordering::Relaxed))
                .map(|(h, v)| (h.to_owned(), v));
            match verdict {
                Some((hash, Verdict::Freeze)) if !self.is_throttled(&hash) => {
                    self.states
                        .insert(hash.clone(), ShaderPolicyState::Throttled);
                    let why = "over the GPU budget, now frozen";
                    nanatype_diag::report!(crate::diag::SHADER_DISABLED; "{}: {why}", short(&hash));
                    self.notices
                        .push(format!("Skin shader {} {why}", short(&hash)));
                }
                Some((hash, _)) => {
                    let throttled = self.is_throttled(&hash);
                    let (reason, why) = if throttled {
                        (
                            DisableReason::OverBudgetWhenFrozen,
                            "over the GPU budget even when frozen",
                        )
                    } else {
                        (DisableReason::OverBudget, "over the GPU budget")
                    };
                    self.disable(&hash, reason, Some(why));
                }
                None => {}
            }
            self.spent.clear();
            self.window = Some(now);
        }
        let s = self.spent.entry(hash.to_owned()).or_default();
        s.cost = s.cost.saturating_add(cost);
        s.animated |= animated;
    }

    pub fn watch_guard(&mut self, hash: &str) -> bool {
        self.guard.watch(hash)
    }

    pub fn draw_guard(&mut self, hash: &str, now: Instant) -> bool {
        self.guard.draw(hash, now)
    }

    pub fn note_draw(
        &mut self,
        generation: DeviceGeneration,
        hash: String,
        cost: u64,
        animated: bool,
        now: Instant,
    ) {
        self.last_drawn.insert((generation, hash.clone()), now);
        self.spend(&hash, cost, animated, now);
    }

    pub fn register_material(&mut self, key: u64, hash: String, cost: u32) {
        self.material_programs.insert(key, (hash, cost));
    }

    pub fn material_program(&self, key: u64) -> Option<(String, u32)> {
        self.material_programs.get(&key).cloned()
    }
}

/// Replace the GPU budget (for tests that drive programs over it); returns
/// the previous one.
#[doc(hidden)]
pub fn set_cost_budget(budget: u64) -> u64 {
    COST_BUDGET.swap(budget, Ordering::Relaxed)
}

/// Start the crash guard with its record at `path`. Programs that were on
/// trial when the last run ended are disabled, with a notice.
pub fn init_guard(path: PathBuf) {
    store().init_guard(path);
}

/// A clean exit: no program on trial crashed the process.
pub fn close_guard() {
    store().close_guard();
}

/// Wait for any queued background crash-guard writes to reach disk.
#[doc(hidden)]
pub fn flush_guard() {
    store().flush_guard();
}

/// The operator turned advanced effects back on: retry every disabled,
/// frozen or crash-blocked program.
pub fn retry_disabled() {
    store().retry_disabled();
    super::renderer::clear_reported();
}

pub fn take_notices() -> Vec<String> {
    store().take_notices()
}

pub(super) fn stopped(hash: &str) -> bool {
    store().is_stopped(hash)
}

pub(super) fn throttled(hash: &str) -> bool {
    store().is_throttled(hash)
}

/// Put the material program keyed `key` under the effects' protection.
pub fn guard_material(key: u64, program: &nanatype_theme::ShaderProgram) {
    store().register_material(key, program.hash.clone(), program.cost);
}

/// NanaUI asks before it compiles or draws a skin material and reports what
/// each draw shaded: materials share the effects' GPU budget, crash guard and
/// device-loss strikes (on devices an effect has drawn on, the only ones
/// whose loss NanaType can see). One over budget is disabled at once, since a static
/// material has nothing to freeze. Programs NanaType did not register (the
/// framework's own) always run.
pub(super) struct MaterialGate;

impl nana_ui::MaterialPolicy for MaterialGate {
    fn admit(&self, generation: Option<DeviceGeneration>, program: u64) -> bool {
        let mut s = store();
        let Some((hash, _)) = s.material_program(program) else {
            return true;
        };
        if let Some(generation) = generation {
            s.observe(generation, None);
        }
        // watch, not draw: NanaUI also asks here before it compiles, and a
        // compile can outlast HEALTHY_AFTER. The healthy clock starts in charge.
        !s.is_disabled(&hash) && s.watch_guard(&hash)
    }

    fn charge(&self, generation: Option<DeviceGeneration>, program: u64, pixels: u64) {
        let mut s = store();
        let Some((hash, cost)) = s.material_program(program) else {
            return;
        };
        let now = Instant::now();
        if let Some(generation) = generation {
            s.last_drawn.insert((generation, hash.clone()), now);
        }
        s.draw_guard(&hash, now);
        s.spend(
            &hash,
            u64::from(cost.max(1)).saturating_mul(pixels),
            false,
            now,
        );
    }
}
