//! What the stage shows and how: the toolbar's view and size choices, the
//! canvas strip's motion, zoom and background, the sample text, and the
//! stage itself, drawn through the skin renderer on a private [`Stage`].
//!
//! [`Stage`]: super::Stage
use super::{Ui, controls::hint, kit, preview};
use nana_ui::runtime::view::{AnyView, IntoView, dynamic, signal, watch_effect, widget};
use nana_ui::runtime::{
    Activate, Checkbox, Popover, PopoverClosed, PopoverToggled, Select, SelectChanged,
    SelectOption, Stack, TextChanged, ToggleChanged,
};
use nana_ui::{ControlSize, PopoverPlacement, icons_tabler as ti};
use nana_ui_core::{AlignSpec, LengthSpec, SemanticColorRole};
use std::sync::Arc;

/// The size presets the toolbar offers: label and logical width × height.
const PRESETS: [(&str, [&str; 2]); 3] = [
    ("pkgedit-landscape", ["800", "240"]),
    ("pkgedit-portrait", ["300", "600"]),
    ("pkgedit-narrow", ["240", "160"]),
];
/// A dimension field in the stage settings.
const DIMENSION_W: f32 = 88.0;
/// The live preview's selects.
const COMPACT_SELECT_W: f32 = 120.0;
/// The sample text under the stage.
const SAMPLE_H: f32 = 52.0;

fn bound_select(
    key: &'static str,
    options: Vec<SelectOption>,
    value: impl Fn() -> String + Send + 'static,
    changed: impl Fn(&str) + Send + 'static,
) -> AnyView {
    widget(
        Select::new(None::<String>)
            .size(ControlSize::Small)
            .options(options),
    )
    .key(key)
    .bind(move |s: &mut Select| {
        let v = Some(Arc::<str>::from(value()));
        if s.value != v {
            s.value = v;
        }
    })
    .on(move |e: &SelectChanged| changed(&e.value))
    .into_any()
}

/// Display, history or input box.
pub fn surface(ui: Ui) -> AnyView {
    let surfaces = [
        preview::Surface::Display,
        preview::Surface::History,
        preview::Surface::Input,
    ];
    bound_select(
        "package-editor-preview-surface",
        ["windows-display", "windows-history", "windows-input"]
            .iter()
            .enumerate()
            .map(|(i, k)| SelectOption::new(i.to_string(), ui.now_t(k)))
            .collect(),
        move || {
            surfaces
                .iter()
                .position(|s| *s == ui.package_editor_surface.get())
                .unwrap_or(0)
                .to_string()
        },
        move |value| {
            if let Ok(i) = value.parse::<usize>()
                && let Some(s) = surfaces.get(i)
            {
                ui.package_editor_surface.set(*s);
            }
        },
    )
}

/// Landscape, portrait, narrow, the skin's own size, or what was typed.
pub fn size_preset(ui: Ui) -> AnyView {
    let mut options = PRESETS
        .iter()
        .map(|(key, [w, h])| SelectOption::new(*key, format!("{} {w}×{h}", ui.now_t(key))))
        .collect::<Vec<_>>();
    options.push(SelectOption::new(
        "default",
        ui.now_t("pkgedit-default-size"),
    ));
    options.push(SelectOption::new("custom", ui.now_t("pkgedit-custom-size")));
    bound_select(
        "editor-preview-preset",
        options,
        move || {
            let size = ui.preview_size.get();
            if size.iter().all(|v| v.trim().is_empty()) {
                return "default".into();
            }
            PRESETS
                .iter()
                .find(|(_, s)| size[0] == s[0] && size[1] == s[1])
                .map_or("custom", |(k, _)| *k)
                .into()
        },
        move |value| {
            if value == "default" {
                ui.preview_size.set(Default::default());
            } else if let Some((_, [w, h])) = PRESETS.iter().find(|(k, _)| *k == value) {
                ui.preview_size.set([(*w).into(), (*h).into()]);
            }
        },
    )
}

/// The stage's logical width and height.
fn dimensions(ui: Ui) -> AnyView {
    widget(
        Stack::row(0.)
            .gap_step(kit::gap::step::TIGHT)
            .align(AlignSpec::Center),
    )
    .children(
        (0..2)
            .map(|i| {
                widget(Stack::row(0.).width(LengthSpec::Px(DIMENSION_W)))
                    .children(
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
                        .on(move |e: &TextChanged| {
                            ui.preview_size.update(|size| size[i] = e.value.to_string())
                        }),
                    )
                    .into_any()
            })
            .collect::<Vec<_>>(),
    )
    .into_any()
}

/// Static, playing, reduced motion, or without advanced effects.
pub fn motion(ui: Ui) -> AnyView {
    bound_select(
        "editor-preview-mode",
        [
            "pkgedit-static",
            "pkgedit-play",
            "pkgedit-reduced",
            "pkgedit-effects-off",
        ]
        .iter()
        .enumerate()
        .map(|(i, k)| SelectOption::new(i.to_string(), ui.now_t(k)))
        .collect(),
        move || ui.preview_mode.get().to_string(),
        move |value| {
            if let Ok(mode) = value.parse() {
                ui.preview_mode.set(mode);
            }
        },
    )
}

/// Play the entrance again.
pub fn replay(ui: Ui) -> AnyView {
    let host = ui.preview_host;
    widget(super::shell::tool(ti::REFRESH, ui.now_t("pkgedit-replay")))
        .key("editor-preview-replay")
        .on(move |_: &Activate| {
            ui.preview_mode.set(0);
            ui.preview_mode.set(1);
            host.preview_playback.set((0, 0));
            host.shader_preview.set(false);
            host.shader_preview.set(true);
        })
        .into_any()
}

/// Fit the stage, or a fixed zoom.
pub fn zoom(ui: Ui) -> AnyView {
    bound_select(
        "editor-preview-zoom",
        [
            ("fit", "pkgedit-fit"),
            ("0.5", "50%"),
            ("1", "100%"),
            ("2", "200%"),
        ]
        .map(|(v, k)| SelectOption::new(v, if v == "fit" { ui.now_t(k) } else { k.into() }))
        .to_vec(),
        move || {
            ui.preview_viewport
                .get()
                .zoom
                .map(|z| z.to_string())
                .unwrap_or_else(|| "fit".into())
        },
        move |value| ui.preview_viewport.update(|v| v.zoom = value.parse().ok()),
    )
}

/// Checkerboard, white or dark behind the skin.
pub fn background(ui: Ui) -> AnyView {
    bound_select(
        "editor-preview-background",
        ["pkgedit-checker", "pkgedit-white", "pkgedit-dark"]
            .iter()
            .enumerate()
            .map(|(i, k)| SelectOption::new(i.to_string(), ui.now_t(k)))
            .collect(),
        move || ui.preview_viewport.get().background.to_string(),
        move |value| {
            ui.preview_viewport
                .update(|v| v.background = value.parse().unwrap_or(0))
        },
    )
}

/// Less-used stage options behind one button: a typed size and the
/// personal NanaType settings.
pub fn settings(ui: Ui) -> AnyView {
    let open = signal(false);
    let host = ui.preview_host;
    widget(
        Popover::new()
            .trigger_icon(ti::ADJUSTMENTS, ui.now_t("pkgedit-stage-settings"))
            .bare_trigger(true)
            .compact_trigger(true)
            .placement(PopoverPlacement::Bottom)
            .width(kit::popover::MD),
    )
    .key("editor-stage-settings")
    .bind(move |p: &mut Popover| p.open = open.get())
    .on(move |e: &PopoverToggled| open.set(e.open))
    .on(move |_: &PopoverClosed| open.set(false))
    .children(
        widget(Stack::column(0.).gap_step(kit::gap::step::CONTROL)).children((
            hint(ui.now_t("pkgedit-stage-size")),
            dimensions(ui),
            size_error(ui),
            widget(Checkbox::new(
                ui.now_t("pkgedit-simulate"),
                ui.preview_simulate.get_untracked(),
            ))
            .key("editor-preview-simulate")
            .bind(move |c: &mut Checkbox| c.checked = ui.preview_simulate.get())
            .on(move |e: &ToggleChanged| {
                host.skin_drafts.set(Default::default());
                ui.preview_simulate.set(e.checked);
            }),
        )),
    )
    .into_any()
}

/// The sample speaker and text the stage shows.
pub fn sample(ui: Ui) -> AnyView {
    widget(
        Stack::row(0.)
            .gap_step(kit::gap::step::CONTROL)
            .align(AlignSpec::Start)
            .width(LengthSpec::Fill),
    )
    .children((
        widget(
            Stack::row(0.)
                .width(LengthSpec::Px(kit::field::XS))
                .shrink(0.),
        )
        .children(
            widget(
                kit::text_input(ui.preview_speaker.get_untracked())
                    .placeholder(ui.now_t("pkgedit-speaker"))
                    .size(ControlSize::Small),
            )
            .key("editor-preview-speaker")
            .on(move |e: &TextChanged| ui.preview_speaker.set(e.value.to_string())),
        ),
        widget(
            Stack::row(0.)
                .grow(1.)
                .shrink(1.)
                .min_width(LengthSpec::Px(0.)),
        )
        .children(
            widget(kit::text_area(ui.preview_text.get_untracked()).height(SAMPLE_H))
                .key("editor-preview-text")
                .on(move |e: &TextChanged| ui.preview_text.set(e.value.to_string())),
        ),
    ))
    .into_any()
}

/// The parameter test values of the draft skin, never written to it.
pub fn params(ui: Ui) -> AnyView {
    let host = ui.preview_host;
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
                .unwrap_or_else(|| hint(ui.now_t("pkgedit-params-none")))
        },
    )
    .into_any()
}

fn size_error(ui: Ui) -> AnyView {
    dynamic(
        move || ui.preview_size.get(),
        move |size| {
            if size.iter().enumerate().any(|(i, v)| {
                !v.trim().is_empty()
                    && v.trim().parse::<f32>().ok().is_none_or(|v| {
                        !v.is_finite() || !(if i == 0 { 160. } else { 96. }..=8192.).contains(&v)
                    })
            }) {
                widget(
                    kit::Type::Caption
                        .text(ui.now_t("pkgedit-preview-size-error"))
                        .color(SemanticColorRole::Danger),
                )
                .key("editor-preview-size-error")
                .into_any()
            } else {
                widget(Stack::column(0.)).into_any()
            }
        },
    )
    .into_any()
}

/// Shown while the stage draws an older skin than the source.
pub fn stale(ui: Ui) -> AnyView {
    widget(
        nana_ui::runtime::Button::new(ui.now_t("pkgedit-preview-stale"))
            .icon(ti::CLOCK_PAUSE)
            .kind(nana_ui::ButtonKind::Ghost)
            .size(ControlSize::Small),
    )
    .key("editor-preview-stale")
    .visible(move || {
        ui.package_editor
            .with(|v| v.as_ref().is_some_and(|v| v.preview_stale))
    })
    .into_any()
}

/// The draft skin on its private stage, `width` wide.
pub fn stage(ui: Ui) -> AnyView {
    let width = signal(560_f32);
    let host = ui.preview_host;
    let prepared = signal(None::<(usize, nanatype_i18n::Locale, Arc<preview::StageAssets>)>);
    let stage = dynamic(
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
            .width(LengthSpec::Fill)
            .align(AlignSpec::Center),
    )
    .key("package-editor-preview")
    .on(move |e: &nana_ui::runtime::SizeChanged| width.set(e.width))
    .children((stage, size_error(ui)))
    .into_any()
}

/// The stage alone with its zoom: the files workspace's live preview.
pub fn compact(ui: Ui) -> AnyView {
    widget(
        Stack::column(0.)
            .gap_step(kit::gap::step::CONTROL)
            .padding_step(kit::inset::step::PANEL, kit::inset::step::BAR_Y)
            .width(LengthSpec::Fill),
    )
    .children((
        widget(
            Stack::row(0.)
                .gap_step(kit::gap::step::TIGHT)
                .align(AlignSpec::Center)
                .wrap(true),
        )
        .children((
            super::shell::sized(COMPACT_SELECT_W, motion(ui)),
            replay(ui),
            super::shell::sized(COMPACT_SELECT_W, zoom(ui)),
            stale(ui),
        )),
        stage(ui),
    ))
    .into_any()
}
