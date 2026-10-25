use std::sync::atomic::{AtomicU32, Ordering};

use super::*;

use nana_ui::runtime::{NumberChanged, NumberInput, TextInput, TextSubmitted};

/// A step number's circle.
const STEP_DOT: f32 = 22.0;

/// A numbered step: a numbered accent circle, then what to do.
pub(super) fn step(n: u8, body: impl IntoView) -> AnyView {
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::CONTROL)
            .align(AlignSpec::Center),
    )
    .children((
        widget(
            Stack::row(0.0)
                .width(LengthSpec::Px(STEP_DOT))
                .height(LengthSpec::Px(STEP_DOT))
                .align(AlignSpec::Center)
                .justify(JustifySpec::Center)
                .radius(RadiusTier::Full)
                .surface_mix(SemanticColorMix::alpha(SemanticColorRole::Accent, 0.12))
                .with_layout(|l| l.flex_shrink = Some(0.0)),
        )
        .children(widget(
            Text::new(n.to_string())
                .style(Type::Overline.style())
                .color(SemanticColorRole::AccentOnSoft),
        )),
        widget(
            Stack::row(0.0)
                .gap_step(gap::step::CONTROL)
                .align(AlignSpec::Center)
                .wrap(true)
                .grow(1.0)
                .shrink(1.0),
        )
        .children(body),
    ))
    .into_any()
}

/// Getting the display into OBS: enable output before selecting its live sender
/// name, other capture methods folded under one line. (The sample line is
/// the page header's.)
pub(super) fn obs_guide(ui: Ui) -> AnyView {
    let body = |key: &str| {
        widget(
            Text::new(ui.now_t(key))
                .style(Type::Body.style())
                .color(SemanticColorRole::Text),
        )
        .into_any()
    };
    let sender = widget(crate::ui::kit::tag_frame()).children((
        widget(Text::new(String::new()).style(Type::Small.mono())).bind(move |t: &mut Text| {
            let name = ui.settings.with(|s| s.output.display.name.clone());
            if t.value != name {
                t.value = name;
            }
        }),
        widget(
            IconButton::new(ti::COPY, ui.now_t("output-spout-copy"))
                .size(ControlSize::Small)
                .with_tooltip(ui.now_t("output-spout-copy")),
        )
        .on_cx(move |_, _: &Activate, cx| {
            let name = ui
                .settings
                .with_untracked(|s| s.output.display.name.clone());
            cx.dispatch_program_all(Msg::CopyText(name))
        }),
    ));
    let enable = || {
        step(
            1,
            (
                body("output-obs-step-enable"),
                widget(
                    Button::new(ui.now_t("output-obs-enable"))
                        .size(ControlSize::Small)
                        .kind(ButtonKind::Primary),
                )
                .bind(move |b: &mut Button| {
                    let enabled = ui.settings.with(|s| s.output.display.enabled);
                    b.label = ui.now_t(if enabled {
                        "output-obs-enabled"
                    } else {
                        "output-obs-enable"
                    });
                    b.disabled = enabled;
                })
                .on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program_all(edit(|s| s.output.display.enabled = true));
                }),
                link_badge(ui, |s| &s.spout_display),
            ),
        )
    };
    let steps: Vec<AnyView> = if cfg!(windows) {
        vec![
            enable(),
            step(2, body("output-obs-step-plugin")),
            step(3, (body("output-obs-step-source"), sender)),
            step(4, body("output-obs-step-alpha")),
        ]
    } else if cfg!(target_os = "macos") {
        vec![
            enable(),
            step(2, body("output-obs-step-syphon")),
            step(3, (body("output-obs-step-syphon-source"), sender)),
            step(4, body("output-obs-step-alpha")),
        ]
    } else {
        let _ = sender;
        vec![hint(ui.now_t("output-spout-unsupported"))]
    };
    let other_open = signal(!(cfg!(windows) || cfg!(target_os = "macos")));
    let other = widget(
        Stack::column(0.0)
            .gap_step(gap::step::CONTROL)
            .align(AlignSpec::Start),
    )
    .children((
        widget(
            Button::new(ui.now_t("output-obs-other"))
                .kind(ButtonKind::Text)
                .size(ControlSize::Small)
                .icon(ti::CHEVRON_RIGHT),
        )
        .bind(move |b: &mut Button| {
            let icon = if other_open.get() {
                ti::CHEVRON_DOWN
            } else {
                ti::CHEVRON_RIGHT
            };
            if b.icon != Some(icon) {
                b.icon = Some(icon);
            }
        })
        .on(move |_: &Activate| other_open.set(!other_open.get_untracked())),
        widget(Stack::column(0.0).with_layout(|l| {
            l.padding_left = Some(LengthSpec::Px(space::MD));
        }))
        .visible(move || other_open.get())
        .children(hint(ui.now_t(if cfg!(target_os = "macos") {
            "output-obs-mac"
        } else {
            "output-obs-window"
        }))),
    ));
    widget(
        Stack::column(0.0)
            .gap_step(nana_ui_core::SpacingStep::Lg)
            .padding_step(inset::step::CARD, inset::step::ROW_Y),
    )
    .children((steps, other))
    .into_any()
}

/// How an output's settings are read and written.
type Read = fn(&Settings) -> &nanatype_settings::SpoutOutput;
type Write = fn(&mut Settings) -> &mut nanatype_settings::SpoutOutput;

/// `w × h`, as the size hints spell it.
fn size_text([w, h]: [u32; 2]) -> String {
    format!("{w} × {h}")
}

/// The preset that follows the window (`SpoutOutput.size` stays `None`).
const FOLLOW_WINDOW: &str = "window";
/// Shown in the fields, and kept as the other side, while following the window.
const SIZE_BASE: [u32; 2] = [1280, 720];

/// Presets that already fit 1080p. Larger ones would be scaled down on save.
const SIZE_PRESETS: [([u32; 2], &str); 6] = [
    ([1280, 720], "16:9 · 720p"),
    ([1920, 1080], "16:9 · 1080p"),
    ([720, 1280], "9:16 · 720p"),
    ([1080, 1920], "9:16 · 1080p"),
    ([1080, 1080], "1:1 · 1080p"),
    ([1440, 1080], "4:3 · 1080p"),
];

fn preset_key([width, height]: [u32; 2]) -> String {
    format!("{width}x{height}")
}

fn shown_size(saved: Option<[u32; 2]>) -> [u32; 2] {
    saved.unwrap_or(SIZE_BASE)
}

/// The fixed size after editing `axis` (0 width, 1 height). The other side
/// stays as shown, and the pair is fitted into 1080p. A value that is not a
/// size leaves the shown pair unchanged.
fn commit_side(saved: Option<[u32; 2]>, axis: usize, value: f64) -> [u32; 2] {
    let shown = shown_size(saved);
    let mut size = shown;
    if let Some(side) = size.get_mut(axis) {
        let next = value.round();
        if next.is_finite() && next >= 0.0 {
            *side = next as u32;
        }
    }
    nanatype_settings::fit_output_size(f64::from(size[0]), f64::from(size[1])).unwrap_or(shown)
}

/// `"window"` follows the window. A known preset stores that size.
fn chosen_size(value: &str) -> Option<Option<[u32; 2]>> {
    if value == FOLLOW_WINDOW {
        return Some(None);
    }
    let (width, height) = value.split_once('x')?;
    let size = [width.parse::<u32>().ok()?, height.parse::<u32>().ok()?];
    SIZE_PRESETS
        .iter()
        .any(|(preset, _)| *preset == size)
        .then_some(Some(size))
}

/// Follow-window, a matching preset, or empty when the size is custom.
fn preset_choice(saved: Option<[u32; 2]>) -> String {
    match saved {
        None => FOLLOW_WINDOW.to_owned(),
        Some(size) if SIZE_PRESETS.iter().any(|(preset, _)| *preset == size) => preset_key(size),
        Some(_) => String::new(),
    }
}

fn size_options(ui: Ui) -> Vec<(String, String)> {
    let mut options = vec![(
        FOLLOW_WINDOW.to_owned(),
        I18N.t_in(ui.locale.get(), "output-spout-size-window"),
    )];
    options.extend(
        SIZE_PRESETS
            .iter()
            .map(|(size, name)| (preset_key(*size), (*name).to_owned())),
    );
    options
}

/// One side. An outside change replaces the field; a draft being typed stays.
fn side_field(ui: Ui, axis: usize, label_key: &'static str, read: Read, write: Write) -> AnyView {
    let initial = shown_size(ui.settings.with_untracked(|s| read(s).size))[axis];
    let shown = Arc::new(AtomicU32::new(initial));
    widget(
        Stack::row(0.0)
            .width(LengthSpec::Px(field::XS))
            .with_layout(|l| l.flex_shrink = Some(0.0)),
    )
    .children(
        widget({
            let mut input = crate::ui::kit::number_input(f64::from(initial))
                .range(
                    f64::from(nanatype_settings::OUTPUT_MIN),
                    f64::from(nanatype_settings::OUTPUT_MAX_LONG),
                )
                .label(ui.now_t(label_key))
                .size(ControlSize::Small);
            crate::ui::tnum(Arc::make_mut(&mut input.style.layout));
            input
        })
        .bind(move |input: &mut NumberInput| {
            let value = shown_size(ui.settings.with(|s| read(s).size))[axis];
            if shown.swap(value, Ordering::Relaxed) != value {
                input.assign(f64::from(value));
            }
        })
        .on_cx(move |_, e: &NumberChanged, cx| {
            let value = e.value;
            cx.dispatch_program_all(edit(move |s| {
                write(s).size = Some(commit_side(read(s).size, axis, value));
            }));
        }),
    )
    .into_any()
}

/// Width × height, then the presets.
fn size_editor(ui: Ui, read: Read, write: Write) -> AnyView {
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::TIGHT)
            .align(AlignSpec::Center)
            .wrap(true),
    )
    .children((
        side_field(ui, 0, "output-size-width", read, write),
        widget(Type::Caption.text("×")),
        side_field(ui, 1, "output-size-height", read, write),
        live_select(
            ui,
            move || size_options(ui),
            move |s| preset_choice(read(s).size),
            move |s, value| {
                if let Some(size) = chosen_size(value) {
                    write(s).size = size;
                }
            },
        ),
    ))
    .into_any()
}

/// The OBS source choices: none, then the program scene's inputs (capture
/// sources first), then the saved source when this scene lacks it.
fn source_options(ui: Ui, read: Read) -> Vec<(String, String)> {
    let locale = ui.locale.get();
    let saved = ui.settings.with(|s| read(s).obs_source.clone());
    let mut options = vec![(String::new(), I18N.t_in(locale, "obs-source-none"))];
    ui.obs.with(|state| {
        if let Some(scene) = state.scene.as_deref() {
            for item in crate::app::obs::candidates(scene) {
                options.push((item.source.clone(), item.source.clone()));
            }
        }
    });
    if !saved.is_empty() && !options.iter().any(|(v, _)| *v == saved) {
        let label = I18N.tr_in(
            locale,
            "obs-source-absent",
            &[("name", saved.clone().into())],
        );
        options.push((saved, label));
    }
    options
}

/// What sets the output's size now, as the source row's hint.
fn sizing_hint(ui: Ui, read: Read) -> Option<String> {
    use crate::app::obs::Sizing;
    let locale = ui.locale.get();
    let state = ui.obs.get();
    let sizing = ui
        .settings
        .with(|s| crate::app::obs::sizing(s, &state, read(s)));
    let scene_arg = |scene: String| [("scene", scene.into())];
    Some(match sizing {
        Sizing::Manual => I18N.t_in(locale, "obs-size-pick"),
        Sizing::Offline => I18N.t_in(locale, "obs-size-offline"),
        Sizing::Missing { scene } => I18N.tr_in(locale, "obs-size-missing", &scene_arg(scene)),
        Sizing::NoBounds { scene } => I18N.tr_in(locale, "obs-size-no-bounds", &scene_arg(scene)),
        Sizing::Following { bounds, size } => {
            let boxed = [bounds[0].round() as u32, bounds[1].round() as u32];
            if boxed == size {
                I18N.tr_in(
                    locale,
                    "obs-size-following",
                    &[("size", size_text(size).into())],
                )
            } else {
                I18N.tr_in(
                    locale,
                    "obs-size-clamped",
                    &[
                        ("box", size_text(boxed).into()),
                        ("size", size_text(size).into()),
                    ],
                )
            }
        }
    })
}

/// Which OBS source this output is, and where its size comes from; a
/// source without a bounding box gets a notice card below the row, with a
/// button that turns one on.
fn obs_source_row(ui: Ui, kind: WindowKind, read: Read, write: Write) -> AnyView {
    let no_bounds = move || {
        let state = ui.obs.get();
        ui.settings.with(|s| {
            matches!(
                crate::app::obs::sizing(s, &state, read(s)),
                crate::app::obs::Sizing::NoBounds { .. }
            )
        })
    };
    let row = row_live_hint(
        ui,
        "obs-source",
        move || sizing_hint(ui, read),
        live_select(
            ui,
            move || source_options(ui, read),
            move |s| read(s).obs_source.clone(),
            move |s, v| write(s).obs_source = v.to_owned(),
        ),
    );
    let notice = widget(
        Stack::column(0.0)
            .padding_step(inset::step::CARD, inset::step::ROW_Y)
            .with_layout(|l| l.padding_top = Some(LengthSpec::Px(0.0))),
    )
    .visible(no_bounds)
    .children(
        // One line beside a taller button: centred, not top-aligned.
        nana_ui::runtime::view::callout(
            ti::ALERT_TRIANGLE,
            StatusTone::Warning,
            ui.now_t("obs-bounds-notice"),
        )
        .trailing(primary(ui, "obs-fit-bounds", move || {
            Msg::ObsFitBounds(kind)
        }))
        .css(nana_ui::runtime::view::css! { align-items: center; }),
    );
    widget(Stack::column(0.0))
        .children((row, notice))
        .into_any()
}

/// A row with the card's hairline above it, for a column built by hand.
fn below_divider(row: AnyView) -> AnyView {
    widget(Stack::column(0.0))
        .children((divider(), row))
        .into_any()
}

/// The obs-websocket password: kept in the system's secret store, never in
/// the settings file; once saved it shows as saved, with a way to forget it.
fn obs_password_field(ui: Ui) -> AnyView {
    let draft = signal(String::new());
    let save = move || {
        let password = draft.get_untracked();
        draft.set(String::new());
        Msg::ObsPassword(password)
    };
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::TIGHT)
            .align(AlignSpec::Center)
            .wrap(true),
    )
    .children((
        widget(Stack::row(0.0).width(LengthSpec::Px(field::SM))).children(
            widget(
                crate::ui::kit::text_input("")
                    .secure(true)
                    .placeholder(ui.now_t("obs-password-placeholder"))
                    .size(ControlSize::Small),
            )
            .bind(move |input: &mut TextInput| {
                let text = draft.get();
                if input.state.value.as_ref() != text.as_str() {
                    input.state.value = text.into();
                }
            })
            .on(move |e: &TextChanged| draft.set(e.value.chars().take(256).collect()))
            .on_cx(move |_, _: &TextSubmitted, cx| cx.dispatch_program_all(save())),
        ),
        primary(ui, "obs-password-save", save),
        widget(
            Stack::row(0.0)
                .gap_step(gap::step::TIGHT)
                .align(AlignSpec::Center),
        )
        .visible(move || ui.obs.get().password_saved)
        .children((
            widget(
                Text::new(ui.now_t("obs-password-saved"))
                    .style(Type::Small.style())
                    .color(SemanticColorRole::Muted),
            ),
            button(ui, "obs-password-clear", || Msg::ObsPassword(String::new())),
        )),
    ))
    .into_any()
}

/// The obs-websocket link: on, where, and the password.
fn obs_link(ui: Ui) -> AnyView {
    toggled_section(
        ui,
        "",
        row(
            ui,
            "obs-link",
            Some("obs-link-hint"),
            crate::ui::kit::hstack((
                link_badge(ui, |s| &s.obs),
                switch(
                    ui,
                    |s| s.output.obs.enabled,
                    |s, v| s.output.obs.enabled = v,
                ),
            )),
        ),
        |s| s.output.obs.enabled,
        vec![
            row(
                ui,
                "obs-address",
                None,
                address(
                    ui,
                    (|s| s.output.obs.host.clone(), |s, v| s.output.obs.host = v),
                    (|s| s.output.obs.port, |s, v| s.output.obs.port = v),
                ),
            ),
            row(
                ui,
                "obs-password",
                Some("obs-password-hint"),
                obs_password_field(ui),
            ),
        ],
    )
}

pub(super) fn output(ui: Ui) -> AnyView {
    // One card for every window's sender: a window's row leads, and its
    // sender's details fold away with its switch.
    let spout = |kind: WindowKind, title: &'static str| -> [(AnyView, Shown); 2] {
        let link: fn(&crate::ui::Status) -> &LinkState = match kind {
            WindowKind::Display => |s| &s.spout_display,
            WindowKind::PeerDisplay => |s| &s.spout_peer_display,
            _ => |s| &s.spout_history,
        };
        let read: Read = match kind {
            WindowKind::Display => |s| &s.output.display,
            WindowKind::PeerDisplay => |s| &s.output.peer_display,
            _ => |s| &s.output.history,
        };
        let write: Write = match kind {
            WindowKind::Display => |s| &mut s.output.display,
            WindowKind::PeerDisplay => |s| &mut s.output.peer_display,
            _ => |s| &mut s.output.history,
        };
        // The sender's name is fixed: shown only so the user knows which one
        // to pick in OBS, with a copy button.
        let name = row(
            ui,
            "output-spout-name",
            Some(if cfg!(target_os = "macos") {
                "output-spout-name-hint-syphon"
            } else {
                "output-spout-name-hint"
            }),
            widget(crate::ui::kit::tag_frame())
                .children((
                    widget(
                        Text::new(read(&ui.settings.get_untracked()).name.clone())
                            .style(Type::Small.mono()),
                    )
                    .bind(move |t: &mut Text| {
                        let name = ui.settings.with(|s| read(s).name.clone());
                        if t.value != name {
                            t.value = name;
                        }
                    }),
                    widget(
                        IconButton::new(ti::COPY, ui.now_t("output-spout-copy"))
                            .size(ControlSize::Small)
                            .with_tooltip(ui.now_t("output-spout-copy")),
                    )
                    .on_cx(move |_, _: &Activate, cx| {
                        let current = read(&ui.settings.get_untracked()).name.clone();
                        cx.dispatch_program_all(Msg::CopyText(current))
                    }),
                ))
                .into_any(),
        );
        // Only while OBS is linked; it carries its own hairline so a hidden
        // row leaves none behind.
        let source = widget(Stack::column(0.0))
            .visible(move || ui.settings.with(|s| s.output.obs.enabled))
            .children(below_divider(obs_source_row(ui, kind, read, write)))
            .into_any();
        let size = row_live_hint(
            ui,
            "output-spout-size",
            move || {
                ui.settings
                    .with(|s| s.output.obs.enabled && !read(s).obs_source.is_empty())
                    .then(|| I18N.t_in(ui.locale.get(), "output-spout-size-fallback"))
            },
            size_editor(ui, read, write),
        );
        let straight = row_tip(
            ui,
            "output-spout-straight",
            "output-spout-straight-hint",
            switch(
                ui,
                move |s| read(s).straight_alpha,
                move |s, v| write(s).straight_alpha = v,
            ),
        );
        [
            (
                row(
                    ui,
                    title,
                    None,
                    crate::ui::kit::hstack((
                        link_badge(ui, link),
                        switch(
                            ui,
                            move |s| read(s).enabled,
                            move |s, v| write(s).enabled = v,
                        ),
                    )),
                ),
                None,
            ),
            shown_if(
                widget(Stack::column(0.0))
                    .children((name, source, below_divider(size), below_divider(straight)))
                    .into_any(),
                move || ui.settings.with(|s| read(s).enabled),
            ),
        ]
    };
    let senders = [
        spout(WindowKind::Display, "output-display"),
        spout(WindowKind::PeerDisplay, "output-peer-display"),
        spout(WindowKind::History, "output-history"),
    ]
    .into_iter()
    .flatten()
    .collect();
    widget(Stack::column(0.0).gap_step(gap::step::SECTION))
        .children((
            section(ui, "output-obs-title", vec![obs_guide(ui)]),
            obs_link(ui),
            section_with(
                ui,
                if cfg!(target_os = "macos") {
                    "output-syphon"
                } else {
                    "output-spout"
                },
                senders,
            ),
        ))
        .into_any()
}

#[cfg(test)]
mod tests {
    use super::{SIZE_PRESETS, chosen_size, commit_side};

    #[test]
    fn editing_a_side_while_following_the_window_starts_from_the_base() {
        assert_eq!(commit_side(None, 1, 480.0), [1280, 480]);
        assert_eq!(commit_side(Some([1920, 1080]), 0, 1600.0), [1600, 1080]);
    }

    #[test]
    fn editing_a_side_fits_the_pair_into_1080p() {
        assert_eq!(commit_side(Some([1920, 1080]), 1, 1920.0), [1080, 1080]);
    }

    #[test]
    fn a_side_that_is_not_a_size_keeps_the_shown_pair() {
        assert_eq!(commit_side(Some([1280, 720]), 1, 0.0), [1280, 720]);
    }

    #[test]
    fn choosing_the_window_clears_the_size_and_a_preset_stores_it() {
        assert_eq!(chosen_size("window"), Some(None));
        assert_eq!(chosen_size("1080x1920"), Some(Some([1080, 1920])));
        assert_eq!(chosen_size("3840x2160"), None);
    }

    #[test]
    fn presets_already_fit_1080p() {
        for ([width, height], _) in SIZE_PRESETS {
            assert_eq!(
                nanatype_settings::fit_output_size(f64::from(width), f64::from(height)),
                Some([width, height])
            );
        }
    }
}
