//! Skin preparation and the independent, neutral application theme.
//! Only content boxes use skin colours; operator controls use NanaUI tokens.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use nana_ui::CompiledTheme;
use nana_ui::runtime::{BoxPaint, Gradient, PaintContext, PaintPath, PaintShadow, Painter};
use nana_ui::theme::{SemanticColor, SemanticPalette, ThemeDefinition, ThemeId};
use nana_ui_core::{
    EffectTokens, ElevationRole, RadiusTier, SemanticColorRole, ShadowToken, ThemeAppearance,
    ThemeMetrics,
};
use nanatype_model::Rgba8;
use nanatype_settings::{ChromeMode, SkinOverrides};
use nanatype_theme::{FontSpec, Skin};

/// The skin with the user's personalisation applied.
pub fn with_overrides(skin: &Skin, o: &SkinOverrides) -> Skin {
    let mut s = skin.clone();
    if let Some(c) = o.text_color {
        s.dialogue.text.color = c;
        s.history.text.color = c;
    }
    if let Some(c) = o.name_color {
        s.dialogue.nameplate.color = c;
        s.dialogue.nameplate.shape.gradient = None;
        s.history.name_color = c;
    }
    if let Some(c) = o.box_color {
        // A gradient would paint over the chosen colour.
        s.dialogue.box_.color = c;
        s.dialogue.box_.shape.gradient = None;
        s.dialogue.box_.shape.material = None;
        if s.has_dialogue_artwork() {
            // Full authored plates have fixed print colours, including their
            // baked fallback. An explicit personal box colour takes priority
            // and uses the skin's native contour, tail and external shadow.
            s.dialogue.box_.frame = None;
            s.dialogue
                .shader_effects
                .retain(|e| e.target != nanatype_theme::ShaderTarget::Artwork);
        }
    }
    if let Some(c) = o.accent {
        s.chrome.accent = c;
        s.dialogue.continue_.color = c;
    }
    if let Some(size) = o.text_size {
        s.dialogue.text.size = size;
    }
    if let Some(region) = o.region {
        s.dialogue.region = region;
    }
    if let Some(overflow) = o.overflow {
        s.dialogue.overflow = overflow;
    }
    if o.overflow_auto_ms.is_some() {
        s.dialogue.overflow_auto_ms = o.overflow_auto_ms;
    }
    s
}

/// The skin as drawn: the user's personalisation applied, and each font
/// role's family resolved. `bundled` are the families registered from the
/// skin's own font files (body, name, accent), which always come first.
/// `language` is the UI locale tag: roles the skin leaves at the default
/// stack take that language's system stack, and roles with their own stack
/// keep it first and fall back to the language's faces after it.
pub fn prepare(
    base: &Skin,
    overrides: &SkinOverrides,
    bundled: &[Option<String>; 3],
    language: &str,
) -> Skin {
    let mut skin = with_overrides(base, overrides);
    let locale_stack = FontSpec::locale_families(language);
    for (spec, family) in skin.fonts.roles_mut().into_iter().zip(bundled) {
        if spec.is_default_stack() {
            spec.families = locale_stack.clone();
        } else {
            for f in &locale_stack {
                if !spec.families.iter().any(|g| g.eq_ignore_ascii_case(f)) {
                    spec.families.push(f.clone());
                }
            }
        }
        if let Some(family) = family {
            // Registered just now, so the installed cache may not know it
            // yet; it always comes first.
            spec.families.insert(0, family.clone());
            continue;
        }
        // NanaUI takes a single family name, so the fallback list is resolved
        // here: the first installed family goes first.
        if let Some(i) = spec.families.iter().position(|f| installed(f))
            && i > 0
        {
            let family = spec.families.remove(i);
            spec.families.insert(0, family);
        }
    }
    skin
}

/// Whether the skin's dialogue text will be drawn in a stand-in font: it
/// names its own body families, none of them is available ([`available`]),
/// and it brings no font file this build can use (its own, or an embedded
/// `builtin:` one). Skins on the default system stack never are.
pub fn uses_fallback_font(skin: &Skin) -> bool {
    let body = &skin.fonts.body;
    if body.is_default_stack() || body.has_usable_file() {
        return false;
    }
    !body.families.iter().any(|f| available(f))
}

/// The families NanaUI embeds itself (its UI face), always there to shape
/// with whatever is installed.
pub const NANA_UI_FAMILIES: &[&str] = &[nana_ui_core::fonts::UI_FONT_FAMILY];

/// Whether text asked for in `family` is drawn in that family: one NanaUI
/// embeds ([`NANA_UI_FAMILIES`]), or one installed on this system.
pub fn available(family: &str) -> bool {
    NANA_UI_FAMILIES
        .iter()
        .any(|f| f.eq_ignore_ascii_case(family))
        || installed(family)
}

/// Whether the text engine shapes with `family` itself rather than a
/// fallback. Cached: the answer only changes when fonts are installed.
fn installed(family: &str) -> bool {
    static CACHE: LazyLock<Mutex<HashMap<String, bool>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    *cache.entry(family.to_owned()).or_insert_with(|| {
        nana_ui::shaped_face_families(family, "Aa字")
            .iter()
            .any(|f| f.eq_ignore_ascii_case(family))
    })
}

pub fn rgba(c: Rgba8) -> [f32; 4] {
    [
        f32::from(c.r) / 255.0,
        f32::from(c.g) / 255.0,
        f32::from(c.b) / 255.0,
        f32::from(c.a) / 255.0,
    ]
}

fn mix(a: Rgba8, b: Rgba8, t: f32) -> Rgba8 {
    let m = |x: u8, y: u8| (f32::from(x) * (1.0 - t) + f32::from(y) * t).round() as u8;
    Rgba8::rgba(m(a.r, b.r), m(a.g, b.g), m(a.b, b.b), m(a.a, b.a))
}

fn luminance(c: Rgba8) -> f32 {
    let lin = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
}

/// WCAG contrast ratio between two opaque colours.
pub fn contrast(a: Rgba8, b: Rgba8) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// The foreground that reads best on `bg`: the dark ink or white, whichever
/// has the higher contrast ratio. Mid-tone backgrounds where the soft ink
/// cannot reach 4.5:1 fall back to pure black, which always can.
pub fn on(bg: Rgba8) -> Rgba8 {
    let ink = Rgba8::rgb(16, 22, 32);
    let (dark, light) = (contrast(ink, bg), contrast(Rgba8::WHITE, bg));
    if dark < light {
        Rgba8::WHITE
    } else if dark >= 4.5 {
        ink
    } else {
        Rgba8::BLACK
    }
}

/// The accent as a foreground on `surface` (focus rings, soft-fill text,
/// slider and switch marks): the accent itself when it already reaches
/// `min`:1, otherwise mixed toward `text` just far enough that it does.
pub fn accent_fg(accent: Rgba8, surface: Rgba8, text: Rgba8, min: f32) -> Rgba8 {
    if contrast(accent, surface) >= min {
        return accent;
    }
    (1..=20)
        .map(|i| mix(accent, text, i as f32 / 20.0))
        .find(|c| contrast(*c, surface) >= min)
        .unwrap_or(text)
}

pub fn rgba8(c: SemanticColor) -> Rgba8 {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Rgba8::rgb(b(c.r), b(c.g), b(c.b))
}

pub fn dark_mode(mode: ChromeMode, system_dark: bool) -> bool {
    match mode {
        ChromeMode::System => system_dark,
        ChromeMode::Light => false,
        ChromeMode::Dark => true,
    }
}

/// CIE L* (0–100) of an opaque colour: the lightness step a reader sees.
pub fn lightness(c: Rgba8) -> f32 {
    let y = luminance(c);
    if y > 216.0 / 24389.0 {
        116.0 * y.cbrt() - 16.0
    } else {
        y * 24389.0 / 27.0
    }
}

/// How far a colour is from grey, 0–1 (sRGB channel spread).
pub fn chroma(c: Rgba8) -> f32 {
    let hi = c.r.max(c.g).max(c.b);
    let lo = c.r.min(c.g).min(c.b);
    f32::from(hi - lo) / 255.0
}

/// Standard NanaUI colours, shared by controls and rich operator text.
pub fn palette(dark: bool) -> SemanticPalette {
    if dark {
        SemanticPalette::dark()
    } else {
        SemanticPalette::light()
    }
}

/// Radius steps 4 / 8 / 12 / 16 / 20 (and 24): swatches and tags, controls,
/// popovers, cards, floating rails, large surfaces. Chips are pills.
fn metrics() -> ThemeMetrics {
    ThemeMetrics {
        radius_xs: 4.0,
        radius_sm: 8.0,
        radius_md: 12.0,
        radius_lg: 16.0,
        radius_xl: 20.0,
        radius_xxl: 24.0,
        ..nana_ui_core::UI_METRICS
    }
}

/// The lifted-surface shadow cards and menus share: a soft blue-tinted
/// `0 8px 24px text@16%` on a light chrome, `0 10px 30px -12px #00000080`
/// on a dark one.
fn effects(dark: bool) -> EffectTokens {
    let (base, surface) = if dark {
        let shadow = ShadowToken {
            color: SemanticColor::rgba8(0, 0, 0, 0.5),
            offset_x: 0.0,
            offset_y: 10.0,
            blur_radius: 30.0,
            spread_radius: -12.0,
            inset: false,
        };
        (EffectTokens::DARK, shadow)
    } else {
        let shadow = ShadowToken {
            color: SemanticColor {
                a: 0.16,
                ..palette(dark).text
            },
            offset_x: 0.0,
            offset_y: 8.0,
            blur_radius: 24.0,
            spread_radius: 0.0,
            inset: false,
        };
        (EffectTokens::LIGHT, shadow)
    };
    EffectTokens { surface, ..base }
}

/// Compile application chrome without consulting the selected skin.
pub fn compile(dark: bool) -> Arc<CompiledTheme> {
    let base = if dark {
        ThemeDefinition::NANA_DARK
    } else {
        ThemeDefinition::NANA_LIGHT
    };
    // Toasts arrive and leave on the app's motion tokens.
    let toast = nana_ui_core::ToastRecipe {
        motion: crate::ui::motion::toast(),
        ..base
            .components
            .toast
            .unwrap_or(nana_ui_core::ToastRecipe::DEFAULT)
    };
    let mut def = base
        .with_metrics(metrics())
        .with_effects(effects(dark))
        .with_toast(toast)
        .with_id(ThemeId::new(if dark {
            "nanatype.ui.dark"
        } else {
            "nanatype.ui.light"
        }))
        .with_display_name("NanaType");
    def.motion.hover_color_ms = crate::ui::motion::HOVER_MS as u16;
    Arc::new(def.compile().expect("built-in application theme is valid"))
}

/// `rgba` with its alpha replaced.
fn alpha(rgba: [f32; 4], a: f32) -> [f32; 4] {
    [rgba[0], rgba[1], rgba[2], a]
}

/// A settings card: the surface at the card radius on the theme's lifted
/// shadow. Light chromes drop the outline for the soft blue-tinted shadow;
/// dark chromes keep a 1px soft border, since a shadow barely reads on dark.
pub struct CardPainter;

impl Painter for CardPainter {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        let rect = cx.bounds();
        let r = cx.radius(RadiusTier::Lg);
        let mut card = BoxPaint::fill(SemanticColorRole::Surface).shadow(ElevationRole::Surface);
        if cx.theme_appearance() == ThemeAppearance::Dark {
            card = card.border(SemanticColorRole::BorderSoft, 1.0);
        }
        cx.rounded_rect(rect, r, card);
    }

    fn paint_key(&self) -> u64 {
        0
    }
}

/// The right end of a call to action's fill (accent → accent.light): up to
/// 22% toward white, as far as the label keeps 4.5:1 on it.
fn accent_light(accent: [f32; 4]) -> [f32; 4] {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let base = Rgba8::rgb(q(accent[0]), q(accent[1]), q(accent[2]));
    let label = on(base);
    let end = (0..=11)
        .rev()
        .map(|i| mix(base, Rgba8::WHITE, i as f32 * 0.02))
        .find(|c| contrast(label, *c) >= 4.5)
        .unwrap_or(base);
    alpha(rgba(end), accent[3])
}

/// The view's one primary action: a solid accent fill lightening to the
/// right (accent → accent.light at 90°) on an accent-tinted
/// `0 3px 8px accent@33%` shadow, its label in the on-accent colour.
pub struct CtaPainter;

impl Painter for CtaPainter {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        let state = cx.state();
        if state.disabled {
            cx.draw_default();
            return;
        }
        let rect = cx.bounds();
        let r = cx.radius(RadiusTier::Sm);
        let mut path = PaintPath::new();
        path.rounded_rect(rect, [r; 4]);
        let role = if state.hovered || state.pressed {
            SemanticColorRole::AccentStrong
        } else {
            SemanticColorRole::Accent
        };
        let accent = cx.color(role);
        let dark = cx.theme_appearance() == ThemeAppearance::Dark;
        cx.shadow(
            &path,
            PaintShadow::Custom {
                color: alpha(accent, if dark { 0.25 } else { 0.33 }).into(),
                offset: [0.0, 3.0],
                blur: if dark { 10.0 } else { 8.0 },
                spread: 0.0,
                inset: false,
            },
        );
        let fill = Gradient::linear([rect.x, rect.y], [rect.x + rect.width, rect.y])
            .stop(0.0, accent)
            .stop(1.0, accent_light(accent));
        cx.fill_path(&path, fill);
        // The framework's own look over it: the label and the focus ring
        // (the node itself has no fill of its own).
        cx.draw_default();
    }

    fn paint_key(&self) -> u64 {
        1
    }
}

/// `button` as the view's call to action: [`CtaPainter`] over a solid accent
/// fill, its label in the on-accent colour.
pub fn cta(button: nana_ui::runtime::Button) -> nana_ui::runtime::Button {
    let mut st = button.style.clone();
    // The painter draws the fill, so the node has none of its own (a
    // disabled button shows the plain primary tint instead).
    st.foreground = Some(SemanticColorRole::AccentText);
    st.background = None;
    st.border = None;
    st.interaction.disabled.background = Some(SemanticColorRole::AccentSoft);
    st.painter = Some(CtaPainter.into());
    button.style(st)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nanatype_theme::{SkinOrigin, SkinRegistry};

    #[test]
    fn a_skin_is_flagged_only_when_none_of_its_body_fonts_is_available() {
        let mut skin = Skin::default();
        // The default system stack is never flagged.
        assert!(!uses_fallback_font(&skin));
        skin.fonts.body.families = vec!["No Such Font 7f3a".into(), "Nor This 7f3b".into()];
        assert!(uses_fallback_font(&skin));
        // A built-in font this build does not embed still falls back.
        skin.fonts.body.file = Some("builtin:fonts/NotInThisBuild.ttf".into());
        assert!(uses_fallback_font(&skin));
        // The skin's own file is always used.
        skin.fonts.body.file = Some("fonts/own.ttf".into());
        assert!(!uses_fallback_font(&skin));
        // Installed: NanaUI registers Noto Sans SC itself.
        skin.fonts.body.file = None;
        skin.fonts.body.families = vec!["Noto Sans SC".into()];
        assert!(!uses_fallback_font(&skin));
        // A later family that is available is enough: it is what the text
        // is drawn in, not a stand-in.
        skin.fonts.body.families = vec!["No Such Font 7f3a".into(), "noto sans sc".into()];
        assert!(!uses_fallback_font(&skin));
    }

    #[test]
    fn nana_ui_embedded_families_are_always_available() {
        for f in NANA_UI_FAMILIES {
            assert!(available(f), "{f}");
        }
        assert!(!available("No Such Font 7f3a"));
    }

    #[test]
    fn echo_skins_are_not_flagged() {
        // Their stacks end in NanaUI's own Noto Sans SC, so their text never
        // shows in a stand-in, whatever is installed.
        let reg = SkinRegistry::new(None, "test");
        for id in [
            "nanatype.echo_vanilla",
            "nanatype.echo_glass",
            "nanatype.echo_bubble",
            "nanatype.echo_void",
        ] {
            let skin = reg.load(id).expect("builtin skin loads").skin;
            assert!(
                skin.fonts
                    .body
                    .families
                    .iter()
                    .any(|f| NANA_UI_FAMILIES.iter().any(|n| n.eq_ignore_ascii_case(f))),
                "{id} lists a NanaUI family"
            );
            assert!(!uses_fallback_font(&skin), "{id}");
        }
    }

    #[test]
    fn a_registered_bundled_family_stays_first() {
        let mut skin = Skin::default();
        skin.fonts.body.families = vec!["Noto Sans SC".into(), "SimSun".into()];
        let bundled = [Some("Bundled Face 7f3a".to_owned()), None, None];
        let prepared = prepare(&skin, &SkinOverrides::default(), &bundled, "zh-CN");
        // Not moved behind the installed Noto Sans SC.
        assert_eq!(
            prepared.fonts.body.families[..2],
            ["Bundled Face 7f3a", "Noto Sans SC"]
        );
    }

    #[test]
    fn on_accent_text_reaches_4_5_for_every_builtin_skin() {
        let reg = SkinRegistry::new(None, "test");
        let mut n = 0;
        for e in reg
            .entries()
            .iter()
            .filter(|e| matches!(e.origin, SkinOrigin::Builtin))
        {
            let skin = reg.load(&e.id).expect("builtin skin loads").skin;
            let accent = skin.chrome.accent;
            let r = contrast(on(accent), accent);
            assert!(r >= 4.5, "{}: on(accent) is {r:.2}:1", e.id);
            n += 1;
        }
        assert_eq!(n, 25);
    }

    #[test]
    fn builtin_content_colors_are_readable() {
        let reg = SkinRegistry::new(None, "test");
        for e in reg.entries() {
            let skin = reg.load(&e.id).expect("skin loads").skin;
            let c = &skin.chrome;
            let r = contrast(c.text, c.surface);
            assert!(r >= 4.5, "{}: content text on surface is {r:.2}:1", e.id);
            for min in [3.0, 4.5] {
                let fg = accent_fg(c.accent, c.surface, c.text, min);
                let r = contrast(fg, c.surface);
                assert!(r >= min, "{}: accent_fg {r:.2}:1 < {min}", e.id);
            }
        }
    }

    #[test]
    fn new_streaming_nameplates_have_readable_foregrounds() {
        let reg = SkinRegistry::new(None, "test");
        for name in [
            "gothic",
            "celestial",
            "forest",
            "tidal",
            "lantern",
            "citrus",
        ] {
            let skin = reg.load(&format!("nanatype.{name}")).unwrap().skin;
            let p = &skin.dialogue.nameplate;
            assert!(
                contrast(p.text_color, p.color) >= 4.5,
                "{name}: nameplate contrast"
            );
            if let Some(gradient) = &p.shape.gradient {
                for (position, color) in &gradient.stops {
                    assert!(
                        contrast(p.text_color, skin.color(*color)) >= 4.5,
                        "{name}: nameplate gradient contrast at {position}"
                    );
                }
            }
        }
    }

    #[test]
    fn appearance_is_independent_of_skins() {
        for system_dark in [false, true] {
            assert_eq!(dark_mode(ChromeMode::System, system_dark), system_dark);
            assert!(!dark_mode(ChromeMode::Light, system_dark));
            assert!(dark_mode(ChromeMode::Dark, system_dark));
        }
        for dark in [false, true] {
            let p = palette(dark);
            assert!(contrast(rgba8(p.text), rgba8(p.surface)) >= 4.5);
            assert!(contrast(rgba8(p.text), rgba8(p.background)) >= 4.5);
            let compiled = compile(dark);
            assert_eq!(
                compiled.appearance(),
                if dark {
                    ThemeAppearance::Dark
                } else {
                    ThemeAppearance::Light
                }
            );
        }
    }
}
