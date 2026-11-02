//! The one application theme every window installs: NanaUI's palette (its
//! blue accent), the app's radius steps, its shadows and motion. It never
//! reads the selected skin — only content boxes use skin colours.
//!
//! [`Chrome`] picks the rendition: light or dark, in high contrast or not,
//! and the UI scale its controls are drawn at. Output windows always take
//! the plain one ([`Chrome::output`]).
//!
//! To change how the app looks, change it here: every window, every page
//! and every part in [`super`] reads these tokens.

use std::sync::Arc;

use nana_ui::CompiledTheme;
use nana_ui::theme::{SemanticColor, SemanticPalette, ThemeDefinition, ThemeId};
use nana_ui_core::{EffectTokens, ShadowToken, ThemeMetrics};

/// How the application's own windows are painted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Chrome {
    pub dark: bool,
    /// Opaque black and white with the strongest accents (≥7:1 text).
    pub high_contrast: bool,
    /// Controls and spacing they size by, as a factor (1 = as designed).
    pub scale: f32,
}

impl Chrome {
    /// The plain rendition of `dark`: what output windows always take.
    pub const fn output(dark: bool) -> Self {
        Self {
            dark,
            high_contrast: false,
            scale: 1.0,
        }
    }
}

impl Chrome {
    /// The chrome `settings` ask for, given what the system reports.
    pub fn of(settings: &nanatype_settings::Settings, system_dark: bool, system_hc: bool) -> Self {
        Self {
            dark: crate::skin_theme::dark_mode(settings.skin.chrome, system_dark),
            high_contrast: settings.accessibility.high_contrast(system_hc),
            scale: settings.accessibility.scale(),
        }
    }
}

/// The app windows' theme for `chrome`, and the output windows' plain one
/// (the same `Arc` when they agree).
pub fn compile_both(chrome: Chrome) -> (Arc<CompiledTheme>, Arc<CompiledTheme>) {
    let output = compile(Chrome::output(chrome.dark));
    if chrome == Chrome::output(chrome.dark) {
        (output.clone(), output)
    } else {
        (compile(chrome), output)
    }
}

impl From<bool> for Chrome {
    fn from(dark: bool) -> Self {
        Self::output(dark)
    }
}

/// The palette controls and rich operator text share.
pub fn palette(chrome: impl Into<Chrome>) -> SemanticPalette {
    let chrome = chrome.into();
    match (chrome.dark, chrome.high_contrast) {
        (false, false) => SemanticPalette::light(),
        (true, false) => SemanticPalette::dark(),
        (dark, true) => high_contrast(dark),
    }
}

/// NanaUI's high-contrast palettes, every role opaque: white on black with
/// a yellow accent, or black on white with a deep blue one. The light faint
/// grey is a step darker than NanaUI's so it clears 7:1 on every surface.
fn high_contrast(dark: bool) -> SemanticPalette {
    if dark {
        SemanticPalette::high_contrast()
    } else {
        SemanticPalette {
            faint: SemanticColor::rgb8(72, 72, 72),
            ..SemanticPalette::high_contrast_light()
        }
    }
}

/// The fill behind selected text in a plain text field (the markup source).
/// NanaUI's default, `accent_soft`, is the accent at 10% (light) or 14%
/// (dark): on a field's background that is all but invisible. This is the
/// same accent, strong enough to stand off the field in either appearance
/// (1.5:1) while the text over it stays readable (4.5:1). Dark stops short of
/// light's alpha: past 0.40 the lighter fill starts to wash out the text.
///
/// In high contrast it is the palette's solid selected fill instead.
pub fn selection(chrome: impl Into<Chrome>) -> [f32; 4] {
    let chrome = chrome.into();
    let p = palette(chrome);
    if chrome.high_contrast {
        return p.selected.as_rgba_array();
    }
    SemanticColor {
        a: if chrome.dark { 0.40 } else { 0.45 },
        ..p.accent
    }
    .as_rgba_array()
}

/// Radius steps 4 / 8 / 12 / 16 / 20 (and 24). Swatches and keycaps use
/// `Xs`; controls, chips and rows `Sm`; popovers, menus and list cards `Md`;
/// settings cards `Lg`; floating rails `Xl`. Control heights, paddings and
/// the switch grow with `scale`, so scaled text keeps fitting its control;
/// radii stay.
fn metrics(scale: f32) -> ThemeMetrics {
    let base = nana_ui_core::UI_METRICS;
    let s = |v: f32| (v * scale).round();
    ThemeMetrics {
        radius_xs: 4.0,
        radius_sm: 8.0,
        radius_md: 12.0,
        radius_lg: 16.0,
        radius_xl: 20.0,
        radius_xxl: 24.0,
        compact_control_height: s(base.compact_control_height),
        control_height: s(base.control_height),
        compact_control_padding_x: s(base.compact_control_padding_x),
        control_padding_x: s(base.control_padding_x),
        selection_height: s(base.selection_height),
        icon_button_size: s(base.icon_button_size),
        field_padding_x: s(base.field_padding_x),
        list_item_padding_x: s(base.list_item_padding_x),
        large_control_padding_x: s(base.large_control_padding_x),
        switch: nana_ui_core::SwitchMetrics {
            track_width: s(base.switch.track_width),
            track_height: s(base.switch.track_height),
            label_gap: s(base.switch.label_gap),
        },
        ..base
    }
}

/// The lifted-surface shadow menus and popovers share: a soft
/// `0 8px 24px text@16%` on a light chrome, `0 10px 30px -12px #00000080`
/// on a dark one. Cards in a page are flat.
fn effects(chrome: Chrome) -> EffectTokens {
    let dark = chrome.dark;
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
                ..palette(chrome).text
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

/// Compile the application theme for one rendition (a `bool` is the plain
/// light or dark one).
pub fn compile(chrome: impl Into<Chrome>) -> Arc<CompiledTheme> {
    let chrome = chrome.into();
    let dark = chrome.dark;
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
        .with_palette(palette(chrome))
        .with_metrics(metrics(chrome.scale))
        .with_effects(effects(chrome))
        .with_toast(toast)
        .with_id(ThemeId::new(match (dark, chrome.high_contrast) {
            (false, false) => "nanatype.ui.light",
            (true, false) => "nanatype.ui.dark",
            (false, true) => "nanatype.ui.light.high-contrast",
            (true, true) => "nanatype.ui.dark.high-contrast",
        }))
        .with_display_name("NanaType");
    def.motion.hover_color_ms = crate::ui::motion::HOVER_MS as u16;
    if chrome.high_contrast {
        // Outlines and rules twice as heavy, and a title bar that matches
        // the opaque background rather than a tinted strip.
        def.tokens.border.hairline = 2.0;
        def.tokens.titlebar = Some(def.tokens.palette.background);
    }
    Arc::new(def.compile().expect("built-in application theme is valid"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skin_theme::{contrast, dark_mode, rgba8};
    use nana_ui_core::ThemeAppearance;
    use nanatype_settings::ChromeMode;

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

    #[test]
    fn the_compiled_theme_carries_the_app_palette_and_steps() {
        for dark in [false, true] {
            let compiled = compile(dark);
            assert_eq!(compiled.palette(), palette(dark));
            assert_eq!(compiled.metrics().radius_sm, 8.0);
            assert_eq!(compiled.metrics().radius_lg, 16.0);
        }
    }

    #[test]
    fn a_text_selection_stands_off_the_field_and_keeps_its_text_readable() {
        let over = |fill: [f32; 4], bg: SemanticColor| SemanticColor {
            r: fill[0] * fill[3] + bg.r * (1.0 - fill[3]),
            g: fill[1] * fill[3] + bg.g * (1.0 - fill[3]),
            b: fill[2] * fill[3] + bg.b * (1.0 - fill[3]),
            a: 1.0,
        };
        for dark in [false, true] {
            let p = palette(dark);
            // A field rests on `background` and deepens toward `surface`.
            for bg in [p.background, p.surface, p.subtle] {
                let selected = over(selection(dark), bg);
                let stand_off = contrast(rgba8(selected), rgba8(bg));
                assert!(stand_off >= 1.5, "{dark}: selection {stand_off:.2}:1");
                let text = contrast(rgba8(p.text), rgba8(selected));
                assert!(text >= 4.5, "{dark}: selected text {text:.2}:1");
                // NanaUI's default fill, for the record of what this fixes.
                let soft = over(p.accent_soft.as_rgba_array(), bg);
                assert!(contrast(rgba8(soft), rgba8(bg)) < stand_off);
            }
        }
    }

    #[test]
    fn the_quiet_text_roles_stay_readable() {
        for dark in [false, true] {
            let p = palette(dark);
            for bg in [p.surface, p.background, p.subtle] {
                let r = contrast(rgba8(p.muted), rgba8(bg));
                assert!(r >= 4.5, "muted on a {dark} chrome is {r:.2}:1");
            }
        }
    }

    #[test]
    fn high_contrast_reaches_seven_to_one_and_paints_nothing_translucent() {
        for dark in [false, true] {
            let chrome = Chrome {
                dark,
                high_contrast: true,
                scale: 1.0,
            };
            let p = palette(chrome);
            let ratio = |a: SemanticColor, b: SemanticColor| contrast(rgba8(a), rgba8(b));
            let roles = [
                p.background,
                p.surface,
                p.subtle,
                p.raised,
                p.header,
                p.hover,
                p.active,
                p.selected,
                p.selected_hover,
                p.selected_pressed,
                p.border,
                p.border_soft,
                p.border_strong,
                p.text,
                p.muted,
                p.faint,
                p.accent,
                p.accent_strong,
                p.accent_soft,
                p.accent_soft_hover,
                p.accent_soft_pressed,
                p.accent_on_soft,
                p.accent_text,
                p.focus_surface,
                p.focus_border,
                p.focus_text,
                p.highlight,
                p.highlight_text,
                p.success,
                p.warning,
                p.danger,
            ];
            assert!(
                roles.iter().all(|c| c.a == 1.0),
                "{dark}: a role is translucent"
            );
            let surfaces = [p.background, p.surface, p.subtle, p.raised, p.header];
            for bg in surfaces {
                for (name, fg) in [("text", p.text), ("muted", p.muted), ("faint", p.faint)] {
                    let r = ratio(fg, bg);
                    assert!(r >= 7.0, "{dark}: {name} is {r:.2}:1");
                }
                // Outlines, rules, the focus indicator and control marks.
                for (name, c) in [
                    ("border", p.border),
                    ("border_soft", p.border_soft),
                    ("border_strong", p.border_strong),
                    ("focus_border", p.focus_border),
                    ("focus_surface", p.focus_surface),
                    ("accent", p.accent),
                ] {
                    let r = ratio(c, bg);
                    assert!(r >= 3.0, "{dark}: {name} is {r:.2}:1");
                }
            }
            for (name, fg, bg) in [
                ("text on hover", p.text, p.hover),
                ("text on pressed", p.text, p.active),
                ("text on selected", p.text, p.selected),
                ("text on selected hover", p.text, p.selected_hover),
                ("accent text", p.accent_text, p.accent),
                ("accent text, strong", p.accent_text, p.accent_strong),
                ("soft accent text", p.accent_on_soft, p.accent_soft),
                ("focus text", p.focus_text, p.focus_surface),
                ("highlight text", p.highlight_text, p.highlight),
            ] {
                let r = ratio(fg, bg);
                assert!(r >= 7.0, "{dark}: {name} is {r:.2}:1");
            }
            // Selected text sits on the solid selected fill.
            let sel = selection(chrome);
            assert_eq!(sel[3], 1.0);
            let sel = SemanticColor::rgba(sel[0], sel[1], sel[2], 1.0);
            assert!(ratio(p.text, sel) >= 7.0);
            let compiled = compile(chrome);
            assert_eq!(compiled.palette(), p);
            assert_ne!(compiled.palette(), compile(dark).palette());
        }
    }

    #[test]
    fn the_ui_scale_grows_controls_and_keeps_radii() {
        let base = compile(false);
        let big = compile(Chrome {
            dark: false,
            high_contrast: false,
            scale: 2.0,
        });
        let (b, m) = (base.metrics(), big.metrics());
        assert_eq!(m.control_height, b.control_height * 2.0);
        assert_eq!(m.icon_button_size, b.icon_button_size * 2.0);
        assert_eq!(m.switch.track_width, b.switch.track_width * 2.0);
        assert_eq!(m.radius_sm, b.radius_sm);
        // Output windows never take either.
        assert_eq!(Chrome::from(true), Chrome::output(true));
        assert_eq!(*compile(Chrome::output(false)), *base);
    }
}
