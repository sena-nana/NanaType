use super::{Ui, controls::hint, kit, preview};
use nana_ui::runtime::view::{AnyView, IntoView, dynamic, signal, watch_effect, widget};
use nana_ui::runtime::{
    Activate, Button, Checkbox, Select, SelectChanged, SelectOption, Stack, TextChanged,
    ToggleChanged,
};
use nana_ui::{ButtonKind, ControlSize};
use nana_ui_core::{LengthSpec, SemanticColorRole};
use std::sync::Arc;

fn choice(ui: Ui, key: &str, action: impl Fn() + Send + 'static) -> AnyView {
    widget(
        Button::new(ui.now_t(key))
            .kind(ButtonKind::Ghost)
            .size(ControlSize::Small),
    )
    .on(move |_: &Activate| action())
    .into_any()
}
pub fn view(ui: Ui) -> AnyView {
    let width = signal(560_f32);
    let host = ui.preview_host;
    let surfaces = [
        preview::Surface::Display,
        preview::Surface::History,
        preview::Surface::Input,
    ];
    let surface = widget(
        Select::new(Some("0"))
            .options(
                ["windows-display", "windows-history", "windows-input"]
                    .iter()
                    .enumerate()
                    .map(|(i, k)| SelectOption::new(i.to_string(), ui.now_t(k)))
                    .collect::<Vec<_>>(),
            )
            .size(ControlSize::Small),
    )
    .key("package-editor-preview-surface")
    .bind(move |s: &mut Select| {
        let i = surfaces
            .iter()
            .position(|s| *s == ui.package_editor_surface.get())
            .unwrap_or(0)
            .to_string();
        let v = Some(Arc::<str>::from(i));
        if s.value != v {
            s.value = v;
        }
    })
    .on(move |e: &SelectChanged| {
        if let Ok(i) = e.value.parse::<usize>()
            && let Some(s) = surfaces.get(i)
        {
            ui.package_editor_surface.set(*s);
        }
    });
    let dimensions = (0..2)
        .map(|i| {
            widget(
                kit::text_input(ui.preview_size.get_untracked()[i].clone())
                    .placeholder(ui.now_t(if i == 0 {
                        "pkgedit-width"
                    } else {
                        "pkgedit-height"
                    }))
                    .size(ControlSize::Small),
            )
            .key(format!("editor-preview-dimension-{i}"))
            .bind(move |input: &mut nana_ui::runtime::TextInput| {
                let value = ui.preview_size.get()[i].clone();
                if input.state.value.as_str() != value {
                    input.state = nana_ui::runtime::TextInputState::new(value);
                }
            })
            .on(move |e: &TextChanged| ui.preview_size.update(|size| size[i] = e.value.to_string()))
            .into_any()
        })
        .collect::<Vec<_>>();
    let controls = widget(Stack::column(0.).gap_step(kit::gap::step::CONTROL)).children((
        widget(Stack::row(0.).wrap(true).gap_step(kit::gap::step::CONTROL)).children((
            surface,
            widget(Checkbox::new(
                ui.now_t("pkgedit-simulate"),
                ui.preview_simulate.get_untracked(),
            ))
            .bind(move |c: &mut Checkbox| c.checked = ui.preview_simulate.get())
            .on(move |e: &ToggleChanged| {
                host.skin_drafts.set(Default::default());
                ui.preview_simulate.set(e.checked);
            }),
        )),
        widget(Stack::row(0.).wrap(true).gap_step(kit::gap::step::CONTROL)).children((
            widget(Stack::column(0.).max_width(120.).width(LengthSpec::Fill)).children(dimensions),
            choice(ui, "pkgedit-landscape", move || {
                ui.preview_size.set(["800".into(), "240".into()])
            }),
            choice(ui, "pkgedit-portrait", move || {
                ui.preview_size.set(["300".into(), "600".into()])
            }),
            choice(ui, "pkgedit-narrow", move || {
                ui.preview_size.set(["240".into(), "160".into()])
            }),
            choice(ui, "pkgedit-default-size", move || {
                ui.preview_size.set(Default::default())
            }),
        )),
        widget(
            Select::new(Some("0")).size(ControlSize::Small).options(
                [
                    "pkgedit-static",
                    "pkgedit-play",
                    "pkgedit-reduced",
                    "pkgedit-effects-off",
                ]
                .iter()
                .enumerate()
                .map(|(i, k)| SelectOption::new(i.to_string(), ui.now_t(k)))
                .collect::<Vec<_>>(),
            ),
        )
        .key("editor-preview-mode")
        .bind(move |s: &mut Select| {
            let v = Some(Arc::<str>::from(ui.preview_mode.get().to_string()));
            if s.value != v {
                s.value = v;
            }
        })
        .on(move |e: &SelectChanged| {
            if let Ok(mode) = e.value.parse() {
                ui.preview_mode.set(mode);
            }
        }),
        widget(Stack::row(0.).wrap(true)).children((
            choice(ui, "pkgedit-replay", move || {
                ui.preview_mode.set(0);
                ui.preview_mode.set(1);
                host.preview_playback.set((0, 0));
                host.shader_preview.set(false);
                host.shader_preview.set(true);
            }),
            widget(
                Select::new(Some("fit")).size(ControlSize::Small).options(
                    [
                        ("fit", "pkgedit-fit"),
                        ("0.5", "50%"),
                        ("1", "100%"),
                        ("2", "200%"),
                    ]
                    .map(|(v, k)| {
                        SelectOption::new(v, if v == "fit" { ui.now_t(k) } else { k.into() })
                    }),
                ),
            )
            .bind(move |s: &mut Select| {
                let v = Some(Arc::<str>::from(
                    ui.preview_viewport
                        .get()
                        .zoom
                        .map(|z| z.to_string())
                        .unwrap_or_else(|| "fit".into()),
                ));
                if s.value != v {
                    s.value = v;
                }
            })
            .on(move |e: &SelectChanged| {
                ui.preview_viewport
                    .update(|v| v.zoom = e.value.parse().ok())
            }),
            widget(
                Select::new(Some("0")).size(ControlSize::Small).options(
                    ["pkgedit-checker", "pkgedit-white", "pkgedit-dark"]
                        .iter()
                        .enumerate()
                        .map(|(i, k)| SelectOption::new(i.to_string(), ui.now_t(k)))
                        .collect::<Vec<_>>(),
                ),
            )
            .bind(move |s: &mut Select| {
                let v = Some(Arc::<str>::from(
                    ui.preview_viewport.get().background.to_string(),
                ));
                if s.value != v {
                    s.value = v;
                }
            })
            .on(move |e: &SelectChanged| {
                ui.preview_viewport
                    .update(|v| v.background = e.value.parse().unwrap_or(0))
            }),
        )),
        widget(
            kit::text_input(ui.preview_speaker.get_untracked())
                .placeholder(ui.now_t("pkgedit-speaker"))
                .size(ControlSize::Small),
        )
        .key("editor-preview-speaker")
        .on(move |e: &TextChanged| ui.preview_speaker.set(e.value.to_string())),
        widget(kit::text_area(ui.preview_text.get_untracked()).height(88.))
            .key("editor-preview-text")
            .on(move |e: &TextChanged| ui.preview_text.set(e.value.to_string())),
        dynamic(
            move || {
                ui.package_editor
                    .with(|v| v.as_ref().is_some_and(|v| v.preview_stale))
            },
            move |stale| {
                if *stale {
                    hint(ui.now_t("pkgedit-preview-stale"))
                } else {
                    widget(Stack::column(0.)).into_any()
                }
            },
        ),
    ));
    let prepared = signal(None::<(usize, nanatype_i18n::Locale, Arc<preview::StageAssets>)>);
    let canvas = dynamic(
        move || {
            (
                ui.canvas_preview
                    .with(|s| s.as_ref().map(|s| Arc::as_ptr(s) as usize)),
                ui.package_editor
                    .with(|v| v.as_ref().map(|v| Arc::as_ptr(&v.report) as usize)),
                ui.package_editor_surface.get(),
                ui.locale.get(),
            )
        },
        move |(_, _, surface, _)| {
            let Some(view) = ui.package_editor.get_untracked() else {
                return widget(Stack::column(0.)).into_any();
            };
            let (Some(skin), Some(files)) = (view.report.skin.clone(), view.report.files.clone())
            else {
                return hint(ui.now_t("pkgedit-preview-none"));
            };
            let key = Arc::as_ptr(&view.report) as usize;
            let locale = ui.locale.get_untracked();
            let cached = prepared
                .get_untracked()
                .filter(|(k, l, _)| *k == key && *l == locale)
                .map(|(_, _, a)| a);
            let assets = cached.unwrap_or_else(|| {
                let assets = Arc::new(preview::StageAssets::prepare(
                    (*skin).clone(),
                    move |p| files.read(p).ok(),
                    locale.tag(),
                ));
                prepared.set(Some((key, locale, assets.clone())));
                assets
            });
            let assets = ui
                .canvas_preview
                .get_untracked()
                .map(|skin| Arc::new(assets.with_dialogue(skin.dialogue.clone())))
                .unwrap_or(assets);
            let base = assets.base.clone();
            watch_effect(move || {
                let mut settings = if ui.preview_simulate.get() {
                    (*ui.settings.get()).clone()
                } else {
                    nanatype_settings::Settings::default()
                };
                let mode = ui.preview_mode.get();
                settings.skin.motion = mode == 1;
                settings.skin.advanced_effects = mode != 3;
                let mut draft = preview::Draft::load(
                    &settings,
                    &base.meta.id,
                    host,
                    Some(base.dialogue.overscan),
                );
                if let Some(current) = host
                    .skin_drafts
                    .with_untracked(|m| m.get(&base.meta.id).cloned())
                {
                    draft.params = current.params;
                }
                let size = ui.preview_size.get();
                for pane in [&mut draft.display, &mut draft.history] {
                    pane.width = size[0].clone();
                    pane.height = size[1].clone();
                }
                if draft.profile(&base).is_err() {
                    let previous = host
                        .skin_drafts
                        .with_untracked(|m| m.get(&base.meta.id).cloned());
                    for pane in [&mut draft.display, &mut draft.history] {
                        pane.width = previous
                            .as_ref()
                            .map(|d| d.display.width.clone())
                            .unwrap_or_default();
                        pane.height = previous
                            .as_ref()
                            .map(|d| d.display.height.clone())
                            .unwrap_or_default();
                    }
                }
                let text = ui.preview_text.get();
                draft.display.text = text.clone();
                draft.history.text = text;
                host.preview_name.set(ui.preview_speaker.get());
                host.reduced_motion
                    .set(mode == 2 || ui.reduced_motion.get());
                host.shader_preview.set(mode == 1);
                host.settings.set(Arc::new(settings));
                host.skin_drafts.update(|m| {
                    m.insert(base.meta.id.clone(), draft);
                });
            });
            preview::stage_view_options(
                host,
                assets,
                view.report.urls.clone(),
                *surface,
                width,
                "skin-preview-box",
                Some(ui.preview_viewport),
            )
        },
    );
    widget(
        Stack::column(0.)
            .gap_step(kit::gap::step::CONTROL)
            .width(LengthSpec::Fill),
    )
    .key("package-editor-preview")
    .on(move |e: &nana_ui::runtime::SizeChanged| width.set(e.width))
    .children((
        controls,
        super::canvas_controls::view(ui),
        dynamic(
            move || {
                ui.package_editor
                    .with(|v| v.as_ref().map(|v| Arc::as_ptr(&v.report) as usize))
            },
            move |_| {
                ui.package_editor
                    .get_untracked()
                    .and_then(|v| v.report.skin.clone())
                    .and_then(|skin| {
                        nanatype_frame::ui::param_controls::view(host, &skin.meta.id.clone(), skin)
                    })
            },
        ),
        canvas,
        dynamic(
            move || ui.preview_size.get(),
            move |size| {
                if size.iter().enumerate().any(|(i, v)| {
                    !v.trim().is_empty()
                        && v.trim().parse::<f32>().ok().is_none_or(|v| {
                            !v.is_finite()
                                || !(if i == 0 { 160. } else { 96. }..=8192.).contains(&v)
                        })
                }) {
                    widget(
                        kit::Type::Caption
                            .text(ui.now_t("pkgedit-preview-size-error"))
                            .color(SemanticColorRole::Danger),
                    )
                    .into_any()
                } else {
                    widget(Stack::column(0.)).into_any()
                }
            },
        ),
    ))
    .into_any()
}
