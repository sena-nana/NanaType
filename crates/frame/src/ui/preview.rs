//! Isolated, scaled samples of a skin through the real dialogue, history and
//! input renderers. Samples use private signals: no live draft, history or
//! source is written.
use std::sync::Arc;

use nana_ui::runtime::Stack;
use nana_ui::runtime::view::{AnyView, IntoView, Signal, dynamic, widget};
use nana_ui_core::{
    BackgroundImage, LengthSpec, OverflowSpec, PointerEventsSpec, PositionSpec, TransformOrigin,
};
use nanatype_model::{HistoryItem, Rgba8, SourceId, SourceKind};
use nanatype_settings::{Settings, SkinPersonalization, SurfacePersonalization};
use nanatype_theme::Skin;

use super::controls::hint;
use super::kit::{gap, inset};
use super::{Stage, dialogue, history};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pane {
    pub font: String,
    pub size: String,
    pub line_height: String,
    pub color: String,
    pub width: String,
    pub height: String,
    pub text: String,
}

impl Pane {
    fn from_saved(p: &SurfacePersonalization, text: String) -> Self {
        Self {
            font: p.font_family.clone().unwrap_or_default(),
            size: p.text_size.map(|v| v.to_string()).unwrap_or_default(),
            line_height: p.line_height.map(|v| v.to_string()).unwrap_or_default(),
            color: p.text_color.map(|v| v.to_hex()).unwrap_or_default(),
            width: p.size.map(|v| v[0].to_string()).unwrap_or_default(),
            height: p.size.map(|v| v[1].to_string()).unwrap_or_default(),
            text,
        }
    }

    fn parse(&self, default_size: [f32; 2]) -> Result<SurfacePersonalization, ()> {
        let number = |s: &str| -> Result<Option<f32>, ()> {
            if s.trim().is_empty() {
                Ok(None)
            } else {
                s.trim().parse().map(Some).map_err(|_| ())
            }
        };
        let width = number(&self.width)?;
        let height = number(&self.height)?;
        let p = SurfacePersonalization {
            font_family: (!self.font.trim().is_empty()).then(|| self.font.trim().to_owned()),
            text_size: number(&self.size)?,
            line_height: number(&self.line_height)?,
            text_color: if self.color.trim().is_empty() {
                None
            } else {
                Some(self.color.trim().parse::<Rgba8>().map_err(|_| ())?)
            },
            size: if width.is_none() && height.is_none() {
                None
            } else {
                Some([
                    width.unwrap_or(default_size[0]),
                    height.unwrap_or(default_size[1]),
                ])
            },
        };
        p.valid().then_some(p).ok_or(())
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Draft {
    pub display: Pane,
    pub history: Pane,
}

#[derive(Debug, Clone, Copy)]
pub struct InvalidProfile;
impl std::fmt::Display for InvalidProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid preview font, dimensions or color")
    }
}
impl std::error::Error for InvalidProfile {}

impl Draft {
    /// The saved personalization of skin `id`, as fields. A legacy region
    /// override is padded by `overscan` (that skin's; the active skin's when
    /// `None`).
    pub fn load(settings: &Settings, id: &str, ui: Stage, overscan: Option<f32>) -> Self {
        let p = settings
            .skin
            .personalizations
            .get(id)
            .cloned()
            .unwrap_or_else(|| SkinPersonalization {
                display: SurfacePersonalization {
                    text_size: settings.skin.overrides.text_size,
                    text_color: settings.skin.overrides.text_color,
                    size: settings.overlays.display.size.or_else(|| {
                        settings.skin.overrides.region.map(|[w, h]| {
                            let pad = overscan
                                .unwrap_or_else(|| ui.skin.with_untracked(|s| s.dialogue.overscan))
                                * 2.0;
                            [w + pad, h + pad]
                        })
                    }),
                    ..Default::default()
                },
                history: SurfacePersonalization {
                    text_color: settings.skin.overrides.text_color,
                    size: settings.overlays.history.size,
                    ..Default::default()
                },
            });
        Self {
            display: Pane::from_saved(&p.display, ui.now_t("skin-edit-sample")),
            history: Pane::from_saved(&p.history, ui.now_t("skin-edit-history-sample")),
        }
    }

    pub fn pane(&self, history: bool) -> &Pane {
        if history {
            &self.history
        } else {
            &self.display
        }
    }
    pub fn pane_mut(&mut self, history: bool) -> &mut Pane {
        if history {
            &mut self.history
        } else {
            &mut self.display
        }
    }

    pub fn profile(&self, base: &Skin) -> Result<SkinPersonalization, InvalidProfile> {
        Ok(SkinPersonalization {
            display: self
                .display
                .parse(default_size(base, false))
                .map_err(|()| InvalidProfile)?,
            history: self
                .history
                .parse(default_size(base, true))
                .map_err(|()| InvalidProfile)?,
        })
    }
}

pub fn default_size(skin: &Skin, history: bool) -> [f32; 2] {
    if history {
        [skin.history.width, skin.history.height]
    } else {
        skin.dialogue
            .region
            .map(|v| v + skin.dialogue.overscan * 2.0)
    }
}

/// The unsaved fields of skin `id`, or its saved ones (see [`Draft::load`]).
pub fn draft(ui: Stage, id: &str, overscan: Option<f32>) -> Draft {
    ui.skin_drafts
        .with(|map| map.get(id).cloned())
        .unwrap_or_else(|| {
            ui.settings
                .with_untracked(|s| Draft::load(s, id, ui, overscan))
        })
}

pub fn change(ui: Stage, id: &str, overscan: Option<f32>, f: impl FnOnce(&mut Draft)) {
    let mut value = draft(ui, id, overscan);
    f(&mut value);
    ui.skin_drafts.update(|map| {
        map.insert(id.to_owned(), value);
    });
}

/// Whether the skin previews on screen have effects that move, so the window
/// showing them needs frames.
pub fn animating(ui: Stage, now: u64) -> bool {
    super::shader_effects::preview_animating(ui)
        || super::text_light::preview_animating(ui, now)
        || super::text_scramble::preview_animating(ui, now)
}

/// Which window a sample draws the skin in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Surface {
    #[default]
    Display,
    History,
    /// The input window's skin preview: the box around a private editor.
    Input,
}
impl Surface {
    pub fn of(history: bool) -> Self {
        if history {
            Self::History
        } else {
            Self::Display
        }
    }
}

/// Sample data uses private signals; no production draft, history or source is written.
pub fn sample(
    ui: Stage,
    base: &Skin,
    id: &str,
    draft: &Draft,
    surface: Surface,
    urls: Arc<std::collections::HashMap<String, String>>,
) -> Option<(AnyView, [f32; 2])> {
    let profile = draft.profile(base).ok()?;
    let history_mode = surface == Surface::History;
    let mut settings = (*ui.settings.get_untracked()).clone();
    settings.skin.id = id.to_owned();
    // The input window draws its box from the display profile, with the
    // skin preview on and the sample speaker in the name plate.
    settings.general.input_skin_preview = true;
    settings.general.speaker_name = ui.now_t("skins-preview-name");
    settings
        .skin
        .personalizations
        .insert(id.to_owned(), profile);
    settings.overlays.history.lock = nanatype_settings::Lock::Locked;
    settings.history.hide_latest = nanatype_settings::HideLatest::Never;
    settings.history.fade_after_s = None;
    settings.history.chat.enabled = true;
    settings.history.show_time = true;
    let skin = Arc::new(crate::skin_theme::prepare(
        &crate::skin_theme::personalized(base, &settings, id),
        &Default::default(),
        &Default::default(),
        ui.locale.get_untracked().tag(),
    ));
    let canvas = if history_mode {
        crate::canvas::Canvas::History
    } else {
        crate::canvas::Canvas::Display
    };
    let (w, h) = crate::canvas::size_of(canvas, &skin, &settings);
    let mut preview = Stage::new(Arc::new(settings), skin.clone(), ui.locale.get_untracked());
    preview.asset_urls.set(urls.clone());
    preview.reduced_motion = ui.reduced_motion;
    preview.now = ui.now;
    preview.shader_preview = ui.shader_preview;
    preview.live_preview = ui.live_preview;
    let view = if history_mode {
        let rows = draft
            .history
            .text
            .lines()
            .enumerate()
            .map(|(i, text)| {
                let doc = nanatype_markup::parse(text).doc;
                let item = HistoryItem {
                    source: SourceId::local(),
                    kind: SourceKind::Typewriter,
                    line: None,
                    author: if i % 2 == 0 {
                        ui.now_t("skins-preview-name")
                    } else {
                        ui.now_t("skin-edit-long-name")
                    },
                    avatar: None,
                    doc: doc.clone(),
                    at: 1_700_000_000_000 + i as u64 * 180_000,
                    uid: None,
                    reply: None,
                };
                nanatype_core::HistoryRow {
                    key: i as u64,
                    item,
                    doc: Arc::new(doc),
                    reveal: None,
                }
            })
            .collect();
        preview.history.set(Arc::new(rows));
        history::history_window(preview).into_any()
    } else if surface == Surface::Input {
        preview
            .draft
            .set(Arc::new(nanatype_markup::parse(&draft.display.text).doc));
        widget(
            Stack::column(0.0)
                .width(LengthSpec::Px(w))
                .height(LengthSpec::Px(h)),
        )
        .key("skin-preview-input")
        .children(dialogue::dialogue_box(
            preview,
            dialogue::Source::Draft,
            Some(super::editor_view::preview_editor(preview)),
        ))
        .into_any()
    } else {
        let ctx = dialogue::BoxCtx {
            skin,
            urls,
            source: dialogue::Source::Preview,
            preview_name: ui.now_t("skins-preview-name"),
        };
        dialogue::preview_box(
            preview,
            ctx,
            &nanatype_markup::parse(&draft.display.text).doc,
        )
    };
    Some((view, [w, h]))
}

/// An isolated, scaled sample of `raw` through the real dialogue and history
/// renderers. Fonts come from `read_font`; nothing touches production state.
pub fn stage(
    ui: Stage,
    mut raw: Skin,
    read_font: impl Fn(&str) -> Option<Vec<u8>>,
    urls: Arc<std::collections::HashMap<String, String>>,
    surface: Surface,
    width: Signal<f32>,
) -> (AnyView, Arc<Skin>) {
    let mut scope = nana_ui::HostFontScope::new();
    for font in nanatype_theme::BuiltinFonts::embedded().fonts() {
        let _ = scope.add_bytes(font.bytes);
    }
    let mut register = |role: &nanatype_theme::FontSpec| {
        let bytes = role.file.as_deref().and_then(|file| {
            if nanatype_theme::is_builtin_font(file) {
                nanatype_theme::builtin_font(file).map(|b| b.to_vec())
            } else {
                read_font(file)
            }
        });
        scope.add_bytes(bytes?).ok()?.families().first().cloned()
    };
    let families = [&raw.fonts.body, &raw.fonts.name, &raw.fonts.accent].map(&mut register);
    if let Some(role) = &mut raw.fonts.latin
        && let Some(family) = register(role)
    {
        role.families.insert(0, family);
    }
    let scope = Arc::new(scope);
    let base = Arc::new(crate::skin_theme::prepare(
        &raw,
        &Default::default(),
        &families,
        ui.locale.get_untracked().tag(),
    ));
    let sample_id = raw.meta.id.clone();
    let sample_overscan = base.dialogue.overscan;
    let sample_base = base.clone();
    let view = dynamic(
        move || {
            (
                draft(ui, &sample_id, Some(sample_overscan)),
                width.get(),
                ui.chrome_dark.get(),
                ui.shader_preview.get(),
                ui.settings.with(|s| s.skin.overrides.clone()),
            )
        },
        move |(draft, available, dark, _, _)| {
            let _keep_fonts = &scope;
            let Some((view, [w, h])) = sample(
                ui,
                &sample_base,
                &sample_base.meta.id,
                draft,
                surface,
                urls.clone(),
            ) else {
                return hint(ui.now_t("skin-edit-invalid"));
            };
            // Limit tall previews as well as wide ones, without altering their layout.
            let dark = *dark;
            let scale = ((available - 24.0).max(1.0) / w).min(340.0 / h).min(1.0);
            let stage = widget(
                Stack::column(0.0)
                    .width(LengthSpec::Px(w * scale))
                    .height(LengthSpec::Px(h * scale))
                    .with_layout(|l| {
                        l.position = PositionSpec::Relative;
                        l.overflow_x = OverflowSpec::Hidden;
                        l.overflow_y = OverflowSpec::Hidden;
                    }),
            )
            .key("skin-preview-box")
            .children(
                widget(
                    Stack::column(0.0)
                        .width(LengthSpec::Px(w))
                        .height(LengthSpec::Px(h))
                        .with_layout(move |l| {
                            l.position = PositionSpec::Absolute;
                            l.offset_left = Some(LengthSpec::Px(0.0));
                            l.offset_top = Some(LengthSpec::Px(0.0));
                            l.pointer_events = Some(PointerEventsSpec::None);
                            l.transform = Some(super::surface::affine(0.0, 0.0, 0.0, scale));
                            l.transform_origin = Some(TransformOrigin {
                                x: LengthSpec::Px(0.0),
                                y: LengthSpec::Px(0.0),
                            });
                        }),
                )
                .key("skin-preview-canvas")
                .children(view),
            );
            widget(Stack::column(0.0).gap_step(gap::step::CONTROL))
                .children((
                    widget(
                        Stack::column(0.0)
                            .padding_step(inset::step::PANEL, inset::step::PANEL)
                            .with_layout(move |l| {
                                l.paint.background_image =
                                    Some(BackgroundImage::url(super::kit::checker_url(dark)));
                            }),
                    )
                    .children(stage),
                    hint(format!(
                        "{} × {} px · {:.0}%",
                        w.round(),
                        h.round(),
                        scale * 100.0
                    )),
                ))
                .into_any()
        },
    )
    .into_any();
    (view, base)
}
