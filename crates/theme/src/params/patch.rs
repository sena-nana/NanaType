//! Typed field-path patching for [`Skin`], eliminating whole-struct TOML
//! serialization round-trips when resolving user parameters.

use nanatype_model::Rgba8;
use serde::de::DeserializeOwned;
use serde::de::value::{Error as DeError, StrDeserializer};

use crate::manifest::*;
use crate::params::{Resolved, Seg};
use crate::responsive::{DialogueLayout, DialogueLayoutRule, ResponsiveDialogue};
use crate::wgsl::{ShaderActive, ShaderEffect, ShaderSpec, ShaderTarget, SlotBinding};

pub(crate) trait PatchField {
    fn patch(&mut self, path: &[Seg], value: &Resolved, integer: bool) -> Result<(), String>;
}

fn leaf_err(expected: &str) -> String {
    format!("expected {expected}")
}

fn de_enum<T: DeserializeOwned>(value: &Resolved, expected: &str) -> Result<T, String> {
    let text = match value {
        Resolved::Enum { id, .. } | Resolved::Text(id) => id.as_str(),
        _ => return Err(leaf_err(expected)),
    };
    T::deserialize(StrDeserializer::<DeError>::new(text)).map_err(|e| e.to_string())
}

impl PatchField for f32 {
    fn patch(&mut self, path: &[Seg], value: &Resolved, integer: bool) -> Result<(), String> {
        if !path.is_empty() {
            return Err(leaf_err("number"));
        }
        let Resolved::Number(v) = value else {
            return Err(leaf_err("number"));
        };
        *self = if integer { v.round() } else { *v };
        Ok(())
    }
}

impl PatchField for f64 {
    fn patch(&mut self, path: &[Seg], value: &Resolved, integer: bool) -> Result<(), String> {
        if !path.is_empty() {
            return Err(leaf_err("number"));
        }
        let Resolved::Number(v) = value else {
            return Err(leaf_err("number"));
        };
        *self = f64::from(if integer { v.round() } else { *v });
        Ok(())
    }
}

impl PatchField for u16 {
    fn patch(&mut self, path: &[Seg], value: &Resolved, _integer: bool) -> Result<(), String> {
        if !path.is_empty() {
            return Err(leaf_err("unsigned integer"));
        }
        let Resolved::Number(v) = value else {
            return Err(leaf_err("unsigned integer"));
        };
        let r = v.round();
        if !r.is_finite() || r < 0.0 || r > f32::from(u16::MAX) {
            return Err("integer out of range".into());
        }
        *self = r as u16;
        Ok(())
    }
}

impl PatchField for u32 {
    fn patch(&mut self, path: &[Seg], value: &Resolved, _integer: bool) -> Result<(), String> {
        if !path.is_empty() {
            return Err(leaf_err("unsigned integer"));
        }
        let Resolved::Number(v) = value else {
            return Err(leaf_err("unsigned integer"));
        };
        let r = f64::from(*v).round();
        if !r.is_finite() || r < 0.0 || r > f64::from(u32::MAX) {
            return Err("integer out of range".into());
        }
        *self = r as u32;
        Ok(())
    }
}

impl PatchField for bool {
    fn patch(&mut self, path: &[Seg], value: &Resolved, _integer: bool) -> Result<(), String> {
        if !path.is_empty() {
            return Err(leaf_err("bool"));
        }
        let Resolved::Bool(b) = value else {
            return Err(leaf_err("bool"));
        };
        *self = *b;
        Ok(())
    }
}

impl PatchField for String {
    fn patch(&mut self, path: &[Seg], value: &Resolved, _integer: bool) -> Result<(), String> {
        if !path.is_empty() {
            return Err(leaf_err("string"));
        }
        *self = match value {
            Resolved::Color(c) => c.to_hex(),
            Resolved::Enum { id, .. } => id.clone(),
            Resolved::Text(t) => t.clone(),
            _ => return Err(leaf_err("string")),
        };
        Ok(())
    }
}

impl PatchField for Rgba8 {
    fn patch(&mut self, path: &[Seg], value: &Resolved, _integer: bool) -> Result<(), String> {
        if !path.is_empty() {
            return Err(leaf_err("color"));
        }
        *self = match value {
            Resolved::Color(c) => *c,
            Resolved::Enum { id, .. } | Resolved::Text(id) => {
                id.parse::<Rgba8>().map_err(|_| leaf_err("color"))?
            }
            _ => return Err(leaf_err("color")),
        };
        Ok(())
    }
}

impl PatchField for ColorRef {
    fn patch(&mut self, path: &[Seg], value: &Resolved, _integer: bool) -> Result<(), String> {
        if !path.is_empty() {
            return Err(leaf_err("color"));
        }
        *self = match value {
            Resolved::Color(c) => ColorRef::Rgba(*c),
            Resolved::Enum { id, .. } | Resolved::Text(id) => {
                if let Some(role) = ColorRole::parse(id.trim()) {
                    ColorRef::Role(role)
                } else {
                    ColorRef::Rgba(id.parse::<Rgba8>().map_err(|_| leaf_err("color"))?)
                }
            }
            _ => return Err(leaf_err("color")),
        };
        Ok(())
    }
}

impl PatchField for Len {
    fn patch(&mut self, path: &[Seg], value: &Resolved, integer: bool) -> Result<(), String> {
        if !path.is_empty() {
            return Err(leaf_err("length"));
        }
        *self = match value {
            Resolved::Number(v) => Len::Px(if integer { v.round() } else { *v }),
            Resolved::Enum { id, .. } | Resolved::Text(id) => id
                .trim()
                .strip_suffix('%')
                .and_then(|n| n.trim().parse::<f32>().ok())
                .filter(|n| n.is_finite())
                .map(Len::Percent)
                .ok_or_else(|| leaf_err("length"))?,
            _ => return Err(leaf_err("length")),
        };
        Ok(())
    }
}

macro_rules! impl_patch_enum {
    ($($ty:ty),* $(,)?) => {
        $(
            impl PatchField for $ty {
                fn patch(
                    &mut self,
                    path: &[Seg],
                    value: &Resolved,
                    _integer: bool,
                ) -> Result<(), String> {
                    if !path.is_empty() {
                        return Err(leaf_err(stringify!($ty)));
                    }
                    *self = de_enum(value, stringify!($ty))?;
                    Ok(())
                }
            }
        )*
    };
}

impl_patch_enum!(
    Appearance,
    Anchor,
    GradientKind,
    BorderStyleSpec,
    ContourPreset,
    MotifPreset,
    Edge,
    DecorLayer,
    DecorWhen,
    FontRole,
    DecorMotionKind,
    ContinueMotion,
    TextAlign,
    Side,
    TransitionScope,
    AvatarShape,
    HistoryEntrance,
    HistoryEase,
    HistoryAlign,
    HistoryLayout,
    HistoryEventLayout,
    EventPriceColor,
    nanatype_model::EntranceKind,
    nanatype_model::Overflow,
    ShaderTarget,
    ShaderActive,
);

impl<T: PatchField> PatchField for Option<T> {
    fn patch(&mut self, path: &[Seg], value: &Resolved, integer: bool) -> Result<(), String> {
        self.as_mut()
            .ok_or_else(|| "missing optional field".to_owned())?
            .patch(path, value, integer)
    }
}

impl PatchField for SlotBinding {
    fn patch(&mut self, _path: &[Seg], _value: &Resolved, _integer: bool) -> Result<(), String> {
        Err("shader bindings cannot be param sites".into())
    }
}

impl<T: PatchField> PatchField for Box<T> {
    fn patch(&mut self, path: &[Seg], value: &Resolved, integer: bool) -> Result<(), String> {
        (**self).patch(path, value, integer)
    }
}

impl<T: PatchField> PatchField for Vec<T> {
    fn patch(&mut self, path: &[Seg], value: &Resolved, integer: bool) -> Result<(), String> {
        let Some((Seg::Index(i), rest)) = path.split_first() else {
            return Err("expected array index".into());
        };
        self.get_mut(*i)
            .ok_or_else(|| "array index out of bounds".to_owned())?
            .patch(rest, value, integer)
    }
}

impl<T: PatchField, const N: usize> PatchField for [T; N] {
    fn patch(&mut self, path: &[Seg], value: &Resolved, integer: bool) -> Result<(), String> {
        let Some((Seg::Index(i), rest)) = path.split_first() else {
            return Err("expected array index".into());
        };
        self.get_mut(*i)
            .ok_or_else(|| "array index out of bounds".to_owned())?
            .patch(rest, value, integer)
    }
}

impl<A: PatchField, B: PatchField> PatchField for (A, B) {
    fn patch(&mut self, path: &[Seg], value: &Resolved, integer: bool) -> Result<(), String> {
        let Some((Seg::Index(i), rest)) = path.split_first() else {
            return Err("expected tuple index".into());
        };
        match i {
            0 => self.0.patch(rest, value, integer),
            1 => self.1.patch(rest, value, integer),
            _ => Err("tuple index out of bounds".into()),
        }
    }
}

impl<T: PatchField> PatchField for std::collections::BTreeMap<String, T> {
    fn patch(&mut self, path: &[Seg], value: &Resolved, integer: bool) -> Result<(), String> {
        let Some((Seg::Key(k), rest)) = path.split_first() else {
            return Err("expected table key".into());
        };
        self.get_mut(k)
            .ok_or_else(|| format!("unknown key `{k}`"))?
            .patch(rest, value, integer)
    }
}

macro_rules! impl_patch_struct {
    ($ty:ty { $($key:literal => $field:ident),* $(,)? }) => {
        impl PatchField for $ty {
            fn patch(&mut self, path: &[Seg], value: &Resolved, integer: bool) -> Result<(), String> {
                let Some((Seg::Key(k), rest)) = path.split_first() else {
                    return Err(concat!("expected key on ", stringify!($ty)).into());
                };
                match k.as_str() {
                    $($key => self.$field.patch(rest, value, integer),)*
                    _ => Err(format!("unknown field `{k}`")),
                }
            }
        }
    };
    ($ty:ty, flatten: $flat:ident { $($key:literal => $field:ident),* $(,)? }) => {
        impl PatchField for $ty {
            fn patch(&mut self, path: &[Seg], value: &Resolved, integer: bool) -> Result<(), String> {
                let Some((Seg::Key(k), rest)) = path.split_first() else {
                    return Err(concat!("expected key on ", stringify!($ty)).into());
                };
                match k.as_str() {
                    $($key => self.$field.patch(rest, value, integer),)*
                    _ => self.$flat.patch(path, value, integer),
                }
            }
        }
    };
}

impl_patch_struct!(Chrome {
    "appearance" => appearance,
    "accent" => accent,
    "surface" => surface,
    "text" => text,
});

impl_patch_struct!(NineSlice {
    "image" => image,
    "slice" => slice,
    "fill" => fill,
    "scale" => scale,
    "repeat" => repeat,
    "layout_inset" => layout_inset,
});

impl_patch_struct!(TextShadowSpec {
    "color" => color,
    "dx" => dx,
    "dy" => dy,
    "blur" => blur,
});

impl_patch_struct!(TextStrokeSpec {
    "color" => color,
    "width" => width,
});

impl_patch_struct!(GradientSpec {
    "kind" => kind,
    "angle" => angle,
    "center" => center,
    "circle" => circle,
    "stops" => stops,
});

impl_patch_struct!(ShadowSpec {
    "color" => color,
    "dx" => dx,
    "dy" => dy,
    "blur" => blur,
    "spread" => spread,
    "inset" => inset,
});

impl_patch_struct!(ShapeSpec {
    "material" => material,
    "border_material" => border_material,
    "contour" => contour,
    "gradient" => gradient,
    "radii" => radii,
    "border_widths" => border_widths,
    "border_style" => border_style,
    "shadows" => shadows,
});

impl_patch_struct!(TailSpec {
    "side" => side,
    "at" => at,
    "width" => width,
    "length" => length,
    "lean" => lean,
    "color" => color,
    "shadow" => shadow,
});

impl_patch_struct!(DecorMotionSpec {
    "kind" => kind,
    "period_s" => period_s,
    "amplitude" => amplitude,
    "phase" => phase,
});

impl_patch_struct!(DecorSpec, flatten: shape {
    "aspect_ratio" => aspect_ratio,
    "max_fraction" => max_fraction,
    "min_window" => min_window,
    "shader" => shader,
    "image" => image,
    "motif" => motif,
    "layer" => layer,
    "anchor" => anchor,
    "offset" => offset,
    "size" => size,
    "inset" => inset,
    "color" => color,
    "radius" => radius,
    "border_color" => border_color,
    "border_width" => border_width,
    "polygon" => polygon,
    "rotate" => rotate,
    "opacity" => opacity,
    "text" => text,
    "text_color" => text_color,
    "text_size" => text_size,
    "font" => font,
    "when" => when,
    "motion" => motion,
});

impl_patch_struct!(BoxSpec, flatten: shape {
    "color" => color,
    "border_color" => border_color,
    "border_width" => border_width,
    "inner_border_color" => inner_border_color,
    "inner_border_width" => inner_border_width,
    "radius" => radius,
    "glow" => glow,
    "glow_blur" => glow_blur,
    "shadow" => shadow,
    "frame" => frame,
    "width_fraction" => width_fraction,
    "margin" => margin,
});

impl_patch_struct!(TextSpec {
    "font_family" => font_family,
    "material" => material,
    "color" => color,
    "size" => size,
    "line_height" => line_height,
    "letter_spacing" => letter_spacing,
    "stroke" => stroke,
    "shadow" => shadow,
    "align" => align,
});

impl_patch_struct!(NameplateSpec, flatten: shape {
    "visible" => visible,
    "color" => color,
    "text_color" => text_color,
    "size" => size,
    "radius" => radius,
    "padding" => padding,
    "offset" => offset,
    "frame" => frame,
    "stroke" => stroke,
    "border_color" => border_color,
    "border_width" => border_width,
    "text_shadow" => text_shadow,
    "anchor" => anchor,
    "rotate" => rotate,
});

impl_patch_struct!(PortraitSpec {
    "visible" => visible,
    "side" => side,
    "size" => size,
    "offset" => offset,
    "image" => image,
    "variants" => variants,
    "radius" => radius,
});

impl_patch_struct!(ContinueSpec {
    "glyph" => glyph,
    "color" => color,
    "size" => size,
    "image" => image,
    "frames" => frames,
    "fps" => fps,
    "anchor" => anchor,
    "bob_px" => bob_px,
    "offset" => offset,
    "motion" => motion,
    "period_s" => period_s,
    "image_size" => image_size,
});

impl_patch_struct!(EntranceSpec {
    "material" => material,
    "scope" => scope,
    "kind" => kind,
    "ms" => ms,
});

impl_patch_struct!(ExitSpec {
    "material" => material,
    "scope" => scope,
    "ms" => ms,
});

impl_patch_struct!(ScrambleSpec {
    "alphabet" => alphabet,
    "interval_ms" => interval_ms,
    "entrance_ms" => entrance_ms,
    "exit_hold_ms" => exit_hold_ms,
    "exit_ms" => exit_ms,
});

impl_patch_struct!(TextLightSpec {
    "color" => color,
    "intensity" => intensity,
    "radius" => radius,
});

impl_patch_struct!(nanatype_playback::PunctPauses {
    "comma_ms" => comma_ms,
    "stop_ms" => stop_ms,
    "ellipsis_ms" => ellipsis_ms,
});

impl_patch_struct!(RevealSpec {
    "base_ms" => base_ms,
    "cjk_factor" => cjk_factor,
    "space_ms" => space_ms,
    "punct" => punct,
    "entrance" => entrance,
    "exit" => exit,
    "light" => light,
    "scramble" => scramble,
});

impl_patch_struct!(SweepSpec {
    "enabled" => enabled,
    "color" => color,
    "intensity" => intensity,
    "period_s" => period_s,
});

impl_patch_struct!(ReplySpec {
    "visible" => visible,
    "size" => size,
    "color" => color,
    "opacity" => opacity,
});

impl_patch_struct!(DialogueLayout {
    "padding" => padding,
    "box" => box_,
    "nameplate" => nameplate,
    "portrait" => portrait,
    "continue" => continue_,
    "decor" => decor,
    "shader_effects" => shader_effects,
});

impl_patch_struct!(DialogueLayoutRule {
    "max_width" => max_width,
    "max_height" => max_height,
    "portrait" => portrait,
    "layout" => layout,
});

impl_patch_struct!(ResponsiveDialogue {
    "landscape" => landscape,
    "portrait" => portrait,
    "rules" => rules,
});

impl_patch_struct!(Dialogue {
    "responsive" => responsive,
    "shader_effects" => shader_effects,
    "region" => region,
    "overscan" => overscan,
    "padding" => padding,
    "box" => box_,
    "text" => text,
    "nameplate" => nameplate,
    "portrait" => portrait,
    "continue" => continue_,
    "reveal" => reveal,
    "overflow" => overflow,
    "overflow_auto_ms" => overflow_auto_ms,
    "background" => background,
    "tail" => tail,
    "decor" => decor,
    "reply" => reply,
    "sweep" => sweep,
});

impl_patch_struct!(MentionSpec {
    "fill" => fill,
    "text_color" => text_color,
    "scale" => scale,
    "avatar" => avatar,
});

impl_patch_struct!(HistoryChatSpec {
    "text_scale" => text_scale,
    "bubble_opacity" => bubble_opacity,
});

impl_patch_struct!(HistoryEventBadge {
    "color" => color,
    "text_color" => text_color,
    "radius" => radius,
    "border_color" => border_color,
    "border_width" => border_width,
});

impl_patch_struct!(HistoryEventStyle, flatten: shape {
    "layout" => layout,
    "color" => color,
    "border_color" => border_color,
    "border_width" => border_width,
    "radius" => radius,
    "padding" => padding,
    "text_color" => text_color,
    "name_color" => name_color,
    "icon" => icon,
    "icon_color" => icon_color,
    "price_color" => price_color,
    "opacity" => opacity,
    "text_scale" => text_scale,
    "badge" => badge,
});

impl_patch_struct!(HistoryEvents, flatten: style {
    "gift" => gift,
    "super_chat" => super_chat,
    "member" => member,
});

impl_patch_struct!(HistoryAvatar {
    "shape" => shape,
    "size" => size,
    "ring_color" => ring_color,
    "ring_width" => ring_width,
    "color" => color,
    "radius" => radius,
});

impl_patch_struct!(HistorySeparator {
    "color" => color,
    "width" => width,
    "inset" => inset,
    "fade" => fade,
});

impl_patch_struct!(HistoryRecede {
    "step" => step,
    "floor" => floor,
});

impl_patch_struct!(HistoryNamePlate, flatten: shape {
    "color" => color,
    "text_color" => text_color,
    "radius" => radius,
    "padding" => padding,
    "border_color" => border_color,
    "border_width" => border_width,
});

impl_patch_struct!(HistoryPanel, flatten: shape {
    "color" => color,
    "border_color" => border_color,
    "border_width" => border_width,
    "radius" => radius,
    "padding" => padding,
    "margin" => margin,
});

impl_patch_struct!(HistoryTitle {
    "text" => text,
    "color" => color,
    "size" => size,
    "weight" => weight,
    "background" => background,
    "height" => height,
    "padding" => padding,
});

impl_patch_struct!(History {
    "group_speakers" => group_speakers,
    "padding" => padding,
    "margin" => margin,
    "row_padding" => row_padding,
    "column_inset" => column_inset,
    "width" => width,
    "height" => height,
    "avatar" => avatar,
    "name_color" => name_color,
    "name_size" => name_size,
    "name_stroke" => name_stroke,
    "bubble_color" => bubble_color,
    "bubble_border" => bubble_border,
    "bubble_radius" => bubble_radius,
    "bubble_padding" => bubble_padding,
    "bubble_frame" => bubble_frame,
    "bubble_material" => bubble_material,
    "bubble_contour" => bubble_contour,
    "text" => text,
    "gap" => gap,
    "entrance" => entrance,
    "entrance_ms" => entrance_ms,
    "entrance_ease" => entrance_ease,
    "time_color" => time_color,
    "bubble_gradient" => bubble_gradient,
    "bubble_radii" => bubble_radii,
    "bubble_border_width" => bubble_border_width,
    "bubble_shadows" => bubble_shadows,
    "name_plate" => name_plate,
    "layout" => layout,
    "name_width" => name_width,
    "name_weight" => name_weight,
    "time_size" => time_size,
    "time_seconds" => time_seconds,
    "panel" => panel,
    "title" => title,
    "decor" => decor,
    "hide_latest" => hide_latest,
    "chat" => chat,
    "event" => event,
    "reply" => reply,
    "name_format" => name_format,
    "align" => align,
    "separator" => separator,
    "recede" => recede,
});

impl_patch_struct!(EffectDefaults {
    "shake_px" => shake_px,
    "shake_hz" => shake_hz,
    "wave_px" => wave_px,
    "wave_period_s" => wave_period_s,
    "jump_px" => jump_px,
    "jump_period_s" => jump_period_s,
    "pulse_amount" => pulse_amount,
    "pulse_period_s" => pulse_period_s,
    "flicker_amount" => flicker_amount,
    "flicker_hz" => flicker_hz,
    "rainbow_period_s" => rainbow_period_s,
    "screen_shake_px" => screen_shake_px,
    "contrast_guard" => contrast_guard,
});

impl_patch_struct!(ShaderSpec {
    "source" => source,
    "period_s" => period_s,
    "intensity" => intensity,
    "textures" => textures,
    "params" => params,
    "static_fallback" => static_fallback,
    "animated" => animated,
    "bindings" => bindings,
    "owns_continue" => owns_continue,
    "active" => active,
    "cadence" => cadence,
});

impl_patch_struct!(ShaderEffect {
    "shader" => shader,
    "target" => target,
});

impl_patch_struct!(Skin {
    "chrome" => chrome,
    "dialogue" => dialogue,
    "history" => history,
    "effects" => effects,
    "mention" => mention,
    "shaders" => shaders,
});
