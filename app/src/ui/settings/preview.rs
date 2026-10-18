//! Isolated, unsaved skin editing. Only Save dispatches an application message.
use std::sync::Arc;

use nana_ui::runtime::view::{
    AnyView, IntoView, Signal, dynamic, segmented_option, signal, widget,
};
use nana_ui::runtime::{
    Activate, Button, SegmentedControl, SegmentedOptionChosen, Select, SelectChanged, SelectOption,
    SizeChanged, Stack, TextArea, TextChanged, TextInput, TextInputState,
};
use nana_ui::{ButtonKind, ControlSize};
use nana_ui_core::{
    AlignSpec, BackgroundImage, LengthSpec, OverflowSpec, PointerEventsSpec, PositionSpec,
    TransformOrigin,
};
use nanatype_model::{HistoryItem, Rgba8, SourceId, SourceKind};
use nanatype_settings::{Settings, SkinPersonalization, SurfacePersonalization};
use nanatype_theme::Skin;

use super::controls::{hint, label};
use crate::msg::Msg;
use crate::ui::{Ui, dialogue, history};

/// Both skin catalogue layouts prepare the same isolated preview on selection.
pub(super) fn select(
    ui: Ui,
    previewing: Signal<Option<String>>,
    row: &crate::ui::SkinRow,
) -> Option<Msg> {
    if row.error.is_some() {
        return None;
    }
    previewing.set(Some(row.id.clone()));
    let play = ui.shader_preview.get_untracked();
    let static_materials = row
        .skin
        .as_ref()
        .is_some_and(|s| !s.materials.is_empty() || s.shaders.values().any(|s| s.static_fallback));
    (play || static_materials).then(|| Msg::PrepareSkinPreview {
        id: row.id.clone(),
        play,
    })
}

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

impl Draft {
    pub fn load(settings: &Settings, id: &str, ui: Ui) -> Self {
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
                            let pad = ui
                                .skins
                                .with_untracked(|rows| {
                                    rows.iter()
                                        .find(|r| r.id == id)
                                        .and_then(|r| r.skin.as_ref())
                                        .map(|s| s.dialogue.overscan)
                                })
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

    fn pane(&self, history: bool) -> &Pane {
        if history {
            &self.history
        } else {
            &self.display
        }
    }
    fn pane_mut(&mut self, history: bool) -> &mut Pane {
        if history {
            &mut self.history
        } else {
            &mut self.display
        }
    }

    pub fn profile(&self, base: &Skin) -> Result<SkinPersonalization, ()> {
        Ok(SkinPersonalization {
            display: self.display.parse(default_size(base, false))?,
            history: self.history.parse(default_size(base, true))?,
        })
    }
}

fn default_size(skin: &Skin, history: bool) -> [f32; 2] {
    if history {
        [skin.history.width, skin.history.height]
    } else {
        skin.dialogue
            .region
            .map(|v| v + skin.dialogue.overscan * 2.0)
    }
}

fn draft(ui: Ui, id: &str) -> Draft {
    ui.skin_drafts
        .with(|map| map.get(id).cloned())
        .unwrap_or_else(|| ui.settings.with_untracked(|s| Draft::load(s, id, ui)))
}

fn change(ui: Ui, id: &str, f: impl FnOnce(&mut Draft)) {
    let mut value = draft(ui, id);
    f(&mut value);
    ui.skin_drafts.update(|map| {
        map.insert(id.to_owned(), value);
    });
}

#[derive(Clone, Copy)]
enum Field {
    Font,
    Size,
    Line,
    Color,
    Width,
    Height,
}
impl Field {
    fn value(self, p: &mut Pane) -> &mut String {
        match self {
            Self::Font => &mut p.font,
            Self::Size => &mut p.size,
            Self::Line => &mut p.line_height,
            Self::Color => &mut p.color,
            Self::Width => &mut p.width,
            Self::Height => &mut p.height,
        }
    }
    fn key(self) -> &'static str {
        match self {
            Self::Font => "skin-edit-font",
            Self::Size => "skin-edit-size",
            Self::Line => "skin-edit-line",
            Self::Color => "skin-edit-color",
            Self::Width => "skin-edit-width",
            Self::Height => "skin-edit-height",
        }
    }
}

fn field(ui: Ui, id: &str, history: bool, field: Field, placeholder: String) -> AnyView {
    let read_id = id.to_owned();
    let write_id = id.to_owned();
    let read = move || field.value(draft(ui, &read_id).pane_mut(history)).clone();
    widget(
        Stack::column(4.0)
            .width(LengthSpec::Percent(48.0))
            .with_layout(|l| {
                l.min_width = Some(LengthSpec::Px(0.0));
            }),
    )
    .children((
        hint(ui.now_t(field.key())),
        widget(
            TextInput::new(read())
                .placeholder(placeholder)
                .size(ControlSize::Small),
        )
        .key(field.key())
        .bind(move |input: &mut TextInput| {
            let text = read();
            if input.state.value.as_str() != text {
                input.state = TextInputState::new(text);
            }
        })
        .on(move |e: &TextChanged| {
            change(ui, &write_id, |d| {
                *field.value(d.pane_mut(history)) = e.value.to_string()
            });
        }),
    ))
    .into_any()
}

fn controls(ui: Ui, id: &str, base: Arc<Skin>, is_history: bool, fallback_font: bool) -> AnyView {
    let text = if is_history {
        &base.history.text
    } else {
        &base.dialogue.text
    };
    let dimensions = default_size(&base, is_history);
    let default = ui.now_t("common-skin-default");
    let fields = [
        (Field::Font, default.clone()),
        (Field::Size, format!("{} px", text.size)),
        (Field::Line, format!("{} ×", text.line_height)),
        (Field::Color, text.color.to_hex()),
        (Field::Width, dimensions[0].to_string()),
        (Field::Height, dimensions[1].to_string()),
    ]
    .into_iter()
    .map(|(field_, placeholder)| field(ui, id, is_history, field_, placeholder))
    .collect::<Vec<_>>();
    let mut fonts = vec![SelectOption::new("", default)];
    let mut families: Vec<String> = nanatype_theme::BuiltinFonts::embedded()
        .fonts()
        .iter()
        .filter_map(|f| f.family())
        .collect();
    families.extend(
        [
            "Microsoft YaHei UI",
            "SimSun",
            "Yu Gothic UI",
            "Meiryo UI",
            "PingFang SC",
            "Noto Sans SC",
            "Noto Sans JP",
            "Arial",
            "Consolas",
        ]
        .map(str::to_owned),
    );
    families.sort();
    families.dedup();
    fonts.extend(
        families
            .into_iter()
            .map(|f| SelectOption::new(f.clone(), f)),
    );
    let font_id = id.to_owned();
    let font_choices = widget(Stack::column(4.0).width(LengthSpec::Fill)).children((
        hint(ui.now_t("skin-edit-font-presets")),
        widget(
            Select::new(Some(String::new()))
                .options(fonts)
                .size(ControlSize::Small),
        )
        .on(move |e: &SelectChanged| {
            change(ui, &font_id, |d| {
                d.pane_mut(is_history).font = e.value.to_string()
            });
        }),
    ));
    let sample_id = id.to_owned();
    let write_sample_id = id.to_owned();
    let sample = widget(TextArea::new(draft(ui, id).pane(is_history).text.clone()).height(100.0))
        .key("skin-edit-sample-text")
        .bind(move |area: &mut TextArea| {
            let text = draft(ui, &sample_id).pane(is_history).text.clone();
            if area.state.value.as_str() != text {
                area.state = TextInputState::new(text);
            }
        })
        .on(move |e: &TextChanged| {
            change(ui, &write_sample_id, |d| {
                d.pane_mut(is_history).text = e.value.to_string()
            });
        });
    let sample_reset_id = id.to_owned();
    let reset_id = id.to_owned();
    let discard_id = id.to_owned();
    let save_id = id.to_owned();
    let valid_id = id.to_owned();
    let valid_base = base.clone();
    let save_base = base.clone();
    let status_id = id.to_owned();
    let status = dynamic(
        move || {
            (
                draft(ui, &status_id),
                ui.settings
                    .with(|s| s.skin.personalizations.get(&status_id).cloned()),
            )
        },
        move |(d, saved)| {
            let Ok(profile) = d.profile(&base) else {
                return hint(ui.now_t("skin-edit-invalid"));
            };
            let mut note = ui.now_t(if saved.as_ref() == Some(&profile) {
                "skin-edit-saved"
            } else {
                "skin-edit-draft-hint"
            });
            let font = &d.pane(is_history).font;
            let missing = if font.trim().is_empty() {
                fallback_font
            } else {
                !nana_ui::shaped_face_families(font, "Aa字")
                    .iter()
                    .any(|f| f.eq_ignore_ascii_case(font.trim()))
            };
            if missing {
                note = format!("{}\n{note}", ui.now_t("skin-edit-font-missing"));
            }
            hint(note)
        },
    );
    // Keep controls mounted while the sample rerenders, preserving IME/selection.
    widget(Stack::column(8.0).width(LengthSpec::Fill))
        .children((
            widget(Stack::row(8.0).wrap(true).width(LengthSpec::Fill)).children(fields),
            font_choices,
            label(ui.now_t("skin-edit-test-text")),
            sample,
            widget(Button::new(ui.now_t("skin-edit-sample-reset")).size(ControlSize::Small)).on(
                move |_: &Activate| {
                    change(ui, &sample_reset_id, |d| {
                        d.pane_mut(is_history).text = ui.now_t(if is_history {
                            "skin-edit-history-sample"
                        } else {
                            "skin-edit-sample"
                        });
                    })
                },
            ),
            status,
            widget(Stack::row(8.0).wrap(true)).children((
                widget(Button::new(ui.now_t("skin-edit-reset")).size(ControlSize::Small)).on(
                    move |_: &Activate| {
                        change(ui, &reset_id, |d| {
                            let text = d.pane(is_history).text.clone();
                            *d.pane_mut(is_history) = Pane {
                                text,
                                ..Default::default()
                            };
                        })
                    },
                ),
                widget(Button::new(ui.now_t("skin-edit-discard")).size(ControlSize::Small)).on(
                    move |_: &Activate| {
                        ui.skin_drafts.update(|m| {
                            m.remove(&discard_id);
                        });
                    },
                ),
                widget(
                    Button::new(ui.now_t("skin-edit-save"))
                        .kind(ButtonKind::Primary)
                        .size(ControlSize::Small),
                )
                .key("skin-edit-save")
                .bind(move |b: &mut Button| {
                    b.disabled = draft(ui, &valid_id).profile(&valid_base).is_err()
                })
                .on_cx(move |_, _: &Activate, cx| {
                    if let Ok(profile) = draft(ui, &save_id).profile(&save_base) {
                        cx.dispatch_program_all(Msg::SaveSkinPersonalization {
                            id: save_id.clone(),
                            profile,
                        });
                    }
                }),
            )),
        ))
        .into_any()
}

/// Sample data uses private signals; no production draft, history or source is written.
pub fn sample(
    ui: Ui,
    base: &Skin,
    id: &str,
    draft: &Draft,
    history_mode: bool,
    urls: Arc<std::collections::HashMap<String, String>>,
) -> Option<(AnyView, [f32; 2])> {
    let profile = draft.profile(base).ok()?;
    let mut settings = (*ui.settings.get_untracked()).clone();
    settings.skin.id = id.to_owned();
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
    let kind = if history_mode {
        crate::windows::WindowKind::History
    } else {
        crate::windows::WindowKind::Display
    };
    let (w, h) = crate::windows::size_of(kind, &skin, &settings);
    let mut preview = Ui::new(Arc::new(settings), skin.clone(), ui.locale.get_untracked());
    preview.asset_urls.set(urls.clone());
    preview.reduced_motion = ui.reduced_motion;
    preview.now = ui.now;
    preview.shader_preview = ui.shader_preview;
    preview.page = ui.page;
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

pub fn view(ui: Ui, previewing: Signal<Option<String>>) -> AnyView {
    let width = signal(0.0_f32);
    dynamic(
        move || {
            (
                previewing
                    .get()
                    .unwrap_or_else(|| ui.settings.with(|s| s.skin.id.clone())),
                ui.skin_preview_history.get(),
                ui.locale.get(),
                ui.skins.with(|s| Arc::as_ptr(s) as usize),
            )
        },
        move |(id, history_mode, _, _)| {
            let id = id.clone();
            let history_mode = *history_mode;
            let row = ui
                .skins
                .with_untracked(|rows| rows.iter().find(|r| r.id == id).cloned());
            let Some(row) = row.filter(|r| r.skin.is_some() && r.error.is_none()) else {
                return hint(ui.now_t("skin-edit-unavailable"));
            };
            let mut raw = (*row.skin.unwrap()).clone();
            let mut scope = nana_ui::HostFontScope::new();
            for font in nanatype_theme::BuiltinFonts::embedded().fonts() {
                let _ = scope.add_bytes(font.bytes);
            }
            let mut register = |role: &nanatype_theme::FontSpec| {
                let bytes = role.file.as_deref().and_then(|file| {
                    if nanatype_theme::is_builtin_font(file) {
                        nanatype_theme::builtin_font(file).map(|b| b.to_vec())
                    } else {
                        row.folder
                            .as_ref()
                            .and_then(|dir| std::fs::read(dir.join(file)).ok())
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
            let sample_id = id.clone();
            let sample_base = base.clone();
            let sample = dynamic(
                move || {
                    (
                        draft(ui, &sample_id),
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
                        history_mode,
                        row.image_urls.clone(),
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
                                    l.transform =
                                        Some(crate::ui::surface::affine(0.0, 0.0, 0.0, scale));
                                    l.transform_origin = Some(TransformOrigin {
                                        x: LengthSpec::Px(0.0),
                                        y: LengthSpec::Px(0.0),
                                    });
                                }),
                        )
                        .key("skin-preview-canvas")
                        .children(view),
                    );
                    widget(Stack::column(8.0))
                        .children((
                            widget(Stack::column(0.0).padding_xy(12.0, 12.0).with_layout(
                                move |l| {
                                    l.paint.background_image =
                                        Some(BackgroundImage::url(super::pages::checker_url(dark)));
                                },
                            ))
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
            );
            let apply_id = id.clone();
            let apply_read = id.clone();
            let play_id = id.clone();
            widget(Stack::column(12.0).width(LengthSpec::Fill))
                .key("skin-preview")
                .children((
                    widget(Stack::row(8.0).wrap(true).align(AlignSpec::Center)).children((
                        label(row.name),
                        widget(SegmentedControl::new().size(ControlSize::Small)).children((
                            segmented_option(ui.now_t("windows-display"))
                                .selected(move || !ui.skin_preview_history.get())
                                .on(move |_: &SegmentedOptionChosen| {
                                    ui.skin_preview_history.set(false)
                                }),
                            segmented_option(ui.now_t("windows-history"))
                                .selected(move || ui.skin_preview_history.get())
                                .on(move |_: &SegmentedOptionChosen| {
                                    ui.skin_preview_history.set(true)
                                }),
                        )),
                        dynamic(
                            move || ui.settings.with(|s| s.skin.id == apply_read),
                            move |active| {
                                let id = apply_id.clone();
                                (!*active).then(|| {
                                    widget(
                                        Button::new(ui.now_t("common-apply"))
                                            .size(ControlSize::Small),
                                    )
                                    .key("apply-preview")
                                    .on_cx(
                                        move |_, _: &Activate, cx| {
                                            let id = id.clone();
                                            cx.dispatch_program_all(super::controls::edit(
                                                move |s| s.skin.id = id,
                                            ));
                                        },
                                    )
                                })
                            },
                        ),
                        widget(
                            Button::new(ui.now_t("skins-play-effects")).size(ControlSize::Small),
                        )
                        .bind(move |b: &mut Button| {
                            b.label = ui.now_t(if ui.shader_preview.get() {
                                "skins-stop-effects"
                            } else {
                                "skins-play-effects"
                            });
                        })
                        .on_cx(move |_, _: &Activate, cx| {
                            if ui.shader_preview.get_untracked() {
                                ui.shader_preview.set(false);
                            } else {
                                cx.dispatch_program_all(Msg::PrepareSkinPreview {
                                    id: play_id.clone(),
                                    play: true,
                                });
                            }
                        }),
                    )),
                    hint(row.description),
                    widget(
                        Stack::row(12.0)
                            .wrap(true)
                            .align(AlignSpec::Start)
                            .width(LengthSpec::Fill),
                    )
                    .children((
                        widget(
                            Stack::column(0.0)
                                .width(LengthSpec::Px(300.0))
                                .grow(1.0)
                                .shrink(1.0)
                                .with_layout(|l| l.min_width = Some(LengthSpec::Px(280.0))),
                        )
                        .key("skin-preview-stage-column")
                        .on(move |e: &SizeChanged| {
                            width.set_if_changed(e.width);
                        })
                        .children(sample),
                        widget(
                            Stack::column(0.0)
                                .width(LengthSpec::Px(300.0))
                                .grow(1.0)
                                .shrink(1.0)
                                .with_layout(|l| l.min_width = Some(LengthSpec::Px(280.0))),
                        )
                        .key("skin-preview-controls-column")
                        .children(controls(
                            ui,
                            &id,
                            base,
                            history_mode,
                            row.fallback_font,
                        )),
                    )),
                ))
                .into_any()
        },
    )
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nana_ui::runtime::{DocumentId, RuntimeDocument};
    use nana_ui_devtools::agent::{AgentSession, RuntimeAgentSession};

    fn fixture() -> (Ui, Arc<Skin>) {
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        let skin = registry.load("nanatype.halo").unwrap().skin;
        let ui = Ui::new(
            Arc::new(Settings::default()),
            skin.clone(),
            nanatype_i18n::Locale::ZhCn,
        );
        ui.skins.set(Arc::new(crate::ui::skin_rows(
            &registry,
            nanatype_i18n::Locale::ZhCn,
        )));
        (ui, skin)
    }

    fn click(session: &mut RuntimeAgentSession, name: &str) {
        session.flush().unwrap();
        let b = session
            .accessibility_dump()
            .into_iter()
            .find(|n| n.role == "button" && n.label.as_deref() == Some(name))
            .unwrap()
            .bounds;
        session
            .click_xy(b.x + b.width / 2.0, b.y + b.height / 2.0)
            .unwrap();
        session.flush().unwrap();
    }

    #[test]
    fn drafts_keep_each_skin_and_surface_separate_and_validate_both_before_save() {
        let (ui, base) = fixture();
        let original = ui.settings.get_untracked();
        change(ui, "nanatype.halo", |d| {
            d.display.size = "48".into();
            d.history.line_height = "2".into();
        });
        change(ui, "nanatype.night", |d| d.display.size = "32".into());
        let p = draft(ui, "nanatype.halo").profile(&base).unwrap();
        assert_eq!(p.display.text_size, Some(48.0));
        assert_eq!(p.history.text_size, None);
        assert_eq!(p.history.line_height, Some(2.0));
        assert_eq!(draft(ui, "nanatype.night").display.size, "32");
        assert!(Arc::ptr_eq(&ui.settings.get_untracked(), &original));
        for bad in ["NaN", "inf", "201", "7", "abc"] {
            change(ui, "nanatype.halo", |d| d.history.size = bad.into());
            assert!(draft(ui, "nanatype.halo").profile(&base).is_err(), "{bad}");
        }
        change(ui, "nanatype.halo", |d| {
            d.history.size.clear();
            d.display.width = "300".into();
        });
        let p = draft(ui, "nanatype.halo").profile(&base).unwrap();
        assert_eq!(p.display.size.unwrap()[0], 300.0);
        assert_eq!(
            p.display.size.unwrap()[1],
            base.dialogue.region[1] + 2.0 * base.dialogue.overscan
        );
    }

    #[test]
    fn native_controls_edit_only_the_draft_and_save_the_browsed_skin_without_activation() {
        let (ui, base) = fixture();
        let active = ui.settings.get_untracked();
        let source = ui.source.get_untracked();
        let history = ui.history.get_untracked();
        let id = "nanatype.night";
        let doc_id = DocumentId::new(1).unwrap();
        let mut doc = RuntimeDocument::new(doc_id);
        let render_base = base.clone();
        doc.context_mut()
            .mount_view_root(doc_id, move || controls(ui, id, render_base, false, false))
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, 700, 1400).unwrap();
        session.flush().unwrap();
        let input = session
            .accessibility_dump()
            .into_iter()
            .find(|n| {
                n.agent_path
                    .as_deref()
                    .is_some_and(|id| id.contains("skin-edit-size"))
            })
            .expect("body size field");
        session.click_node(input.id).unwrap();
        session.type_text("42").unwrap();
        session.flush().unwrap();
        assert_eq!(draft(ui, id).display.size, "42");
        assert_eq!(ui.settings.get_untracked(), active);
        assert_eq!(ui.source.get_untracked(), source);
        assert!(Arc::ptr_eq(&ui.history.get_untracked(), &history));
        assert!(
            session
                .document_mut()
                .context_mut()
                .take_program_messages()
                .is_empty()
        );
        change(ui, id, |d| d.history.size = "201".into());
        session.flush().unwrap();
        assert!(
            session
                .accessibility_dump()
                .iter()
                .any(
                    |n| n.label.as_deref() == Some(ui.now_t("skin-edit-save").as_str())
                        && n.disabled
                )
        );
        click(&mut session, &ui.now_t("skin-edit-save"));
        assert!(
            session
                .document_mut()
                .context_mut()
                .take_program_messages()
                .is_empty()
        );
        change(ui, id, |d| d.history.size.clear());
        session.flush().unwrap();
        click(&mut session, &ui.now_t("skin-edit-save"));
        let messages = session.document_mut().context_mut().take_program_messages();
        let saved = messages
            .into_iter()
            .find_map(|m| m.downcast::<Msg>().ok())
            .unwrap();
        let Msg::SaveSkinPersonalization {
            id: saved_id,
            profile,
        } = *saved
        else {
            panic!("save message");
        };
        assert_eq!(saved_id, id);
        assert_eq!(profile.display.text_size, Some(42.0));
        assert_eq!(ui.settings.get_untracked(), active);
        click(&mut session, &ui.now_t("skin-edit-reset"));
        assert_eq!(
            draft(ui, id).profile(&base).unwrap().display,
            SurfacePersonalization::default()
        );
        change(ui, id, |d| d.history.size = "72".into());
        click(&mut session, &ui.now_t("skin-edit-discard"));
        assert!(!ui.skin_drafts.with_untracked(|m| m.contains_key(id)));
    }

    #[test]
    fn sample_does_not_write_to_live_settings_history_or_editors() {
        let (ui, base) = fixture();
        let settings = ui.settings.get_untracked();
        let history = ui.history.get_untracked();
        let source = ui.source.get_untracked();
        let draft = ui.draft.get_untracked();
        let mut edited = Draft::load(&settings, &settings.skin.id, ui);
        edited.history.size = "56".into();
        let doc_id = DocumentId::new(2).unwrap();
        let mut doc = RuntimeDocument::new(doc_id);
        doc.context_mut()
            .mount_view_root(doc_id, move || {
                sample(ui, &base, "nanatype.night", &edited, true, Arc::default())
                    .unwrap()
                    .0
            })
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, 300, 560).unwrap();
        session.flush().unwrap();
        assert_eq!(ui.settings.get_untracked(), settings);
        assert!(Arc::ptr_eq(&ui.history.get_untracked(), &history));
        assert!(Arc::ptr_eq(&ui.draft.get_untracked(), &draft));
        assert_eq!(ui.source.get_untracked(), source);
        assert!(
            session
                .document_mut()
                .context_mut()
                .take_program_messages()
                .is_empty()
        );
    }

    #[test]
    fn tall_and_maximum_previews_scale_the_entire_logical_canvas() {
        let (ui, _) = fixture();
        let id = "nanatype.halo";
        let doc_id = DocumentId::new(4).unwrap();
        let mut doc = RuntimeDocument::new(doc_id);
        doc.context_mut()
            .mount_view_root(doc_id, move || view(ui, signal(None)))
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, 720, 1400).unwrap();
        for history in [false, true] {
            ui.skin_preview_history.set(history);
            for [w, h] in [
                [300.0, 1200.0],
                [800.0, 2400.0],
                [160.0, 8192.0],
                [8192.0, 8192.0],
                [8192.0, 96.0],
                [300.0, 240.0],
            ] {
                change(ui, id, |d| {
                    let pane = if history {
                        &mut d.history
                    } else {
                        &mut d.display
                    };
                    pane.width = w.to_string();
                    pane.height = h.to_string();
                    pane.size = "200".into();
                });
                session.flush().unwrap();
                let nodes = session.accessibility_dump();
                let bounds = |key| {
                    nodes
                        .iter()
                        .find(|n| {
                            n.agent_path
                                .as_deref()
                                .is_some_and(|p| p.rsplit('/').next() == Some(key))
                        })
                        .unwrap_or_else(|| panic!("missing {key}"))
                        .bounds
                        .clone()
                };
                let column = bounds("skin-preview-stage-column");
                let stage = bounds("skin-preview-box");
                let canvas = bounds("skin-preview-canvas");
                let scale = ((column.width - 24.0) / w).min(340.0 / h).min(1.0);
                for actual in [&stage, &canvas] {
                    assert!(
                        (actual.width - w * scale).abs() < 0.5,
                        "{history} {w}x{h}: {actual:?}"
                    );
                    assert!(
                        (actual.height - h * scale).abs() < 0.5,
                        "{history} {w}x{h}: {actual:?}"
                    );
                    assert!(
                        actual.x >= column.x && actual.x + actual.width <= column.x + column.width
                    );
                }
                assert!((stage.x - canvas.x).abs() < 0.5);
                assert!((stage.y - canvas.y).abs() < 0.5);
                let pane = draft(ui, id);
                assert_eq!(
                    if history {
                        pane.history.size
                    } else {
                        pane.display.size
                    },
                    "200"
                );
            }
        }
    }

    #[test]
    fn minimum_history_keeps_room_for_messages_and_restores_its_title_when_enlarged() {
        let (ui, _) = fixture();
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        let base = registry.load("nanatype.celestial").unwrap().skin;
        let edited = Draft::load(&ui.settings.get_untracked(), "nanatype.celestial", ui);
        let doc_id = DocumentId::new(3).unwrap();
        let mut doc = RuntimeDocument::new(doc_id);
        doc.context_mut()
            .mount_view_root(doc_id, move || {
                sample(
                    ui,
                    &base,
                    "nanatype.celestial",
                    &edited,
                    true,
                    Arc::default(),
                )
                .unwrap()
                .0
            })
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, 160, 96).unwrap();
        session.flush().unwrap();
        let nodes = session.accessibility_dump();
        assert!(!nodes.iter().any(|n| n.label.as_deref() == Some("HISTORY")));
        for node in &nodes {
            if node.label.as_ref().is_some_and(|s| !s.is_empty()) {
                assert!(
                    node.bounds.x >= -1.0 && node.bounds.x + node.bounds.width <= 161.0,
                    "minimum history overflows: {:?}",
                    node
                );
            }
        }
        session.set_viewport(300, 560, 1.0).unwrap();
        session.flush().unwrap();
        assert!(
            session
                .accessibility_dump()
                .iter()
                .any(|n| n.label.as_deref() == Some("HISTORY"))
        );
    }
    #[test]
    fn an_unsaved_skin_preview_preserves_legacy_window_geometry_and_region() {
        let (ui, base) = fixture();
        let mut settings = (*ui.settings.get_untracked()).clone();
        settings.overlays.display.size = Some([900.0, 240.0]);
        settings.overlays.history.size = Some([300.0, 560.0]);
        let p = Draft::load(&settings, "other.skin", ui)
            .profile(&base)
            .unwrap();
        assert_eq!(p.display.size, settings.overlays.display.size);
        assert_eq!(p.history.size, settings.overlays.history.size);
        settings.overlays.display.size = None;
        settings.skin.overrides.region = Some([420.0, 180.0]);
        let p = Draft::load(&settings, "other.skin", ui)
            .profile(&base)
            .unwrap();
        assert_eq!(
            p.display.size,
            Some([
                420.0 + 2.0 * base.dialogue.overscan,
                180.0 + 2.0 * base.dialogue.overscan
            ])
        );
    }
}
