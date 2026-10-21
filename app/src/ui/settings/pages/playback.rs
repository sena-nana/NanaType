use super::*;

/// Reads 0 as "Off".
pub(super) fn off_at_zero(locale: Locale, v: f64) -> Option<String> {
    (v <= 0.0).then(|| I18N.t_in(locale, "playback-auto-off"))
}

/// Reads 0 as "Skin default".
pub(super) fn skin_at_zero(locale: Locale, v: f64) -> Option<String> {
    (v <= 0.0).then(|| I18N.t_in(locale, "common-skin-default"))
}

pub(super) fn after_key(a: &AfterLine) -> String {
    match a {
        AfterLine::Hold => "hold".into(),
        AfterLine::Fade { .. } => "fade".into(),
        AfterLine::Clear { .. } => "clear".into(),
    }
}

pub(super) fn after_delay(a: &AfterLine) -> f64 {
    match a {
        AfterLine::Fade { delay_ms, .. } | AfterLine::Clear { delay_ms } => {
            f64::from(*delay_ms) / 1000.0
        }
        AfterLine::Hold => 3.0,
    }
}

pub(super) fn playback(ui: Ui) -> AnyView {
    let entrance_opts = select_keys(
        ui,
        &[
            ("skin", "tb-entrance-skin"),
            ("none", "tb-entrance-none"),
            ("fade", "tb-entrance-fade"),
            ("pop", "tb-entrance-pop"),
            ("slide", "tb-entrance-slide"),
        ],
    );
    let mut devices = vec![("default".to_owned(), ui.now_t("playback-device-default"))];
    devices.extend(
        ui.status
            .get_untracked()
            .audio_devices
            .iter()
            .map(|d| (d.clone(), d.clone())),
    );
    let holds = move || {
        ui.settings
            .with(|s| matches!(s.playback.after, AfterLine::Hold))
    };
    widget(Stack::column(0.0).gap_step(gap::step::SECTION))
        .children((
            section(
                ui,
                "",
                vec![
                    toggle_row!(ui, "playback-typing", playback.typing),
                    row(
                        ui,
                        "playback-speed",
                        None,
                        slider(
                            ui,
                            (0.25, 3.0, 0.05),
                            "×",
                            |s| f64::from(1.0 / s.playback.speed_scale.max(0.01)),
                            |s, v| s.playback.speed_scale = (1.0 / v.max(0.01)) as f32,
                        ),
                    ),
                    toggle_row!(ui, "playback-punct", playback.punctuation_pauses),
                    row(
                        ui,
                        "playback-entrance",
                        None,
                        select(
                            ui,
                            entrance_opts,
                            |s| {
                                s.playback
                                    .entrance
                                    .map(|e| e.key().to_owned())
                                    .unwrap_or_else(|| "skin".into())
                            },
                            |s, v| s.playback.entrance = EntranceKind::from_key(v),
                        ),
                    ),
                ],
            ),
            section_with(
                ui,
                "playback-flow",
                vec![
                    (
                        row(
                            ui,
                            "playback-after",
                            None,
                            segmented(
                                ui,
                                select_keys(
                                    ui,
                                    &[
                                        ("hold", "playback-after-hold"),
                                        ("fade", "playback-after-fade"),
                                        ("clear", "playback-after-clear"),
                                    ],
                                ),
                                |s| after_key(&s.playback.after),
                                |s, v| {
                                    let delay = (after_delay(&s.playback.after) * 1000.0) as u32;
                                    s.playback.after = match v {
                                        "fade" => AfterLine::Fade {
                                            delay_ms: delay,
                                            per_unit_ms: 40,
                                        },
                                        "clear" => AfterLine::Clear { delay_ms: delay },
                                        _ => AfterLine::Hold,
                                    }
                                },
                            ),
                        ),
                        None,
                    ),
                    shown_if(
                        row(
                            ui,
                            "playback-after-delay",
                            None,
                            slider(
                                ui,
                                (0.5, 30.0, 0.5),
                                "s",
                                |s| after_delay(&s.playback.after),
                                |s, v| {
                                    let ms = (v * 1000.0) as u32;
                                    match &mut s.playback.after {
                                        AfterLine::Fade { delay_ms, .. }
                                        | AfterLine::Clear { delay_ms } => *delay_ms = ms,
                                        AfterLine::Hold => {}
                                    }
                                },
                            ),
                        ),
                        move || !holds(),
                    ),
                    (
                        row(
                            ui,
                            "playback-auto",
                            None,
                            slider_with(
                                ui,
                                (0.0, 10.0, 0.5),
                                "s",
                                |locale, v| {
                                    Some(if v <= 0.0 {
                                        I18N.t_in(locale, "playback-auto-off")
                                    } else {
                                        I18N.tr_in(
                                            locale,
                                            "playback-auto-value",
                                            &[("seconds", trim_num(v, 0.5).into())],
                                        )
                                    })
                                },
                                |s| {
                                    s.playback
                                        .auto_advance_ms
                                        .map(|v| f64::from(v) / 1000.0)
                                        .unwrap_or(0.0)
                                },
                                |s, v| {
                                    s.playback.auto_advance_ms =
                                        (v > 0.0).then_some((v * 1000.0) as u32)
                                },
                            ),
                        ),
                        None,
                    ),
                    (
                        row(
                            ui,
                            "playback-queue",
                            None,
                            segmented(
                                ui,
                                select_keys(
                                    ui,
                                    &[
                                        ("replace", "playback-queue-replace-short"),
                                        ("finish", "playback-queue-finish-short"),
                                        ("queue", "playback-queue-queue-short"),
                                    ],
                                ),
                                |s| match s.playback.queue {
                                    QueuePolicy::Replace => "replace".into(),
                                    QueuePolicy::FinishThenNext => "finish".into(),
                                    QueuePolicy::Queue => "queue".into(),
                                },
                                |s, v| {
                                    s.playback.queue = match v {
                                        "finish" => QueuePolicy::FinishThenNext,
                                        "queue" => QueuePolicy::Queue,
                                        _ => QueuePolicy::Replace,
                                    }
                                },
                            ),
                        ),
                        None,
                    ),
                    (
                        // When the system already asks for less motion, say so:
                        // either one reduces it.
                        row_live_hint(
                            ui,
                            "playback-reduced",
                            move || {
                                ui.system_reduced_motion.get().then(|| {
                                    I18N.t_in(ui.locale.get(), "playback-reduced-system-on")
                                })
                            },
                            switch(
                                ui,
                                |s| s.playback.reduced_motion,
                                |s, v| s.playback.reduced_motion = v,
                            ),
                        ),
                        None,
                    ),
                ],
            ),
            section(
                ui,
                "playback-audio",
                vec![
                    toggle_row!(ui, "playback-sound", sound.typing),
                    row(
                        ui,
                        "playback-volume",
                        None,
                        percent(
                            ui,
                            |s| f64::from(s.sound.master),
                            |s, v| s.sound.master = v as f32,
                        ),
                    ),
                    row(
                        ui,
                        "playback-volume-typing",
                        None,
                        percent(
                            ui,
                            |s| f64::from(s.sound.typing_volume),
                            |s, v| s.sound.typing_volume = v as f32,
                        ),
                    ),
                    row(
                        ui,
                        "playback-volume-effects",
                        None,
                        percent(
                            ui,
                            |s| f64::from(s.sound.effects_volume),
                            |s, v| s.sound.effects_volume = v as f32,
                        ),
                    ),
                    row(
                        ui,
                        "playback-device",
                        None,
                        select(
                            ui,
                            devices,
                            |s| s.sound.device.clone().unwrap_or_else(|| "default".into()),
                            |s, v| s.sound.device = (v != "default").then(|| v.to_owned()),
                        ),
                    ),
                ],
            ),
        ))
        .into_any()
}
