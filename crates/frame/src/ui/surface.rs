//! Painting the skin's shapes. The dialogue box, the name plate, history
//! bubbles, decorations and tails all describe themselves as a [`Surface`],
//! and [`Surface::apply`] is the one place that writes NanaUI paint fields,
//! so every shape supports the same fills, gradients, borders and shadows.

use nana_ui_core::{
    BackgroundImage, BackgroundImageFit, BackgroundPosition, BackgroundRepeat, BorderImageSlice,
    BorderImageSpec, BorderStyle, BoxShadowSpec, ClipPath, ClipPoint, CssGradient, GradientStop,
    LayoutStyle, LengthSpec, LinearGradient, PaintTransform, RadialGradient, TextShadowSpec,
    TextStrokeSpec,
};
use nanatype_theme::{
    BorderStyleSpec, ColorRef, DecorSpec, GradientKind, GradientSpec, NineSlice, ShadowSpec,
    ShapeSpec, Skin, TextShadowSpec as SkinShadow, TextStrokeSpec as SkinStroke,
};

use super::c;
#[path = "surface_painter.rs"]
mod painter;
pub use painter::SurfacePainter;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Border {
    /// `[top, right, bottom, left]`.
    pub widths: [f32; 4],
    pub color: [f32; 4],
    pub style: BorderStyleSpec,
}

/// Everything a skin can paint on one node.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Surface {
    pub material: Option<nana_ui_core::MaterialRef>,
    pub border_material: Option<nana_ui_core::MaterialRef>,
    pub contour: Option<nanatype_theme::ContourPreset>,
    pub motif: Option<nanatype_theme::MotifPreset>,
    pub tail: Option<nanatype_theme::TailSpec>,
    pub fill: Option<[f32; 4]>,
    pub gradient: Option<BackgroundImage>,
    /// `[top_left, top_right, bottom_right, bottom_left]`.
    pub radii: [f32; 4],
    pub border: Option<Border>,
    pub shadows: Vec<BoxShadowSpec>,
    /// A nine-slice image and its drawn border widths; replaces fill and border.
    pub frame: Option<(BorderImageSpec, [f32; 4])>,
    /// Polygon clip, points in percent of the node.
    pub polygon: Option<Vec<[f32; 2]>>,
    pub rotate_deg: f32,
    pub opacity: Option<f32>,
}

/// One precedence rule for layout paint, retained paths and coverage masks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SurfaceRoute {
    Frame,
    Motif(nanatype_theme::MotifPreset),
    Contour(nanatype_theme::ContourPreset),
    Style,
}

pub fn gradient(skin: &Skin, g: &GradientSpec) -> BackgroundImage {
    let stops = g
        .stops
        .iter()
        .map(|(position, color)| GradientStop {
            paint_color: None,
            position: *position,
            color: c(skin.color(*color)),
        })
        .collect();
    BackgroundImage::Gradient(match g.kind {
        GradientKind::Linear => CssGradient::Linear(LinearGradient {
            angle_deg: g.angle,
            stops,
        }),
        GradientKind::Radial => CssGradient::Radial(RadialGradient {
            circle: g.circle,
            center: g.center.map(LengthSpec::Percent),
            stops,
        }),
    })
}

pub fn shadow(skin: &Skin, s: &ShadowSpec) -> BoxShadowSpec {
    BoxShadowSpec {
        paint_color: None,
        offset_x: s.dx,
        offset_y: s.dy,
        blur_radius: s.blur,
        spread_radius: s.spread,
        color: c(skin.color(s.color)),
        inset: s.inset,
    }
}

fn shadows(skin: &Skin, list: &[ShadowSpec]) -> Vec<BoxShadowSpec> {
    list.iter().map(|s| shadow(skin, s)).collect()
}

/// A nine-slice frame and the border widths it is drawn with (`slice × scale`).
pub fn nine_slice(frame: &NineSlice, url: String) -> (BorderImageSpec, [f32; 4]) {
    let mut spec = BorderImageSpec::from_source(BackgroundImage::url(url));
    spec.slice = frame.slice.map(|v| BorderImageSlice::Number(v as f32));
    spec.fill = frame.fill;
    (
        spec,
        if frame.layout_inset {
            frame.slice.map(|v| v as f32 * frame.scale)
        } else {
            [0.; 4]
        },
    )
}

/// A real outline behind the glyphs.
pub fn text_stroke(l: &mut LayoutStyle, s: &SkinStroke) {
    l.paint.text_stroke = Some(TextStrokeSpec {
        width: s.width,
        color: Some(c(s.color)),
        paint_color: None,
    });
    l.paint.paint_order_stroke_first = true;
}

pub fn text_shadow(l: &mut LayoutStyle, s: &SkinShadow) {
    l.paint.text_shadows = vec![TextShadowSpec {
        offset_x: s.dx,
        offset_y: s.dy,
        blur_radius: s.blur,
        color: c(s.color),
        paint_color: None,
    }];
}

fn set_border_widths(l: &mut LayoutStyle, [t, r, b, left]: [f32; 4]) {
    l.border_top_width = Some(t);
    l.border_right_width = Some(r);
    l.border_bottom_width = Some(b);
    l.border_left_width = Some(left);
}

impl Surface {
    pub fn materials(mut self, enabled: bool) -> Self {
        if !enabled {
            self.material = None;
            self.border_material = None;
        }
        self
    }

    /// A frame image, when the skin names one and it resolved to a URL.
    fn framed(frame: Option<&NineSlice>, url: Option<String>) -> Option<Self> {
        Some(Self {
            frame: Some(nine_slice(frame?, url?)),
            ..Self::default()
        })
    }

    /// A fill with the shape fields every part shares.
    fn shaped(
        skin: &Skin,
        fill: Option<ColorRef>,
        shape: &ShapeSpec,
        radius: f32,
        border: Option<ColorRef>,
        border_width: f32,
    ) -> Self {
        let widths = shape.border_widths(border_width);
        Self {
            material: super::materials::resolve(skin, shape.material.as_deref()),
            border_material: super::materials::resolve(skin, shape.border_material.as_deref()),
            contour: shape.contour,
            fill: fill.map(|f| c(skin.color(f))),
            gradient: shape.gradient.as_ref().map(|g| gradient(skin, g)),
            radii: shape.radii(radius),
            border: border
                .filter(|_| widths.iter().any(|w| *w > 0.0))
                .map(|color| Border {
                    widths,
                    color: c(skin.color(color)),
                    style: shape.border_style,
                }),
            shadows: shadows(skin, &shape.shadows),
            ..Self::default()
        }
    }

    /// The dialogue box: fill, gradient, border, shadow layers, or a frame image.
    pub fn dialogue_box(skin: &Skin, frame_url: Option<String>) -> Self {
        let b = &skin.dialogue.box_;
        if b.shape.material.is_some() {
            let mut surface = Self::shaped(
                skin,
                Some(b.color.into()),
                &b.shape,
                b.radius,
                Some(b.border_color.into()),
                b.border_width,
            );
            if b.frame.as_ref().is_some_and(|f| !f.fill) {
                surface.frame = b
                    .frame
                    .as_ref()
                    .zip(frame_url)
                    .map(|(frame, url)| nine_slice(frame, url));
            } else {
                surface.gradient = frame_url
                    .map(|url| BackgroundImage::url_with_fit(url, BackgroundImageFit::Stretch));
            }
            return surface;
        }
        Self::framed(b.frame.as_ref(), frame_url)
            .map(|mut s| {
                if s.frame.as_ref().is_some_and(|(frame, _)| !frame.fill) {
                    s.fill = Some(c(b.color));
                }
                // A procedural plate replaces the fill and keyline. Keep its
                // authored external shadow, which lives outside the plate canvas.
                if skin.has_dialogue_artwork() {
                    // The bitmap cuts retain their painted corner/header
                    // dimensions. They are not layout borders: artwork and
                    // native text share the original full border-box canvas.
                    if let Some((_, widths)) = &mut s.frame {
                        *widths = [0.; 4];
                    }
                    s.shadows = shadows(skin, &b.shadow_layers())
                        .into_iter()
                        .filter(|s| !s.inset)
                        .collect();
                    if !s.shadows.is_empty() {
                        s.radii = b.shape.radii(b.radius);
                    }
                }
                s
            })
            .unwrap_or_else(|| Self {
                tail: skin.dialogue.tail.clone(),
                shadows: shadows(skin, &b.shadow_layers()),
                ..Self::shaped(
                    skin,
                    Some(b.color.into()),
                    &b.shape,
                    b.radius,
                    Some(b.border_color.into()),
                    b.border_width,
                )
            })
    }

    pub fn nameplate(skin: &Skin, frame_url: Option<String>) -> Self {
        let p = &skin.dialogue.nameplate;
        let s = Self::framed(p.frame.as_ref(), frame_url).unwrap_or_else(|| {
            Self::shaped(
                skin,
                Some(p.color.into()),
                &p.shape,
                p.radius,
                p.border_color,
                p.border_width,
            )
        });
        Self {
            rotate_deg: p.rotate,
            ..s
        }
    }

    /// A history bubble.
    pub fn bubble(skin: &Skin, frame_url: Option<String>) -> Self {
        let h = &skin.history;
        if h.bubble_material.is_some() {
            let shape = ShapeSpec {
                material: h.bubble_material.clone(),
                ..Default::default()
            };
            let mut surface = Self::shaped(
                skin,
                Some(h.bubble_color.into()),
                &shape,
                h.bubble_radius,
                h.bubble_border.map(ColorRef::from),
                h.bubble_border_width,
            );
            surface.gradient = frame_url
                .map(|url| BackgroundImage::url_with_fit(url, BackgroundImageFit::Stretch));
            return surface;
        }
        Self::framed(h.bubble_frame.as_ref(), frame_url).unwrap_or_else(|| {
            let shape = ShapeSpec {
                contour: h.bubble_contour,
                gradient: h.bubble_gradient.clone(),
                radii: h.bubble_radii,
                shadows: h.bubble_shadows.clone(),
                ..ShapeSpec::default()
            };
            Self::shaped(
                skin,
                Some(h.bubble_color.into()),
                &shape,
                h.bubble_radius,
                h.bubble_border.map(ColorRef::from),
                h.bubble_border_width,
            )
        })
    }

    /// The pill behind a history row's name, when the skin has one.
    pub fn history_name_plate(skin: &Skin) -> Option<Self> {
        let p = skin.history.name_plate.as_ref()?;
        Some(Self::shaped(
            skin,
            Some(p.color),
            &p.shape,
            p.radius,
            p.border_color,
            p.border_width,
        ))
    }

    /// The surface behind the whole history list, when the skin has one.
    pub fn history_panel(skin: &Skin) -> Option<Self> {
        let p = skin.history.panel.as_ref()?;
        Some(Self::shaped(
            skin,
            Some(p.color.into()),
            &p.shape,
            p.radius,
            p.border_color,
            p.border_width,
        ))
    }

    pub fn decor(skin: &Skin, d: &DecorSpec) -> Self {
        Self {
            motif: d.motif,
            polygon: (d.polygon.len() >= 3).then(|| d.polygon.clone()),
            rotate_deg: d.rotate,
            opacity: (d.opacity < 1.0).then_some(d.opacity),
            ..Self::shaped(
                skin,
                d.color,
                &d.shape,
                d.radius,
                d.border_color,
                d.border_width,
            )
        }
    }

    /// Artwork uses the same placement and transforms as vector decorations.
    pub fn decor_image(skin: &Skin, d: &DecorSpec, image_url: Option<String>) -> Self {
        let mut surface = Self::decor(skin, d);
        if let Some(url) = image_url {
            surface.fill = None;
            surface.motif = None;
            surface.contour = None;
            surface.polygon = None;
            let mut image = BackgroundImage::url_with_fit(url, BackgroundImageFit::Contain);
            if let BackgroundImage::Url {
                repeat, position, ..
            } = &mut image
            {
                *repeat = BackgroundRepeat::NoRepeat;
                *position = BackgroundPosition::center();
            }
            surface.gradient = Some(image);
        }
        surface
    }

    /// One triangle of a tail.
    pub fn tail_part(color: [f32; 4], points: &[[f32; 2]]) -> Self {
        Self {
            fill: Some(color),
            polygon: Some(points.to_vec()),
            ..Self::default()
        }
    }

    /// Write this surface into `l`. Leaves position, size and padding alone.
    pub fn apply(&self, l: &mut LayoutStyle) {
        l.paint.background_material = self.material.clone();
        l.paint.border_material = self.border_material.clone();
        // Surface owns these fields. Clear them even when changing routes on
        // the same retained node (image -> path -> ordinary box, for example).
        l.background = None;
        l.paint.background_image = None;
        l.paint.border_image = None;
        l.border_radius = None;
        l.paint.border_radii = None;
        l.border_color = None;
        l.border_style = None;
        set_border_widths(l, [0.0; 4]);
        l.paint.box_shadows.clear();
        if let Some((spec, widths)) = &self.frame {
            if !spec.fill {
                l.background = self.fill;
            }
            l.paint.border_image = Some(spec.clone());
            set_border_widths(l, *widths);
        } else {
            l.background = self.fill;
            l.paint.background_image = self.gradient.clone();
            l.border_radius = Some(self.radii[0]);
            l.paint.border_radii = (self.radii.iter().any(|r| *r != self.radii[0]))
                .then(|| self.radii.map(LengthSpec::Px));
            if let Some(b) = &self.border {
                set_border_widths(l, b.widths);
                l.border_color = Some(b.color);
                l.border_style = Some(match b.style {
                    BorderStyleSpec::Solid => BorderStyle::Solid,
                    BorderStyleSpec::Dashed => BorderStyle::Dashed,
                    BorderStyleSpec::Dotted => BorderStyle::Dotted,
                });
            }
        }
        // External shadows belong to the box, including an image-backed box.
        // They must survive the frame route as well as the ordinary fill route.
        l.paint.box_shadows = self.shadows.clone();
        if self.frame.is_some() {
            l.border_radius = Some(self.radii[0]);
            l.paint.border_radii = (self.radii.iter().any(|r| *r != self.radii[0]))
                .then(|| self.radii.map(LengthSpec::Px));
        }
        l.paint.clip_path = self
            .polygon
            .as_ref()
            .filter(|_| !self.is_vector())
            .map(|pts| {
                ClipPath::Polygon(
                    pts.iter()
                        .map(|p| ClipPoint {
                            x: LengthSpec::Percent(p[0]),
                            y: LengthSpec::Percent(p[1]),
                        })
                        .collect(),
                )
            });
        l.transform = (self.rotate_deg != 0.0).then(|| affine(0.0, 0.0, self.rotate_deg, 1.0));
        l.opacity = self.opacity;
    }

    pub fn contour_radius(&self) -> f32 {
        self.radii.into_iter().fold(0.0_f32, f32::max)
    }

    pub fn is_vector(&self) -> bool {
        matches!(
            self.route(),
            SurfaceRoute::Contour(_) | SurfaceRoute::Motif(_)
        )
    }

    pub fn integrates_tail(&self) -> bool {
        matches!(self.route(), SurfaceRoute::Contour(_)) && self.tail.is_some()
    }

    pub(super) fn route(&self) -> SurfaceRoute {
        if self.frame.is_some() {
            SurfaceRoute::Frame
        } else if let Some(motif) = self.motif {
            SurfaceRoute::Motif(motif)
        } else if let Some(contour) = self.contour {
            SurfaceRoute::Contour(contour)
        } else {
            SurfaceRoute::Style
        }
    }

    pub(super) fn border_width(&self) -> f32 {
        self.border
            .as_ref()
            .map_or(0.0, |b| b.widths.into_iter().fold(0.0_f32, f32::max))
    }

    pub(super) fn contour_path(&self, size: [f32; 2]) -> Option<nanatype_theme::ShapePath> {
        let SurfaceRoute::Contour(contour) = self.route() else {
            return None;
        };
        Some(nanatype_theme::contour_path(
            contour,
            size,
            self.contour_radius(),
            self.border_width() * 0.5,
            self.tail.as_ref(),
        ))
    }

    /// The one attachment path for live views, history, and thumbnails.
    /// Keeping layout border widths preserves the legacy text viewport.
    pub fn stack(&self, stack: nana_ui::runtime::Stack) -> nana_ui::runtime::Stack {
        let stack = stack.with_layout(|l| self.apply(l));
        let mut style = stack.node_style();
        // A retained node can return to a legacy or image skin on hot reload.
        style.painter = self
            .is_vector()
            .then(|| SurfacePainter::new(self.clone()).into());
        stack.style(style)
    }

    pub fn optional_stack(
        surface: Option<&Self>,
        stack: nana_ui::runtime::Stack,
    ) -> nana_ui::runtime::Stack {
        match surface {
            Some(s) => s.stack(stack),
            None => stack,
        }
    }

    /// Actual path bounds, including an integrated tail, before rotation.
    pub fn paint_bounds(&self, size: [f32; 2]) -> [f32; 4] {
        match self.route() {
            SurfaceRoute::Frame | SurfaceRoute::Style => [0.0, 0.0, size[0], size[1]],
            SurfaceRoute::Motif(motif) => {
                let paths = nanatype_theme::motif_paths(motif, size);
                let mut bounds = [0.0, 0.0, size[0], size[1]];
                for p in paths {
                    if let Some([x, y, w, h]) = p.path.bounds() {
                        let pad = p.stroke_width * 0.5;
                        let right = (bounds[0] + bounds[2]).max(x + w + pad);
                        let bottom = (bounds[1] + bounds[3]).max(y + h + pad);
                        bounds[0] = bounds[0].min(x - pad);
                        bounds[1] = bounds[1].min(y - pad);
                        bounds[2] = right - bounds[0];
                        bounds[3] = bottom - bounds[1];
                    }
                }
                bounds
            }
            SurfaceRoute::Contour(_) => {
                let width = self.border_width();
                let p = self.contour_path(size).expect("contour route");
                p.bounds()
                    .map(|[x, y, w, h]| [x - width * 0.5, y - width * 0.5, w + width, h + width])
                    .unwrap_or([0.0; 4])
            }
        }
    }
}

/// Translate by `(dx, dy)`, rotating (degrees clockwise) and scaling about
/// the transform origin.
pub fn affine(dx: f32, dy: f32, deg: f32, scale: f32) -> PaintTransform {
    let (s, co) = deg.to_radians().sin_cos();
    PaintTransform {
        a: co * scale,
        b: s * scale,
        c: -s * scale,
        d: co * scale,
        e: dx,
        f: dy,
    }
}

/// `percent`% of the parent plus `px`, as a NanaUI length.
pub fn calc(percent: f32, px: f32) -> LengthSpec {
    if percent == 0.0 {
        LengthSpec::Px(px)
    } else if px == 0.0 {
        LengthSpec::Percent(percent)
    } else {
        LengthSpec::CalcPercentOffset {
            percent,
            offset_px: px,
        }
    }
}

pub fn len(l: nanatype_theme::Len) -> LengthSpec {
    match l {
        nanatype_theme::Len::Px(v) => LengthSpec::Px(v),
        nanatype_theme::Len::Percent(v) => LengthSpec::Percent(v),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nanatype_model::Rgba8;
    use nanatype_theme::{GradientSpec, NineSlice, ShadowSpec};

    #[test]
    fn builtin_vector_frames_contain_their_text_viewports() {
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        for entry in registry.entries() {
            let skin = entry.skin.as_ref().unwrap();
            let d = &skin.dialogue;
            let surface = Surface::dialogue_box(skin, None);
            let Some(contour) = surface.contour else {
                continue;
            };
            let [mt, mr, mb, ml] = d.box_.margin;
            let size = [
                (d.region[0] - ml - mr) * d.box_.width_fraction,
                d.region[1] - mt - mb,
            ];
            let path = painter::paint_path(&nanatype_theme::contour_path(
                contour,
                size,
                surface.contour_radius(),
                0.0,
                surface.tail.as_ref(),
            ));
            let [l, t, r, b] = d.padding;
            for point in [
                [l, t],
                [size[0] - r, t],
                [l, size[1] - b],
                [size[0] - r, size[1] - b],
                [size[0] * 0.5, size[1] * 0.5],
            ] {
                assert!(
                    path.contains(point),
                    "{}: text corner {point:?} is outside its frame",
                    entry.id
                );
            }
        }
    }

    #[test]
    fn painter_cache_tracks_the_entire_resolved_surface() {
        use nana_ui::runtime::Painter;
        let base = Surface {
            contour: Some(nanatype_theme::ContourPreset::Cloud),
            fill: Some([1.0, 0.8, 0.9, 0.95]),
            radii: [24.0; 4],
            ..Default::default()
        };
        let key = SurfacePainter::new(base.clone()).paint_key();
        assert_eq!(key, SurfacePainter::new(base.clone()).paint_key());
        let mut changes = vec![base.clone(); 5];
        changes[0].contour = Some(nanatype_theme::ContourPreset::Comic);
        changes[1].fill = Some([1.0, 0.7, 0.9, 0.95]);
        changes[2].motif = Some(nanatype_theme::MotifPreset::Petal);
        changes[3].tail = Some(nanatype_theme::TailSpec::default());
        changes[4].shadows = vec![shadow(
            &Skin::default(),
            &ShadowSpec {
                color: Rgba8::BLACK.into(),
                dx: 4.0,
                dy: 0.0,
                blur: 0.0,
                spread: 0.0,
                inset: false,
            },
        )];
        for changed in changes {
            assert_ne!(key, SurfacePainter::new(changed).paint_key());
        }
    }

    #[test]
    fn nine_slice_frames_keep_precedence_and_do_not_integrate_a_second_tail() {
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        let mut skin = (*registry.load("nanatype.bubble").unwrap().skin).clone();
        // Bubble now bakes its tail and offset shadow into the plate; give the
        // vector fallback both back so the precedence rules stay covered.
        skin.dialogue.tail = Some(nanatype_theme::TailSpec::default());
        skin.dialogue.box_.shape.contour = Some(nanatype_theme::ContourPreset::Rounded);
        skin.dialogue.box_.color = Rgba8::rgb(255, 255, 255);
        skin.dialogue.box_.shape.shadows = vec![ShadowSpec {
            color: Rgba8::BLACK.into(),
            dx: 4.0,
            dy: 4.0,
            blur: 0.0,
            spread: 0.0,
            inset: false,
        }];
        skin.dialogue.box_.frame = Some(NineSlice {
            image: "frame.png".into(),
            slice: [8; 4],
            scale: 1.0,
            repeat: false,
            layout_inset: true,
            fill: true,
        });
        let framed = Surface::dialogue_box(&skin, Some("file:///frame.png".into()));
        assert!(!framed.is_vector() && !framed.integrates_tail());
        assert!(
            !framed.shadows.is_empty(),
            "external shadow lost under baked plate"
        );
        let mut layout = LayoutStyle::default();
        framed.apply(&mut layout);
        assert_eq!(layout.paint.box_shadows, framed.shadows);
        assert_eq!(layout.border_top_width, Some(0.));
        let (image, _) = framed.frame.as_ref().unwrap();
        assert_eq!(image.tiles(24., 24., 100., 80.)[0].dest_w, 8.);
        let mut ordinary_frame = skin.clone();
        ordinary_frame.dialogue.shader_effects.clear();
        assert!(
            Surface::dialogue_box(&ordinary_frame, Some("file:///frame.png".into()))
                .shadows
                .is_empty()
        );
        let fallback = Surface::dialogue_box(&skin, None);
        assert!(fallback.is_vector() && fallback.integrates_tail());
    }

    #[test]
    fn static_authored_plates_keep_layout_shadows_and_user_colour_without_a_shader() {
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        for name in [
            "parchment",
            "adv",
            "vanilla",
            "comic",
            "ink",
            "echo_vanilla",
            "echo_bubble",
        ] {
            let skin = registry.load(&format!("nanatype.{name}")).unwrap().skin;
            assert!(skin.dialogue.shader_effects.is_empty());
            let surface = Surface::dialogue_box(&skin, Some("plate.png".into()));
            let (image, widths) = surface.frame.as_ref().unwrap();
            assert_eq!(*widths, [0.; 4], "{name}");
            assert!(
                image
                    .slice
                    .iter()
                    .any(|s| matches!(s, BorderImageSlice::Number(v) if *v > 0.))
            );
            let overrides = nanatype_settings::SkinOverrides {
                box_color: Some(Rgba8::rgb(1, 2, 3)),
                ..Default::default()
            };
            let personalised = crate::skin_theme::with_overrides(&skin, &overrides);
            assert!(personalised.dialogue.box_.frame.is_none());
            assert_eq!(personalised.dialogue.box_.color, Rgba8::rgb(1, 2, 3));
        }
    }

    #[test]
    fn box_surface_carries_gradient_radii_and_every_shadow_layer() {
        let mut skin = Skin::default();
        let b = &mut skin.dialogue.box_;
        b.shape.gradient = Some(GradientSpec {
            stops: vec![(0.0, Rgba8::WHITE.into()), (1.0, Rgba8::BLACK.into())],
            ..GradientSpec::default()
        });
        b.shape.radii = Some([4.0, 18.0, 18.0, 18.0]);
        b.glow = Some(Rgba8::WHITE);
        b.inner_border_color = Some(Rgba8::BLACK);
        b.shape.shadows = vec![ShadowSpec {
            color: Rgba8::BLACK.into(),
            dx: 0.0,
            dy: 4.0,
            blur: 0.0,
            spread: 0.0,
            inset: false,
        }];
        let s = Surface::dialogue_box(&skin, None);
        assert_eq!(s.shadows.len(), 3);
        assert!(s.shadows[0].inset);
        let mut l = LayoutStyle::default();
        s.apply(&mut l);
        assert!(matches!(
            l.paint.background_image,
            Some(BackgroundImage::Gradient(CssGradient::Linear(_)))
        ));
        assert_eq!(
            l.paint.border_radii,
            Some([4.0, 18.0, 18.0, 18.0].map(LengthSpec::Px))
        );
    }

    #[test]
    fn a_box_colour_override_turns_off_the_gradient() {
        let mut skin = Skin::default();
        skin.dialogue.box_.shape.gradient = Some(GradientSpec {
            stops: vec![(0.0, Rgba8::WHITE.into()), (1.0, Rgba8::BLACK.into())],
            ..GradientSpec::default()
        });
        let overrides = nanatype_settings::SkinOverrides {
            box_color: Some(Rgba8::rgb(1, 2, 3)),
            ..Default::default()
        };
        let s = crate::skin_theme::with_overrides(&skin, &overrides);
        assert!(s.dialogue.box_.shape.gradient.is_none());
        assert_eq!(
            s.color(nanatype_theme::ColorRef::Role(
                nanatype_theme::ColorRole::Box
            )),
            Rgba8::rgb(1, 2, 3)
        );
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        let original = registry.load("nanatype.halo").unwrap().skin;
        let personalised = crate::skin_theme::with_overrides(&original, &overrides);
        assert!(original.dialogue.box_.frame.is_some());
        assert!(personalised.dialogue.box_.frame.is_none());
        assert!(personalised.dialogue.shader_effects.is_empty());
        assert_eq!(personalised.dialogue.box_.color, Rgba8::rgb(1, 2, 3));
    }

    #[test]
    fn overflow_overrides_set_the_mode_and_the_page_turn() {
        let mut skin = Skin::default();
        skin.dialogue.overflow_auto_ms = Some(900);
        let keep = crate::skin_theme::with_overrides(&skin, &Default::default());
        assert_eq!(keep.dialogue.overflow_auto_ms, Some(900));
        let overrides = nanatype_settings::SkinOverrides {
            overflow: Some(nanatype_model::Overflow::Page),
            overflow_auto_ms: Some(2500),
            ..Default::default()
        };
        let s = crate::skin_theme::with_overrides(&skin, &overrides);
        assert_eq!(s.dialogue.overflow, nanatype_model::Overflow::Page);
        assert_eq!(s.dialogue.overflow_auto_ms, Some(2500));
    }

    #[test]
    fn responsive_paper_edges_do_not_stretch_the_illustrated_plate() {
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        let skin = registry.load("nanatype.crayon").unwrap().skin;
        let skin = nanatype_theme::responsive_dialogue(&skin, [300., 1200.]);
        let surface = Surface::dialogue_box(&skin, Some("file:///paper.png".into()));
        let mut style = LayoutStyle::default();
        surface.materials(false).apply(&mut style);
        assert!(!style.paint.border_image.as_ref().unwrap().fill);
        assert_eq!(style.background, Some(c(skin.dialogue.box_.color)));
        assert!(style.paint.background_image.is_none());
    }

    #[test]
    fn frames_draw_at_slice_times_scale() {
        let frame = NineSlice {
            image: "f.png".into(),
            slice: [8, 4, 8, 4],
            scale: 2.0,
            repeat: false,
            layout_inset: true,
            fill: true,
        };
        let (_, widths) = nine_slice(&frame, "file:///f.png".into());
        assert_eq!(widths, [16.0, 8.0, 16.0, 8.0]);
    }

    #[test]
    fn route_precedence_and_reused_layout_do_not_retain_old_paint() {
        let mut surface = Surface {
            contour: Some(nanatype_theme::ContourPreset::Cloud),
            motif: Some(nanatype_theme::MotifPreset::Petal),
            fill: Some([1.0; 4]),
            rotate_deg: 10.0,
            opacity: Some(0.5),
            ..Default::default()
        };
        assert!(matches!(surface.route(), SurfaceRoute::Motif(_)));
        assert!(surface.contour_path([100.0, 50.0]).is_none());
        surface.frame = Some(nine_slice(
            &NineSlice {
                image: "frame.png".into(),
                slice: [4; 4],
                scale: 1.0,
                repeat: false,
                layout_inset: true,
                fill: true,
            },
            "file:///frame.png".into(),
        ));
        let mut layout = LayoutStyle::default();
        surface.apply(&mut layout);
        assert_eq!(surface.route(), SurfaceRoute::Frame);
        assert!(layout.paint.border_image.is_some());
        surface.frame = None;
        surface.motif = None;
        surface.rotate_deg = 0.0;
        surface.opacity = None;
        surface.apply(&mut layout);
        assert!(matches!(surface.route(), SurfaceRoute::Contour(_)));
        assert!(layout.paint.border_image.is_none());
        assert_eq!(layout.border_top_width, Some(0.0));
        assert!(layout.transform.is_none() && layout.opacity.is_none());
        assert!(surface.contour_path([100.0, 50.0]).is_some());
        surface.contour = None;
        assert_eq!(surface.route(), SurfaceRoute::Style);
    }
}
