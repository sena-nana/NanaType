use super::*;

/// While the first-run guide is open it asks for the language and name
/// itself; the settings wait until it is done.
pub(super) fn general(ui: Ui) -> AnyView {
    dynamic(
        move || ui.settings.with(|s| s.general.onboarding_done),
        move |done| {
            if *done {
                settings_body(ui)
            } else {
                onboarding(ui)
            }
        },
    )
    .into_any()
}

/// Edit character `i`, if it is still there.
fn edit_character(
    i: usize,
    f: impl FnOnce(&mut nanatype_settings::Character) + Send + 'static,
) -> Msg {
    edit(move |s| {
        if let Some(c) = s.general.characters.get_mut(i) {
            f(c)
        }
    })
}

/// A small icon button that sends `msg`.
fn icon_action(
    ui: Ui,
    glyph: nana_ui::runtime::Icon,
    key: &str,
    enabled: bool,
    msg: impl Fn() -> Msg + Send + 'static,
) -> AnyView {
    let mut b = IconButton::new(glyph, ui.now_t(key))
        .size(ControlSize::Small)
        .with_tooltip(ui.now_t(key));
    b.disabled = !enabled;
    widget(b)
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(msg()))
        .into_any()
}

/// Character `i` of `n`: name, hotkey, use, order and remove; then its
/// avatar and name colour.
fn character_row(ui: Ui, i: usize, n: usize, active: bool) -> Row {
    let c = ui
        .settings
        .with_untracked(|s| s.general.characters[i].clone());
    let action = Shortcut::Global(GlobalAction::Character(i));
    let has = move |f: fn(&nanatype_settings::Character) -> bool| {
        move || {
            ui.settings
                .with(|s| s.general.characters.get(i).is_some_and(f))
        }
    };
    let first = widget(
        Stack::row(0.0)
            .gap_step(gap::step::CONTROL)
            .align(AlignSpec::Center)
            .width(LengthSpec::Fill),
    )
    .children((
        widget(Stack::row(0.0).width(LengthSpec::Px(field::MD))).children(
            widget(
                crate::ui::kit::text_input(c.name.clone())
                    .label(ui.now_t("general-speaker"))
                    .size(ControlSize::Small)
                    .placeholder(
                        ui.tr("general-character-unnamed", &[("n", (i as u64 + 1).into())]),
                    ),
            )
            .on_cx(move |_, e: &TextChanged, cx| {
                let v = e.value.to_string();
                cx.dispatch_program_all(edit_character(i, move |c| c.name = v))
            }),
        ),
        hotkey_control(ui, action),
        widget(Stack::row(0.0).grow(1.0)),
        if active {
            widget(
                Text::new(ui.now_t("general-character-active"))
                    .style(Type::Small.style())
                    .color(SemanticColorRole::Muted),
            )
            .into_any()
        } else {
            button(ui, "general-character-use", move || {
                edit(move |s| s.general.select_character(i))
            })
        },
        icon_action(ui, ti::ARROW_UP, "general-character-up", i > 0, move || {
            edit(move |s| s.general.move_character(i, -1))
        }),
        icon_action(
            ui,
            ti::ARROW_DOWN,
            "general-character-down",
            i + 1 < n,
            move || edit(move |s| s.general.move_character(i, 1)),
        ),
        // The last character cannot go.
        (n > 1).then(|| {
            confirm(
                *ui,
                ui.now_t("general-character-remove-confirm"),
                ui.now_t("assets-remove"),
                |armed| remove_button(ui, armed),
                move || edit(move |s| s.general.remove_character(i)),
            )
        }),
    ));
    let start = c.color.unwrap_or(Rgba8::WHITE);
    let second = widget(
        Stack::row(0.0)
            .gap_step(gap::step::CONTROL)
            .align(AlignSpec::Center)
            .wrap(true),
    )
    .children((
        button(ui, "general-avatar-pick", move || {
            Msg::PickFile(FileTarget::Avatar(i))
        }),
        widget(Stack::row(0.0))
            .visible(has(|c| c.avatar.is_some()))
            .children(button(ui, "general-avatar-clear", move || {
                edit_character(i, |c| c.avatar = None)
            })),
        widget(Stack::row(0.0).grow(1.0)),
        widget(
            Text::new(ui.now_t("general-character-color"))
                .style(Type::Small.style())
                .color(SemanticColorRole::Muted),
        ),
        widget(ColorField::new(crate::ui::c(start)).size(ControlSize::Small)).on_cx(
            move |_, e: &ColorChanged, cx| {
                let [r, g, b, a] = e.value;
                let to = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                let color = Rgba8::rgba(to(r), to(g), to(b), to(a)); // design-tokens: data
                cx.dispatch_program_all(edit_character(i, move |c| c.color = Some(color)))
            },
        ),
        widget(Stack::row(0.0))
            .visible(has(|c| c.color.is_some()))
            .children(button(ui, "general-character-color-reset", move || {
                edit_character(i, |c| c.color = None)
            })),
    ));
    Row::inset(
        widget(Stack::column(0.0).gap_step(nana_ui_core::SpacingStep::Xs)).children((
            widget(
                Stack::column(0.0)
                    .gap_step(nana_ui_core::SpacingStep::Md)
                    .padding_step(inset::step::CARD, nana_ui_core::SpacingStep::Xl),
            )
            .children((first, second)),
            hotkey_issue_line(ui, action),
        )),
    )
}

/// The characters lines can be said by. Adding, removing, reordering or
/// switching rebuilds the list; typing a name does not.
fn characters(ui: Ui) -> Row {
    Row::inset(dynamic(
        move || {
            ui.settings
                .with(|s| (s.general.characters.len(), s.general.character))
        },
        move |(n, active)| {
            let rows: Vec<Row> = (0..*n)
                .map(|i| character_row(ui, i, *n, i == *active))
                .collect();
            let add = (*n < nanatype_settings::MAX_CHARACTERS).then(|| {
                button(ui, "general-character-add", || {
                    edit(|s| s.general.characters.push(Default::default()))
                })
            });
            let hint = widget(
                Text::new(ui.now_t("general-speaker-hint"))
                    .style(Type::Small.style())
                    .color(SemanticColorRole::Muted),
            );
            Row::inset(widget(Stack::column(0.0)).children((
                divided(rows),
                divider(),
                crate::ui::kit::line(hint, add),
            )))
        },
    ))
}

fn settings_body(ui: Ui) -> AnyView {
    widget(Stack::column(0.0).gap_step(gap::step::SECTION))
        .children((
            section(
                ui,
                "",
                vec![
                    row(ui, "general-language", None, language_select(ui)),
                    row(
                        ui,
                        "general-appearance",
                        None,
                        select(
                            ui,
                            select_keys(
                                ui,
                                &[
                                    ("system", "general-appearance-system"),
                                    ("light", "general-appearance-light"),
                                    ("dark", "general-appearance-dark"),
                                ],
                            ),
                            |s| match s.skin.chrome {
                                ChromeMode::System => "system".into(),
                                ChromeMode::Light => "light".into(),
                                ChromeMode::Dark => "dark".into(),
                            },
                            |s, v| {
                                s.skin.chrome = match v {
                                    "light" => ChromeMode::Light,
                                    "dark" => ChromeMode::Dark,
                                    _ => ChromeMode::System,
                                }
                            },
                        ),
                    ),
                ],
            ),
            section(ui, "general-characters", vec![characters(ui)]),
            section_with(
                ui,
                "general-input",
                vec![
                    (
                        row(
                            ui,
                            "general-presence",
                            None,
                            segmented(
                                ui,
                                select_keys(
                                    ui,
                                    &[
                                        ("resident", "general-presence-resident"),
                                        ("summon", "general-presence-summon"),
                                    ],
                                ),
                                |s| match s.general.input_presence {
                                    InputPresence::Resident => "resident".into(),
                                    InputPresence::Summon => "summon".into(),
                                },
                                |s, v| {
                                    s.general.input_presence = if v == "summon" {
                                        InputPresence::Summon
                                    } else {
                                        InputPresence::Resident
                                    }
                                },
                            ),
                        ),
                        None,
                    ),
                    (
                        row(
                            ui,
                            "general-mode",
                            None,
                            segmented(
                                ui,
                                select_keys(
                                    ui,
                                    &[("compose", "mode-compose"), ("mirror", "mode-mirror")],
                                ),
                                |s| match s.general.input_mode {
                                    InputMode::Compose => "compose".into(),
                                    InputMode::Mirror => "mirror".into(),
                                },
                                |s, v| {
                                    s.general.input_mode = if v == "mirror" {
                                        InputMode::Mirror
                                    } else {
                                        InputMode::Compose
                                    }
                                },
                            ),
                        ),
                        None,
                    ),
                    shown_if(
                        row(
                            ui,
                            "playback-stagger",
                            None,
                            slider(
                                ui,
                                (0.0, 120.0, 5.0),
                                "ms",
                                |s| f64::from(s.playback.mirror_stagger_ms),
                                |s, v| s.playback.mirror_stagger_ms = v as u32,
                            ),
                        ),
                        move || mirror(ui),
                    ),
                    (
                        toggle_row!(ui, "general-autostart", general.launch_at_login),
                        None,
                    ),
                ],
            ),
            section(ui, "general-profiles", profiles(ui)),
            section_with(ui, "general-backup", backup(ui)),
            // A Steam build updates through Steam.
            crate::distribution::UPDATES.then(|| section_with(ui, "general-updates", updates(ui))),
        ))
        .into_any()
}

/// Automatic checks, and an extra proxy for reaching GitHub.
fn updates(ui: Ui) -> Vec<(Row, Shown)> {
    vec![
        (
            toggle_row!(ui, "general-check-updates", general.check_updates),
            None,
        ),
        (
            row_tip(
                ui,
                "general-update-proxy",
                "general-update-proxy-hint",
                code_text(
                    ui,
                    "https://example.com/".into(),
                    field::MD,
                    |s| s.update.proxy.clone(),
                    |s, v| s.update.proxy = v,
                ),
            ),
            None,
        ),
    ]
}
