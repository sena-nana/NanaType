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

fn settings_body(ui: Ui) -> AnyView {
    let avatar_row = {
        let has = ui.settings.get_untracked().general.avatar.is_some();
        widget(Stack::row(0.0).gap_step(gap::step::CONTROL))
            .visible(move || !crate::ui::sources::profile_bound(ui))
            .children((
                button(ui, "general-avatar-pick", || {
                    Msg::PickFile(FileTarget::Avatar)
                }),
                has.then(|| {
                    button(ui, "general-avatar-clear", || {
                        edit(|s| s.general.avatar = None)
                    })
                }),
            ))
            .into_any()
    };
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
                    row(
                        ui,
                        "general-speaker",
                        Some("general-speaker-hint"),
                        speaker_field(ui),
                    ),
                    row(ui, "general-avatar", None, avatar_row),
                    crate::ui::sources::identity_hint(ui),
                ],
            ),
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
            // A Steam build updates through Steam.
            crate::distribution::UPDATES.then(|| section_with(ui, "general-updates", updates(ui))),
        ))
        .into_any()
}

/// Automatic checks, and an extra proxy for reaching GitHub.
fn updates(ui: Ui) -> Vec<(AnyView, Shown)> {
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
