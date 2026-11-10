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

/// Character `i`'s avatar as the chat shows it: the picked picture over the
/// name's initial, on its name colour (or the accent). Picking, clearing,
/// renaming or recolouring redraws it.
fn avatar_preview(ui: Ui, i: usize) -> AnyView {
    dynamic(
        move || {
            ui.settings.with(|s| {
                s.general.characters.get(i).map(|c| {
                    (
                        c.avatar.clone(),
                        crate::ui::character_name(ui, i, c),
                        c.color,
                    )
                })
            })
        },
        move |c| {
            let (avatar, name, color) = c.clone().unwrap_or_default();
            let accent = color.map_or_else(
                || {
                    crate::ui::kit::theme::palette(ui.chrome())
                        .accent
                        .as_rgba_array()
                },
                crate::ui::c,
            );
            crate::ui::danmaku::asset_avatar(ui.stage, avatar, &name, AVATAR_PREVIEW, accent)
        },
    )
    .into_any()
}

/// The avatar preview beside "Pick avatar".
const AVATAR_PREVIEW: f32 = 32.0;

/// Character `i` of `n` on one line: its avatar (pick, clear), name, name
/// colour (reset), hotkey; then which one is in use, order and remove.
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
    let start = c.color.unwrap_or(Rgba8::WHITE);
    // The one in use is the selected mark; any other switches to itself.
    let in_use = {
        let key = if active {
            "general-character-active"
        } else {
            "general-character-use"
        };
        let glyph = if active { ti::CIRCLE_CHECK } else { ti::CIRCLE };
        let b = IconButton::new(glyph, ui.now_t(key))
            .size(ControlSize::Small)
            .selected(active)
            .with_tooltip(ui.now_t(key));
        widget(b)
            .on_cx(move |_, _: &Activate, cx| {
                if !active {
                    cx.dispatch_program_all(edit(move |s| s.general.select_character(i)))
                }
            })
            .into_any()
    };
    // One line; a narrow window moves in-use, order and remove under it.
    let line = widget(
        Stack::row(0.0)
            .gap_step(gap::step::CONTROL)
            .align(AlignSpec::Center)
            .wrap(true)
            .width(LengthSpec::Fill),
    )
    .children((
        // The preview fills what it is given; it keeps its own size here.
        widget(
            Stack::row(0.0)
                .width(LengthSpec::Px(AVATAR_PREVIEW))
                .with_layout(|l| l.flex_shrink = Some(0.0)),
        )
        .children(avatar_preview(ui, i)),
        widget(
            Stack::row(0.0)
                .gap_step(gap::step::HAIR)
                .align(AlignSpec::Center),
        )
        .children((
            icon_action(
                ui,
                ti::FOLDER_OPEN,
                "general-avatar-pick",
                true,
                move || Msg::PickFile(FileTarget::Avatar(i)),
            ),
            widget(Stack::row(0.0))
                .visible(has(|c| c.avatar.is_some()))
                .children(icon_action(
                    ui,
                    ti::X,
                    "general-avatar-clear",
                    true,
                    move || edit_character(i, |c| c.avatar = None),
                )),
        )),
        // The name gives way first when the page is narrow, so the tools
        // stay on the card.
        widget(
            Stack::row(0.0)
                .width(LengthSpec::Px(field::SM))
                .shrink(1.0)
                .with_layout(|l| l.min_width = Some(LengthSpec::Px(field::XS))),
        )
        .children(
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
        widget(
            Stack::row(0.0)
                .gap_step(gap::step::HAIR)
                .align(AlignSpec::Center),
        )
        .children((
            widget(
                Stack::row(0.0)
                    .width(LengthSpec::Px(field::XS))
                    .with_layout(|l| l.flex_shrink = Some(0.0)),
            )
            .children(
                widget(
                    ColorField::new(crate::ui::c(start))
                        .label(ui.now_t("general-character-color"))
                        .size(ControlSize::Small),
                )
                .on_cx(move |_, e: &ColorChanged, cx| {
                    let [r, g, b, a] = e.value;
                    let to = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                    let color = Rgba8::rgba(to(r), to(g), to(b), to(a)); // design-tokens: data
                    cx.dispatch_program_all(edit_character(i, move |c| c.color = Some(color)))
                }),
            ),
            widget(Stack::row(0.0))
                .visible(has(|c| c.color.is_some()))
                .children(icon_action(
                    ui,
                    ti::RESTORE,
                    "general-character-color-reset",
                    true,
                    move || edit_character(i, |c| c.color = None),
                )),
        )),
        hotkey_control(ui, action),
        widget(Stack::row(0.0).grow(1.0)),
        widget(
            Stack::row(0.0)
                .gap_step(gap::step::HAIR)
                .align(AlignSpec::Center)
                .with_layout(|l| l.flex_shrink = Some(0.0)),
        )
        .children((
            in_use,
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
                    move |armed| remove_button(ui, armed),
                    move || edit(move |s| s.general.remove_character(i)),
                )
            }),
        )),
    ));
    Row::inset(
        widget(Stack::column(0.0).gap_step(nana_ui_core::SpacingStep::Xs)).children((
            widget(Stack::column(0.0).padding_step(inset::step::CARD, inset::step::ROW_Y))
                .children(line),
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
            Row::inset(widget(Stack::column(0.0)).children(divided(rows)))
        },
    ))
}

/// The characters' group: what an empty name does and Add on its title.
fn characters_section(ui: Ui) -> AnyView {
    let room = move || {
        ui.settings
            .with(|s| s.general.characters.len() < nanatype_settings::MAX_CHARACTERS)
    };
    section_tools(
        ui,
        "general-characters",
        vec![
            info_tip(ui, "general-speaker-hint"),
            widget(Stack::row(0.0))
                .visible(room)
                .children(tool(ui, ti::PLUS, "general-character-add", || {
                    edit(|s| s.general.characters.push(Default::default()))
                }))
                .into_any(),
        ],
        vec![(characters(ui), None)],
    )
}

/// High contrast and the UI scale of the app's own windows. Reduced motion
/// lives with the other motion settings, under Playback. "Follow system"
/// shows only where the system has a text size (Windows).
fn accessibility(ui: Ui) -> AnyView {
    let (lo, hi, step) = nanatype_settings::UI_SCALE;
    let has_system = move || ui.system_text_scale.get().is_some();
    let follows = move || {
        has_system()
            && ui
                .settings
                .with(|s| s.accessibility.ui_scale_follows_system)
    };
    section_with(
        ui,
        "general-accessibility",
        vec![
            (
                row(
                    ui,
                    "general-contrast",
                    Some("general-contrast-hint"),
                    select(
                        ui,
                        select_keys(
                            ui,
                            &[
                                ("system", "general-appearance-system"),
                                ("off", "general-contrast-off"),
                                ("on", "general-contrast-on"),
                            ],
                        ),
                        |s| match s.accessibility.contrast {
                            ContrastMode::System => "system".into(),
                            ContrastMode::Off => "off".into(),
                            ContrastMode::On => "on".into(),
                        },
                        |s, v| {
                            s.accessibility.contrast = match v {
                                "off" => ContrastMode::Off,
                                "on" => ContrastMode::On,
                                _ => ContrastMode::System,
                            }
                        },
                    ),
                ),
                None,
            ),
            shown_if(
                toggle_row!(
                    ui,
                    "general-ui-scale-system",
                    accessibility.ui_scale_follows_system
                ),
                has_system,
            ),
            shown_if(
                row(
                    ui,
                    "general-ui-scale",
                    Some("general-ui-scale-hint"),
                    slider(
                        ui,
                        (f64::from(lo), f64::from(hi), f64::from(step)),
                        "%",
                        |s| f64::from(s.accessibility.ui_scale),
                        |s, v| s.accessibility.ui_scale = v.round() as u32,
                    ),
                ),
                move || !follows(),
            ),
        ],
    )
}

/// A small in-settings rendering of the quick input bar, so opacity changes
/// are visible without summoning a separate window.
fn quick_preview_row(ui: Ui) -> Row {
    block(
        widget(Stack::column(0.0).gap_step(gap::step::CONTROL)).children((
            Type::Strong.view(ui.now_t("general-quick-preview")),
            quick_preview(ui),
        )),
    )
}

fn quick_preview(ui: Ui) -> AnyView {
    widget(
        Stack::fill_row(0.0)
            .gap_step(gap::step::CONTROL)
            .padding_step(inset::step::BAR_X, inset::step::BAR_Y)
            .align(AlignSpec::Center)
            .surface(SemanticColorRole::Background)
            .radius(RadiusTier::Sm)
            .with_layout(|l| {
                l.height = Some(LengthSpec::Px(crate::windows::QUICK_SIZE.1));
            }),
    )
    .bind(move |bar: &mut Stack| {
        bar.layout_mut().opacity = Some(
            ui.settings
                .with(|s| s.general.quick_opacity)
                .clamp(0.0, 1.0),
        );
    })
    .children((
        widget(IconGlyph::new(ti::MESSAGE).size(icon::LG).decorative()).bind(
            |g: &mut IconGlyph| {
                g.style.foreground = Some(SemanticColorRole::Muted);
            },
        ),
        widget(Text::new(ui.now_t("general-quick-preview-text")).style(Type::Body.style())),
    ))
    .into_any()
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
                    toggle_row!(ui, "general-autostart", general.launch_at_login),
                ],
            ),
            accessibility(ui),
            // A Steam build updates through Steam.
            crate::distribution::UPDATES.then(|| section_with(ui, "general-updates", updates(ui))),
        ))
        .into_any()
}

/// The characters lines can be said by.
pub(super) fn characters_page(ui: Ui) -> AnyView {
    characters_section(ui)
}

/// Profiles, and carrying the settings to another machine.
pub(super) fn profiles_page(ui: Ui) -> AnyView {
    widget(Stack::column(0.0).gap_step(gap::step::SECTION))
        .children((
            section(ui, "general-profiles", profiles(ui)),
            section_with(ui, "general-backup", backup(ui)),
        ))
        .into_any()
}

/// How the input window is there and how it sends, and the quick bar.
pub(super) fn input_page(ui: Ui) -> AnyView {
    widget(Stack::column(0.0).gap_step(gap::step::SECTION))
        .children((section_with(
            ui,
            "",
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
                    row(
                        ui,
                        "general-quick-opacity",
                        Some("general-quick-opacity-hint"),
                        percent(
                            ui,
                            |s| f64::from(s.general.quick_opacity),
                            |s, v| s.general.quick_opacity = v as f32,
                        ),
                    ),
                    None,
                ),
                (quick_preview_row(ui), None),
            ],
        ),))
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
