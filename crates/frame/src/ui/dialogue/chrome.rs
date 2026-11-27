//! Dialogue box chrome: box surface, tail, ornaments, nameplate, portrait,
//! reply quote, continue indicator, and shader layers.
//!
//! Uses [`crate::skin_theme::resolve_dialogue`] (built on Wave 1's
//! `Skin::resolve_dialogue`) so responsive layouts never clone the full `Skin`.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use nana_ui::runtime::view::{
    AnyView, Computed, IntoView, computed, dynamic, watch_effect, widget,
};
use nana_ui::runtime::{NodeStyle, RichTextView, SizeChanged, Stack, Text};
use nana_ui_core::{
    AlignSpec, BackgroundImage, BackgroundImageFit, BackgroundPosition, BackgroundRepeat,
    JustifySpec, LayoutStyle, LengthSpec, OverflowSpec, PointerEventsSpec, PositionSpec,
    RichSpanStyle, RichText,
};
use nanatype_core::{DisplayView, Reveal};
use nanatype_model::{Document, Entrance, EntranceKind, Overflow, Rgba8};
use nanatype_theme::{
    Anchor, ColorRef, ColorRole, ContinueMotion, DecorLayer, DecorSpec, DecorWhen, Dialogue, Edge,
    NameplateSpec, NameplateTypographySpec, ResolvedDialogue, ShapeSpec, Side, Skin, anchor_frac,
    continue_motion, decor_motion, place, sprite_frame, tail_geometry,
};
use unicode_segmentation::UnicodeSegmentation;

use super::super::editor_geometry::Capture;
use super::super::overflow;
use super::super::surface::{self, Border, Surface, affine, calc, len};
use super::super::{I18N, Stage, c, restyle};
use super::text_node::{Base, Timing, document_on, skin_mask};
use super::track::{box_presence, drawn, drawn_for, drawn_untracked_for, fresh_preview_line};

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
    fn active(ui: Stage, source: Source) -> Self {
        Self {
            skin: ui.skin.get_untracked(),
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

    fn box_surface_for(&self, resolved: &ResolvedDialogue, materials_enabled: bool) -> Surface {
        let frame = resolved.dialogue.box_.frame.as_ref();
        box_surface_for(
            &self.skin,
            resolved,
            frame.and_then(|f| self.url(&f.image)),
            materials_enabled,
        )
    }
}

fn shaped_surface(
    skin: &Skin,
    fill: Option<ColorRef>,
    shape: &ShapeSpec,
    radius: f32,
    border: Option<ColorRef>,
    border_width: f32,
    materials_enabled: bool,
) -> Surface {
    let widths = shape.border_widths(border_width);
    Surface {
        material: materials_enabled
            .then(|| super::super::materials::resolve(skin, shape.material.as_deref()))
            .flatten(),
        border_material: materials_enabled
            .then(|| super::super::materials::resolve(skin, shape.border_material.as_deref()))
            .flatten(),
        contour: shape.contour,
        fill: fill.map(|f| c(skin.color(f))),
        gradient: shape.gradient.as_ref().map(|g| surface::gradient(skin, g)),
        radii: shape.radii(radius),
        border: border
            .filter(|_| widths.iter().any(|w| *w > 0.0))
            .map(|color| Border {
                widths,
                color: c(skin.color(color)),
                style: shape.border_style,
            }),
        shadows: shape
            .shadows
            .iter()
            .map(|s| surface::shadow(skin, s))
            .collect(),
        ..Surface::default()
    }
}

pub(super) fn box_surface_for(
    skin: &Skin,
    resolved: &ResolvedDialogue,
    frame_url: Option<String>,
    materials_enabled: bool,
) -> Surface {
    let d = &resolved.dialogue;
    let b = &d.box_;
    if b.shape.material.is_some() {
        let mut surface = shaped_surface(
            skin,
            Some(b.color.into()),
            &b.shape,
            b.radius,
            Some(b.border_color.into()),
            b.border_width,
            materials_enabled,
        );
        if b.frame.as_ref().is_some_and(|f| !f.fill) {
            surface.frame = b
                .frame
                .as_ref()
                .zip(frame_url)
                .map(|(frame, url)| surface::nine_slice(frame, url));
        } else {
            surface.gradient = frame_url
                .map(|url| BackgroundImage::url_with_fit(url, BackgroundImageFit::Stretch));
        }
        return surface;
    }
    if let (Some(frame), Some(url)) = (b.frame.as_ref(), frame_url) {
        let mut s = Surface {
            frame: Some(surface::nine_slice(frame, url)),
            ..Surface::default()
        };
        if s.frame.as_ref().is_some_and(|(frame, _)| !frame.fill) {
            s.fill = Some(c(b.color));
        }
        if resolved.has_dialogue_artwork() {
            if let Some((_, widths)) = &mut s.frame {
                *widths = [0.; 4];
            }
            s.shadows = b
                .shadow_layers()
                .iter()
                .map(|sh| surface::shadow(skin, sh))
                .filter(|sh| !sh.inset)
                .collect();
            if !s.shadows.is_empty() {
                s.radii = b.shape.radii(b.radius);
            }
        }
        return s;
    }
    Surface {
        tail: d.tail.clone(),
        shadows: b
            .shadow_layers()
            .iter()
            .map(|sh| surface::shadow(skin, sh))
            .collect(),
        ..shaped_surface(
            skin,
            Some(b.color.into()),
            &b.shape,
            b.radius,
            Some(b.border_color.into()),
            b.border_width,
            materials_enabled,
        )
    }
}

fn nameplate_surface_for(
    skin: &Skin,
    p: &NameplateSpec,
    frame_url: Option<String>,
    materials_enabled: bool,
) -> Surface {
    let s = if let (Some(frame), Some(url)) = (p.frame.as_ref(), frame_url) {
        Surface {
            frame: Some(surface::nine_slice(frame, url)),
            ..Surface::default()
        }
    } else {
        shaped_surface(
            skin,
            Some(p.color.into()),
            &p.shape,
            p.radius,
            p.border_color,
            p.border_width,
            materials_enabled,
        )
    };
    Surface {
        rotate_deg: p.rotate,
        ..s
    }
}

/// Whether this display window writes its dialogue top to bottom; the
/// skin's portrait layout follows the window's shape on its own.
pub(crate) fn vertical_text(ui: Stage) -> bool {
    ui.skin
        .with(|skin| ui.settings.with(|s| ui.canvas().vertical_text(s, skin)))
}

pub(crate) fn canvas_size(ui: Stage, skin: &Skin) -> [f32; 2] {
    let size = ui.display_size.get();
    if size[0] > 0.0 && size[1] > 0.0 {
        return size;
    }
    let (w, h) = crate::canvas::size_of(ui.canvas(), skin, &ui.settings.get());
    [w, h]
}

fn resolve_ctx_dialogue(ui: Stage, ctx: &BoxCtx) -> ResolvedDialogue {
    if ctx.source == Source::Draft {
        return ResolvedDialogue {
            dialogue: ctx.skin.dialogue.clone(),
            decor_indices: (0..ctx.skin.dialogue.decor.len()).collect(),
            portrait: false,
        };
    }
    let [w, h] = canvas_size(ui, &ctx.skin);
    ui.settings
        .with(|s| crate::skin_theme::resolve_dialogue(&ctx.skin, (w, h), &s.skin.overrides))
}

/// A complete dialogue box filling its parent (region + overscan). In the
/// input window `editor` takes the text's place; it is built once so it
/// keeps its selection and undo history.
pub fn dialogue_box(ui: Stage, source: Source, editor: Option<AnyView>) -> AnyView {
    if let Some(editor) = editor {
        return box_view(ui, BoxCtx::active(ui, source), editor).into_any();
    }
    if source == Source::Display {
        ui.track.reset(ui.display.get_untracked());
        let mut mounted_initial = true;
        watch_effect(move || {
            let view = ui.display.get();
            if mounted_initial {
                mounted_initial = false;
                return;
            }
            ui.track.see(view, crate::clock::frame_ms());
        });
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

/// How long this skin's text takes to leave. Scramble retires itself; an
/// exit material does too. Anything else just clears.
fn text_exit_ms(skin: &Skin, materials_enabled: bool) -> u16 {
    if !materials_enabled {
        return 0;
    }
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
    let skin = ctx.skin.clone();
    ui.skin.set(skin.clone());
    ui.preview_name.set(ctx.preview_name.clone());

    let now = crate::clock::now_ms();
    let units = doc.unit_count() as u32;
    let materials_enabled = super::super::materials::enabled(ui);
    let animate_text = ui.shader_preview.get_untracked()
        && !ui.reduced_motion.get_untracked()
        && materials_enabled
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
            unit_times: None,
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
    ui.preview_text_exit_ms
        .set(text_exit_ms(&skin, materials_enabled));
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
    let source = ctx.source;
    let overscan = ctx.skin.dialogue.overscan;
    let resolved_ctx = ctx.clone();
    let resolved: Computed<Arc<ResolvedDialogue>> =
        computed(move || Arc::new(resolve_ctx_dialogue(ui, &resolved_ctx)));
    let plain =
        move || source == Source::Draft && !ui.settings.with(|s| s.general.input_skin_preview);

    let outer_layout = move |l: &mut LayoutStyle| {
        let (overscan, margin) = if source == Source::Draft {
            ui.skin
                .with(|s| (s.dialogue.overscan, s.dialogue.box_.margin))
        } else {
            (overscan, resolved.with(|r| r.dialogue.box_.margin))
        };
        let [top, right, bottom, left] =
            margin.map(|m| LengthSpec::Px(if plain() { 16.0 } else { overscan + m }));
        l.min_width = Some(LengthSpec::Px(0.0));
        l.min_height = Some(LengthSpec::Px(0.0));
        let cap = |v| match v {
            LengthSpec::Px(px) if source != Source::Draft => LengthSpec::min2(
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
        widget(Stack::fill_column(0.0).with_layout(outer_layout)).on(move |e: &SizeChanged| {
            if source != Source::Draft {
                ui.display_size.set_if_changed([e.width, e.height]);
            }
        });
    outer = outer.bind(move |s: &mut Stack| restyle(s, outer_layout));

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
            resolved.with(|r| r.dialogue.box_.width_fraction)
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
    let mut frame = widget(Stack::fill_column(0.0).with_layout(frame_layout));
    frame = frame.bind(move |s: &mut Stack| restyle(s, frame_layout));
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
                l.transforms.transform =
                    (dx != 0.0 || dy != 0.0).then_some(affine(dx, dy, 0.0, 1.0));
            });
        });
    }

    let shaders = responsive_shaders(ui, &ctx, resolved, DecorLayer::Above);
    let shaders_below = responsive_shaders(ui, &ctx, resolved, DecorLayer::Below);
    let box_ctx = ctx.clone();
    let box_stack = move || {
        let materials_enabled = super::super::materials::enabled(ui);
        let (surface, padding) = if plain() {
            (Surface::default(), [0.0; 4])
        } else if source == Source::Draft {
            let skin = ui.skin.get();
            let padding = skin.dialogue.padding;
            let draft_ctx = BoxCtx {
                skin: skin.clone(),
                urls: ui.asset_urls.get(),
                source,
                preview_name: String::new(),
            };
            let draft_resolved = ResolvedDialogue {
                dialogue: skin.dialogue.clone(),
                decor_indices: (0..skin.dialogue.decor.len()).collect(),
                portrait: false,
            };
            (
                draft_ctx.box_surface_for(&draft_resolved, materials_enabled),
                padding,
            )
        } else {
            let r = resolved.get();
            (
                box_ctx.box_surface_for(&r, materials_enabled),
                r.dialogue.padding,
            )
        };
        surface
            .materials(materials_enabled)
            .stack(Stack::fill_column(0.0))
            .with_layout(move |l| {
                l.min_width = Some(LengthSpec::Px(0.0));
                l.min_height = Some(LengthSpec::Px(0.0));
                l.position = PositionSpec::Relative;
                let horizontal = |px| {
                    if source == Source::Draft {
                        LengthSpec::min2(
                            nana_ui_core::LengthAtom::Px(px),
                            nana_ui_core::LengthAtom::Percent(30.0),
                        )
                    } else {
                        LengthSpec::Px(px)
                    }
                };
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
    let text_skin = ctx.skin.clone();
    let text = if source == Source::Draft {
        text
    } else {
        widget(Stack::fill_column(0.))
            .bind(move |s: &mut Stack| {
                let padding = resolved.with(|r| r.dialogue.padding);
                let vertical = ui
                    .settings
                    .with(|s| ui.canvas().vertical_text(s, &text_skin));
                restyle(s, |l| {
                    l.direction = Some(if vertical {
                        nana_ui_core::FlexDirection::Row
                    } else {
                        nana_ui_core::FlexDirection::Column
                    });
                    l.flex_reverse = vertical;
                    let cap = |px, percent| {
                        LengthSpec::min2(
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
                        LengthSpec::min2(
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
            .children((
                column_nameplate(ui, &ctx, resolved),
                (source == Source::Display).then(|| reply_quote(ui)),
                text,
            ))
            .into_any()
    };
    let the_box = the_box.children((
        input_skin_layer(ui, &ctx, resolved, sweep_layer),
        shaders,
        text,
    ));

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
            input_skin_layer(ui, &ctx, resolved, |ui, ctx, resolved| {
                skin_surroundings(ui, ctx, resolved, DecorLayer::Below)
            }),
            shaders_below,
            the_box,
            input_skin_layer(ui, &ctx, resolved, |ui, ctx, resolved| {
                skin_surroundings(ui, ctx, resolved, DecorLayer::Above)
            }),
        )),
    ))
}

/// A dashed outline of the window, shown as its content (`presence`, 0..1)
/// fades out, so an unlocked overlay can be found and moved while empty.
pub(crate) fn empty_placeholder(
    ui: Stage,
    key: &'static str,
    hint: &'static str,
    presence: impl Fn() -> f32 + Send + Sync + 'static,
) -> AnyView {
    let mut st = NodeStyle::default();
    {
        let l = Arc::make_mut(&mut st.layout);
        l.font_size = Some(14.0);
        l.paint_colors.color = Some(nana_ui_core::PaintColor::srgb(c(Rgba8::rgba(
            255, 255, 255, 220,
        ))));
        l.white_space = nana_ui_core::WhiteSpaceSpec::Nowrap;
    }
    widget(Stack::column(0.).with_layout(|l| {
        overlay(l);
        l.paint_colors.background = Some(nana_ui_core::PaintColor::srgb(c(Rgba8::rgba(
            24, 24, 28, 140,
        ))));
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
fn shader_layers(
    ui: Stage,
    ctx: &BoxCtx,
    resolved: &ResolvedDialogue,
    surface: &Surface,
    layer: DecorLayer,
) -> Vec<AnyView> {
    let mut layers = Vec::new();
    let d = &resolved.dialogue;
    let materials_enabled = super::super::materials::enabled(ui);
    for effect in d
        .shader_effects
        .iter()
        .filter(|_| layer == DecorLayer::Above)
    {
        if let Some(v) = super::super::shader_effects::view(
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
    for dec in &d.decor {
        if dec.layer != layer {
            continue;
        }
        let Some(id) = &dec.shader else {
            continue;
        };
        if !decor_shown(&ctx.skin, dec, ctx.source) {
            continue;
        }
        let shape =
            Surface::decor_image(&ctx.skin, dec, dec.image.as_ref().and_then(|p| ctx.url(p)))
                .materials(materials_enabled);
        let Some(v) = super::super::shader_effects::view(
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

fn responsive_shaders(
    ui: Stage,
    ctx: &BoxCtx,
    resolved: Computed<Arc<ResolvedDialogue>>,
    layer: DecorLayer,
) -> AnyView {
    if ctx.source == Source::Draft {
        return input_skin_layer(
            ui,
            ctx,
            resolved,
            match layer {
                DecorLayer::Above => {
                    |ui, ctx, resolved| shader_art(ui, ctx, resolved, DecorLayer::Above)
                }
                DecorLayer::Below => {
                    |ui, ctx, resolved| shader_art(ui, ctx, resolved, DecorLayer::Below)
                }
            },
        );
    }
    let ctx = ctx.clone();
    dynamic(
        move || ui.display_size.get(),
        move |_| shader_art(ui, &ctx, &nana_ui::runtime::view::untrack(|| resolved.get()), layer),
    )
    .css(nana_ui::runtime::view::css! { position: absolute; inset: 0; pointer-events: none; overflow: visible; })
    .into_any()
}

/// The skin's GPU programs on one layer, with the ornaments they paint.
fn shader_art(ui: Stage, ctx: &BoxCtx, resolved: &ResolvedDialogue, layer: DecorLayer) -> AnyView {
    let materials_enabled = super::super::materials::enabled(ui);
    let surface = ctx.box_surface_for(resolved, materials_enabled);
    let layers = shader_layers(ui, ctx, resolved, &surface, layer);
    let art: Vec<_> = resolved
        .dialogue
        .decor
        .iter()
        .enumerate()
        .filter(|(_, d)| {
            d.layer == layer && d.shader.is_some() && decor_shown(&ctx.skin, d, ctx.source)
        })
        .map(|(index, d)| {
            decor_view(
                ui,
                ctx,
                d,
                resolved.decor_indices.get(index).copied().unwrap_or(index),
            )
        })
        .collect();
    (art, layers).into_any()
}

/// Rebuild only skin layers when the draft's skin or resolved images change.
/// Both editors stay outside these scopes, retaining selection and undo history.
fn input_skin_layer(
    ui: Stage,
    ctx: &BoxCtx,
    resolved: Computed<Arc<ResolvedDialogue>>,
    build: fn(Stage, &BoxCtx, &ResolvedDialogue) -> AnyView,
) -> AnyView {
    if ctx.source != Source::Draft {
        let ctx = ctx.clone();
        let skin = ctx.skin.clone();
        let key = move || {
            let vertical = ui.settings.with(|s| ui.canvas().vertical_text(s, &skin));
            (ui.display_size.get(), vertical)
        };
        return dynamic(key, move |_| build(ui, &ctx, &nana_ui::runtime::view::untrack(|| resolved.get())))
            .css(nana_ui::runtime::view::css! { position: absolute; inset: 0; pointer-events: none; overflow: visible; })
            .into_any();
    }
    dynamic(
        move || {
            (
                ui.skin.get(),
                ui.asset_urls.get(),
                ui.settings.with(|s| s.general.input_skin_preview),
            )
        },
        move |(skin, urls, preview)| {
            preview.then(|| {
                let draft_resolved = ResolvedDialogue {
                    dialogue: skin.dialogue.clone(),
                    decor_indices: (0..skin.dialogue.decor.len()).collect(),
                    portrait: false,
                };
                build(
                    ui,
                    &BoxCtx {
                        skin: skin.clone(),
                        urls: urls.clone(),
                        source: Source::Draft,
                        preview_name: String::new(),
                    },
                    &draft_resolved,
                )
            })
        },
    )
    .css(nana_ui::runtime::view::css! { position: absolute; inset: 0; pointer-events: none; overflow: visible; })
    .into_any()
}

fn skin_surroundings(
    ui: Stage,
    ctx: &BoxCtx,
    resolved: &ResolvedDialogue,
    layer: DecorLayer,
) -> AnyView {
    let materials_enabled = super::super::materials::enabled(ui);
    let surface = ctx.box_surface_for(resolved, materials_enabled);
    let (tail_below, tail_above) = tail(&ctx.skin, resolved, &surface);
    let decor = decor_views(ui, ctx, resolved, layer);
    match layer {
        DecorLayer::Below => (decor, tail_below).into_any(),
        DecorLayer::Above => (
            tail_above,
            decor,
            portrait(ui, ctx, &resolved.dialogue),
            nameplate(ui, ctx, resolved),
            continue_indicator(ui, ctx, &resolved.dialogue),
        )
            .into_any(),
    }
}

fn sweep_layer(ui: Stage, ctx: &BoxCtx, resolved: &ResolvedDialogue) -> AnyView {
    let source = ctx.source;
    let materials_enabled = super::super::materials::enabled(ui);
    let sweep = super::super::sweep::effective(&resolved.dialogue.sweep).then(|| {
        let surface = ctx.box_surface_for(resolved, materials_enabled);
        let spec = resolved.dialogue.sweep.clone();
        dynamic(
            move || {
                !ui.reduced_motion.get()
                    && ui.settings.with(|s| s.skin.motion)
                    && match source {
                        Source::Display => !drawn(ui).empty,
                        Source::Draft => true,
                        Source::Preview => false,
                    }
            },
            move |animated| super::super::sweep::view(surface.clone(), spec.clone(), *animated),
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
            let view = drawn_untracked_for(ui, Source::Display);
            let shown = !view.empty
                && skin.dialogue.reply.visible
                && ui
                    .settings
                    .with_untracked(|s| s.integrations.chat.quote_on_display);
            match view.reply.as_ref().filter(|_| shown) {
                Some(reply) => super::super::quote::display_quote_view(
                    ui,
                    &skin,
                    &skin.dialogue.reply,
                    reply,
                    &skin.dialogue.text,
                    super::super::quote::backdrop(
                        skin.dialogue.box_.color,
                        skin.dialogue.box_.frame.is_some(),
                    ),
                ),
                None => widget(Stack::column(0.0)).into_any(),
            }
        },
    );
    widget(Stack::column(0.))
        .bind(move |s: &mut Stack| {
            let skin = ui.skin.get();
            let view = drawn(ui);
            let shown = !view.empty
                && view.reply.is_some()
                && skin.dialogue.reply.visible
                && ui.settings.with(|s| s.integrations.chat.quote_on_display);
            let vertical = ui.settings.with(|s| ui.canvas().vertical_text(s, &skin));
            restyle(s, |l| {
                l.display = Some(if shown {
                    nana_ui_core::DisplaySpec::Flex
                } else {
                    nana_ui_core::DisplaySpec::None
                });
                l.width = Some(if vertical {
                    LengthSpec::Px(super::super::quote::column_width(
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
            l.transforms.transform = (y != 0.0).then(|| {
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
            base.sticker_scale = ui.settings.with_untracked(|s| s.assets.sticker_scale);
            base.light_preview = source == Source::Preview;
            if let Some((at_ms, ms)) = view.exit.and_then(|e| e.presentation()) {
                base.light_exit = Some((at_ms, ms));
                if base.layout_material.is_none() {
                    base.masks.entrance = None;
                }
                let elapsed = crate::clock::now_ms().saturating_sub(at_ms);
                if super::super::materials::enabled(ui)
                    && let Some(spec) = &skin.dialogue.reveal.exit
                {
                    base.masks.exit = skin_mask(
                        &skin,
                        spec.material.as_deref(),
                        spec.scope,
                        u64::from(ms).saturating_sub(elapsed),
                    );
                }
                base.masks.freeze_reveal = Some(std::time::Duration::ZERO);
            }
            let (doc, reveal) = view.page();
            let follow =
                (skin.dialogue.overflow != Overflow::Clip).then(|| overflow::PageKey::of(&view));
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

/// The speaker's own name colour for the box's source, tracked.
fn speaker_color(ui: Stage, ctx: &BoxCtx) -> impl Fn() -> Option<Rgba8> + Send + 'static {
    let source = ctx.source;
    move || match source {
        Source::Display => drawn(ui).speaker.color,
        Source::Draft => ui.speaker.with(|s| s.color),
        Source::Preview => None,
    }
}

/// `st` with the name in `color`, when the speaker has one.
fn tinted(st: &NodeStyle, color: Option<Rgba8>) -> NodeStyle {
    let mut st = st.clone();
    if let Some(color) = color {
        Arc::make_mut(&mut st.layout).paint_colors.color =
            Some(nana_ui_core::PaintColor::srgb(c(color)));
    }
    st
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

fn plate_in_column(p: &NameplateSpec, vertical: bool) -> bool {
    vertical && anchor_frac(p.anchor)[0] < 1.0
}

fn plate_label_style(ui: Stage, skin: &Skin, d: &Dialogue) -> NodeStyle {
    let p = &d.nameplate;
    let mut st = NodeStyle::default();
    let l = Arc::make_mut(&mut st.layout);
    l.paint.text_material = super::super::materials::enabled(ui)
        .then(|| super::super::materials::resolve(skin, d.text.material.as_deref()))
        .flatten();
    l.font_size = Some(p.size);
    l.font_weight = Some(skin.fonts.name.weight);
    l.text_ext.font_family = skin.fonts.name.families.first().cloned();
    l.paint_colors.color = Some(nana_ui_core::PaintColor::srgb(c(p.text_color)));
    l.white_space = nana_ui_core::WhiteSpaceSpec::Nowrap;
    if let Some(stroke) = &p.stroke {
        surface::text_stroke(l, stroke);
    }
    if let Some(s) = &p.text_shadow {
        surface::text_shadow(l, s);
    }
    st
}

/// Build per-grapheme [`RichText`] for a nameplate with [`NameplateTypographySpec`].
fn build_nameplate_rich(
    name: &str,
    typo: &NameplateTypographySpec,
    skin: &Skin,
    speaker_col: Option<Rgba8>,
    p: &NameplateSpec,
) -> RichText {
    let mut b = RichText::builder();
    for (idx, g) in name.graphemes(true).enumerate() {
        let mut span = RichSpanStyle::new();
        if idx == 0 {
            if (typo.initial_scale - 1.0).abs() > f32::EPSILON {
                span = span.size_em(typo.initial_scale);
            }
            let col = if typo.speaker_color_on_initial {
                speaker_col
                    .or_else(|| typo.initial_color.map(|cr| skin.color(cr)))
                    .unwrap_or(p.text_color)
            } else if let Some(cr) = typo.initial_color {
                skin.color(cr)
            } else {
                speaker_col.unwrap_or(p.text_color)
            };
            span = span.color(nana_ui_core::PaintColor::srgb(c(col)));
            if let Some(w) = typo.initial_weight {
                span = span.weight(w);
            }
        } else {
            let scale = if typo.glyph_scales.is_empty() {
                1.0
            } else {
                typo.glyph_scales[(idx - 1) % typo.glyph_scales.len()]
            };
            if (scale - 1.0).abs() > f32::EPSILON {
                span = span.size_em(scale);
            }
            let col = if typo.speaker_color_on_initial {
                p.text_color
            } else {
                speaker_col.unwrap_or(p.text_color)
            };
            span = span.color(nana_ui_core::PaintColor::srgb(c(col)));
        }
        if typo.letter_spacing != 0.0 {
            span = span.letter_spacing(typo.letter_spacing);
        }
        b.push(g, span);
    }
    b.build()
}

fn column_nameplate(ui: Stage, ctx: &BoxCtx, resolved: Computed<Arc<ResolvedDialogue>>) -> AnyView {
    let ctx = ctx.clone();
    let label_ctx = ctx.clone();
    let named = has_speaker(ui, &ctx);
    let name = speaker(ui, &ctx);
    let color = speaker_color(ui, &ctx);
    let spec = move |skin: &Skin| {
        let vertical = ui.settings.with(|s| ui.canvas().vertical_text(s, skin));
        let r = resolved.get();
        let p = &r.dialogue.nameplate;
        let shown = plate_in_column(p, vertical) && p.visible;
        (r, shown)
    };
    widget(Stack::column(0.0))
        .key("dialogue-nameplate-column")
        .bind(move |s: &mut Stack| {
            let (r, shown) = spec(&ctx.skin);
            let shown = shown && named();
            let p = &r.dialogue.nameplate;
            let materials_enabled = super::super::materials::enabled(ui);
            let plate = nameplate_surface_for(
                &ctx.skin,
                p,
                p.frame.as_ref().and_then(|f| ctx.url(&f.image)),
                materials_enabled,
            );
            let [along, across] = p.padding;
            let gap = (p.size * 0.5).round().max(6.0);
            let rotate = p.rotate;
            *s = plate
                .stack(Stack::column(0.0).padding_xy(across, along))
                .with_layout(move |l| {
                    l.display = Some(if shown {
                        nana_ui_core::DisplaySpec::Flex
                    } else {
                        nana_ui_core::DisplaySpec::None
                    });
                    l.align_self = Some(AlignSpec::Start);
                    l.width = Some(LengthSpec::Shrink);
                    l.height = Some(LengthSpec::Shrink);
                    l.flex_grow = Some(0.0);
                    l.flex_shrink = Some(0.0);
                    l.min_height = Some(LengthSpec::Px(0.0));
                    l.max_height = Some(LengthSpec::Percent(100.0));
                    l.margin_left = Some(LengthSpec::Px(gap));
                    l.pointer_events = Some(PointerEventsSpec::None);
                    l.transforms.transform = (rotate != 0.0).then(|| affine(0.0, 0.0, rotate, 1.0));
                });
        })
        .children(
            if let Some(typo) = label_ctx.skin.dialogue.nameplate.typography.clone() {
                let col_skin = label_ctx.skin.clone();
                widget(RichTextView::new(RichText::default()).decorative())
                    .bind(move |v: &mut RichTextView| {
                        let (r, shown) = spec(&col_skin);
                        if shown {
                            let p = &r.dialogue.nameplate;
                            let mut st = plate_label_style(ui, &col_skin, &r.dialogue);
                            let l = Arc::make_mut(&mut st.layout);
                            l.writing_mode = Some(nana_ui_core::WritingModeSpec::VerticalRl);
                            l.text_ext.text_orientation =
                                Some(nana_ui_core::TextOrientationSpec::Mixed);
                            l.max_height = Some(LengthSpec::Px(
                                (ui.text_area_height.get() - 2.0 * p.padding[0]).max(p.size),
                            ));
                            l.text_overflow_ellipsis = true;
                            let sc = color();
                            let st = tinted(
                                &st,
                                if typo.speaker_color_on_initial {
                                    None
                                } else {
                                    sc
                                },
                            );
                            if v.style != st {
                                v.style = st;
                            }
                            let rich = build_nameplate_rich(&name(), &typo, &col_skin, sc, p);
                            if v.value != rich {
                                v.value = rich;
                            }
                        }
                    })
                    .into_any()
            } else {
                widget(Text::new(""))
                    .bind(move |t: &mut Text| {
                        let (r, shown) = spec(&label_ctx.skin);
                        if shown {
                            let p = &r.dialogue.nameplate;
                            let mut st = plate_label_style(ui, &label_ctx.skin, &r.dialogue);
                            let l = Arc::make_mut(&mut st.layout);
                            l.writing_mode = Some(nana_ui_core::WritingModeSpec::VerticalRl);
                            l.text_ext.text_orientation =
                                Some(nana_ui_core::TextOrientationSpec::Mixed);
                            l.max_height = Some(LengthSpec::Px(
                                (ui.text_area_height.get() - 2.0 * p.padding[0]).max(p.size),
                            ));
                            l.text_overflow_ellipsis = true;
                            let st = tinted(&st, color());
                            if t.style != st {
                                t.style = st;
                            }
                        }
                        let n = name();
                        if t.value != n {
                            t.value = n;
                        }
                    })
                    .into_any()
            },
        )
        .into_any()
}

pub(super) fn nameplate(ui: Stage, ctx: &BoxCtx, resolved: &ResolvedDialogue) -> AnyView {
    let skin = &ctx.skin;
    let d = &resolved.dialogue;
    let source = ctx.source;
    let p = &d.nameplate;
    let vertical = source != Source::Draft
        && ui
            .settings
            .with_untracked(|s| ui.canvas().vertical_text(s, skin));
    if plate_in_column(p, vertical) {
        return None::<AnyView>.into_any();
    }
    let artwork_clearance = if resolved.has_dialogue_artwork() || d.responsive.is_some() {
        d.padding[2]
    } else {
        0.
    };
    let right_clearance = if p.anchor == Anchor::TopLeft {
        d.decor
            .iter()
            .filter(|dec| {
                dec.anchor == Anchor::TopRight && dec.offset[1] <= 24.0 && dec.opacity > 0.0
            })
            .filter_map(|dec| match dec.size[0] {
                nanatype_theme::Len::Px(w) => Some((w - dec.offset[0]).max(0.0) + 8.0),
                _ => None,
            })
            .fold(artwork_clearance, f32::max)
    } else {
        0.0
    };
    let materials_enabled = super::super::materials::enabled(ui);
    let plate = nameplate_surface_for(
        skin,
        p,
        p.frame.as_ref().and_then(|f| ctx.url(&f.image)),
        materials_enabled,
    );
    let mut st = plate_label_style(ui, skin, d);
    if right_clearance > 0.0 || ctx.source != Source::Draft {
        let l = Arc::make_mut(&mut st.layout);
        l.min_width = Some(LengthSpec::Px(0.0));
        l.max_width = Some(LengthSpec::Percent(100.0));
        l.flex_shrink = Some(1.0);
        l.text_overflow_ellipsis = true;
    }
    let visible = p.visible;
    let name = speaker(ui, ctx);
    let named = has_speaker(ui, ctx);
    let color = speaker_color(ui, ctx);
    let [ox, oy] = p.offset;
    if vertical {
        let l = Arc::make_mut(&mut st.layout);
        l.writing_mode = Some(nana_ui_core::WritingModeSpec::VerticalRl);
        l.text_ext.text_orientation = Some(nana_ui_core::TextOrientationSpec::Mixed);
        l.white_space = nana_ui_core::WhiteSpaceSpec::Nowrap;
        l.max_height = Some(LengthSpec::Px(
            (ui.display_size.get_untracked()[1] * 0.25).max(p.size),
        ));
        l.text_overflow_ellipsis = true;
    }
    let rotate = p.rotate;
    let anchor = p.anchor;
    let pad = p.padding;
    widget(anchor_layer(anchor, 3))
        .visible(move || visible && named())
        .children(
            widget(
                plate
                    .stack(Stack::row(0.0).padding_xy(pad[0], pad[1]))
                    .with_layout(move |l| {
                        l.transforms.transform = Some(affine(ox, oy, rotate, 1.0));
                        if right_clearance > 0.0 || source != Source::Draft {
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
            .children(if let Some(typo) = p.typography.clone() {
                let skin_for_rich = skin.clone();
                let plate_spec = p.clone();
                widget(
                    RichTextView::new(RichText::default())
                        .decorative()
                        .style(st.clone()),
                )
                .bind(move |v: &mut RichTextView| {
                    let sc = color();
                    let want = tinted(
                        &st,
                        if typo.speaker_color_on_initial {
                            None
                        } else {
                            sc
                        },
                    );
                    if v.style != want {
                        v.style = want;
                    }
                    let rich =
                        build_nameplate_rich(&name(), &typo, &skin_for_rich, sc, &plate_spec);
                    if v.value != rich {
                        v.value = rich;
                    }
                })
                .into_any()
            } else {
                widget(Text::new("").style(st.clone()))
                    .bind(move |t: &mut Text| {
                        let want = tinted(&st, color());
                        if t.style != want {
                            t.style = want;
                        }
                        let n = name();
                        if t.value != n {
                            t.value = n;
                        }
                    })
                    .into_any()
            }),
        )
        .into_any()
}

fn portrait(ui: Stage, ctx: &BoxCtx, d: &Dialogue) -> impl IntoView + use<> {
    let p = d.portrait.clone();
    let source = ctx.source;
    let [w, h] = p.size;
    let show_in_preview = ui.preview_options.show_portrait;
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
        (source != Source::Preview || show_in_preview)
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
            l.paint.background_layers = url
                .map(|u| BackgroundImage::url_with_fit(u, BackgroundImageFit::Contain))
                .into_iter()
                .collect();
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
                super::super::materials::enabled(ui),
            )
        },
    )
    .css(nana_ui::runtime::view::css! { position: absolute; inset: 0; pointer-events: none; overflow: visible; })
    .into_any()
}

/// Whether a decor belongs on a box of `source` at all.
fn decor_shown(skin: &Skin, dec: &DecorSpec, source: Source) -> bool {
    match dec.when {
        DecorWhen::Always => true,
        DecorWhen::Speaker => !skin.speaker_decor_hidden,
        DecorWhen::Display => source != Source::Draft,
    }
}

/// One decoration, absolutely placed on its parent; `visible` hides it.
pub(crate) fn decor<F>(
    skin: &Skin,
    dec: &DecorSpec,
    visible: Option<F>,
    image_url: Option<String>,
    materials_enabled: bool,
) -> AnyView
where
    F: Fn() -> bool + Send + 'static,
{
    decor_on(
        skin,
        dec,
        visible,
        image_url,
        false,
        None,
        materials_enabled,
    )
}

fn decor_on<F>(
    skin: &Skin,
    dec: &DecorSpec,
    visible: Option<F>,
    image_url: Option<String>,
    animate: bool,
    edit: Option<(Stage, usize)>,
    materials_enabled: bool,
) -> AnyView
where
    F: Fn() -> bool + Send + 'static,
{
    let shape = Surface::decor_image(skin, dec, image_url).materials(materials_enabled);
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
        super::super::motion::periodic(node, motion.period_s.max(0.5), move |t| {
            let (dy, alpha) = decor_motion(&motion, t, false);
            (affine(0.0, dy, rotate, 1.0), opacity * alpha)
        });
        el = el.node_ref(node);
        motion_ref = Some(node);
    }
    if let Some((ui, index)) = edit {
        if let Some(node) = motion_ref {
            super::super::editor_geometry::track(ui, format!("decor.{index}"), node);
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
        l.text_ext.font_family = font.families.first().cloned();
        l.paint_colors.color = Some(nana_ui_core::PaintColor::srgb(
            c(skin.color(dec.text_color)),
        ));
        l.white_space = nana_ui_core::WhiteSpaceSpec::Nowrap;
        l.line_height = Some(nana_ui_core::LineHeightSpec::Absolute(dec.text_size * 1.2));
    }
    el.children(widget(Text::new(dec.text.clone()).decorative().style(st)))
        .into_any()
}

/// Non-shader decorations for `layer`. Shader-bearing decor items (both above
/// and below) are rendered exclusively by [`shader_art`].
fn decor_views(
    ui: Stage,
    ctx: &BoxCtx,
    resolved: &ResolvedDialogue,
    layer: DecorLayer,
) -> Vec<AnyView> {
    resolved
        .dialogue
        .decor
        .iter()
        .enumerate()
        .filter(|(_, d)| {
            d.layer == layer && d.shader.is_none() && decor_shown(&ctx.skin, d, ctx.source)
        })
        .map(|(index, d)| {
            decor_view(
                ui,
                ctx,
                d,
                resolved.decor_indices.get(index).copied().unwrap_or(index),
            )
        })
        .collect()
}

/// The tail's shadow triangles (drawn under the box) and its border and
/// fill triangles (drawn over the box border, so the tail opens into it).
fn tail(
    skin: &Skin,
    resolved: &ResolvedDialogue,
    surface: &Surface,
) -> (Vec<AnyView>, Vec<AnyView>) {
    let d = &resolved.dialogue;
    let Some(t) = &d.tail else {
        return (Vec::new(), Vec::new());
    };
    let b = &d.box_;
    if surface.integrates_tail() {
        return (Vec::new(), Vec::new());
    }
    let edge = t.side;
    let border = if resolved.has_dialogue_artwork() {
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

fn continue_indicator(ui: Stage, ctx: &BoxCtx, d: &Dialogue) -> impl IntoView + use<> {
    let ctx = ctx.clone();
    let d = d.clone();
    let source = ctx.source;
    let shown = d.continue_.image.is_some() || !d.continue_.glyph.is_empty();
    dynamic(
        move || {
            let view = ui.display.get();
            shown
                && source == Source::Display
                && !view.empty
                && view.continue_indicator
                && !ui.reduced_motion.get()
        },
        move |animate| continue_indicator_on(ui, &ctx, &d, *animate).into_any(),
    )
    .css(nana_ui::runtime::view::css! { position: absolute; inset: 0; pointer-events: none; overflow: visible; })
}

fn continue_indicator_on(
    ui: Stage,
    ctx: &BoxCtx,
    d: &Dialogue,
    animate: bool,
) -> impl IntoView + use<> {
    let source = ctx.source;
    let cs = d.continue_.clone();
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
                    .with_layout(move |l| l.paint.background_layers = vec![first]),
            );
            if frames > 1 && animate {
                let shown = AtomicU32::new(0);
                el = el.bind(move |s: &mut Stack| {
                    let f = sprite_frame(frames, fps, ui.now.get() as f32 / 1000.0);
                    if shown.swap(f, Ordering::Relaxed) != f {
                        let image = sheet[f as usize].clone();
                        restyle(s, |l| l.paint.background_layers = vec![image]);
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
                l.paint_colors.color = Some(nana_ui_core::PaintColor::srgb(c(cs.color)));
                l.white_space = nana_ui_core::WhiteSpaceSpec::Nowrap;
            }
            widget(Text::new(cs.glyph.clone()).decorative().style(st))
                .key("dialogue-continue-glyph")
                .editor_part(ui, "continue")
                .into_any()
        }
    };
    let [ox, oy] = cs.offset;
    let (motion, bob, period) = (cs.motion, cs.bob_px, cs.period_s);
    let mut mark = widget(
        Stack::row(0.0)
            .with_layout(move |l| l.transforms.transform = Some(affine(ox, oy, 0.0, 1.0))),
    );
    if animate && motion != ContinueMotion::None {
        let node = nana_ui::runtime::view::node_ref();
        super::super::motion::periodic(node, period, move |t| {
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
                && !super::super::shader_effects::owns_continue(ui, source, &skin)
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
