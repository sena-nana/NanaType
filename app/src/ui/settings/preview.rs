//! Isolated, unsaved skin editing. Only Save dispatches an application message.
use std::sync::{Arc, OnceLock};

use nana_ui::ControlSize;
use nana_ui::runtime::view::{
    AnyView, IntoView, Signal, dynamic, on_mount, segmented_option, signal, widget,
};
use nana_ui::runtime::{
    Activate, Button, IconButton, SegmentedControl, SegmentedOptionChosen, Select, SelectChanged,
    SelectOption, SizeChanged, Stack, TextArea, TextChanged, TextInput, TextInputState,
};
use nana_ui_core::{AlignSpec, LengthSpec};
use nanatype_settings::Settings;
use nanatype_theme::Skin;

use super::controls::{hint, label};
use crate::msg::Msg;
use crate::ui::Ui;
use crate::ui::kit::{self, gap};
use nana_ui::icons_tabler as ti;
pub use nanatype_frame::ui::preview::{
    Draft, InvalidProfile, Pane, StageAssets, Surface, default_size, sample, stage,
    stage_placeholder, stage_view,
};

/// The overscan of skin `id` as the catalogue loaded it.
fn overscan(ui: Ui, id: &str) -> Option<f32> {
    ui.skins.with_untracked(|rows| {
        rows.iter()
            .find(|r| r.id == id)
            .and_then(|r| r.skin.as_ref())
            .map(|s| s.dialogue.overscan)
    })
}

/// [`Draft::load`] with the overscan of the catalogue's skin `id`.
pub fn load_draft(settings: &Settings, id: &str, ui: Ui) -> Draft {
    Draft::load(settings, id, *ui, overscan(ui, id))
}

pub(super) fn draft(ui: Ui, id: &str) -> Draft {
    nanatype_frame::ui::preview::draft(*ui, id, overscan(ui, id))
}

pub(super) fn change(ui: Ui, id: &str, f: impl FnOnce(&mut Draft)) {
    nanatype_frame::ui::preview::change(*ui, id, overscan(ui, id), f)
}

/// The sample text field.
const SAMPLE_H: f32 = 100.0;
/// The stage and the fields side by side: each column's basis and the
/// least it narrows to before they stack.
const COLUMN_W: f32 = 300.0;
const COLUMN_MIN_W: f32 = 280.0;

/// How a preview asks the program for its stage: views cannot post program
/// messages outside an event, so the program installs this once.
static REQUEST: OnceLock<Box<dyn Fn(String, bool) + Send + Sync>> = OnceLock::new();

/// Install how the preview asks for skin `id`'s stage (and whether its
/// effects play): the program prepares it off the UI thread.
pub fn set_requester(request: impl Fn(String, bool) + Send + Sync + 'static) {
    let _ = REQUEST.set(Box::new(request));
}

fn request(id: String, play: bool) {
    if let Some(request) = REQUEST.get() {
        request(id, play);
    }
}

/// Both skin catalogue layouts preview a skin on selection; the preview
/// asks for its stage once it shows (see [`view`]).
pub(super) fn select(previewing: Signal<Option<String>>, row: &crate::ui::SkinRow) {
    if row.error.is_none() {
        previewing.set(Some(row.id.clone()));
    }
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
        Stack::column(0.0)
            .gap_step(gap::step::TIGHT)
            .width(LengthSpec::Percent(48.0))
            .with_layout(|l| {
                l.min_width = Some(LengthSpec::Px(0.0));
            }),
    )
    .children((
        hint(ui.now_t(field.key())),
        widget(
            kit::text_input(read())
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

/// The sample text under the stage: its caption with a reset at the far
/// end, then the text itself.
fn sample_editor(ui: Ui, id: &str, is_history: bool) -> AnyView {
    let sample_id = id.to_owned();
    let write_sample_id = id.to_owned();
    let sample =
        widget(kit::text_area(draft(ui, id).pane(is_history).text.clone()).height(SAMPLE_H))
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
    widget(
        Stack::column(0.0)
            .gap_step(gap::step::TIGHT)
            .width(LengthSpec::Fill),
    )
    .children((
        // The sample's caption, with its reset at the far end.
        widget(
            Stack::row(0.0)
                .gap_step(gap::step::CONTROL)
                .align(AlignSpec::Center)
                .width(LengthSpec::Fill),
        )
        .children((
            hint(ui.now_t("skin-edit-test-text")),
            kit::spacer(),
            widget(
                IconButton::new(ti::RESTORE, ui.now_t("skin-edit-sample-reset"))
                    .size(ControlSize::Small)
                    .with_tooltip(ui.now_t("skin-edit-sample-reset")),
            )
            .on(move |_: &Activate| {
                change(ui, &sample_reset_id, |d| {
                    d.pane_mut(is_history).text = ui.now_t(if is_history {
                        "skin-edit-history-sample"
                    } else {
                        "skin-edit-sample"
                    });
                })
            }),
        )),
        sample,
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
    let font_choices = widget(
        Stack::column(0.0)
            .gap_step(gap::step::TIGHT)
            .width(LengthSpec::Fill),
    )
    .children((
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
    let reset_id = id.to_owned();
    let discard_id = id.to_owned();
    let save_id = id.to_owned();
    let valid_id = id.to_owned();
    let valid_base = base.clone();
    let save_base = base.clone();
    let params = super::skin_params::view(ui, id, base.clone());
    let status_id = id.to_owned();
    let pick_id = id.to_owned();
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
            // A built-in font the draft names is registered by the sample;
            // register it here too, whichever of them runs first.
            ui.skin_preview.with_untracked(|p| {
                if let Some(p) = p.as_ref().filter(|p| p.id == pick_id) {
                    p.assets.pick_family(font);
                }
            });
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
    widget(
        Stack::column(0.0)
            .gap_step(gap::step::CONTROL)
            .width(LengthSpec::Fill),
    )
    .children((
        params,
        widget(
            Stack::row(0.0)
                .gap_step(gap::step::CONTROL)
                .wrap(true)
                .width(LengthSpec::Fill),
        )
        .children(fields),
        font_choices,
        // Where the draft stands on the left, what can be done with it on
        // the right.
        widget(
            Stack::row(0.0)
                .gap_step(gap::step::CONTROL)
                .align(AlignSpec::Center)
                .width(LengthSpec::Fill)
                .wrap(true),
        )
        .children((
            widget(kit::grow_column(gap::HAIR)).children(status),
            widget(
                Stack::row(0.0)
                    .gap_step(gap::step::CONTROL)
                    .align(AlignSpec::Center)
                    .with_layout(|l| l.flex_shrink = Some(0.0)),
            )
            .children((
                widget(Button::new(ui.now_t("skin-edit-reset")).size(ControlSize::Small)).on(
                    move |_: &Activate| {
                        change(ui, &reset_id, |d| {
                            let text = d.pane(is_history).text.clone();
                            *d.pane_mut(is_history) = Pane {
                                text,
                                ..Default::default()
                            };
                            d.params = Default::default();
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
                widget(crate::ui::kit::cta(
                    Button::new(ui.now_t("skin-edit-save")).size(ControlSize::Small),
                ))
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
        )),
    ))
    .into_any()
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
                ui.skin_preview
                    .with(|p| p.as_ref().map(|p| Arc::as_ptr(p) as usize)),
            )
        },
        move |(id, history_mode, locale, _, _)| {
            let id = id.clone();
            let history_mode = *history_mode;
            let row = ui
                .skins
                .with_untracked(|rows| rows.iter().find(|r| r.id == id).cloned());
            let Some(row) = row.filter(|r| r.skin.is_some() && r.error.is_none()) else {
                return hint(ui.now_t("skin-edit-unavailable"));
            };
            let surface = Surface::of(history_mode);
            // The page shows at once; the stage follows when prepared off
            // the UI thread (fonts, theming, effects).
            let prepared = ui
                .skin_preview
                .with_untracked(|p| p.clone().filter(|p| p.shows(&row, locale.tag())));
            let (sample, controls) = match prepared {
                Some(p) => (
                    stage_view(
                        *ui,
                        p.assets.clone(),
                        row.image_urls.clone(),
                        surface,
                        width,
                    ),
                    Some(controls(
                        ui,
                        &id,
                        p.assets.base.clone(),
                        history_mode,
                        row.fallback_font,
                    )),
                ),
                None => {
                    let ask = id.clone();
                    on_mount(move |_| request(ask, ui.shader_preview.get_untracked()));
                    let skin = row.skin.as_ref().expect("checked above");
                    (stage_placeholder(*ui, skin, surface, width), None)
                }
            };
            let apply_id = id.clone();
            let apply_read = id.clone();
            let play_id = id.clone();
            widget(
                Stack::column(0.0)
                    .gap_step(gap::step::GROUP)
                    .width(LengthSpec::Fill),
            )
            .key("skin-preview")
            .children((
                // The skin and which surface it shows on the left; applying
                // it and playing its effects at the far end.
                widget(
                    Stack::row(0.0)
                        .gap_step(gap::step::CONTROL)
                        .align(AlignSpec::Center)
                        .width(LengthSpec::Fill),
                )
                .children((
                    label(row.name),
                    widget(SegmentedControl::new().size(ControlSize::Small)).children((
                        segmented_option(ui.now_t("windows-display"))
                            .selected(move || !ui.skin_preview_history.get())
                            .on(move |_: &SegmentedOptionChosen| {
                                ui.skin_preview_history.set(false)
                            }),
                        segmented_option(ui.now_t("windows-history"))
                            .selected(move || ui.skin_preview_history.get())
                            .on(move |_: &SegmentedOptionChosen| ui.skin_preview_history.set(true)),
                    )),
                    kit::spacer(),
                    // Shown in place rather than swapped by `dynamic`, whose
                    // full-width branch pushed the effects button onto a
                    // line of its own.
                    widget(Button::new(ui.now_t("common-apply")).size(ControlSize::Small))
                        .key("apply-preview")
                        .visible(move || ui.settings.with(|s| s.skin.id != apply_read))
                        .on_cx(move |_, _: &Activate, cx| {
                            let id = apply_id.clone();
                            cx.dispatch_program_all(super::controls::edit(move |s| s.skin.id = id));
                        }),
                    widget(Button::new(ui.now_t("skins-play-effects")).size(ControlSize::Small))
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
                (!row.description.is_empty()).then(|| hint(row.description)),
                widget(
                    Stack::row(0.0)
                        .gap_step(gap::step::GROUP)
                        .wrap(true)
                        .align(AlignSpec::Start)
                        .width(LengthSpec::Fill),
                )
                .children((
                    widget(
                        Stack::column(0.0)
                            .width(LengthSpec::Px(COLUMN_W))
                            .grow(1.0)
                            .shrink(1.0)
                            .with_layout(|l| l.min_width = Some(LengthSpec::Px(COLUMN_MIN_W))),
                    )
                    .key("skin-preview-stage-column")
                    .on(move |e: &SizeChanged| {
                        width.set_if_changed(e.width);
                    })
                    .children(
                        widget(
                            Stack::column(0.0)
                                .gap_step(gap::step::GROUP)
                                .width(LengthSpec::Fill),
                        )
                        .children((sample, sample_editor(ui, &id, history_mode))),
                    ),
                    widget(
                        Stack::column(0.0)
                            .width(LengthSpec::Px(COLUMN_W))
                            .grow(1.0)
                            .shrink(1.0)
                            .with_layout(|l| l.min_width = Some(LengthSpec::Px(COLUMN_MIN_W))),
                    )
                    .key("skin-preview-controls-column")
                    .children(controls),
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
    use nanatype_settings::SurfacePersonalization;

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

    /// A standalone skin with one param of each kind and a preset.
    fn param_skin() -> Arc<Skin> {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(nanatype_theme::MANIFEST),
            r##"
[meta]
id = "test.params"
[params.fur]
type = "color"
default = "#fff7ec"
name = { zh-CN = "毛色" }
[params.size]
type = "number"
default = 28
min = 16
max = 40
step = 2
[params.align]
type = "enum"
default = "start"
options = [{ id = "start" }, { id = "center" }]
[params.shadow]
type = "bool"
default = false
[presets.night]
name = { zh-CN = "夜晚" }
values = { fur = "#2b2730", size = 32 }
[dialogue.box]
color = { param = "fur" }
[dialogue.text]
size = { param = "size" }
align = { param = "align" }
"##,
        )
        .unwrap();
        let files: Arc<dyn nanatype_theme::SkinFiles> =
            Arc::new(nanatype_theme::DirFiles::new(dir.path()));
        let loaded = nanatype_theme::load(
            files,
            nanatype_theme::SkinOrigin::Directory(dir.path().into()),
            &(),
            env!("CARGO_PKG_VERSION"),
        )
        .unwrap();
        loaded.skin
    }

    #[test]
    fn param_controls_edit_the_draft_keep_valid_values_and_save_them() {
        let (ui, _) = fixture();
        let base = param_skin();
        let id = "test.params";
        let doc_id = DocumentId::new(5).unwrap();
        let mut doc = RuntimeDocument::new(doc_id);
        let render_base = base.clone();
        doc.context_mut()
            .mount_view_root(doc_id, move || controls(ui, id, render_base, false, false))
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, 700, 1800).unwrap();
        session.flush().unwrap();
        let fur = session
            .accessibility_dump()
            .into_iter()
            .find(|n| {
                n.agent_path
                    .as_deref()
                    .is_some_and(|p| p.contains("skin-param-fur"))
            })
            .expect("hex field for the colour param");
        session.click_node(fur.id).unwrap();
        session.type_text("zz").unwrap();
        session.flush().unwrap();
        // Unparsable text stays in the field; the sample keeps the last
        // valid colour and the draft can still be saved.
        let d = draft(ui, id);
        assert!(d.params.raw.contains_key("fur"));
        assert!(!d.params.values.contains_key("fur"));
        assert!(d.profile(&base).unwrap().params.is_empty());
        assert!(session.accessibility_dump().iter().any(|n| {
            n.label.as_deref() == Some(ui.now_t("skin-params-invalid-color").as_str())
        }));

        change(ui, id, |d| {
            let _ = d.params.input(&base, "fur", "#102030");
        });
        let p = draft(ui, id).profile(&base).unwrap();
        assert_eq!(
            p.params.get("fur"),
            Some(&nanatype_model::ParamValue::Text("#102030".into()))
        );

        // A preset chip applies its values; matching values light it up.
        click(&mut session, "夜晚");
        let d = draft(ui, id);
        assert_eq!(d.params.matching_preset(&base), Some("night"));
        assert!(d.params.raw.is_empty());
        click(&mut session, &ui.now_t("skin-edit-save"));
        let saved = session
            .document_mut()
            .context_mut()
            .take_program_messages()
            .into_iter()
            .find_map(|m| m.downcast::<Msg>().ok())
            .unwrap();
        let Msg::SaveSkinPersonalization { profile, .. } = *saved else {
            panic!("save message");
        };
        // Only values that differ from the defaults are kept.
        assert_eq!(profile.params.len(), 2);
        assert_eq!(
            profile.params.get("size"),
            Some(&nanatype_model::ParamValue::Number(32.0))
        );

        click(&mut session, &ui.now_t("skin-params-default"));
        assert!(draft(ui, id).params.values.is_empty());
        change(ui, id, |d| {
            d.params
                .set("shadow", nanatype_model::ParamValue::Bool(true))
        });
        click(&mut session, &ui.now_t("skin-edit-reset"));
        assert!(draft(ui, id).params.values.is_empty());
    }

    #[test]
    fn sample_does_not_write_to_live_settings_history_or_editors() {
        let (ui, base) = fixture();
        let settings = ui.settings.get_untracked();
        let history = ui.history.get_untracked();
        let source = ui.source.get_untracked();
        let draft = ui.draft.get_untracked();
        let mut edited = load_draft(&settings, &settings.skin.id, ui);
        edited.history.size = "56".into();
        let doc_id = DocumentId::new(2).unwrap();
        let mut doc = RuntimeDocument::new(doc_id);
        doc.context_mut()
            .mount_view_root(doc_id, move || {
                sample(
                    *ui,
                    &base,
                    "nanatype.night",
                    &edited,
                    Surface::History,
                    Arc::default(),
                )
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

    fn node_keys(session: &mut RuntimeAgentSession) -> Vec<String> {
        session.flush().unwrap();
        session
            .accessibility_dump()
            .into_iter()
            .filter_map(|n| n.agent_path)
            .filter_map(|p| p.rsplit('/').next().map(str::to_owned))
            .collect()
    }

    #[test]
    fn the_page_shows_before_its_stage_and_a_repeated_selection_keeps_it() {
        let (ui, _) = fixture();
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        let id = "nanatype.sakura";
        let doc_id = DocumentId::new(6).unwrap();
        let mut doc = RuntimeDocument::new(doc_id);
        let previewing = signal(Some(id.to_owned()));
        doc.context_mut()
            .mount_view_root(doc_id, move || view(ui, previewing))
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, 720, 1400).unwrap();
        let keys = node_keys(&mut session);
        assert!(keys.iter().any(|k| k == "skin-preview-loading"), "{keys:?}");
        assert!(!keys.iter().any(|k| k == "skin-preview-box"));
        assert!(keys.iter().any(|k| k == "apply-preview"));

        crate::ui::prepare_skin_preview(ui, &registry, id, false).unwrap();
        let keys = node_keys(&mut session);
        assert!(keys.iter().any(|k| k == "skin-preview-box"), "{keys:?}");
        assert!(!keys.iter().any(|k| k == "skin-preview-loading"));
        let stage = ui.skin_preview.get_untracked().unwrap();
        let rows = ui.skins.get_untracked();
        // Selecting it again, or playing its effects, keeps the stage and
        // the catalogue as they are.
        crate::ui::prepare_skin_preview(ui, &registry, id, false).unwrap();
        crate::ui::prepare_skin_preview(ui, &registry, id, true).unwrap();
        assert!(Arc::ptr_eq(
            &ui.skin_preview.get_untracked().unwrap(),
            &stage
        ));
        assert!(Arc::ptr_eq(&ui.skins.get_untracked(), &rows));
    }

    #[test]
    fn tall_and_maximum_previews_scale_the_entire_logical_canvas() {
        let (ui, _) = fixture();
        let id = "nanatype.halo";
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        crate::ui::prepare_skin_preview(ui, &registry, id, false).unwrap();
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
        let edited = load_draft(&ui.settings.get_untracked(), "nanatype.celestial", ui);
        let doc_id = DocumentId::new(3).unwrap();
        let mut doc = RuntimeDocument::new(doc_id);
        doc.context_mut()
            .mount_view_root(doc_id, move || {
                sample(
                    *ui,
                    &base,
                    "nanatype.celestial",
                    &edited,
                    Surface::History,
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
        let p = load_draft(&settings, "other.skin", ui)
            .profile(&base)
            .unwrap();
        assert_eq!(p.display.size, settings.overlays.display.size);
        assert_eq!(p.history.size, settings.overlays.history.size);
        settings.overlays.display.size = None;
        settings.skin.overrides.region = Some([420.0, 180.0]);
        let p = load_draft(&settings, "other.skin", ui)
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
