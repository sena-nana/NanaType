//! Rich text node construction and text-presentation mode selection for
//! dialogue, history, and preview scenes.

use std::sync::Arc;

use nana_ui::runtime::view::{AnyView, IntoView, widget};
use nana_ui::runtime::{NodeStyle, RichTextView, TextHorizontalAlignment};
use nana_ui_core::{LengthSpec, PointerEventsSpec, PositionSpec};
use nanatype_core::Reveal;
use nanatype_model::{Document, Entrance, EntranceKind, Inline, Rgba8};
use nanatype_theme::{
    EffectDefaults, Skin, TextAlign, TextShadowSpec as SkinShadow, TextStrokeSpec as SkinStroke,
};

use super::super::overflow;
use super::super::rich::{Env, Look, RevealPlan, to_rich_with_palette};
use super::super::surface::{self};
use super::super::{Stage, c};
use super::chrome::{Source, vertical_text};
use super::track::drawn_untracked_for;

/// Base text look a document is drawn with (skin dialogue text or history text).
#[derive(Clone, PartialEq)]
pub struct Base {
    pub responsive_text: bool,
    pub material: Option<nana_ui_core::MaterialRef>,
    pub color: Rgba8,
    pub size: f32,
    pub line_height: f32,
    /// Stickers' height in line heights (the user's sticker size).
    pub sticker_scale: f32,
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
    pub layout_material: Option<super::super::pipeline::LayoutMaterial>,
    pub light_exit: Option<(u64, u16)>,
    pub light_preview: bool,
    pub align: TextAlign,
    pub mention: super::super::rich::MentionLook,
    /// The box under the text, for the display's contrast guard and
    /// skin-aware glows; `None` in the editor.
    pub ink: Option<super::super::rich::BoxInk>,
}

pub(super) fn skin_mask(
    skin: &Skin,
    name: Option<&str>,
    scope: nanatype_theme::TransitionScope,
    ms: u64,
) -> Option<nana_ui_core::PresentationMask> {
    Some(nana_ui_core::PresentationMask {
        material: super::super::materials::resolve(skin, name)?,
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
            material: super::super::materials::resolve(skin, t.material.as_deref()),
            color: t.color,
            size: t.size,
            line_height: t.line_height,
            sticker_scale: 1.0,
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
            mention: super::super::rich::MentionLook::from_skin(skin),
            ink: super::super::rich::BoxInk::new(skin, fill, t),
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
        base.layout_material = super::super::pipeline::LayoutMaterial::entrance(skin);
        base
    }

    /// The input editor's text: the dialogue text, nothing animating in.
    pub fn editor(skin: &Skin) -> Self {
        let entrance = Entrance {
            kind: EntranceKind::None,
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
            sticker_scale: self.sticker_scale,
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
        l.text_ext.font_family = self.family.clone();
        l.text_ext.letter_spacing = Some(self.letter_spacing);
        l.white_space = nana_ui_core::WhiteSpaceSpec::PreWrap;
        l.text_ext.overflow_wrap = Some(nana_ui_core::OverflowWrapSpec::Anywhere);
        l.min_width = Some(LengthSpec::Px(0.0));
        l.paint_colors.color = Some(nana_ui_core::PaintColor::srgb(c(self.color)));
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
        l.text_ext.text_typography.wrap_style = Some(nana_ui_core::TextWrapStyleSpec::Pretty);
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

/// Mutually exclusive presentation strategy for a rendered text node.
enum TextPresentationMode {
    Plain,
    LayoutMaterial(super::super::pipeline::LayoutMaterial),
    TextLight {
        spec: nanatype_theme::TextLightSpec,
        times: Vec<Option<u64>>,
        text: String,
    },
    TextScramble {
        spec: nanatype_theme::ScrambleSpec,
        times: Vec<Option<u64>>,
        text: String,
    },
}

/// Resolve effective entrance and whether an explicit tag/setting overrides the
/// skin's own entrance mask/scramble. Shared with `history` so frame demand
/// uses the exact same entrance duration rules.
pub fn resolve_entrance(
    doc: &Document,
    setting: Option<EntranceKind>,
    base_entrance: Entrance,
) -> (Entrance, bool) {
    let tagged = doc.entrance();
    let entrance = match (tagged, setting) {
        (Some(tagged), _) => Entrance {
            kind: tagged.kind,
            ms: if base_entrance.ms == 0 { 0 } else { tagged.ms },
        },
        (None, Some(kind)) => Entrance {
            kind,
            ms: if kind == EntranceKind::None {
                0
            } else {
                base_entrance.ms
            },
        },
        (None, None) => base_entrance,
    };
    (entrance, tagged.is_some() || setting.is_some())
}

/// `doc` as one rich text node in `window`, with its reveal and effects.
pub fn document(ui: Stage, doc: &Document, base: &Base, timing: Timing) -> AnyView {
    document_on(ui, doc, base, timing, None, Source::Display)
}

fn select_presentation_mode(
    ui: Stage,
    base: &Base,
    entrance: Entrance,
    overridden: bool,
    materials_enabled: bool,
    reveal: Option<&RevealPlan>,
    text: &str,
) -> TextPresentationMode {
    if base.reduced_motion || !materials_enabled {
        return TextPresentationMode::Plain;
    }
    if !overridden
        && reveal.is_some()
        && entrance.kind != EntranceKind::None
        && entrance.ms > 0
        && ui.settings.with_untracked(|s| s.playback.typing)
        && let Some(material) = &base.layout_material
    {
        return TextPresentationMode::LayoutMaterial(material.clone());
    }
    if (!overridden || base.light_exit.is_some())
        && let Some(spec) = &base.scramble
        && let Some(plan) = reveal
    {
        return TextPresentationMode::TextScramble {
            spec: spec.clone(),
            times: plan.at_ms.clone(),
            text: text.to_owned(),
        };
    }
    if let Some(spec) = &base.light
        && spec.intensity > 0.0
        && spec.color.a > 0
        && let Some(plan) = reveal
    {
        return TextPresentationMode::TextLight {
            spec: spec.clone(),
            times: plan.at_ms.clone(),
            text: text.to_owned(),
        };
    }
    TextPresentationMode::Plain
}

/// [`document`] for the typewriter's page `follow`: the node is recorded in
/// `ui.text_node` and its reveal times published for the host to follow.
pub(super) fn document_on(
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
    let (rich, palette) = to_rich_with_palette(doc, &env);
    let setting = ui.settings.with_untracked(|s| s.playback.entrance);
    let materials_enabled = super::super::materials::enabled(ui);
    let (mut entrance, overridden) = resolve_entrance(doc, setting, base.entrance);
    if base.scramble.is_some() && (base.reduced_motion || !materials_enabled) {
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
        for ((_, unit), at) in super::super::rich::unit_graphemes(rich.text()).zip(&mut plan.at_ms)
        {
            // Zero is the native renderer's already-shown sentinel.
            if unit.is_some_and(|u| u >= r.shown) && *at == Some(0) {
                *at = Some(1);
            }
        }
    }
    let mode = select_presentation_mode(
        ui,
        base,
        entrance,
        overridden,
        materials_enabled,
        reveal.as_ref(),
        rich.text(),
    );
    if let (Some(key), Some(plan)) = (follow, &reveal) {
        overflow::publish(overflow::PageText {
            key,
            text_len: rich.text().len(),
            text_trimmed: rich.text().trim_end().len(),
            glyphs: overflow::glyphs(rich.text(), &plan.at_ms),
        });
    }
    let mut text_style = base.node_style();
    if !materials_enabled {
        Arc::make_mut(&mut text_style.layout).paint.text_material = None;
    }
    let mut view = RichTextView::new(rich)
        .decorative()
        .style(text_style)
        .width(LengthSpec::Fill);
    if reveal.is_some() || has_effects(doc) {
        // The framework stamps `reveal.start` with the compositor clock taken
        // before this flush. Offsets use this frame's anchor; an exit keeps
        // the glyphs that are already on screen.
        let schedule = reveal.map(|plan| {
            plan.schedule(
                std::time::Duration::ZERO,
                if base.masks.exit.is_some() || base.light_exit.is_some() {
                    drawn_untracked_for(ui, source)
                        .exit
                        .and_then(|e| e.presentation())
                        .map_or_else(crate::clock::frame_ms, |(at_ms, _)| at_ms)
                } else {
                    crate::clock::frame_ms()
                },
                base.reduced_motion,
            )
        });
        view =
            view.presentation_chains(palette.chains(&base.effects, base.reduced_motion), schedule);
        if materials_enabled
            && !base.reduced_motion
            && let Some(p) = &mut view.presentation
        {
            p.transitions = match &mode {
                TextPresentationMode::TextScramble { spec, .. } => {
                    super::super::text_scramble::transitions(
                        spec,
                        base.light_exit,
                        base.light_preview,
                    )
                }
                _ => base.masks.clone(),
            };
            if overridden {
                p.transitions.entrance = None;
            }
        }
        if let Some(t) = view
            .presentation
            .as_ref()
            .and_then(|p| super::super::rich::glyph_resampling(doc, p))
        {
            Arc::make_mut(&mut view.style.layout).transforms.transform = Some(t);
        }
    }
    let doc = doc.clone();
    let look = look.clone();
    let responsive = base.responsive_text;
    let update_view = move |v: &mut RichTextView| {
        if responsive {
            let vertical = vertical_text(ui);
            let l = Arc::make_mut(&mut v.style.layout);
            l.writing_mode = Some(if vertical {
                nana_ui_core::WritingModeSpec::VerticalRl
            } else {
                nana_ui_core::WritingModeSpec::HorizontalTb
            });
            l.text_ext.text_orientation = Some(nana_ui_core::TextOrientationSpec::Mixed);
            l.height = vertical.then(|| LengthSpec::Px(ui.text_area_height.get().max(1.)));
            l.width = Some(LengthSpec::Fill);
            l.flex_shrink = Some(0.0);
        }
        let urls = ui.asset_urls.get();
        let (mut rich, _) = to_rich_with_palette(
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
            super::super::rich::vertical_objects(&mut rich, &look, h * 0.5);
        }
        if v.value != rich {
            v.value = rich;
        }
        if responsive {
            let vertical = vertical_text(ui);
            let width = vertical.then(|| {
                super::super::rich::vertical_extent(
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
    match mode {
        TextPresentationMode::LayoutMaterial(material) => super::super::pipeline::view(
            view,
            material,
            ui.layout_epoch,
            update_view,
            follow.map(|_| ui.text_node),
        ),
        TextPresentationMode::TextScramble { spec, times, text } => {
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
                        p.transitions.exit = Some(super::super::text_scramble::object_exit(
                            exit,
                            base.light_preview,
                        ));
                    }
                    let update = update.clone();
                    widget(object_view)
                        .bind(move |v: &mut RichTextView| {
                            update(v);
                            super::super::text_scramble::objects_only(&mut v.value);
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
            super::super::text_scramble::wrap(
                ui,
                view.node_ref(node).into_any(),
                objects,
                node,
                spec,
                base.color,
                super::super::text_light::TextSchedule {
                    at_ms: &times,
                    value: &text,
                    exit: base.light_exit,
                    preview: base.light_preview,
                },
            )
        }
        TextPresentationMode::TextLight { spec, times, text } => {
            let view = widget(view).bind(update_view);
            let node = if follow.is_some() {
                ui.text_node
            } else {
                nana_ui::runtime::view::node_ref()
            };
            super::super::text_light::wrap(
                ui,
                view.node_ref(node).into_any(),
                node,
                spec,
                super::super::text_light::TextSchedule {
                    at_ms: &times,
                    value: &text,
                    exit: base.light_exit,
                    preview: base.light_preview,
                },
                entrance.ms,
            )
        }
        TextPresentationMode::Plain => {
            let view = widget(view).bind(update_view);
            match follow {
                Some(_) => view.node_ref(ui.text_node).into_any(),
                None => view.into_any(),
            }
        }
    }
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
