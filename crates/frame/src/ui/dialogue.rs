//! The dialogue box.
//!
//! One builder draws the typewriter, the input editor's box and the history
//! bubbles, so they cannot drift apart. The text itself is a single NanaUI
//! rich text node (a `RichTextView`, or the `RichTextEditor` in the input
//! window) built from the document by [`super::rich`]; the typewriter reveal
//! and the per-character effects are glyph presentation evaluated on the GPU,
//! so nothing reshapes while a line plays.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use std::collections::HashMap;
use unicode_segmentation::UnicodeSegmentation;

use nana_ui::runtime::view::{AnyView, IntoView, dynamic, widget};
use nana_ui::runtime::{
    NodeStyle, RichTextView, SizeChanged, Stack, Text, TextHorizontalAlignment,
};
use nana_ui_core::{
    AlignSpec, BackgroundImage, BackgroundImageFit, BackgroundPosition, BackgroundRepeat,
    JustifySpec, LayoutStyle, LengthSpec, OverflowSpec, PointerEventsSpec, PositionSpec,
};
use nanatype_core::{DisplayView, Reveal};
use nanatype_model::{Document, Entrance, EntranceKind, Inline, Overflow, Rgba8};
use nanatype_theme::{
    Anchor, ColorRef, ColorRole, ContinueMotion, DecorLayer, DecorSpec, DecorWhen, Edge,
    EffectDefaults, Side, Skin, TextAlign, TextShadowSpec as SkinShadow,
    TextStrokeSpec as SkinStroke, anchor_frac, continue_motion, decor_motion, place, sprite_frame,
    tail_geometry,
};

use super::editor_geometry::Capture;
use super::overflow;
use super::rich::{Env, Look, RevealPlan, effect_table, to_rich};
use super::surface::{self, Surface, affine, calc, len};
use super::{I18N, Stage, c, restyle};

/// Base text look a document is drawn with (skin dialogue text or history text).
#[derive(Clone, PartialEq)]
pub struct Base {
    pub responsive_text: bool,
    pub material: Option<nana_ui_core::MaterialRef>,
    pub color: Rgba8,
    pub size: f32,
    pub line_height: f32,
    pub weight: u16,
    pub family: Option<String>,
    pub name_family: Option<String>,
    pub accent_family: Option<String>,
    pub latin: Option<(String, u16)>,
    pub letter_spacing: f32,
    pub shadow: Option<SkinShadow>,
    pub stroke: Option<SkinStroke>,
    pub effects: EffectDefaults,
    pub entrance: Entrance,
    pub reduced_motion: bool,
    pub masks: nana_ui_core::TextTransitions,
    pub light: Option<nanatype_theme::TextLightSpec>,
    pub scramble: Option<nanatype_theme::ScrambleSpec>,
    /// The entrance material, when its inputs depend on the text layout.
    pub layout_material: Option<super::pipeline::LayoutMaterial>,
    pub light_exit: Option<(u64, u16)>,
    pub light_preview: bool,
    pub align: TextAlign,
    pub mention: super::rich::MentionLook,
    /// The box under the text, for the display's contrast guard and
    /// skin-aware glows; `None` in the editor.
    pub ink: Option<super::rich::BoxInk>,
}

fn skin_mask(
    skin: &Skin,
    name: Option<&str>,
    scope: nanatype_theme::TransitionScope,
    ms: u64,
) -> Option<nana_ui_core::PresentationMask> {
    Some(nana_ui_core::PresentationMask {
        material: super::materials::resolve(skin, name)?,
        space: match scope {
            nanatype_theme::TransitionScope::Glyph => nana_ui_core::PresentationMaskSpace::Local,
            nanatype_theme::TransitionScope::Text => nana_ui_core::PresentationMaskSpace::Target,
        },
        start: std::time::Duration::ZERO,
        duration: std::time::Duration::from_millis(ms),
    })
}

impl Base {
    fn from_text(
        skin: &Skin,
        t: &nanatype_theme::TextSpec,
        fill: Rgba8,
        entrance: Entrance,
        reduced: bool,
    ) -> Self {
        Self {
            responsive_text: false,
            light: skin.dialogue.reveal.light.clone(),
            scramble: skin.dialogue.reveal.scramble.clone(),
            layout_material: None,
            light_exit: None,
            light_preview: false,
            masks: nana_ui_core::TextTransitions {
                entrance: if !reduced && entrance.kind != EntranceKind::None && entrance.ms > 0 {
                    let spec = &skin.dialogue.reveal.entrance;
                    skin_mask(
                        skin,
                        spec.material.as_deref(),
                        spec.scope,
                        u64::from(entrance.ms),
                    )
                } else {
                    None
                },
                ..Default::default()
            },
            material: super::materials::resolve(skin, t.material.as_deref()),
            color: t.color,
            size: t.size,
            line_height: t.line_height,
            weight: skin.fonts.body.weight,
            family: t
                .font_family
                .clone()
                .or_else(|| skin.fonts.body.families.first().cloned()),
            name_family: skin.fonts.name.families.first().cloned(),
            accent_family: skin.fonts.accent.families.first().cloned(),
            latin: if t.font_family.is_none() {
                skin.fonts
                    .latin
                    .as_ref()
                    .and_then(|f| f.families.first().map(|family| (family.clone(), f.weight)))
            } else {
                None
            },
            letter_spacing: t.letter_spacing,
            shadow: t.shadow.clone(),
            stroke: t.stroke.clone(),
            effects: skin.effects.clone(),
            entrance,
            reduced_motion: reduced,
            align: t.align,
            mention: super::rich::MentionLook::from_skin(skin),
            ink: super::rich::BoxInk::new(skin, fill, t),
        }
    }

    pub fn dialogue(skin: &Skin, entrance: Entrance, reduced: bool) -> Self {
        let mut base = Self::from_text(
            skin,
            &skin.dialogue.text,
            skin.dialogue.box_.color,
            entrance,
            reduced,
        );
        base.responsive_text = skin.dialogue.responsive.is_some();
        base.layout_material = super::pipeline::LayoutMaterial::entrance(skin);
        base
    }

    /// The input editor's text: the dialogue text, nothing animating in.
    pub fn editor(skin: &Skin) -> Self {
        let entrance = Entrance {
            kind: nanatype_model::EntranceKind::None,
            ms: 0,
        };
        Self {
            responsive_text: false,
            ink: None,
            material: None,
            masks: Default::default(),
            light: None,
            scramble: None,
            layout_material: None,
            ..Self::dialogue(skin, entrance, false)
        }
    }

    pub fn history(skin: &Skin, entrance: Entrance, reduced: bool) -> Self {
        let mut base = Self::from_text(
            skin,
            &skin.history.text,
            skin.history.bubble_color,
            entrance,
            reduced,
        );
        base.light = None;
        base.scramble = None;
        if skin.dialogue.reveal.light.is_some()
            || skin.dialogue.reveal.scramble.is_some()
            || skin.entrance_needs_text_layout()
        {
            base.masks = Default::default();
        }
        base
    }

    pub fn look(&self) -> Look {
        Look {
            size: self.size,
            color: self.color,
            weight: self.weight,
            line_height: self.line_height,
            body: self.family.clone(),
            name: self.name_family.clone(),
            accent: self.accent_family.clone(),
            latin: self.latin.clone(),
            mention: self.mention.clone(),
            ink: self.ink.clone(),
        }
    }

    /// The node style every rich text of this look shares: size, family,
    /// colour, line height and the skin's outline and shadow.
    pub fn node_style(&self) -> NodeStyle {
        let mut st = NodeStyle::default();
        let l = Arc::make_mut(&mut st.layout);
        l.paint.text_material = self.material.clone();
        l.font_size = Some(self.size);
        l.font_weight = Some(self.weight);
        l.font_family = self.family.clone();
        l.letter_spacing = Some(self.letter_spacing);
        l.white_space = nana_ui_core::WhiteSpaceSpec::PreWrap;
        l.overflow_wrap = Some(nana_ui_core::OverflowWrapSpec::Anywhere);
        l.min_width = Some(LengthSpec::Px(0.0));
        l.color = Some(c(self.color));
        l.line_height = Some(nana_ui_core::LineHeightSpec::Absolute(
            self.size * self.line_height,
        ));
        if let Some(s) = &self.shadow {
            surface::text_shadow(l, s);
        }
        if let Some(s) = &self.stroke {
            surface::text_stroke(l, s);
        }
        // Look ahead when breaking, so a paragraph does not end on a lone
        // character (`text-wrap-style: pretty`).
        l.text_typography.wrap_style = Some(nana_ui_core::TextWrapStyleSpec::Pretty);
        st.text_horizontal_alignment = match self.align {
            TextAlign::Start => TextHorizontalAlignment::Start,
            TextAlign::Center => TextHorizontalAlignment::Center,
            TextAlign::End => TextHorizontalAlignment::End,
        };
        st
    }
}

/// How a document's units appear over time.
#[derive(Clone)]
pub enum Timing {
    /// Everything shown, nothing animates on entry.
    Static,
    /// Per-unit reveal times from a [`Reveal`].
    Reveal(Reveal),
}

/// `doc` as one rich text node in `window`, with its reveal and effects.
pub fn document(ui: Stage, doc: &Document, base: &Base, timing: Timing) -> AnyView {
    document_on(ui, doc, base, timing, None, Source::Display)
}

/// [`document`] for the typewriter's page `follow`: the node is recorded in
/// `ui.text_node` and its reveal times published for the host to follow.
fn document_on(
    ui: Stage,
    doc: &Document,
    base: &Base,
    timing: Timing,
    follow: Option<overflow::PageKey>,
    source: Source,
) -> AnyView {
    let urls = ui.asset_urls.get_untracked();
    let look = base.look();
    let env = Env {
        look: &look,
        urls: &urls,
        editor: false,
        locale: ui.locale.get_untracked(),
    };
    let rich = to_rich(doc, &env);
    let mut entrance =
        ui.settings
            .with_untracked(|s| s.playback.entrance)
            .map_or(base.entrance, |kind| Entrance {
                kind,
                ms: if kind == EntranceKind::None {
                    0
                } else {
                    base.entrance.ms
                },
            });
    if base.scramble.is_some() && (base.reduced_motion || !super::materials::enabled(ui)) {
        entrance = Entrance {
            kind: EntranceKind::None,
            ms: 0,
        };
    }
    let mut reveal = match &timing {
        Timing::Static => None,
        Timing::Reveal(r) => Some(RevealPlan::new(&rich, r, entrance)),
    };
    if base.scramble.is_some()
        && let Timing::Reveal(r) = &timing
        && r.segment.is_some()
        && let Some(plan) = &mut reveal
    {
        for ((_, unit), at) in super::rich::unit_graphemes(rich.text()).zip(&mut plan.at_ms) {
            // Zero is the native renderer's already-shown sentinel.
            if unit.is_some_and(|u| u >= r.shown) && *at == Some(0) {
                *at = Some(1);
            }
        }
    }
    if base.scramble.is_some()
        && !base.light_preview
        && let Some(plan) = &mut reveal
        && let Some(times) = &drawn_untracked_for(ui, source).mirror_reveal_ms
    {
        let mut unit = 0;
        for (i, g) in rich.text().graphemes(true).enumerate() {
            if g != "\n" {
                if plan.at_ms[i].is_some() {
                    plan.at_ms[i] = times.get(unit).copied();
                }
                unit += 1;
            }
        }
    }
    let scramble_plan = base
        .scramble
        .clone()
        .filter(|_| !base.reduced_motion && super::materials::enabled(ui))
        .filter(|_| {
            ui.settings
                .with_untracked(|s| s.playback.entrance.is_none())
                || base.light_exit.is_some()
        })
        .zip(
            reveal
                .as_ref()
                .map(|p| (p.at_ms.clone(), rich.text().to_owned())),
        );
    let light_plan = base
        .light
        .clone()
        .filter(|s| s.intensity > 0. && s.color.a > 0)
        .filter(|_| !base.reduced_motion && super::materials::enabled(ui))
        .zip(
            reveal
                .as_ref()
                .map(|p| (p.at_ms.clone(), rich.text().to_owned())),
        );
    let layout_material = base
        .layout_material
        .clone()
        .filter(|_| reveal.is_some() && entrance.kind != EntranceKind::None && entrance.ms > 0)
        .filter(|_| !base.reduced_motion && super::materials::enabled(ui))
        .filter(|_| {
            ui.settings
                .with_untracked(|s| s.playback.entrance.is_none() && s.playback.typing)
        });
    if let (Some(key), Some(plan)) = (follow, &reveal) {
        overflow::publish_on(
            ui.track_slot,
            overflow::PageText {
                key,
                text_len: rich.text().len(),
                text_trimmed: rich.text().trim_end().len(),
                glyphs: overflow::glyphs(rich.text(), &plan.at_ms),
            },
        );
    }
    let mut text_style = base.node_style();
    if !super::materials::enabled(ui) {
        Arc::make_mut(&mut text_style.layout).paint.text_material = None;
    }
    let mut view = RichTextView::new(rich)
        .decorative()
        .style(text_style)
        .width(LengthSpec::Fill);
    if reveal.is_some() || has_effects(doc) {
        // The framework stamps `reveal.start` with the compositor clock taken
        // before this flush. Offsets use this frame's anchor, the same instant
        // the preview origin and the effect clocks use. Sampling `now_ms`
        // again while the text is built would be later, and the glyphs would
        // show ahead of the light, scramble and shaders. An exit keeps the
        // glyphs that are already on screen.
        let schedule = reveal.map(|plan| {
            plan.schedule(
                std::time::Duration::ZERO,
                if base.masks.exit.is_some() || base.light_exit.is_some() {
                    drawn_untracked_for(ui, source)
                        .text_exit
                        .map_or_else(crate::clock::frame_ms, |e| e.at_ms)
                } else {
                    crate::clock::frame_ms()
                },
                base.reduced_motion,
            )
        });
        view = view.presentation_chains(effect_table(&base.effects, base.reduced_motion), schedule);
        if super::materials::enabled(ui)
            && !base.reduced_motion
            && let Some(p) = &mut view.presentation
        {
            p.transitions = base.masks.clone();
            if let Some((spec, _)) = &scramble_plan {
                p.transitions =
                    super::text_scramble::transitions(spec, base.light_exit, base.light_preview);
            }
            if ui
                .settings
                .with_untracked(|s| s.playback.entrance.is_some())
            {
                p.transitions.entrance = None;
            }
        }
    }
    let doc = doc.clone();
    let look = look.clone();
    let responsive = base.responsive_text;
    let update = move |v: &mut RichTextView| {
        if responsive {
            let vertical = vertical_text(ui);
            let l = Arc::make_mut(&mut v.style.layout);
            l.writing_mode = Some(if vertical {
                nana_ui_core::WritingModeSpec::VerticalRl
            } else {
                nana_ui_core::WritingModeSpec::HorizontalTb
            });
            l.text_orientation = Some(nana_ui_core::TextOrientationSpec::Mixed);
            l.height = vertical.then(|| LengthSpec::Px(ui.text_area_height.get().max(1.)));
            l.width = Some(LengthSpec::Fill);
            l.flex_shrink = Some(0.0);
        }
        let urls = ui.asset_urls.get();
        let mut rich = to_rich(
            &doc,
            &Env {
                look: &look,
                urls: &urls,
                editor: false,
                locale: ui.locale.get(),
            },
        );
        if responsive && vertical_text(ui) {
            let h = ui.display_size.get()[1];
            super::rich::vertical_objects(&mut rich, &look, h * 0.5);
        }
        if v.value != rich {
            v.value = rich;
        }
    };
    let update_view = move |v: &mut RichTextView| {
        update(v);
        if responsive {
            let vertical = vertical_text(ui);
            let width = vertical.then(|| {
                super::rich::vertical_extent(
                    &v.value,
                    &v.style,
                    ui.text_area_height.get().max(1.),
                    ui.locale.get(),
                )
            });
            let l = Arc::make_mut(&mut v.style.layout);
            l.width = Some(width.map_or(LengthSpec::Fill, LengthSpec::Px));
            l.position = if vertical {
                PositionSpec::Absolute
            } else {
                PositionSpec::Static
            };
            l.offset_right = vertical.then_some(LengthSpec::Px(0.));
            l.offset_top = vertical.then_some(LengthSpec::Px(0.));
        }
    };
    if let Some(material) = layout_material {
        return super::pipeline::view(
            view,
            material,
            ui.layout_epoch,
            update_view,
            follow.map(|_| ui.text_node),
        );
    }
    if let Some((spec, (times, text))) = scramble_plan {
        let update = Arc::new(update_view);
        let objects = base
            .light_exit
            .filter(|_| !view.value.objects().is_empty())
            .map(|exit| {
                let mut object_view = view.clone();
                let mut object_base = base.clone();
                object_base.color = Rgba8::rgba(0, 0, 0, 0);
                object_base.stroke = None;
                object_base.shadow = None;
                object_base.material = None;
                object_view.style = object_base.node_style();
                if let Some(p) = &mut object_view.presentation {
                    p.transitions.entrance = None;
                    p.transitions.exit =
                        Some(super::text_scramble::object_exit(exit, base.light_preview));
                }
                let update = update.clone();
                widget(object_view)
                    .bind(move |v: &mut RichTextView| {
                        update(v);
                        super::text_scramble::objects_only(&mut v.value);
                        let l = Arc::make_mut(&mut v.style.layout);
                        l.position = PositionSpec::Absolute;
                        l.offset_top = Some(LengthSpec::Px(0.));
                        l.pointer_events = Some(PointerEventsSpec::None);
                    })
                    .into_any()
            });
        let view = widget(view).bind(move |v: &mut RichTextView| update(v));
        let node = if follow.is_some() {
            ui.text_node
        } else {
            nana_ui::runtime::view::node_ref()
        };
        return super::text_scramble::wrap(
            ui,
            view.node_ref(node).into_any(),
            objects,
            node,
            spec,
            base.color,
            super::text_light::TextSchedule {
                at_ms: &times,
                value: &text,
                exit: base.light_exit,
                preview: base.light_preview,
            },
        );
    }
    let view = widget(view).bind(update_view);
    if let Some((spec, (times, text))) = light_plan {
        let node = if follow.is_some() {
            ui.text_node
        } else {
            nana_ui::runtime::view::node_ref()
        };
        return super::text_light::wrap(
            ui,
            view.node_ref(node).into_any(),
            node,
            spec,
            super::text_light::TextSchedule {
                at_ms: &times,
                value: &text,
                exit: base.light_exit,
                preview: base.light_preview,
            },
            entrance.ms,
        );
    }
    match follow {
        Some(_) => view.node_ref(ui.text_node).into_any(),
        None => view.into_any(),
    }
}

/// What a dialogue box shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    /// The typewriter: the display view with its reveal.
    Display,
    /// The input window: the editor sits where the text goes.
    Draft,
    /// The skin picker's sample: a fixed line, nothing animating on the box.
    Preview,
}

/// Everything a box is built from, so the skin picker can draw a skin that
/// is not the active one.
#[derive(Clone)]
pub struct BoxCtx {
    pub skin: Arc<Skin>,
    /// `skin:<path>` → URL for the skin's images.
    pub urls: Arc<HashMap<String, String>>,
    pub source: Source,
    /// The speaker a preview shows.
    pub preview_name: String,
}

impl BoxCtx {
    fn resolved(&self, ui: Stage) -> Self {
        if self.source == Source::Draft {
            return self.clone();
        }
        Self {
            skin: Arc::new(nanatype_theme::responsive_dialogue(
                &self.skin,
                canvas_size(ui, &self.skin),
            )),
            ..self.clone()
        }
    }
    fn active(ui: Stage, source: Source) -> Self {
        Self {
            skin: super::materials::render_skin(ui, ui.skin.get_untracked()),
            urls: ui.asset_urls.get_untracked(),
            source,
            preview_name: if source == Source::Preview {
                ui.preview_name.get_untracked()
            } else {
                Default::default()
            },
        }
    }

    fn url(&self, path: &str) -> Option<String> {
        self.urls.get(&format!("skin:{path}")).cloned()
    }

    fn box_surface(&self) -> Surface {
        if self.skin.dialogue.shader_effects.iter().any(|e| {
            e.target == nanatype_theme::ShaderTarget::Artwork
                && self
                    .skin
                    .shaders
                    .get(&e.shader)
                    .is_some_and(|s| s.static_fallback)
                && self.skin.shader_programs.contains_key(&e.shader)
        }) {
            // The same authored canvas supplies the still and moving silhouette.
            // Do not paint a stretched raster or a second generic frame under it.
            let native = Surface::dialogue_box(&self.skin, None);
            return Surface {
                radii: native.radii,
                shadows: native.shadows.into_iter().filter(|s| !s.inset).collect(),
                ..Default::default()
            };
        }
        let frame = self.skin.dialogue.box_.frame.as_ref();
        Surface::dialogue_box(&self.skin, frame.and_then(|f| self.url(&f.image)))
    }
}

/// Whether this display window writes its dialogue top to bottom; the
/// skin's portrait layout follows the window's shape on its own.
pub(super) fn vertical_text(ui: Stage) -> bool {
    ui.skin
        .with(|skin| ui.settings.with(|s| ui.canvas().vertical_text(s, skin)))
}

pub(super) fn canvas_size(ui: Stage, skin: &Skin) -> [f32; 2] {
    let size = ui.display_size.get();
    if size[0] > 0.0 && size[1] > 0.0 {
        return size;
    }
    let (w, h) = crate::canvas::size_of(ui.canvas(), skin, &ui.settings.get());
    [w, h]
}

/// A complete dialogue box filling its parent (region + overscan). In the
/// input window `editor` takes the text's place; it is built once so it
/// keeps its selection and undo history.
pub fn dialogue_box(ui: Stage, source: Source, editor: Option<AnyView>) -> AnyView {
    if let Some(editor) = editor {
        return box_view(ui, BoxCtx::active(ui, source), editor).into_any();
    }
    if source == Source::Display {
        *stage(ui.track_slot) = Track::new();
    }
    let key = move || {
        let skin = ui.skin.get();
        let settings = ui.settings.get();
        (
            Arc::as_ptr(&skin) as usize,
            (
                settings.playback.entrance,
                settings.skin.advanced_effects,
                ui.canvas().overlay(&settings).lock,
            ),
            ui.locale.get(),
            ui.portraits.with(|p| Arc::as_ptr(p) as usize),
            (source == Source::Preview).then(|| ui.preview_display.with(|v| v.revision)),
            ui.shader_preview.get(),
        )
    };
    dynamic(key, move |_| {
        box_view(
            ui,
            BoxCtx::active(ui, source),
            content(ui, source).into_any(),
        )
    })
    .into_any()
}

/// The box fades in and rises into place when a line appears on an empty
/// box, and fades out when it clears. No overshoot: a box that bounces
/// reads as jitter on stream.
const ENTER_MS: u64 = 180;
const EXIT_MS: u64 = 160;
const ENTER_RISE: f32 = 8.0;

/// The typewriter as drawn. While the box plays its exit, the line that just
/// left stays on it, so the text, name and portrait fade out together.
struct Track {
    /// The last line on the box.
    shown: Option<Arc<DisplayView>>,
    /// Whether the box has been looked at since it was mounted: a box that
    /// mounts with a line on it shows it at once.
    observed: bool,
    leaving: bool,
    entered_at: Option<u64>,
    left_at: u64,
}

impl Track {
    const fn new() -> Self {
        Self {
            shown: None,
            observed: false,
            leaving: false,
            entered_at: None,
            left_at: 0,
        }
    }

    /// `view` as drawn, noting when the box appears or clears.
    fn see(&mut self, view: Arc<DisplayView>, now: u64, slot: usize) -> Arc<DisplayView> {
        let first = !std::mem::replace(&mut self.observed, true);
        if !view.empty {
            if !first && (self.shown.is_none() || self.leaving) {
                self.entered_at = Some(now);
                MOVING_UNTIL[slot].store(now + ENTER_MS, Ordering::Relaxed);
            }
            self.leaving = false;
            self.shown = Some(view.clone());
            return view;
        }
        match &self.shown {
            Some(shown) => {
                if !self.leaving {
                    self.leaving = true;
                    self.left_at = now;
                    MOVING_UNTIL[slot].store(now + EXIT_MS, Ordering::Relaxed);
                }
                shown.clone()
            }
            None => view,
        }
    }

    /// Opacity and downward offset of the box at `now`.
    fn presence(&self, now: u64, reduced: bool) -> (f32, f32) {
        if self.leaving {
            let t = (now.saturating_sub(self.left_at) as f32 / EXIT_MS as f32).clamp(0.0, 1.0);
            return (1.0 - t * t * t, 0.0);
        }
        let Some(entered_at) = self.entered_at else {
            return (1.0, 0.0);
        };
        let t = (now.saturating_sub(entered_at) as f32 / ENTER_MS as f32).clamp(0.0, 1.0);
        let e = 1.0 - (1.0 - t).powi(3);
        (e, if reduced { 0.0 } else { ENTER_RISE * (1.0 - e) })
    }
}

static STAGES: [Mutex<Track>; super::TRACK_SLOTS] =
    [const { Mutex::new(Track::new()) }; super::TRACK_SLOTS];
/// Until when (ms) the box is entering or leaving, for frame requests.
static MOVING_UNTIL: [AtomicU64; super::TRACK_SLOTS] =
    [const { AtomicU64::new(0) }; super::TRACK_SLOTS];

fn stage(slot: usize) -> std::sync::MutexGuard<'static, Track> {
    STAGES[slot].lock().unwrap_or_else(|e| e.into_inner())
}

/// The typewriter's view as drawn (see [`Track`]), tracked.
fn drawn(ui: Stage) -> Arc<DisplayView> {
    drawn_for(ui, Source::Display)
}

fn drawn_untracked(ui: Stage) -> Arc<DisplayView> {
    drawn_untracked_for(ui, Source::Display)
}

fn drawn_for(ui: Stage, source: Source) -> Arc<DisplayView> {
    match source {
        Source::Preview => ui.preview_display.get(),
        Source::Display | Source::Draft => {
            let view = ui.display.get();
            stage(ui.track_slot).see(view, crate::clock::now_ms(), ui.track_slot)
        }
    }
}

fn drawn_untracked_for(ui: Stage, source: Source) -> Arc<DisplayView> {
    match source {
        Source::Preview => ui.preview_display.get_untracked(),
        Source::Display | Source::Draft => {
            let view = ui.display.get_untracked();
            stage(ui.track_slot).see(view, crate::clock::now_ms(), ui.track_slot)
        }
    }
}

/// How long this skin's text takes to leave. Scramble retires itself; an
/// exit material does too. Anything else just clears.
fn text_exit_ms(skin: &Skin) -> u16 {
    if let Some(scramble) = &skin.dialogue.reveal.scramble {
        return scramble.exit_ms;
    }
    skin.dialogue
        .reveal
        .exit
        .as_ref()
        .filter(|exit| exit.ms > 0)
        .filter(|exit| {
            exit.material
                .as_ref()
                .is_some_and(|name| skin.material_programs.contains_key(name))
        })
        .map(|exit| exit.ms)
        .unwrap_or(0)
}

/// A still box showing `doc` in `ctx.skin`, for the skin picker.
pub fn preview_box(ui: Stage, ctx: BoxCtx, doc: &Document) -> AnyView {
    let skin = super::materials::render_skin(ui, ctx.skin.clone());
    ui.skin.set(skin.clone());
    ui.preview_name.set(ctx.preview_name.clone());

    let now = crate::clock::now_ms();
    let units = doc.unit_count() as u32;
    let animate_text = ui.shader_preview.get_untracked()
        && !ui.reduced_motion.get_untracked()
        && super::materials::enabled(ui)
        && (skin.dialogue.reveal.light.is_some()
            || skin.dialogue.reveal.scramble.is_some()
            || skin.entrance_needs_text_layout());
    let entrance = if animate_text {
        Entrance {
            kind: skin.dialogue.reveal.entrance.kind,
            ms: skin.dialogue.reveal.entrance.ms,
        }
    } else {
        Entrance {
            kind: EntranceKind::None,
            ms: 0,
        }
    };
    let reveal = if animate_text {
        Reveal {
            visible_from: 0,
            shown: 0,
            hidden_from: units,
            segment: Some(nanatype_core::SegmentReveal {
                origin_ms: now,
                start: 0,
                reveal_ms: (0..units).map(|i| i * 65).collect::<Vec<_>>().into(),
            }),
        }
    } else {
        Reveal::full(units)
    };
    let mut display = DisplayView::empty(entrance);
    display.revision = 1;
    display.line = Some(fresh_preview_line());
    display.doc = Arc::new(doc.clone());
    display.speaker.name = ctx.preview_name.clone();
    display.reveal = reveal;
    display.empty = false;
    ui.preview_text_exit_ms.set(text_exit_ms(&skin));
    ui.preview_display.set(Arc::new(display));
    dialogue_box(ui, Source::Preview, None)
}

/// An absolutely placed overlay that never takes the pointer.
fn overlay(l: &mut LayoutStyle) {
    l.position = PositionSpec::Absolute;
    l.pointer_events = Some(PointerEventsSpec::None);
}

/// A layer covering the box that puts its child at `anchor`; the child then
/// moves by a paint transform, so content-sized parts (the name plate, the
/// continue mark) anchor anywhere without knowing their own size.
fn anchor_layer(anchor: Anchor, z: i32) -> Stack {
    const ALIGN: [AlignSpec; 3] = [AlignSpec::Start, AlignSpec::Center, AlignSpec::End];
    const JUSTIFY: [JustifySpec; 3] = [JustifySpec::Start, JustifySpec::Center, JustifySpec::End];
    let [fx, fy] = anchor_frac(anchor).map(|f| (f * 2.0) as usize);
    Stack::overlay_layer()
        .align(ALIGN[fy])
        .justify(JUSTIFY[fx])
        .with_layout(move |l| {
            // Parts sit on and past the box edge.
            l.overflow_x = OverflowSpec::Visible;
            l.overflow_y = OverflowSpec::Visible;
            l.z_index = Some(z);
        })
}

/// The box and everything around it:
///
/// ```text
/// outer (overscan + margin)
///  └ frame (the box's border box; fade and shake move all of it)
///     ├ decor below ├ tail shadow
///     ├ box └ text
///     ├ tail ├ decor above ├ portrait ├ name plate ├ continue mark
/// ```
fn box_view(ui: Stage, ctx: BoxCtx, content: AnyView) -> impl IntoView {
    let skin = ctx.skin.clone();
    let source = ctx.source;
    let d = &skin.dialogue;
    let overscan = d.overscan;
    let outer_ctx = ctx.clone();
    let frame_ctx = ctx.clone();
    let padding_ctx = ctx.clone();
    let plain =
        move || source == Source::Draft && !ui.settings.with(|s| s.general.input_skin_preview);

    let outer_layout = move |l: &mut LayoutStyle| {
        let (overscan, margin) = if source == Source::Draft {
            ui.skin
                .with(|s| (s.dialogue.overscan, s.dialogue.box_.margin))
        } else {
            (overscan, outer_ctx.resolved(ui).skin.dialogue.box_.margin)
        };
        let [top, right, bottom, left] =
            margin.map(|m| LengthSpec::Px(if plain() { 16.0 } else { overscan + m }));
        l.min_width = Some(LengthSpec::Px(0.0));
        l.min_height = Some(LengthSpec::Px(0.0));
        let cap = |v| match v {
            LengthSpec::Px(px) if source != Source::Draft => LengthSpec::Min2(
                nana_ui_core::LengthAtom::Px(px),
                nana_ui_core::LengthAtom::Percent(20.0),
            ),
            v => v,
        };
        l.padding_left = Some(cap(left));
        l.padding_right = Some(cap(right));
        l.padding_top = Some(cap(top));
        l.padding_bottom = Some(cap(bottom));
        l.align_items = AlignSpec::Center;
    };
    let mut outer =
        widget(Stack::fill_column(0.0).with_layout(&outer_layout)).on(move |e: &SizeChanged| {
            if source != Source::Draft {
                ui.display_size.set_if_changed([e.width, e.height]);
            }
        });
    outer = outer.bind(move |s: &mut Stack| restyle(s, &outer_layout));
    // Moving and scaling the native window are the default gestures, so
    // dialogue content must leave pointer and wheel events for the host.
    let drag_window = source == Source::Display
        && ui
            .settings
            .with_untracked(|s| ui.canvas().overlay(s).lock != nanatype_settings::Lock::Locked);
    let frame_layout = move |l: &mut LayoutStyle| {
        let fraction = if plain() {
            1.0
        } else if source == Source::Draft {
            ui.skin.with(|s| s.dialogue.box_.width_fraction)
        } else {
            frame_ctx.resolved(ui).skin.dialogue.box_.width_fraction
        };
        l.width = Some(LengthSpec::Percent(fraction * 100.0));
        l.min_width = Some(LengthSpec::Px(0.0));
        l.min_height = Some(LengthSpec::Px(0.0));
        l.position = PositionSpec::Relative;
        l.overflow_x = OverflowSpec::Visible;
        l.overflow_y = OverflowSpec::Visible;
        if drag_window {
            l.pointer_events = Some(PointerEventsSpec::None);
        }
    };
    let mut frame = widget(Stack::fill_column(0.0).with_layout(&frame_layout));
    frame = frame.bind(move |s: &mut Stack| restyle(s, &frame_layout));
    if source == Source::Display {
        frame = frame.bind(move |s: &mut Stack| {
            // Entrance and exit, whole-box fade and screen shake.
            let view = drawn(ui);
            let now = ui.now.get();
            let reduced = ui.reduced_motion.get();
            let (opacity, rise) = box_presence(ui, &view, now, reduced);
            let (dx, dy) = match view.shake {
                Some(sh) if now < sh.at_ms + u64::from(sh.ms) && !reduced => {
                    let left = 1.0 - (now - sh.at_ms) as f32 / sh.ms.max(1) as f32;
                    let step = now / 16;
                    (
                        noise(1, step) * sh.amp * left,
                        noise(2, step) * sh.amp * left,
                    )
                }
                _ => (0.0, 0.0),
            };
            let dy = dy + rise;
            restyle(s, |l| {
                l.opacity = Some(opacity);
                l.transform = (dx != 0.0 || dy != 0.0).then_some(affine(dx, dy, 0.0, 1.0));
            });
        });
    }

    // Only replace the container's paint/layout; the editor stays mounted.
    let shaders = responsive_shaders(ui, &ctx, DecorLayer::Above);
    let shaders_below = responsive_shaders(ui, &ctx, DecorLayer::Below);
    let box_stack = move || {
        let (surface, padding) = if plain() {
            (Surface::default(), [0.0; 4])
        } else if source == Source::Draft {
            let skin = ui.skin.get();
            let padding = skin.dialogue.padding;
            // The same box as the display, so an artwork shader is not
            // painted over a stretched still of itself.
            let ctx = BoxCtx {
                skin,
                urls: ui.asset_urls.get(),
                source,
                preview_name: String::new(),
            };
            (ctx.box_surface(), padding)
        } else {
            let resolved = padding_ctx.resolved(ui);
            (resolved.box_surface(), resolved.skin.dialogue.padding)
        };
        surface
            .materials(super::materials::enabled(ui))
            .stack(Stack::fill_column(0.0))
            .with_layout(move |l| {
                l.min_width = Some(LengthSpec::Px(0.0));
                l.min_height = Some(LengthSpec::Px(0.0));
                l.position = PositionSpec::Relative;
                // Large broadcast skins reserve wide decorative columns.
                // Keep an editable column when their input window is narrow.
                let horizontal = |px| {
                    if source == Source::Draft {
                        LengthSpec::Min2(
                            nana_ui_core::LengthAtom::Px(px),
                            nana_ui_core::LengthAtom::Percent(30.0),
                        )
                    } else {
                        LengthSpec::Px(px)
                    }
                };
                // Display GPU layers use the border box as their coordinate
                // space. Keep text padding in a separate child, otherwise
                // absolute ornaments inherit a shifted, smaller content box.
                let p = if source == Source::Draft {
                    padding
                } else {
                    [0.; 4]
                };
                l.padding_left = Some(horizontal(p[0]));
                l.padding_top = Some(LengthSpec::Px(p[1]));
                l.padding_right = Some(horizontal(p[2]));
                l.padding_bottom = Some(LengthSpec::Px(p[3]));
            })
    };
    let mut the_box = widget(box_stack());
    the_box = the_box.bind(move |s: &mut Stack| *s = box_stack());
    let material_padding = ctx.skin.dialogue.box_.shape.material.is_some();
    let text = text_port(ui, source, content);
    let text_ctx = ctx.clone();
    let text = if source == Source::Draft {
        text
    } else {
        widget(Stack::fill_column(0.))
            .bind(move |s: &mut Stack| {
                let padding = text_ctx.resolved(ui).skin.dialogue.padding;
                let vertical = ui
                    .settings
                    .with(|s| ui.canvas().vertical_text(s, &text_ctx.skin));
                restyle(s, |l| {
                    l.direction = Some(if vertical {
                        nana_ui_core::FlexDirection::Row
                    } else {
                        nana_ui_core::FlexDirection::Column
                    });
                    l.flex_reverse = vertical;
                    let cap = |px, percent| {
                        LengthSpec::Min2(
                            nana_ui_core::LengthAtom::Px(px),
                            nana_ui_core::LengthAtom::Percent(percent),
                        )
                    };
                    l.padding_left = Some(if vertical {
                        LengthSpec::Px(padding[0])
                    } else {
                        cap(padding[0], 25.0)
                    });
                    l.padding_top = Some(if vertical {
                        LengthSpec::Px(padding[1])
                    } else {
                        cap(padding[1], 20.0)
                    });
                    l.padding_right = Some(if vertical {
                        LengthSpec::Px(padding[2])
                    } else if material_padding {
                        LengthSpec::Min2(
                            nana_ui_core::LengthAtom::Px(padding[2]),
                            nana_ui_core::LengthAtom::Percent(18.),
                        )
                    } else {
                        cap(padding[2], 25.0)
                    });
                    l.padding_bottom = Some(if vertical {
                        LengthSpec::Px(padding[3])
                    } else {
                        cap(padding[3], 20.0)
                    });
                    l.min_width = Some(LengthSpec::Px(0.));
                    l.min_height = Some(LengthSpec::Px(0.));
                });
            })
            .children(((source == Source::Display).then(|| reply_quote(ui)), text))
            .into_any()
    };
    let the_box = the_box.children((input_skin_layer(ui, &ctx, sweep_layer), shaders, text));

    // An empty box is fully transparent, so a movable window would be
    // invisible until its first line: outline it while it can be dragged.
    let placeholder = drag_window.then(|| {
        empty_placeholder(
            ui,
            "dialogue-empty-placeholder",
            "display-placeholder",
            move || {
                let view = drawn(ui);
                box_presence(ui, &view, ui.now.get(), ui.reduced_motion.get()).0
            },
        )
    });
    outer.children((
        placeholder,
        frame.children((
            input_skin_layer(ui, &ctx, |ui, ctx| {
                skin_surroundings(ui, ctx, DecorLayer::Below)
            }),
            shaders_below,
            the_box,
            input_skin_layer(ui, &ctx, |ui, ctx| {
                skin_surroundings(ui, ctx, DecorLayer::Above)
            }),
        )),
    ))
}

/// The display box's opacity and entrance rise at `now`: hidden while empty,
/// then its entrance, exit and `[fade]`.
fn box_presence(ui: Stage, view: &DisplayView, now: u64, reduced: bool) -> (f32, f32) {
    let fade = view
        .fade
        .map(|(at, ms)| 1.0 - (now.saturating_sub(at) as f32 / ms.max(1) as f32).clamp(0.0, 1.0))
        .unwrap_or(1.0);
    let (shown, rise) = stage(ui.track_slot).presence(now, reduced);
    (if view.empty { 0.0 } else { fade * shown }, rise)
}

/// A dashed outline of the window, shown as its content (`presence`, 0..1)
/// fades out, so an unlocked overlay can be found and moved while empty.
pub(super) fn empty_placeholder(
    ui: Stage,
    key: &'static str,
    hint: &'static str,
    presence: impl Fn() -> f32 + Send + Sync + 'static,
) -> AnyView {
    let mut st = NodeStyle::default();
    {
        let l = Arc::make_mut(&mut st.layout);
        l.font_size = Some(14.0);
        l.color = Some(c(Rgba8::rgba(255, 255, 255, 220)));
        l.white_space_nowrap = true;
    }
    widget(Stack::column(0.).with_layout(|l| {
        overlay(l);
        l.background = Some(c(Rgba8::rgba(24, 24, 28, 140)));
    }))
    .css(nana_ui::runtime::view::css! {
        inset: 6px;
        align-items: center;
        justify-content: center;
        border: 1.5px dashed rgba(255, 255, 255, 0.6);
        border-radius: 10px;
    })
    .bind(move |s: &mut Stack| restyle(s, |l| l.opacity = Some(1.0 - presence())))
    .key(key)
    .children(
        widget(Text::new(ui.now_t(hint)).style(st)).bind(move |t: &mut Text| {
            let value = I18N.t_in(ui.locale.get(), hint);
            if t.value != value {
                t.value = value;
            }
        }),
    )
    .into_any()
}

/// Custom GPU paint always precedes the text, even for ornaments whose
/// static artwork is drawn above the box.
fn shader_layers(ui: Stage, ctx: &BoxCtx, surface: &Surface, layer: DecorLayer) -> Vec<AnyView> {
    let mut layers = Vec::new();
    for effect in ctx
        .skin
        .dialogue
        .shader_effects
        .iter()
        .filter(|_| layer == DecorLayer::Above)
    {
        if let Some(v) = super::shader_effects::view(
            ui,
            ctx.source,
            &ctx.skin,
            &effect.shader,
            surface.clone(),
            effect.target,
        ) {
            layers.push(v);
        }
    }
    for dec in &ctx.skin.dialogue.decor {
        if dec.layer != layer {
            continue;
        }
        let Some(id) = &dec.shader else {
            continue;
        };
        if !decor_shown(dec, ctx.source) {
            continue;
        }
        let shape =
            Surface::decor_image(&ctx.skin, dec, dec.image.as_ref().and_then(|p| ctx.url(p)));
        let Some(v) = super::shader_effects::view(
            ui,
            ctx.source,
            &ctx.skin,
            id,
            shape,
            nanatype_theme::ShaderTarget::Background,
        ) else {
            continue;
        };
        let placement = place(dec.anchor, dec.offset, dec.size);
        let [width, height] = dec.size.map(len);
        let dec = dec.clone();
        let source = ctx.source;
        layers.push(
            widget(Stack::row(0.).with_layout(move |l| {
                overlay(l);
                if let Some([t, r, b, left]) = dec.inset {
                    l.offset_top = Some(LengthSpec::Px(t));
                    l.offset_right = Some(LengthSpec::Px(r));
                    l.offset_bottom = Some(LengthSpec::Px(b));
                    l.offset_left = Some(LengthSpec::Px(left));
                } else {
                    l.offset_left = Some(calc(placement.left.percent, placement.left.px));
                    l.offset_top = Some(calc(placement.top.percent, placement.top.px));
                    l.width = Some(width);
                    l.height = Some(height);
                }
                // The proxy rotates its quad and samples in ornament-local
                // coordinates; NanaUI's custom GPU context exposes only AABBs.
                l.opacity = Some(dec.opacity);
            }))
            .visible(move || match source {
                Source::Preview => true,
                _ if dec.when != DecorWhen::Speaker => true,
                Source::Display => !drawn(ui).speaker.name.trim().is_empty(),
                Source::Draft => ui.speaker.with(|s| !s.name.trim().is_empty()),
            })
            .children(v)
            .into_any(),
        );
    }
    layers
}

fn responsive_shaders(ui: Stage, ctx: &BoxCtx, layer: DecorLayer) -> AnyView {
    // The input window's box follows its skin and the preview switch.
    if ctx.source == Source::Draft {
        return match layer {
            DecorLayer::Above => {
                input_skin_layer(ui, ctx, |ui, ctx| shader_art(ui, ctx, DecorLayer::Above))
            }
            DecorLayer::Below => {
                input_skin_layer(ui, ctx, |ui, ctx| shader_art(ui, ctx, DecorLayer::Below))
            }
        };
    }
    let ctx = ctx.clone();
    dynamic(move || ui.display_size.get(), move |_| shader_art(ui, &ctx.resolved(ui), layer))
        .css(nana_ui::runtime::view::css! { position: absolute; inset: 0; pointer-events: none; overflow: visible; }).into_any()
}

/// The skin's GPU programs on one layer, with the ornaments they paint.
fn shader_art(ui: Stage, ctx: &BoxCtx, layer: DecorLayer) -> AnyView {
    let layers = shader_layers(ui, ctx, &ctx.box_surface(), layer);
    let art: Vec<_> = ctx
        .skin
        .dialogue
        .decor
        .iter()
        .enumerate()
        .filter(|(_, d)| d.layer == layer && d.shader.is_some() && decor_shown(d, ctx.source))
        .map(|(index, d)| {
            decor_view(
                ui,
                ctx,
                d,
                ctx.skin.decor_indices.get(index).copied().unwrap_or(index),
            )
        })
        .collect();
    (art, layers).into_any()
}

/// Rebuild only skin layers when the draft's skin or resolved images change.
/// Both editors stay outside these scopes, retaining selection and undo history.
fn input_skin_layer(ui: Stage, ctx: &BoxCtx, build: fn(Stage, &BoxCtx) -> AnyView) -> AnyView {
    if ctx.source != Source::Draft {
        let ctx = ctx.clone();
        let skin = ctx.skin.clone();
        // The name plate turns with the writing direction.
        let key = move || {
            let vertical = ui.settings.with(|s| ui.canvas().vertical_text(s, &skin));
            (ui.display_size.get(), vertical)
        };
        return dynamic(key, move |_| build(ui, &ctx.resolved(ui)))
            .css(nana_ui::runtime::view::css! { position: absolute; inset: 0; pointer-events: none; overflow: visible; }).into_any();
    }
    dynamic(
        move || (
            ui.skin.get(),
            ui.asset_urls.get(),
            ui.settings.with(|s| s.general.input_skin_preview),
        ),
        move |(skin, urls, preview)| {
            preview.then(|| {
                build(ui, &BoxCtx {
                    skin: skin.clone(),
                    urls: urls.clone(),
                    source: Source::Draft,
                    preview_name: String::new(),
                })
            })
        },
    )
    .css(nana_ui::runtime::view::css! { position: absolute; inset: 0; pointer-events: none; overflow: visible; })
    .into_any()
}

fn skin_surroundings(ui: Stage, ctx: &BoxCtx, layer: DecorLayer) -> AnyView {
    let (tail_below, tail_above) = tail(ctx, &ctx.box_surface());
    let decor = decor_views(ui, ctx, layer);
    match layer {
        DecorLayer::Below => (decor, tail_below).into_any(),
        DecorLayer::Above => (
            tail_above,
            decor,
            portrait(ui, ctx),
            nameplate(ui, ctx),
            continue_indicator(ui, ctx),
        )
            .into_any(),
    }
}

fn sweep_layer(ui: Stage, ctx: &BoxCtx) -> AnyView {
    let source = ctx.source;
    let sweep = super::sweep::effective(&ctx.skin.dialogue.sweep).then(|| {
        let surface = ctx.box_surface();
        let spec = ctx.skin.dialogue.sweep.clone();
        dynamic(
            move || {
                !ui.reduced_motion.get()
                    && ui.settings.with(|s| s.skin.motion)
                    && match source {
                        Source::Display => !drawn(ui).empty,
                        // Built only while the input window's skin preview is on.
                        Source::Draft => true,
                        Source::Preview => false,
                    }
            },
            move |animated| super::sweep::view(surface.clone(), spec.clone(), *animated),
        )
        .css(nana_ui::runtime::view::css! { position: absolute; inset: 0; pointer-events: none; })
        .into_any()
    });
    sweep.into_any()
}

/// A fixed quote above horizontal text or to the right of vertical text.
/// It stays outside the body's scrollport when following long text.
fn reply_quote(ui: Stage) -> AnyView {
    let quote = dynamic(
        move || drawn(ui).revision,
        move |_| {
            let skin = ui.skin.get_untracked();
            let view = drawn_untracked(ui);
            let shown = !view.empty
                && skin.dialogue.reply.visible
                && ui
                    .settings
                    .with_untracked(|s| s.integrations.bilibili.quote_on_display);
            match view.reply.as_ref().filter(|_| shown) {
                Some(reply) => super::quote::display_quote_view(
                    ui,
                    &skin,
                    &skin.dialogue.reply,
                    reply,
                    &skin.dialogue.text,
                    super::quote::backdrop(
                        skin.dialogue.box_.color,
                        skin.dialogue.box_.frame.is_some(),
                    ),
                ),
                None => widget(Stack::column(0.0)).into_any(),
            }
        },
    );
    // A dynamic view owns a fill-width host. Bound the host as well as the
    // quote itself so it cannot consume the body's column space in a row.
    widget(Stack::column(0.))
        .bind(move |s: &mut Stack| {
            let skin = ui.skin.get();
            let view = drawn(ui);
            let shown = !view.empty
                && view.reply.is_some()
                && skin.dialogue.reply.visible
                && ui
                    .settings
                    .with(|s| s.integrations.bilibili.quote_on_display);
            let vertical = ui.settings.with(|s| ui.canvas().vertical_text(s, &skin));
            restyle(s, |l| {
                l.display = Some(if shown {
                    nana_ui_core::DisplaySpec::Flex
                } else {
                    nana_ui_core::DisplaySpec::None
                });
                l.width = Some(if vertical {
                    LengthSpec::Px(super::quote::column_width(
                        &skin.dialogue.reply,
                        &skin.dialogue.text,
                    ))
                } else {
                    LengthSpec::Fill
                });
                l.height = Some(if vertical {
                    LengthSpec::Fill
                } else {
                    LengthSpec::Shrink
                });
                l.flex_grow = Some(0.);
                l.flex_shrink = Some(0.);
            });
        })
        .children(quote)
        .into_any()
}

/// The text area: clips at the box's padding edge. On the typewriter the
/// text sits in a column the host scrolls up as typing passes the bottom.
fn text_port(ui: Stage, source: Source, content: AnyView) -> AnyView {
    let port = widget(Stack::fill_column(0.0).with_layout(|l| {
        l.position = PositionSpec::Relative;
        l.min_width = Some(LengthSpec::Px(0.0));
        l.min_height = Some(LengthSpec::Px(0.0));
        l.overflow_y = OverflowSpec::Hidden;
        l.overflow_x = OverflowSpec::Hidden;
    }))
    .on(move |e: &SizeChanged| {
        if source != Source::Draft {
            ui.text_area_height.set_if_changed(e.height);
        }
    });
    if source != Source::Display {
        return port.children(content).into_any();
    }
    let scroller = widget(Stack::column(0.0).width(LengthSpec::Fill).with_layout(|l| {
        l.position = PositionSpec::Relative;
        l.min_width = Some(LengthSpec::Px(0.0));
        l.flex_shrink = Some(0.0);
    }))
    .bind(move |s: &mut Stack| {
        let y = ui.text_scroll.get();
        let vertical = vertical_text(ui);
        restyle(s, |l| {
            l.height = vertical.then_some(LengthSpec::Percent(100.0));
            l.transform = (y != 0.0).then(|| {
                if vertical {
                    affine(y, 0.0, 0.0, 1.0)
                } else {
                    affine(0.0, -y, 0.0, 1.0)
                }
            });
        });
    })
    .children(content);
    port.node_ref(ui.text_port).children(scroller).into_any()
}

/// One id per preview pass, so shader line-age restarts with the sample.
pub(crate) fn fresh_preview_line() -> nanatype_model::LineId {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    nanatype_model::LineId(NEXT.fetch_add(1, Ordering::Relaxed))
}

/// The typewriter's text, rebuilt when the display view changes.
fn content(ui: Stage, source: Source) -> impl IntoView {
    dynamic(
        move || drawn_for(ui, source).revision,
        move |_| {
            let skin = ui.skin.get_untracked();
            let reduced = ui.reduced_motion.get_untracked();
            let view = drawn_untracked_for(ui, source);
            if view.empty {
                return widget(Stack::column(0.0)).into_any();
            }
            let mut base = Base::dialogue(&skin, view.entrance, reduced);
            base.light_preview = source == Source::Preview;
            if let Some(exit) = view.text_exit {
                base.light_exit = Some((exit.at_ms, exit.ms));
                // Layout-dependent masks encode partially drawn glyphs. Keep
                // that ink frozen while it leaves; dropping the mask would
                // replace unfinished strokes with the whole glyph. Other
                // entrances must not replay on the retirement mount.
                if base.layout_material.is_none() {
                    base.masks.entrance = None;
                }
                let elapsed = crate::clock::now_ms().saturating_sub(exit.at_ms);
                if let Some(spec) = &skin.dialogue.reveal.exit {
                    // Rebase the remaining duration when the presentation is mounted.
                    base.masks.exit = skin_mask(
                        &skin,
                        spec.material.as_deref(),
                        spec.scope,
                        u64::from(exit.ms).saturating_sub(elapsed),
                    );
                }
                base.masks.freeze_reveal = Some(std::time::Duration::ZERO);
            }
            let (doc, reveal) = view.page();
            // Clipped text has nothing to follow.
            let follow = (ui.track_slot > 0 || skin.dialogue.overflow != Overflow::Clip)
                .then(|| overflow::PageKey::of(&view));
            document_on(ui, &doc, &base, Timing::Reveal(reveal), follow, source)
        },
    )
}

/// Pseudo-random in -1..1 from an integer, stable across frames.
fn noise(i: u32, step: u64) -> f32 {
    let mut x = (u64::from(i) << 32 ^ step).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 29;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 32;
    (x as u32) as f32 / u32::MAX as f32 * 2.0 - 1.0
}

/// The speaker's name for the box's source, tracked.
fn speaker(ui: Stage, ctx: &BoxCtx) -> impl Fn() -> String + Send + 'static {
    let source = ctx.source;
    let preview = ctx.preview_name.clone();
    move || match source {
        Source::Display => drawn(ui).speaker.name.clone(),
        Source::Draft => ui.speaker.with(|s| s.name.clone()),
        Source::Preview => preview.clone(),
    }
}

/// Whether a speaker is showing (the typewriter hides when empty), tracked.
fn has_speaker(ui: Stage, ctx: &BoxCtx) -> impl Fn() -> bool + Send + 'static {
    let source = ctx.source;
    let preview = !ctx.preview_name.trim().is_empty();
    move || match source {
        Source::Display => {
            let v = drawn(ui);
            !v.empty && !v.speaker.name.trim().is_empty()
        }
        Source::Draft => ui.speaker.with(|s| !s.name.trim().is_empty()),
        Source::Preview => preview,
    }
}

fn nameplate(ui: Stage, ctx: &BoxCtx) -> impl IntoView + use<> {
    let skin = ctx.skin.clone();
    let source = ctx.source;
    let p = skin.dialogue.nameplate.clone();
    // Reserve the corner ornaments' strip for long names. Ordinary names
    // keep their content width; only an overflowing label is ellipsized.
    let artwork_clearance = if skin.has_dialogue_artwork() || skin.dialogue.responsive.is_some() {
        skin.dialogue.padding[2]
    } else {
        0.
    };
    let right_clearance = if p.anchor == Anchor::TopLeft {
        skin.dialogue
            .decor
            .iter()
            .filter(|d| d.anchor == Anchor::TopRight && d.offset[1] <= 24.0 && d.opacity > 0.0)
            .filter_map(|d| match d.size[0] {
                nanatype_theme::Len::Px(w) => Some((w - d.offset[0]).max(0.0) + 8.0),
                _ => None,
            })
            .fold(artwork_clearance, f32::max)
    } else {
        0.0
    };
    let plate = Surface::nameplate(&skin, p.frame.as_ref().and_then(|f| ctx.url(&f.image)));
    let mut st = NodeStyle::default();
    {
        let l = Arc::make_mut(&mut st.layout);
        l.paint.text_material = super::materials::enabled(ui)
            .then(|| super::materials::resolve(&skin, skin.dialogue.text.material.as_deref()))
            .flatten();
        l.font_size = Some(p.size);
        l.font_weight = Some(skin.fonts.name.weight);
        l.font_family = skin.fonts.name.families.first().cloned();
        l.color = Some(c(p.text_color));
        l.white_space_nowrap = true;
        if right_clearance > 0.0 || ctx.source != Source::Draft {
            l.min_width = Some(LengthSpec::Px(0.0));
            l.max_width = Some(LengthSpec::Percent(100.0));
            l.flex_shrink = Some(1.0);
            l.text_overflow_ellipsis = true;
        }
        if let Some(stroke) = &p.stroke {
            surface::text_stroke(l, stroke);
        }
        if let Some(s) = &p.text_shadow {
            surface::text_shadow(l, s);
        }
    }
    let visible = p.visible;
    let name = speaker(ui, ctx);
    let named = has_speaker(ui, ctx);
    let [ox, oy] = p.offset;
    let vertical = ctx.source != Source::Draft
        && ui
            .settings
            .with_untracked(|s| ui.canvas().vertical_text(s, &skin));
    if vertical {
        let l = Arc::make_mut(&mut st.layout);
        l.writing_mode = Some(nana_ui_core::WritingModeSpec::VerticalRl);
        l.text_orientation = Some(nana_ui_core::TextOrientationSpec::Mixed);
        l.white_space_nowrap = true;
        l.max_height = Some(LengthSpec::Px(
            (ui.display_size.get_untracked()[1] * 0.25).max(p.size),
        ));
        l.text_overflow_ellipsis = true;
    }
    let rotate = p.rotate;
    let anchor = p.anchor;
    widget(anchor_layer(p.anchor, 3))
        .visible(move || visible && named())
        .children(
            widget(
                plate
                    .stack(Stack::row(0.0).padding_xy(p.padding[0], p.padding[1]))
                    .with_layout(move |l| {
                        l.transform = Some(affine(ox, oy, rotate, 1.0));
                        if right_clearance > 0.0 || source != Source::Draft {
                            // The offset toward the far edge comes out of the room.
                            let inward = if anchor_frac(anchor)[0] >= 1.0 {
                                -ox
                            } else {
                                ox
                            };
                            l.min_width = Some(LengthSpec::Px(0.0));
                            l.max_width = Some(calc(100.0, -inward.max(0.0) - right_clearance));
                        }
                    }),
            )
            .key("dialogue-nameplate")
            .editor_part(ui, "nameplate")
            .children(widget(Text::new("").style(st)).bind(move |t: &mut Text| {
                let n = name();
                if t.value != n {
                    t.value = n;
                }
            })),
        )
}

fn portrait(ui: Stage, ctx: &BoxCtx) -> impl IntoView + use<> {
    let p = ctx.skin.dialogue.portrait.clone();
    let source = ctx.source;
    let [w, h] = p.size;
    widget(
        Stack::row(0.0)
            .width(LengthSpec::Px(w))
            .height(LengthSpec::Px(h))
            .with_layout(move |l| {
                overlay(l);
                l.offset_bottom = Some(LengthSpec::Px(p.offset[1]));
                match p.side {
                    Side::Left => l.offset_left = Some(LengthSpec::Px(-w * 0.5 + p.offset[0])),
                    Side::Right => l.offset_right = Some(LengthSpec::Px(-w * 0.5 - p.offset[0])),
                }
                l.border_radius = Some(p.radius);
            }),
    )
    .key("dialogue-portrait")
    .editor_part(ui, "portrait")
    .visible(move || {
        (source != Source::Preview || ui.editor_nodes.is_some())
            && ui.skin.with(|s| s.dialogue.portrait.visible)
            && !(source == Source::Display && drawn(ui).empty)
    })
    .bind(move |s: &mut Stack| {
        let variant = ui.display.with(|v| v.portrait.clone());
        let url = ui.portraits.with(|map| {
            variant
                .as_ref()
                .and_then(|v| map.get(v))
                .or_else(|| map.get("default"))
                .cloned()
        });
        let avatar = match source {
            Source::Draft => ui.speaker.get().avatar,
            Source::Display => drawn(ui).speaker.avatar.clone(),
            Source::Preview => None,
        };
        let url =
            url.or_else(|| avatar.and_then(|a| ui.asset_urls.with(|urls| urls.get(&a).cloned())));
        restyle(s, |l| {
            l.paint.background_image =
                url.map(|u| BackgroundImage::url_with_fit(u, BackgroundImageFit::Contain));
        });
    })
}

fn decor_view(ui: Stage, ctx: &BoxCtx, dec: &DecorSpec, index: usize) -> AnyView {
    let ctx = ctx.clone();
    let dec = dec.clone();
    let source = ctx.source;
    let speaker_only = dec.when == DecorWhen::Speaker;
    let motion_enabled = dec.opacity > 0.0 && dec.motion.is_some_and(|m| m.amplitude > 0.0);
    dynamic(
        move || {
            let view = ui.display.get();
            // The input box is built only while its skin preview is on.
            let live = match source {
                Source::Display => !view.empty,
                Source::Preview => ui.shader_preview.get(),
                Source::Draft => true,
            };
            motion_enabled
                && live
                && ui.settings.with(|s| s.skin.advanced_effects)
                && !ui.reduced_motion.get()
                && (source == Source::Draft
                    || !speaker_only
                    || !view.speaker.name.trim().is_empty())
        },
        move |animate| {
            let speaker = speaker_only.then(|| has_speaker(ui, &ctx));
            decor_on(
                &ctx.skin,
                &dec,
                speaker,
                dec.image.as_ref().and_then(|path| ctx.url(path)),
                *animate,
                Some((ui, index)),
            )
        },
    )
    .css(nana_ui::runtime::view::css! { position: absolute; inset: 0; pointer-events: none; overflow: visible; })
    .into_any()
}

/// Whether a decor belongs on a box of `source` at all.
fn decor_shown(dec: &DecorSpec, source: Source) -> bool {
    dec.when != DecorWhen::Display || source != Source::Draft
}

/// One decoration, absolutely placed on its parent; `visible` hides it.
pub(super) fn decor<F>(
    skin: &Skin,
    dec: &DecorSpec,
    visible: Option<F>,
    image_url: Option<String>,
) -> AnyView
where
    F: Fn() -> bool + Send + 'static,
{
    decor_on(skin, dec, visible, image_url, false, None)
}

fn decor_on<F>(
    skin: &Skin,
    dec: &DecorSpec,
    visible: Option<F>,
    image_url: Option<String>,
    animate: bool,
    edit: Option<(Stage, usize)>,
) -> AnyView
where
    F: Fn() -> bool + Send + 'static,
{
    let shape = Surface::decor_image(skin, dec, image_url);
    let placement = place(dec.anchor, dec.offset, dec.size);
    let [width, height] = dec.size.map(len);
    let inset = dec.inset;
    let mut el = widget(
        shape
            .stack(
                Stack::row(0.0)
                    .align(AlignSpec::Center)
                    .justify(JustifySpec::Center),
            )
            .with_layout(move |l| {
                overlay(l);
                match inset {
                    Some([t, r, b, left]) => {
                        l.offset_top = Some(LengthSpec::Px(t));
                        l.offset_right = Some(LengthSpec::Px(r));
                        l.offset_bottom = Some(LengthSpec::Px(b));
                        l.offset_left = Some(LengthSpec::Px(left));
                    }
                    None => {
                        l.offset_left = Some(calc(placement.left.percent, placement.left.px));
                        l.offset_top = Some(calc(placement.top.percent, placement.top.px));
                        l.width = Some(width);
                        l.height = Some(height);
                    }
                }
            }),
    );
    let mut motion_ref = None;
    if let Some(visible) = visible {
        el = el.visible(visible);
    }
    if animate && let Some(motion) = dec.motion {
        let node = nana_ui::runtime::view::node_ref();
        let rotate = dec.rotate;
        let opacity = dec.opacity;
        super::motion::periodic(node, motion.period_s.max(0.5), move |t| {
            let (dy, alpha) = decor_motion(&motion, t, false);
            (affine(0.0, dy, rotate, 1.0), opacity * alpha)
        });
        el = el.node_ref(node);
        motion_ref = Some(node);
    }
    if let Some((ui, index)) = edit {
        if let Some(node) = motion_ref {
            super::editor_geometry::track(ui, format!("decor.{index}"), node);
        } else {
            el = el.editor_part(ui, format!("decor.{index}"));
        }
    }
    if dec.text.is_empty() {
        return el.into_any();
    }
    let font = skin.fonts.role(dec.font);
    let mut st = NodeStyle::default();
    {
        let l = Arc::make_mut(&mut st.layout);
        l.font_size = Some(dec.text_size);
        l.font_weight = Some(font.weight);
        l.font_family = font.families.first().cloned();
        l.color = Some(c(skin.color(dec.text_color)));
        l.white_space_nowrap = true;
        l.line_height = Some(nana_ui_core::LineHeightSpec::Absolute(dec.text_size * 1.2));
    }
    el.children(widget(Text::new(dec.text.clone()).decorative().style(st)))
        .into_any()
}

fn decor_views(ui: Stage, ctx: &BoxCtx, layer: DecorLayer) -> Vec<AnyView> {
    ctx.skin
        .dialogue
        .decor
        .iter()
        .enumerate()
        .filter(|(_, d)| d.layer == layer && decor_shown(d, ctx.source))
        .filter(|(_, d)| d.shader.is_none() || layer == DecorLayer::Below)
        .map(|(index, d)| {
            decor_view(
                ui,
                ctx,
                d,
                ctx.skin.decor_indices.get(index).copied().unwrap_or(index),
            )
        })
        .collect()
}

/// The tail's shadow triangles (drawn under the box) and its border and
/// fill triangles (drawn over the box border, so the tail opens into it).
fn tail(ctx: &BoxCtx, surface: &Surface) -> (Vec<AnyView>, Vec<AnyView>) {
    let skin = &ctx.skin;
    let Some(t) = &skin.dialogue.tail else {
        return (Vec::new(), Vec::new());
    };
    let b = &skin.dialogue.box_;
    if surface.integrates_tail() {
        return (Vec::new(), Vec::new());
    }
    let edge = t.side;
    let border = if skin.has_dialogue_artwork() {
        // The baked procedural plate contains its own keyline; a native tail
        // still needs the matching authored stroke outside that canvas.
        b.shape.border_widths(b.border_width)[edge.index()]
    } else {
        b.border_widths()[edge.index()]
    };
    let g = tail_geometry(t, border);
    let fill = c(skin.color(t.color.unwrap_or(ColorRef::Role(ColorRole::Box))));
    let along = t.at * 100.0;
    let [x, y, w, h] = g.rect;
    let node = move |dx: f32, dy: f32, shape: Surface| -> AnyView {
        widget(
            shape
                .stack(
                    Stack::row(0.0)
                        .width(LengthSpec::Px(w))
                        .height(LengthSpec::Px(h)),
                )
                .with_layout(move |l| {
                    overlay(l);
                    let (left, top) = match edge {
                        Edge::Bottom => (calc(along, x + dx), calc(100.0, y + dy)),
                        Edge::Top => (calc(along, x + dx), LengthSpec::Px(y + dy)),
                        Edge::Right => (calc(100.0, x + dx), calc(along, y + dy)),
                        Edge::Left => (LengthSpec::Px(x + dx), calc(along, y + dy)),
                    };
                    l.offset_left = Some(left);
                    l.offset_top = Some(top);
                }),
        )
        .into_any()
    };
    let mut below = Vec::new();
    if t.shadow {
        // Hard outer shadows have a crisp outline the tail can repeat.
        for s in b.shadow_layers() {
            if s.blur == 0.0 && s.spread == 0.0 && !s.inset {
                let color = c(skin.color(s.color));
                below.push(node(s.dx, s.dy, Surface::tail_part(color, &g.outer)));
            }
        }
    }
    let mut above = Vec::new();
    if border > 0.0 {
        above.push(node(
            0.0,
            0.0,
            Surface::tail_part(c(b.border_color), &g.outer),
        ));
    }
    above.push(node(0.0, 0.0, Surface::tail_part(fill, &g.inner)));
    (below, above)
}

fn continue_indicator(ui: Stage, ctx: &BoxCtx) -> impl IntoView + use<> {
    let ctx = ctx.clone();
    let source = ctx.source;
    let shown = ctx.skin.dialogue.continue_.image.is_some()
        || !ctx.skin.dialogue.continue_.glyph.is_empty();
    dynamic(
        move || {
            let view = ui.display.get();
            shown
                && source == Source::Display
                && !view.empty
                && view.continue_indicator
                && !ui.reduced_motion.get()
        },
        move |animate| continue_indicator_on(ui, &ctx, *animate).into_any(),
    )
    .css(nana_ui::runtime::view::css! { position: absolute; inset: 0; pointer-events: none; overflow: visible; })
}

fn continue_indicator_on(ui: Stage, ctx: &BoxCtx, animate: bool) -> impl IntoView + use<> {
    let source = ctx.source;
    let cs = ctx.skin.dialogue.continue_.clone();
    let image = cs.image.as_ref().and_then(|i| ctx.url(i));
    let hidden = image.is_none() && cs.glyph.is_empty();
    let skin = ctx.skin.clone();
    let frames = cs.frames.max(1);
    let fps = cs.fps;
    let child: AnyView = match image {
        Some(u) => {
            let [iw, ih] = cs.image_size.unwrap_or([cs.size * 1.4; 2]);
            let sheet: Vec<BackgroundImage> = (0..frames).map(|i| sprite(&u, frames, i)).collect();
            let first = sheet[0].clone();
            let mut el = widget(
                Stack::row(0.0)
                    .width(LengthSpec::Px(iw))
                    .height(LengthSpec::Px(ih))
                    .with_layout(move |l| l.paint.background_image = Some(first)),
            );
            if frames > 1 && animate {
                let shown = AtomicU32::new(0);
                el = el.bind(move |s: &mut Stack| {
                    let f = sprite_frame(frames, fps, ui.now.get() as f32 / 1000.0);
                    if shown.swap(f, Ordering::Relaxed) != f {
                        let image = sheet[f as usize].clone();
                        restyle(s, |l| l.paint.background_image = Some(image));
                    }
                });
            }
            el.key("dialogue-continue-image")
                .editor_part(ui, "continue")
                .into_any()
        }
        None => {
            let mut st = NodeStyle::default();
            {
                let l = Arc::make_mut(&mut st.layout);
                l.font_size = Some(cs.size);
                l.color = Some(c(cs.color));
                l.white_space_nowrap = true;
            }
            widget(Text::new(cs.glyph.clone()).decorative().style(st))
                .key("dialogue-continue-glyph")
                .editor_part(ui, "continue")
                .into_any()
        }
    };
    let [ox, oy] = cs.offset;
    let (motion, bob, period) = (cs.motion, cs.bob_px, cs.period_s);
    let mut mark =
        widget(Stack::row(0.0).with_layout(move |l| l.transform = Some(affine(ox, oy, 0.0, 1.0))));
    if animate && motion != ContinueMotion::None {
        let node = nana_ui::runtime::view::node_ref();
        super::motion::periodic(node, period, move |t| {
            let (dy, opacity, scale) = continue_motion(motion, bob, period, t as f32);
            (affine(ox, oy + dy, 0.0, scale), opacity)
        });
        mark = mark.node_ref(node);
    }
    widget(anchor_layer(cs.anchor, 3))
        .key("dialogue-continue")
        .visible(move || {
            !hidden
                && match source {
                    Source::Display => {
                        let v = drawn(ui);
                        v.continue_indicator && !v.empty
                    }
                    Source::Draft => false,
                    Source::Preview => true,
                }
                && !super::shader_effects::owns_continue(ui, source, &skin)
        })
        .children(mark.children(child))
}

/// Frame `i` of a horizontal sprite sheet of `frames` frames.
fn sprite(url: &str, frames: u32, i: u32) -> BackgroundImage {
    if frames <= 1 {
        return BackgroundImage::url_with_fit(url.to_owned(), BackgroundImageFit::Contain);
    }
    BackgroundImage::Url {
        url: url.to_owned(),
        fit: BackgroundImageFit::Length,
        size_width: Some(LengthSpec::Percent(frames as f32 * 100.0)),
        size_height: Some(LengthSpec::Percent(100.0)),
        position: BackgroundPosition {
            x: LengthSpec::Percent(i as f32 / (frames - 1) as f32 * 100.0),
            y: LengthSpec::Percent(0.0),
        },
        repeat: BackgroundRepeat::NoRepeat,
        sampling: nana_ui_core::ImageSampling::Resample,
    }
}

/// Whether the display needs frames for the box itself (fade, shake, the
/// continue mark). Text reveal and effects request their own frames.
pub fn display_animating(ui: Stage, now: u64) -> bool {
    if now <= MOVING_UNTIL[ui.track_slot].load(Ordering::Relaxed) + 50 {
        return true;
    }
    let view = ui.display.get_untracked();
    if view.empty {
        return false;
    }
    if let Some(exit) = view.text_exit
        && now < exit.at_ms + u64::from(exit.ms)
    {
        return true;
    }
    if let Some((at, ms)) = view.fade
        && now <= at + u64::from(ms) + 50
    {
        return true;
    }
    if let Some(sh) = view.shake
        && now <= sh.at_ms + u64::from(sh.ms) + 50
    {
        return true;
    }
    let moves = ui.skin.with_untracked(|s| {
        let c = &s.dialogue.continue_;
        c.image.is_some() && c.frames > 1
    });
    view.continue_indicator && moves && !ui.reduced_motion.get_untracked()
}

pub fn has_effects(doc: &Document) -> bool {
    doc.paragraphs.iter().any(|p| {
        p.inlines.iter().any(|i| match i {
            Inline::Text(r) => !r.effects.continuous.is_empty(),
            Inline::Ruby(r) => !r.base.effects.continuous.is_empty(),
            Inline::Atom(a) => !a.effects.continuous.is_empty(),
            Inline::Mention(m) => !m.effects.continuous.is_empty(),
            Inline::Marker(_) => false,
        })
    })
}

#[cfg(test)]
mod decor_tests {
    use super::*;

    #[test]
    fn artwork_nameplates_shrink_the_text_before_the_ornament_strip() {
        use nana_ui::runtime::{DocumentId, RuntimeDocument};
        use nana_ui_devtools::agent::RuntimeAgentSession;

        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        for theme in [
            "gothic",
            "celestial",
            "forest",
            "tidal",
            "lantern",
            "citrus",
        ] {
            let skin = registry.load(&format!("nanatype.{theme}")).unwrap().skin;
            let ui = Stage::new(
                Arc::new(nanatype_settings::Settings::default()),
                skin.clone(),
                nanatype_i18n::Locale::ZhCn,
            );
            for name in ["奈奈", "奈奈与联机伙伴・A Very Long Speaker Name"] {
                let ctx = BoxCtx {
                    skin: skin.clone(),
                    urls: Arc::new(HashMap::new()),
                    source: Source::Preview,
                    preview_name: name.into(),
                };
                let id = DocumentId::new(1).unwrap();
                let mut doc = RuntimeDocument::new(id);
                doc.context_mut()
                    .mount_view_root(id, move || {
                        widget(
                            Stack::column(0.)
                                .width(LengthSpec::Px(528.))
                                .height(LengthSpec::Px(220.))
                                .with_layout(|l| l.position = PositionSpec::Relative),
                        )
                        .children(nameplate(ui, &ctx))
                    })
                    .unwrap();
                let mut session = RuntimeAgentSession::new(doc, 528, 220).unwrap();
                session.screenshot_rgba().unwrap();
                let label = session
                    .accessibility_dump()
                    .into_iter()
                    .find(|n| n.label.as_deref() == Some(name))
                    .expect("nameplate text keeps its accessible name");
                assert!(label.bounds.width > 0., "{theme}: name is visible");
                assert!(
                    label.bounds.x + label.bounds.width <= 528. - skin.dialogue.padding[2] + 2.,
                    "{theme}: name extends into the ornament strip: {:?}",
                    label.bounds
                );
                if name == "奈奈" {
                    assert!(
                        label.bounds.width < skin.dialogue.nameplate.size * 3.,
                        "{theme}: short names keep their natural width"
                    );
                }
            }
        }
    }

    #[test]
    fn responsive_resize_keeps_the_text_node_schedule_and_track_independence() {
        use nana_ui::runtime::{DocumentId, RuntimeDocument};
        use nana_ui_devtools::agent::{AgentSession, RuntimeAgentSession};
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        let mut skin = (*registry.load("nanatype.halo").unwrap().skin).clone();
        skin.dialogue.overflow = Overflow::Scroll;
        let ui = Stage::new(
            Arc::new(nanatype_settings::Settings::default()),
            Arc::new(skin),
            nanatype_i18n::Locale::ZhCn,
        );
        let text = nanatype_markup::parse(&format!(
            "[at=42 name=奈奈] [sticker=nana/heart]{}",
            "中文竖排：你好，世界！Hello 2026。[b]字体[/b]与[ruby=かんじ]漢字[/ruby]。\n"
                .repeat(15)
        ))
        .doc;
        ui.display.update(|v| {
            let v = Arc::make_mut(v);
            v.empty = false;
            v.doc = Arc::new(text);
            v.reveal = Reveal::full(600);
            v.reply = Some(nanatype_model::ChatRef::new(
                nanatype_model::ChatUser {
                    name: "这是一位名字非常长的观众 Hello 2026".into(),
                    ..Default::default()
                },
                "这是一条回复引用：中文标点，Hello 2026！需要跟随正文方向变化。",
                nanatype_model::SourceKind::Chat,
            ));
        });
        let original = ui.display.get_untracked();
        let id = DocumentId::new(1).unwrap();
        let mut doc = RuntimeDocument::new(id);
        doc.context_mut()
            .mount_view_root(id, move || dialogue_box(ui, Source::Display, None))
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, 760, 220).unwrap();
        session.flush().unwrap();
        let node = ui.text_node.get_untracked().unwrap();
        let quote_ids: Vec<_> = session
            .accessibility_dump()
            .into_iter()
            .filter(|n| {
                n.agent_path.as_deref().is_some_and(|p| {
                    p.ends_with("reply-quote-name") || p.ends_with("reply-quote-text")
                })
            })
            .map(|n| n.id)
            .collect();
        assert_eq!(quote_ids.len(), 2, "quote name and text are mounted");
        let presentation = session
            .document()
            .context()
            .world()
            .glyph_presentation(node)
            .cloned();
        // The switch picks the writing mode; the window's shape never does.
        for (w, h, vertical) in [
            (300, 600, true),
            (400, 400, false),
            (400, 401, true),
            (1200, 120, false),
            (300, 1200, false),
            (760, 220, true),
            (760, 220, false),
        ] {
            ui.settings
                .update(|s| Arc::make_mut(s).overlays.display.vertical_text = vertical);
            session.set_viewport(w, h, 1.0).unwrap();
            session.flush().unwrap();
            assert_eq!(ui.text_node.get_untracked(), Some(node));
            let world = session.document().context().world();
            for &id in &quote_ids {
                let quote_node = nana_ui::runtime::StableNodeId::new(id).unwrap();
                let (_, quote_layout) = world.text_layout(quote_node).unwrap();
                assert_eq!(
                    quote_layout.is_vertical(),
                    vertical,
                    "reply quote follows its display's writing mode"
                );
                if vertical {
                    let q = world.layout_box(quote_node).unwrap();
                    let port = world
                        .layout_box(ui.text_port.get_untracked().unwrap())
                        .unwrap();
                    assert!(
                        q.x >= port.x + port.width - 1.,
                        "vertical reply must sit to the right of the body: quote={q:?}, body={port:?}"
                    );
                    assert!(
                        q.y >= port.y - 1. && q.y + q.height <= port.y + port.height + 1.,
                        "reply remains inside the text viewport"
                    );
                }
            }
            assert_eq!(world.glyph_presentation(node), presentation.as_ref());
            let style = &world.node_style(node).unwrap().layout;
            assert_eq!(
                style.writing_mode,
                Some(if vertical {
                    nana_ui_core::WritingModeSpec::VerticalRl
                } else {
                    nana_ui_core::WritingModeSpec::HorizontalTb
                })
            );
            let (_, layout) = world.text_layout(node).unwrap();
            assert!(layout.lines.len() > 1);
            if vertical {
                assert!(
                    world.layout_box(node).unwrap().width + 0.5 >= layout.physical_size().0,
                    "the full column stack must own the inline-object coordinate space: body={:?}, physical={:?}, viewport={w}x{h}",
                    world.layout_box(node),
                    layout.physical_size()
                );
                let port = world
                    .layout_box(ui.text_port.get_untracked().unwrap())
                    .unwrap();
                let body = world.layout_box(node).unwrap();
                assert!(
                    port.width > w as f32 * 0.3,
                    "reply must leave space for the body: {port:?}"
                );
                assert!(
                    (body.height - port.height).abs() < 1.0,
                    "columns must wrap at the actual text viewport height"
                );
                assert!(
                    (body.x + body.width - port.x - port.width).abs() < 1.0,
                    "column stack must anchor right for {w}x{h}: body={body:?}, port={port:?}, physical={:?}",
                    layout.physical_size()
                );
                let writing = nana_ui_core::WritingContext::new(
                    nana_ui_core::WritingModeSpec::VerticalRl,
                    nana_ui_core::DirSpec::Ltr,
                );
                let physical = |i: usize| {
                    let b = layout.lines[i].bounds;
                    writing.line_rect_to_page((b.x, b.y, b.width, b.height), port.width)
                };
                assert!(physical(0).0 > physical(1).0, "columns must advance left");
                assert!(physical(0).3 > physical(0).2);
            }
            assert!(Arc::ptr_eq(&ui.display.get_untracked(), &original));
            assert_eq!(ui.remote().display_size.get_untracked(), [0.; 2]);
            // The host remembers a resize through Settings without remounting the text.
            ui.settings
                .update(|s| Arc::make_mut(s).overlays.display.size = Some([w as f32, h as f32]));
            session.flush().unwrap();
            assert_eq!(ui.text_node.get_untracked(), Some(node));
        }
    }

    #[test]
    fn following_the_typing_scrolls_a_mention_label_fully_out_of_view() {
        use nana_ui::runtime::{DocumentId, RuntimeDocument};
        use nana_ui_devtools::agent::RuntimeAgentSession;
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        let ui = Stage::new(
            Arc::new(nanatype_settings::Settings::default()),
            registry.load("nanatype.bubble").unwrap().skin,
            nanatype_i18n::Locale::ZhCn,
        );
        let text = nanatype_markup::parse("[at=1001 name=星野ほたる] 晚上好！今天也谢谢你来～").doc;
        ui.display.update(|v| {
            let v = Arc::make_mut(v);
            v.empty = false;
            v.reveal = Reveal::full(text.unit_count() as u32);
            v.doc = Arc::new(text);
            v.reply = Some(nanatype_model::ChatRef::new(
                nanatype_model::ChatUser {
                    name: "星野ほたる".into(),
                    ..Default::default()
                },
                "主播晚上好！今天打什么字呀",
                nanatype_model::SourceKind::Chat,
            ));
        });
        let id = DocumentId::new(1).unwrap();
        let mut doc = RuntimeDocument::new(id);
        doc.context_mut()
            .mount_view_root(id, move || dialogue_box(ui, Source::Display, None))
            .unwrap();
        // The 300×240 window: the label's line and two of text overflow it.
        let mut session = RuntimeAgentSession::new(doc, 300, 240).unwrap();
        session.flush().unwrap();
        let world = session.document().context().world();
        let (_, layout) = world
            .text_layout(ui.text_node.get_untracked().unwrap())
            .unwrap();
        let port = world
            .layout_box(ui.text_port.get_untracked().unwrap())
            .unwrap();
        let label = layout.objects.first().expect("the mention is laid out");
        let lines = overflow::line_spans(layout);
        let last = lines.len() - 1;
        let scroll = overflow::scroll_target(&lines, last, port.height);
        assert!(
            scroll > 0.0,
            "the line overflows the box: {lines:?}, {port:?}"
        );
        assert!(
            label.rect.y + label.rect.height <= scroll,
            "the label's bottom edge is still in view: {label:?}, scrolled to {scroll}"
        );
        assert!(lines[last].bottom - scroll <= port.height + 0.5);
    }

    #[test]
    fn a_populated_initial_mount_is_opaque_even_at_clock_zero() {
        let mut stage = Track::new();
        let mut view = DisplayView::empty(Entrance {
            kind: EntranceKind::None,
            ms: 0,
        });
        view.empty = false;
        stage.see(Arc::new(view), 0, 0);
        assert_eq!(stage.presence(0, false), (1.0, 0.0));
        assert_eq!(stage.presence(48, false), (1.0, 0.0));
    }

    #[test]
    fn partner_decoration_demand_does_not_follow_the_local_empty_track() {
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        let ui = Stage::new(
            Arc::new(nanatype_settings::Settings::default()),
            registry.load("nanatype.sakura").unwrap().skin,
            nanatype_i18n::Locale::ZhCn,
        );
        let mut remote = (*ui.peer_display.get_untracked()).clone();
        remote.empty = false;
        ui.peer_display.set(Arc::new(remote));
        assert!(!display_animating(ui, 100_000));
        assert!(
            !display_animating(ui.remote(), 100_000),
            "decor uses the compositor clock"
        );
        ui.reduced_motion.set(true);
        assert!(!display_animating(ui.remote(), 100_000));
    }
}

#[cfg(test)]
mod transition_override_tests {
    use super::*;
    use nana_ui::runtime::{DocumentId, RuntimeDocument};
    #[test]
    fn explicit_entrance_overrides_the_skin_mask_in_history_too() {
        let skin = nanatype_theme::SkinRegistry::new(None, "1.0.0")
            .load("nanatype.crayon")
            .unwrap()
            .skin;
        let mut settings = nanatype_settings::Settings::default();
        settings.playback.entrance = Some(EntranceKind::Pop);
        let ui = Stage::new(
            Arc::new(settings),
            skin.clone(),
            nanatype_i18n::Locale::ZhCn,
        );
        let base = Base::history(
            &skin,
            Entrance {
                kind: EntranceKind::Fade,
                ms: 180,
            },
            false,
        );
        let text = nanatype_markup::parse("测试").doc;
        let id = DocumentId::new(1).unwrap();
        let mut doc = RuntimeDocument::new(id);
        let root = doc
            .context_mut()
            .mount_view_root(id, move || {
                document(ui, &text, &base, Timing::Reveal(Reveal::full(2)))
            })
            .unwrap();
        let p = doc
            .context()
            .world()
            .glyph_presentation(root.roots()[0])
            .unwrap();
        assert!(p.reveal.as_ref().unwrap().intro.pop);
        assert!(p.transitions.entrance.is_none());
    }
}
