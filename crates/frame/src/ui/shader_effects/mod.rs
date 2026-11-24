//! Skin-owned pixel functions, rendered beneath dialogue text. The host owns
//! bindings, targets, masks, clocks, error handling and every resource budget.
mod compiler;
mod policy;
mod renderer;

use super::{Stage, dialogue::Source, surface::Surface};
use nana_ui::runtime::view::{AnyView, IntoView, Signal, widget};
use nana_ui::runtime::{
    ComponentView, CustomRenderNode, GpuView, MutationQueue, NodeKind, StableNodeId, UiWorld,
};
use nana_ui_core::{LengthSpec, PointerEventsSpec, PositionSpec};
use nanatype_core::DisplayView;
use nanatype_theme::{ShaderActive, ShaderProgram, ShaderSpec, ShaderTarget, Skin};
use std::sync::{
    Arc, LazyLock, Mutex, Weak,
    atomic::{AtomicBool, Ordering},
};

pub use policy::{
    close_guard, flush_guard, guard_material, init_guard, retry_disabled, set_cost_budget,
    take_notices,
};
pub use renderer::{cache_stats, collect_garbage, register, set_rest_frames};

use policy::{stopped, store, throttled};

pub(super) const RENDERER: &str = "nanatype.skin-shader-v1";
pub(super) const SLOT_FLOATS: usize = nanatype_theme::MAX_SHADER_SLOTS * 4;
pub(super) const SLOT_BYTES: usize = SLOT_FLOATS * 4;
/// Events of a program at rest: settled, fully revealed, no continue mark.
pub(super) const REST_EVENTS: [f32; 4] = [2., 2., 1., 0.];
/// The input box holds a shown line; no typing events.
pub(super) const DRAFT_EVENTS: [f32; 4] = [2., 2., 1., 1.];

pub(super) static MATERIALS: LazyLock<super::effects::Registry<Material>> =
    LazyLock::new(Default::default);

fn runnable(skin: &Skin, id: &str) -> bool {
    skin.shaders
        .get(id)
        .is_some_and(|s| s.intensity > 0. && s.animated)
        && skin
            .shader_programs
            .get(id)
            .is_some_and(|p| !stopped(&p.hash))
}

/// Skin backgrounds may move: skin motion is on and the system does not ask
/// for reduced motion. Off, each program is drawn once at rest and reused.
pub fn motion(ui: Stage) -> bool {
    !ui.reduced_motion.get_untracked() && ui.settings.with_untracked(|s| s.skin.motion)
}

pub fn enabled(ui: Stage) -> bool {
    motion(ui) && ui.settings.with_untracked(|s| s.skin.advanced_effects)
}

pub fn animating(ui: Stage) -> bool {
    wake(ui) != Wake::Idle
}

/// When a program's picture next changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wake {
    /// It holds still until something else redraws it.
    Idle,
    /// The next step of a stepped animation, in session milliseconds.
    At(u64),
    Continuous,
}

impl Wake {
    /// The sooner of two.
    pub fn or(self, other: Self) -> Self {
        match (self, other) {
            (Self::Continuous, _) | (_, Self::Continuous) => Self::Continuous,
            (Self::At(a), Self::At(b)) => Self::At(a.min(b)),
            (Self::At(at), Self::Idle) | (Self::Idle, Self::At(at)) => Self::At(at),
            (Self::Idle, Self::Idle) => Self::Idle,
        }
    }
}

/// Whether `spec` moves whatever the line does.
fn always(spec: &ShaderSpec) -> bool {
    spec.active.is_empty() || spec.active.contains(&ShaderActive::Always)
}

/// Whether `spec` declares itself moving for `events` (see [`event_values`]).
fn active(spec: &ShaderSpec, events: [f32; 4]) -> bool {
    always(spec)
        || spec.active.iter().any(|a| match *a {
            ShaderActive::Always => true,
            ShaderActive::Typing => events[2] < 1.,
            ShaderActive::Waiting => events[3] > 0.5,
            ShaderActive::Line(s) => events[0] < s,
            ShaderActive::Speaker(s) => events[1] < s,
        })
}

/// The frame a moving `spec` needs after `now`.
fn next_frame(spec: &ShaderSpec, now: u64) -> Wake {
    if spec.cadence > 0. {
        let steps = (now as f64 * f64::from(spec.cadence) / 1000.).floor();
        Wake::At(((steps + 1.) * 1000. / f64::from(spec.cadence)).ceil() as u64)
    } else {
        Wake::Continuous
    }
}

/// The loop clock `spec` shows at `now`: held at the start of its step,
/// rounded up so a program reads the step it is in, not the one before.
pub(super) fn stepped(spec: &ShaderSpec, now: u64) -> u64 {
    if spec.cadence > 0. {
        let steps = (now as f64 * f64::from(spec.cadence) / 1000.).floor();
        (steps * 1000. / f64::from(spec.cadence)).ceil() as u64
    } else {
        now
    }
}

/// When the display's programs next change.
pub fn wake(ui: Stage) -> Wake {
    // A renderer can discover a compiler failure after Host::finish. Keep one
    // callback alive so its notice reaches the operator before the track sleeps.
    if store().has_notices() {
        return Wake::Continuous;
    }
    if warming(ui, Source::Display) {
        return Wake::Continuous;
    }
    if !enabled(ui) || ui.display.with_untracked(|d| d.empty) {
        return Wake::Idle;
    }
    let now = crate::clock::frame_ms();
    ui.skin.with_untracked(|s| {
        let size = super::dialogue::canvas_size(ui, s);
        display_shaders(s, size, |ids| {
            let mut events = None;
            ids.iter()
                .filter(|id| runnable(s, id))
                .fold(Wake::Idle, |wake, id| {
                    let spec = &s.shaders[id];
                    // Line state only for programs that wait on it.
                    let moving = always(spec)
                        || active(
                            spec,
                            *events.get_or_insert_with(|| event_values(ui, Source::Display, now)),
                        );
                    if moving {
                        wake.or(next_frame(spec, now))
                    } else {
                        wake
                    }
                })
        })
    })
}

type ResolvedShaders = (Weak<Skin>, [u32; 2], Vec<String>);
thread_local! {
    /// The shaders of the display's dialogue as resolved for its size, for
    /// one skin. Resolving clones the whole skin, and frame scheduling asks
    /// several times per window per loop turn. A layout changes only the
    /// dialogue, so `runnable` can still read the skin's own programs.
    static DISPLAY_SHADERS: std::cell::RefCell<Option<ResolvedShaders>> =
        const { std::cell::RefCell::new(None) };
}

fn display_shaders<R>(skin: &Arc<Skin>, size: [f32; 2], f: impl FnOnce(&[String]) -> R) -> R {
    let key = size.map(f32::to_bits);
    DISPLAY_SHADERS.with_borrow_mut(|cache| {
        let fresh = cache.as_ref().is_some_and(|(cached, at, _)| {
            *at == key && cached.upgrade().is_some_and(|c| Arc::ptr_eq(&c, skin))
        });
        if !fresh {
            let d = nanatype_theme::responsive_dialogue(skin, size).dialogue;
            let ids = d
                .shader_effects
                .into_iter()
                .map(|e| e.shader)
                .chain(d.decor.into_iter().filter_map(|d| d.shader))
                .collect();
            *cache = Some((Arc::downgrade(skin), key, ids));
        }
        f(&cache.as_ref().expect("just filled").2)
    })
}

pub fn preview_animating(ui: Stage) -> bool {
    if store().has_notices() {
        return true;
    }
    if warming(ui, Source::Preview) {
        return true;
    }
    enabled(ui)
        && ui.shader_preview.get_untracked()
        && ui.live_preview.get()
        && MATERIALS.snapshot().into_iter().any(|m| {
            m.source == Source::Preview
                && m.spec.animated
                && m.spec.intensity > 0.
                && !stopped(&m.program.hash)
        })
}

/// Whether the input window's skin preview has programs that move or still
/// compile, so the window needs frames. Its box holds them only while the
/// preview is on.
pub fn input_animating(ui: Stage) -> bool {
    input_wake(ui) != Wake::Idle
}

/// When the input window's skin preview next changes.
pub fn input_wake(ui: Stage) -> Wake {
    if store().has_notices() {
        return Wake::Continuous;
    }
    if warming(ui, Source::Draft) {
        return Wake::Continuous;
    }
    if !enabled(ui) || !ui.settings.with_untracked(|s| s.general.input_skin_preview) {
        return Wake::Idle;
    }
    // Never read a signal under the registry lock (see `warming`).
    let materials = MATERIALS.snapshot();
    let now = crate::clock::frame_ms();
    materials
        .into_iter()
        .filter(|m| {
            m.source == Source::Draft
                && m.ui.draft == ui.draft
                && m.spec.animated
                && m.spec.intensity > 0.
                && !stopped(&m.program.hash)
                && active(&m.spec, DRAFT_EVENTS)
        })
        .fold(Wake::Idle, |wake, m| wake.or(next_frame(&m.spec, now)))
}

pub(super) struct Material {
    pub warming: AtomicBool,
    /// The untransformed layout canvas; preview scaling is paint-only.
    pub logical_size: Arc<Mutex<[f32; 2]>>,
    pub id: u64,
    pub ui: Stage,
    pub source: Source,
    pub surface: Surface,
    pub full_coverage: bool,
    pub program: Arc<ShaderProgram>,
    pub spec: ShaderSpec,
    pub palette: [[f32; 4]; 4],
    /// `param(i)` slots packed from `spec.bindings` and the skin's params.
    pub slots: Box<[f32; SLOT_FLOATS]>,
}

impl Drop for Material {
    fn drop(&mut self) {
        MATERIALS.remove(self.id);
    }
}

/// Pack `bindings` one `vec4` each: scalars in `x`, colours as linear
/// straight-alpha `rgba` (as the palette), lists component by component.
fn pack_slots(
    bindings: &[nanatype_theme::SlotBinding],
    values: &std::collections::BTreeMap<String, nanatype_theme::Resolved>,
) -> Box<[f32; SLOT_FLOATS]> {
    use nanatype_theme::{Resolved, SlotBinding, SlotComponent};
    let mut out = Box::new([0f32; SLOT_FLOATS]);
    let scalar = |id: &str| values.get(id).and_then(Resolved::scalar).unwrap_or(0.);
    for (slot, binding) in out.as_chunks_mut::<4>().0.iter_mut().zip(bindings) {
        match binding {
            SlotBinding::Number(v) => slot[0] = *v,
            SlotBinding::Param(id) => match values.get(id) {
                Some(Resolved::Color(c)) => slot.copy_from_slice(&color(*c)),
                _ => slot[0] = scalar(id),
            },
            SlotBinding::Tuple(parts) => {
                for (dst, part) in slot.iter_mut().zip(parts) {
                    *dst = match part {
                        SlotComponent::Number(v) => *v,
                        SlotComponent::Param(id) => scalar(id),
                    };
                }
            }
        }
    }
    out
}

/// The artwork draws its own continue indicator right now: it declares
/// `owns_continue`, compiled, and is playing rather than frozen or off.
pub fn owns_continue(ui: Stage, source: Source, skin: &Skin) -> bool {
    let playing = !ui.reduced_motion.get()
        && ui
            .settings
            .with(|s| s.skin.advanced_effects && s.skin.motion)
        && (source != Source::Preview || ui.shader_preview.get());
    playing
        && skin.dialogue.shader_effects.iter().any(|e| {
            e.target == ShaderTarget::Artwork
                && skin.shaders.get(&e.shader).is_some_and(|s| s.owns_continue)
                && runnable(skin, &e.shader)
        })
}

fn warming(ui: Stage, source: Source) -> bool {
    // View reconciliation may create/drop materials while holding reactive
    // state. Never read a signal under the global material registry lock.
    MATERIALS.snapshot().into_iter().any(|m| {
        m.source == source
            && m.spec.static_fallback
            && m.warming.load(Ordering::Relaxed)
            && if source == Source::Preview {
                m.ui.now == ui.now && ui.live_preview.get()
            } else if source == Source::Draft {
                m.ui.draft == ui.draft
            } else {
                m.ui.display == ui.display && !ui.display.with_untracked(|d| d.empty)
            }
    })
}

pub(super) fn playing(m: &Material) -> bool {
    m.spec.animated
        && !throttled(&m.program.hash)
        && enabled(m.ui)
        && (m.source != Source::Preview || m.ui.shader_preview.get_untracked())
}

impl PartialEq for Material {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

#[derive(Clone, PartialEq)]
struct EffectView {
    base: GpuView,
    material: Arc<Material>,
}

impl ComponentView for EffectView {
    fn share_layouts(&mut self, share: &mut dyn FnMut(&mut Arc<nana_ui_core::LayoutStyle>)) {
        self.base.share_layouts(share);
    }
    fn node_kind(&self) -> NodeKind {
        self.base.node_kind()
    }
    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        self.base.project(id, world, mutations);
        mutations.set_custom_render(
            id,
            Some(CustomRenderNode::new(
                RENDERER,
                self.material.id.to_string(),
                1,
            )),
        );
    }
}

use super::effects::linear;
fn color(c: nanatype_model::Rgba8) -> [f32; 4] {
    [linear(c.r), linear(c.g), linear(c.b), c.a as f32 / 255.]
}

pub fn view(
    ui: Stage,
    source: Source,
    skin: &Skin,
    id: &str,
    mut surface: Surface,
    target: ShaderTarget,
) -> Option<AnyView> {
    let program = skin.shader_programs.get(id)?.clone();
    let spec = skin.shaders.get(id)?.clone();
    let slots = pack_slots(&spec.bindings, &skin.param_values);
    if spec.intensity == 0. {
        return None;
    }
    if target == ShaderTarget::Border {
        // Outline-only coverage uses the exact same path as the static frame.
        surface.fill = Some([0.; 4]);
        surface.gradient = None;
    }
    if target == ShaderTarget::Artwork {
        // A baked static frame is the fallback, never the coverage source for
        // its live counterpart. The shader owns alpha inside this canvas.
        surface = Surface {
            fill: Some([1.; 4]),
            ..Default::default()
        };
    }
    let id = MATERIALS.next_id();
    let logical_size = Arc::new(Mutex::new([0.0; 2]));
    let material = Arc::new(Material {
        warming: AtomicBool::new(spec.static_fallback),
        logical_size: logical_size.clone(),
        id,
        ui,
        source,
        surface,
        full_coverage: target == ShaderTarget::Artwork,
        program,
        spec,
        palette: [
            color(skin.dialogue.box_.color),
            color(skin.chrome.accent),
            color(skin.dialogue.box_.border_color),
            color(skin.dialogue.text.color),
        ],
        slots,
    });
    MATERIALS.insert(id, &material);
    let mut base = GpuView::new(id);
    let l = Arc::make_mut(&mut base.style.layout);
    l.position = PositionSpec::Absolute;
    l.offset_left = Some(LengthSpec::Px(0.));
    l.offset_top = Some(LengthSpec::Px(0.));
    l.width = Some(LengthSpec::Percent(100.));
    l.height = Some(LengthSpec::Percent(100.));
    l.pointer_events = Some(PointerEventsSpec::None);
    Some(
        widget(EffectView { base, material })
            .on(move |e: &nana_ui::runtime::SizeChanged| {
                *logical_size.lock().unwrap_or_else(|e| e.into_inner()) = [e.width, e.height];
            })
            .into_any(),
    )
}

#[derive(Debug, serde::Serialize)]
pub struct CacheStats {
    pub programs: usize,
    pub masks: usize,
    pub bytes: usize,
    pub pending: usize,
    pub uniforms: usize,
    pub scales: usize,
    pub compilations: u64,
    /// Compiles deferred because both compiler slots were busy.
    pub refused: u64,
    pub uniform_allocations: u64,
    /// Rest frames held for programs drawn frozen.
    pub frozen: usize,
}

#[derive(Default)]
struct EventState {
    line: Option<nanatype_model::LineId>,
    speaker: String,
    line_at: u64,
    speaker_at: u64,
}

struct EventEntry {
    source: u8,
    track_slot: usize,
    signal: Signal<Arc<DisplayView>>,
    state: EventState,
}

const MAX_EVENTS: usize = 32;
static EVENTS: LazyLock<Mutex<Vec<EventEntry>>> = LazyLock::new(Default::default);

/// Line age, speaker age, reveal progress and the continue flag.
/// Keyed by `(source, track_slot, display_signal)` so multiple `Source::Preview`
/// stages do not collide on `(Preview, 0)`, while outputs of the same track share ages.
pub(super) fn event_values(ui: Stage, source: Source, now: u64) -> [f32; 4] {
    let signal = if source == Source::Preview {
        ui.preview_display
    } else {
        ui.display
    };
    let v = signal.get_untracked();
    let source_tag = source as u8;
    let mut states = EVENTS.lock().unwrap_or_else(|e| e.into_inner());
    let pos = states.iter().position(|e| {
        e.source == source_tag && e.track_slot == ui.track_slot && e.signal == signal
    });
    let mut entry = if let Some(pos) = pos {
        states.remove(pos)
    } else {
        if states.len() >= MAX_EVENTS {
            states.remove(0);
        }
        EventEntry {
            source: source_tag,
            track_slot: ui.track_slot,
            signal,
            state: EventState::default(),
        }
    };
    let s = &mut entry.state;
    if s.line != v.line {
        s.line = v.line;
        s.line_at = now;
    }
    if s.speaker != v.speaker.name {
        s.speaker = v.speaker.name.clone();
        s.speaker_at = now;
    }
    let out = [
        now.saturating_sub(s.line_at) as f32 / 1000.,
        now.saturating_sub(s.speaker_at) as f32 / 1000.,
        {
            let total = v
                .reveal
                .hidden_from
                .saturating_sub(v.reveal.visible_from)
                .max(1);
            let mut shown = v.reveal.shown;
            if let Some(seg) = &v.reveal.segment {
                shown = shown.max(
                    seg.start
                        + seg
                            .reveal_ms
                            .partition_point(|offset| seg.origin_ms + u64::from(*offset) <= now)
                            as u32,
                );
            }
            (shown.saturating_sub(v.reveal.visible_from) as f32 / total as f32).clamp(0., 1.)
        },
        if v.continue_indicator { 1. } else { 0. },
    ];
    states.push(entry);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ui() -> Stage {
        let skin = nanatype_theme::SkinRegistry::new(None, "0.1.0")
            .load("nanatype.tidal")
            .unwrap()
            .skin;
        Stage::new(
            Arc::new(nanatype_settings::Settings::default()),
            skin,
            nanatype_i18n::Locale::ZhCn,
        )
    }

    #[test]
    fn events_are_shared_by_outputs_and_isolated_across_preview_stages() {
        let ui_a = ui();
        let ui_b = ui();
        ui_a.preview_display.update(|v| {
            let v = Arc::make_mut(v);
            v.empty = false;
            v.line = Some(nanatype_model::LineId(10));
            v.speaker.name = "Alpha".into();
        });
        ui_b.preview_display.update(|v| {
            let v = Arc::make_mut(v);
            v.empty = false;
            v.line = Some(nanatype_model::LineId(20));
            v.speaker.name = "Beta".into();
        });
        assert_eq!(&event_values(ui_a, Source::Preview, 1000)[..2], &[0., 0.]);
        assert_eq!(&event_values(ui_b, Source::Preview, 1500)[..2], &[0., 0.]);
        // Querying ui_a at 2000 must still see 1s elapsed, never overwritten by ui_b.
        assert_eq!(&event_values(ui_a, Source::Preview, 2000)[..2], &[1., 1.]);
    }

    #[test]
    fn events_are_shared_by_outputs_and_not_retriggered_by_repaints() {
        let ui = ui();
        ui.display.update(|v| {
            let v = Arc::make_mut(v);
            v.empty = false;
            v.line = Some(nanatype_model::LineId(100));
            v.speaker.name = "A".into();
        });
        assert_eq!(&event_values(ui, Source::Display, 1000)[..2], &[0., 0.]);
        assert_eq!(&event_values(ui, Source::Display, 1500)[..2], &[0.5, 0.5]);
        assert_eq!(
            event_values(ui, Source::Display, 1500),
            event_values(ui, Source::Display, 1500)
        );
        ui.display.update(|v| {
            let v = Arc::make_mut(v);
            v.line = Some(nanatype_model::LineId(101));
            v.continue_indicator = true;
        });
        assert_eq!(event_values(ui, Source::Display, 2000), [0., 1., 0., 1.]);
        ui.peer_lanes[0].display.update(|v| {
            let v = Arc::make_mut(v);
            v.empty = false;
            v.line = Some(nanatype_model::LineId(900));
            v.speaker.name = "B".into();
        });
        assert_eq!(
            &event_values(ui.remote(), Source::Display, 2000)[..2],
            &[0., 0.]
        );
        assert_eq!(&event_values(ui, Source::Display, 2500)[..2], &[0.5, 1.5]);
    }

    #[test]
    fn preview_events_follow_the_sample_segment_like_a_live_line() {
        let ui = ui();
        ui.preview_display.update(|v| {
            let v = Arc::make_mut(v);
            v.empty = false;
            v.line = Some(nanatype_model::LineId(7));
            v.speaker.name = "奈奈".into();
            v.reveal.hidden_from = 2;
            v.reveal.segment = Some(nanatype_core::SegmentReveal {
                origin_ms: 1_000,
                start: 0,
                reveal_ms: Arc::from([0u32, 500]),
            });
        });
        let first = event_values(ui, Source::Preview, 1_000);
        assert_eq!(first[0], 0.);
        assert_eq!(first[2], 0.5);
        let later = event_values(ui, Source::Preview, 1_500);
        assert_eq!(later[0], 0.5);
        assert_eq!(later[2], 1.);
        ui.preview_display.update(|v| {
            Arc::make_mut(v).line = Some(nanatype_model::LineId(8));
        });
        assert_eq!(event_values(ui, Source::Preview, 1_800)[0], 0.);
    }

    #[test]
    fn slots_pack_params_as_vec4s_in_the_palette_colour_space() {
        use nanatype_theme::{Resolved, SlotBinding, SlotComponent};
        let values: std::collections::BTreeMap<String, Resolved> = [
            ("size".to_owned(), Resolved::Number(36.)),
            ("on".to_owned(), Resolved::Bool(true)),
            (
                "kind".to_owned(),
                Resolved::Enum {
                    id: "dog".into(),
                    index: 2,
                },
            ),
            (
                "fur".to_owned(),
                Resolved::Color(nanatype_model::Rgba8::rgba(255, 128, 0, 64)),
            ),
        ]
        .into();
        let slots = pack_slots(
            &[
                SlotBinding::Tuple(vec![
                    SlotComponent::Param("size".into()),
                    SlotComponent::Number(4.),
                    SlotComponent::Param("on".into()),
                    SlotComponent::Param("kind".into()),
                ]),
                SlotBinding::Param("fur".into()),
                SlotBinding::Number(7.),
                SlotBinding::Param("missing".into()),
            ],
            &values,
        );
        assert_eq!(&slots[..4], &[36., 4., 1., 2.]);
        assert_eq!(
            &slots[4..8],
            &color(nanatype_model::Rgba8::rgba(255, 128, 0, 64))
        );
        assert_eq!(&slots[8..12], &[7., 0., 0., 0.]);
        assert!(slots[12..].iter().all(|v| *v == 0.));
    }

    #[test]
    fn an_artwork_owns_the_continue_mark_only_while_it_plays() {
        let ui = ui();
        let mut skin = (*ui.skin.get_untracked()).clone();
        let artwork = skin
            .dialogue
            .shader_effects
            .iter()
            .find(|e| e.target == ShaderTarget::Artwork)
            .map(|e| e.shader.clone())
            .expect("tidal has an artwork");
        assert!(!owns_continue(ui, Source::Display, &skin));
        skin.shaders.get_mut(&artwork).unwrap().owns_continue = true;
        assert!(owns_continue(ui, Source::Display, &skin));
        // A preview owns it only while it plays effects.
        assert!(!owns_continue(ui, Source::Preview, &skin));
        ui.reduced_motion.set(true);
        assert!(!owns_continue(ui, Source::Display, &skin));
        ui.reduced_motion.set(false);
        ui.settings
            .update(|s| Arc::make_mut(s).skin.advanced_effects = false);
        assert!(!owns_continue(ui, Source::Display, &skin));
    }

    #[test]
    fn declared_programs_wake_only_while_they_move() {
        let ui = ui();
        let lane = ui.lane(5);
        let mut skin = (*ui.skin.get_untracked()).clone();
        for s in skin.shaders.values_mut() {
            s.active = vec![ShaderActive::Line(0.6)];
        }
        ui.skin.set(Arc::new(skin.clone()));
        lane.display.update(|v| {
            let v = Arc::make_mut(v);
            v.empty = false;
            v.line = Some(nanatype_model::LineId(7));
        });
        let at = |ms| crate::clock::capture_at(ms, || wake(lane));
        assert_eq!(at(10_000), Wake::Continuous);
        assert_eq!(at(10_500), Wake::Continuous);
        assert_eq!(at(10_700), Wake::Idle);
        lane.display
            .update(|v| Arc::make_mut(v).line = Some(nanatype_model::LineId(8)));
        assert_eq!(at(11_000), Wake::Continuous);
        for s in skin.shaders.values_mut() {
            s.active.clear();
            s.cadence = 2.;
        }
        ui.skin.set(Arc::new(skin));
        assert_eq!(at(11_100), Wake::At(11_500));
        assert_eq!(at(11_500), Wake::At(12_000));
        assert_eq!(Wake::At(3).or(Wake::At(2)), Wake::At(2));
        assert_eq!(Wake::Idle.or(Wake::At(2)), Wake::At(2));
        assert_eq!(Wake::At(2).or(Wake::Continuous), Wake::Continuous);
    }

    #[test]
    fn stepped_clocks_hold_until_the_wake() {
        let spec = ShaderSpec {
            cadence: 0.75,
            ..Default::default()
        };
        assert_eq!(stepped(&spec, 1333), 0);
        assert_eq!(stepped(&spec, 1334), 1334);
        let cell = |ms: u64| ((ms as f64 / 16_000. * 4.).fract() * 3.).floor() as u32;
        for k in 0..24u64 {
            let start = (k as f64 * 4_000. / 3.).ceil() as u64;
            assert_eq!(cell(stepped(&spec, start)), (k % 3) as u32, "step {k}");
        }
        for now in [0, 1, 1333, 1334, 2666, 2667, 40_000] {
            let Wake::At(next) = next_frame(&spec, now) else {
                panic!("stepped programs wake at a time");
            };
            assert_eq!(stepped(&spec, next - 1), stepped(&spec, now), "{now}");
            assert!(stepped(&spec, next) > stepped(&spec, now), "{now}");
        }
        assert_eq!(next_frame(&ShaderSpec::default(), 5), Wake::Continuous);
    }

    #[test]
    fn only_enabled_nonempty_display_tracks_request_frames() {
        let ui = ui();
        assert!(!animating(ui));
        ui.display.update(|v| Arc::make_mut(v).empty = false);
        assert!(animating(ui));
        ui.reduced_motion.set(true);
        assert!(!animating(ui));
        ui.reduced_motion.set(false);
        ui.settings
            .update(|s| Arc::make_mut(s).skin.advanced_effects = false);
        assert!(!animating(ui));
        assert!(!preview_animating(ui));
    }

    #[test]
    fn skin_motion_off_freezes_programs_and_the_continue_mark() {
        let ui = ui();
        ui.display.update(|v| Arc::make_mut(v).empty = false);
        let mut skin = (*ui.skin.get_untracked()).clone();
        for s in skin.shaders.values_mut() {
            s.owns_continue = true;
        }
        assert!(animating(ui) && owns_continue(ui, Source::Display, &skin));
        ui.settings.update(|s| Arc::make_mut(s).skin.motion = false);
        assert!(!motion(ui) && !enabled(ui) && !animating(ui));
        assert!(!owns_continue(ui, Source::Display, &skin));
        assert!(!super::super::sweep::animating(ui));
        assert!(super::super::materials::enabled(ui));
    }

    #[test]
    fn printed_classics_have_no_animation_frame_schedule() {
        let ui = ui();
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        ui.display.update(|v| Arc::make_mut(v).empty = false);
        for name in [
            "halo",
            "parchment",
            "adv",
            "vanilla",
            "comic",
            "ink",
            "echo_vanilla",
            "echo_glass",
            "echo_bubble",
            "subtitle",
            "echo_void",
        ] {
            let skin = registry.load(&format!("nanatype.{name}")).unwrap().skin;
            assert!(skin.shaders.values().all(|s| !s.animated));
            assert!(skin.dialogue.shader_effects.is_empty());
            ui.skin.set(skin);
            for size in [[952., 252.], [300., 1200.], [300., 240.], [160., 96.]] {
                ui.display_size.set(size);
                assert!(!animating(ui), "{name} {size:?}");
            }
        }
    }

    #[test]
    fn event_and_stepped_classics_never_loop_at_the_frame_rate() {
        let ui = ui();
        let lane = ui.lane(6);
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        lane.display.update(|v| {
            let v = Arc::make_mut(v);
            v.empty = false;
            v.line = Some(nanatype_model::LineId(1));
        });
        let at = |ms| crate::clock::capture_at(ms, || wake(lane));
        let sizes = [[952., 252.], [300., 1200.], [300., 240.], [160., 96.]];
        ui.skin.set(registry.load("nanatype.night").unwrap().skin);
        lane.display_size.set(sizes[0]);
        assert_eq!(at(60_000), Wake::Continuous);
        for size in sizes {
            lane.display_size.set(size);
            assert_eq!(at(61_000), Wake::Idle, "night {size:?}");
        }
        for (name, waiting, steps) in [
            ("rpg", false, false),
            ("rpg", true, true),
            ("terminal", false, true),
            ("pixel", false, true),
        ] {
            ui.skin
                .set(registry.load(&format!("nanatype.{name}")).unwrap().skin);
            lane.display
                .update(|v| Arc::make_mut(v).continue_indicator = waiting);
            for size in sizes {
                lane.display_size.set(size);
                let wake = at(62_000);
                assert_ne!(wake, Wake::Continuous, "{name} {size:?}");
                if size == sizes[0] {
                    assert_eq!(matches!(wake, Wake::At(_)), steps, "{name}");
                }
            }
        }
    }

    #[test]
    fn frozen_authored_artwork_does_not_request_an_animation_loop() {
        let ui = ui();
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        ui.display.update(|v| Arc::make_mut(v).empty = false);
        for name in [
            "gothic",
            "celestial",
            "forest",
            "tidal",
            "lantern",
            "citrus",
            "crayon",
        ] {
            ui.skin
                .set(registry.load(&format!("nanatype.{name}")).unwrap().skin);
            for size in [[952., 252.], [300., 1200.], [300., 240.], [160., 96.]] {
                ui.display_size.set(size);
                ui.reduced_motion.set(true);
                assert!(!animating(ui), "{name} {size:?}: reduced motion");
                ui.reduced_motion.set(false);
                ui.settings
                    .update(|s| Arc::make_mut(s).skin.advanced_effects = false);
                assert!(!animating(ui), "{name} {size:?}: effects disabled");
                ui.settings
                    .update(|s| Arc::make_mut(s).skin.advanced_effects = true);
                assert_eq!(animating(ui), name != "crayon", "{name} {size:?}");
            }
        }
    }
}
