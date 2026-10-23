//! The one application theme every window installs: NanaUI's palette (its
//! blue accent), the app's radius steps, its shadows and motion. It never
//! reads the selected skin — only content boxes use skin colours.
//!
//! To change how the app looks, change it here: every window, every page
//! and every part in [`super`] reads these tokens.

use std::sync::Arc;

use nana_ui::CompiledTheme;
use nana_ui::theme::{SemanticColor, SemanticPalette, ThemeDefinition, ThemeId};
use nana_ui_core::{EffectTokens, ShadowToken, ThemeMetrics};

/// The palette controls and rich operator text share.
pub fn palette(dark: bool) -> SemanticPalette {
    if dark {
        SemanticPalette::dark()
    } else {
        SemanticPalette::light()
    }
}

/// Radius steps 4 / 8 / 12 / 16 / 20 (and 24). Swatches and keycaps use
/// `Xs`; controls, chips and rows `Sm`; popovers, menus and list cards `Md`;
/// settings cards `Lg`; floating rails `Xl`.
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

/// The lifted-surface shadow menus and popovers share: a soft
/// `0 8px 24px text@16%` on a light chrome, `0 10px 30px -12px #00000080`
/// on a dark one. Cards in a page are flat.
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

/// Compile the application theme for one appearance.
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
        .with_palette(palette(dark))
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
    fn the_quiet_text_roles_stay_readable() {
        for dark in [false, true] {
            let p = palette(dark);
            for bg in [p.surface, p.background, p.subtle] {
                let r = contrast(rgba8(p.muted), rgba8(bg));
                assert!(r >= 4.5, "muted on a {dark} chrome is {r:.2}:1");
            }
        }
    }
}
