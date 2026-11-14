use super::*;
use crate::msg::WindowMsg;

/// The overlay windows, one group each, in page order.
pub(super) const OVERLAYS: [WindowKind; 5] = [
    WindowKind::Display,
    WindowKind::PARTNER,
    WindowKind::History,
    WindowKind::Danmaku,
    WindowKind::Input,
];

/// The group title of `kind`'s window.
fn title_key(kind: WindowKind) -> &'static str {
    match kind {
        WindowKind::Display => "windows-display",
        WindowKind::PeerDisplay(_) => "windows-peer-display",
        WindowKind::History => "windows-history",
        WindowKind::Danmaku => "windows-danmaku",
        _ => "windows-input",
    }
}

/// What assistive technology calls a control: its row's setting and its
/// group's window ("Always on top: History"), as every group repeats the
/// same row labels.
pub(super) fn cell_name(ui: Ui, line_key: &str, kind: WindowKind) -> String {
    I18N.tr_in(
        ui.locale.get_untracked(),
        "windows-cell-name",
        &[
            ("setting", ui.now_t(line_key).into()),
            ("window", ui.now_t(title_key(kind)).into()),
        ],
    )
}

/// A switch bound to one field of `kind`'s overlay settings.
pub(super) fn overlay_switch(
    ui: Ui,
    kind: WindowKind,
    line_key: &str,
    get: fn(&nanatype_settings::OverlaySettings) -> bool,
    set: fn(&mut nanatype_settings::OverlaySettings, bool),
) -> AnyView {
    let read = move |s: &Settings| kind.overlay(s).is_some_and(get);
    widget(
        Switch::new("", read(&ui.settings.get_untracked()))
            .accessible_name(cell_name(ui, line_key, kind)),
    )
    .bind(move |s: &mut Switch| s.checked = ui.settings.with(|x| read(x)))
    .on_cx(move |_, e: &ToggleChanged, cx| {
        let v = e.checked;
        cx.dispatch_program_all(edit(move |s| {
            if let Some(o) = kind.overlay_mut(s) {
                set(o, v);
            }
        }));
    })
    .into_any()
}

/// A select bound to one field of `kind`'s overlay
/// settings; options are `(value, message key)`.
pub(super) fn overlay_select(
    ui: Ui,
    kind: WindowKind,
    line_key: &str,
    options: &[(&'static str, &'static str)],
    get: fn(&nanatype_settings::OverlaySettings) -> &'static str,
    set: fn(&mut nanatype_settings::OverlaySettings, &str),
) -> AnyView {
    let read = move |s: &Settings| kind.overlay(s).map_or("", get);
    let opts: Vec<SelectOption> = select_keys(ui, options)
        .into_iter()
        .map(|(v, l)| SelectOption::new(v, l))
        .collect();
    widget(Stack::row(0.0).width(LengthSpec::Px(field::SM)))
        .children(
            widget(
                Select::new(Some(read(&ui.settings.get_untracked()).to_owned()))
                    .options(opts)
                    .accessible_name(cell_name(ui, line_key, kind))
                    .size(ControlSize::Small),
            )
            .bind(move |s: &mut Select| {
                let v: Arc<str> = ui.settings.with(|x| read(x)).into();
                if s.value.as_ref() != Some(&v) {
                    s.value = Some(v);
                }
            })
            .on_cx(move |_, e: &SelectChanged, cx| {
                let v = e.value.to_string();
                cx.dispatch_program_all(edit(move |s| {
                    if let Some(o) = kind.overlay_mut(s) {
                        set(o, &v);
                    }
                }));
            }),
        )
        .into_any()
}

/// One overlay window's settings as a group: shown, interaction, capture
/// background, on top, vertical text (the displays only) and its size.
/// A setting that does not apply to the window is left out.
fn window_group(ui: Ui, kind: WindowKind) -> AnyView {
    let overlay = kind != WindowKind::Input;
    let display = matches!(kind, WindowKind::Display | WindowKind::PeerDisplay(_));
    let mut rows = vec![row(
        ui,
        "windows-visible",
        None,
        overlay_switch(
            ui,
            kind,
            "windows-visible",
            |o| o.visible,
            |o, v| o.visible = v,
        ),
    )];
    if overlay {
        rows.push(row(
            ui,
            "windows-lock",
            Some("windows-lock-tip"),
            overlay_select(
                ui,
                kind,
                "windows-lock",
                &[
                    ("interactive", "windows-lock-interactive"),
                    ("locked", "windows-lock-locked-short"),
                ],
                |o| match o.lock {
                    Lock::Locked => "locked",
                    Lock::Interactive => "interactive",
                },
                |o, v| {
                    o.lock = if v == "locked" {
                        Lock::Locked
                    } else {
                        Lock::Interactive
                    }
                },
            ),
        ));
        rows.push(row(
            ui,
            "windows-background",
            Some("windows-background-tip"),
            overlay_select(
                ui,
                kind,
                "windows-background",
                &[
                    ("transparent", "windows-background-transparent"),
                    ("chroma", "windows-background-chroma"),
                ],
                |o| match o.background {
                    CaptureBackground::Transparent => "transparent",
                    CaptureBackground::Chroma { .. } => "chroma",
                },
                |o, v| {
                    o.background = if v == "chroma" {
                        CaptureBackground::Chroma {
                            color: nanatype_model::Rgba8::rgb(0, 255, 0), // design-tokens: data
                        }
                    } else {
                        CaptureBackground::Transparent
                    }
                },
            ),
        ));
    }
    rows.push(row(
        ui,
        "windows-on-top",
        None,
        overlay_switch(
            ui,
            kind,
            "windows-on-top",
            |o| o.always_on_top,
            |o, v| o.always_on_top = v,
        ),
    ));
    if display {
        rows.push(row(
            ui,
            "windows-vertical-text",
            Some("windows-vertical-text-tip"),
            overlay_switch(
                ui,
                kind,
                "windows-vertical-text",
                |o| o.vertical_text,
                |o, v| o.vertical_text = v,
            ),
        ));
    }
    rows.push(row(ui, "windows-size", None, size_control(ui, kind)));
    section(ui, title_key(kind), rows)
}

/// The size the window was dragged to ("Follow skin" until it is), and the
/// reset that puts it back where and as big as the skin says.
fn size_control(ui: Ui, kind: WindowKind) -> AnyView {
    let text = move || {
        let size = ui.settings.with(|s| kind.saved_size(s));
        match size {
            Some([w, h]) => format!("{}×{}", w.round(), h.round()),
            None => I18N.t_in(ui.locale.get(), "windows-size-skin"),
        }
    };
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::TIGHT)
            .align(AlignSpec::Center),
    )
    .children((
        widget(
            Text::new(text())
                .style(Type::Small.one_line())
                .color(SemanticColorRole::Muted),
        )
        .bind(move |t: &mut Text| {
            let s = text();
            if t.value != s {
                t.value = s;
            }
        }),
        widget(
            IconButton::new(ti::RESTORE, ui.now_t("windows-reset"))
                .size(ControlSize::Small)
                .with_tooltip(ui.now_t("windows-reset")),
        )
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(Msg::Window(WindowMsg::Reset(kind)));
        }),
    ))
    .into_any()
}

/// The overlay windows, one settings group each.
pub(super) fn windows(ui: Ui) -> AnyView {
    widget(Stack::column(0.0).gap_step(gap::step::SECTION))
        .children(
            OVERLAYS
                .into_iter()
                .map(|kind| {
                    let group = window_group(ui, kind);
                    if kind == WindowKind::Danmaku {
                        widget(Stack::column(0.0))
                            .visible(move || ui.settings.with(|s| s.history.chat.separate))
                            .children(group)
                            .into_any()
                    } else {
                        group
                    }
                })
                .collect::<Vec<_>>(),
        )
        .into_any()
}
