use super::*;

/// The overlay windows, in the matrix's column order.
pub(super) const OVERLAYS: [WindowKind; 4] = [
    WindowKind::Display,
    WindowKind::PeerDisplay,
    WindowKind::History,
    WindowKind::Input,
];
/// Width of the matrix's row labels.
pub(super) const MATRIX_LABEL_W: f32 = 150.0;
/// Space between the matrix's columns.
pub(super) const MATRIX_GAP: f32 = space::MD;

/// A matrix line: a label, then one cell per overlay window.
pub(super) fn matrix_row(label: AnyView, cells: [AnyView; 4]) -> AnyView {
    let cells = cells.map(|cell| {
        widget(
            // A quarter of what the label and the gaps leave, so the columns
            // line up whatever their cells hold.
            Stack::row(0.0).align(AlignSpec::Center).with_layout(|l| {
                l.width = Some(crate::ui::surface::calc(
                    100.0 / 4.0,
                    -(MATRIX_LABEL_W + 4.0 * MATRIX_GAP) / 4.0 - 0.5,
                ));
                l.flex_shrink = Some(0.0);
                l.min_width = Some(LengthSpec::Px(0.0));
            }),
        )
        .children(cell)
        .into_any()
    });
    widget(
        Stack::row(MATRIX_GAP)
            .align(AlignSpec::Center)
            .width(LengthSpec::Fill)
            .padding_xy(INSET, ROW_PAD_Y),
    )
    .children((
        widget(
            Stack::row(4.0)
                .align(AlignSpec::Center)
                .width(LengthSpec::Px(MATRIX_LABEL_W))
                .with_layout(|l| l.flex_shrink = Some(0.0)),
        )
        .children(label),
        cells.into_iter().collect::<Vec<_>>(),
    ))
    .into_any()
}

/// A row label, with an info mark whose tooltip says what the options do.
pub(super) fn matrix_label(ui: Ui, key: &str, tip: Option<&str>) -> AnyView {
    widget(Stack::row(2.0).align(AlignSpec::Center))
        .children((
            widget(Text::new(ui.now_t(key)).style(text_style(13.0, 500))),
            tip.map(|tip| {
                widget(
                    IconButton::new(ti::INFO_CIRCLE, ui.now_t(tip))
                        .size(ControlSize::Small)
                        .kind(ButtonKind::Ghost)
                        .with_tooltip(ui.now_t(tip)),
                )
            }),
        ))
        .into_any()
}

/// "—": the setting does not apply to this window.
pub(super) fn not_applicable() -> AnyView {
    widget(
        Text::new("—")
            .style(text_style(13.0, 400))
            .color(SemanticColorRole::Faint),
    )
    .into_any()
}

/// A switch bound to one field of `kind`'s overlay settings.
pub(super) fn overlay_switch(
    ui: Ui,
    kind: WindowKind,
    get: fn(&nanatype_settings::OverlaySettings) -> bool,
    set: fn(&mut nanatype_settings::OverlaySettings, bool),
) -> AnyView {
    let read = move |s: &Settings| kind.overlay(s).is_some_and(get);
    widget(Switch::new("", read(&ui.settings.get_untracked())))
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

/// A select filling its cell, bound to one field of `kind`'s overlay
/// settings; options are `(value, message key)`.
pub(super) fn overlay_select(
    ui: Ui,
    kind: WindowKind,
    options: &[(&'static str, &'static str)],
    get: fn(&nanatype_settings::OverlaySettings) -> &'static str,
    set: fn(&mut nanatype_settings::OverlaySettings, &str),
) -> AnyView {
    let read = move |s: &Settings| kind.overlay(s).map_or("", get);
    let opts: Vec<SelectOption> = select_keys(ui, options)
        .into_iter()
        .map(|(v, l)| SelectOption::new(v, l))
        .collect();
    widget(Stack::row(0.0).width(LengthSpec::Fill))
        .children(
            widget(
                Select::new(Some(read(&ui.settings.get_untracked()).to_owned()))
                    .options(opts)
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

/// The overlay windows side by side: one line per setting, one
/// column per window, "—" where a setting does not apply.
pub(super) fn windows(ui: Ui) -> AnyView {
    let header = matrix_row(
        widget(Stack::row(0.0)).into_any(),
        OVERLAYS.map(|kind| {
            let key = match kind {
                WindowKind::Display => "windows-display",
                WindowKind::PeerDisplay => "windows-peer-display",
                WindowKind::History => "windows-history",
                _ => "windows-input",
            };
            widget(
                Text::new(ui.now_t(key))
                    .style(one_line(12.0, 600))
                    .color(SemanticColorRole::Muted),
            )
            .into_any()
        }),
    );
    let show = matrix_row(
        matrix_label(ui, "windows-visible", None),
        OVERLAYS.map(|kind| overlay_switch(ui, kind, |o| o.visible, |o, v| o.visible = v)),
    );
    let lock = matrix_row(
        matrix_label(ui, "windows-lock", Some("windows-lock-tip")),
        OVERLAYS.map(|kind| {
            if kind == WindowKind::Input {
                return not_applicable();
            }
            overlay_select(
                ui,
                kind,
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
            )
        }),
    );
    let background = matrix_row(
        matrix_label(ui, "windows-background", Some("windows-background-tip")),
        OVERLAYS.map(|kind| {
            if kind == WindowKind::Input {
                return not_applicable();
            }
            overlay_select(
                ui,
                kind,
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
                            color: nanatype_model::Rgba8::rgb(0, 255, 0),
                        }
                    } else {
                        CaptureBackground::Transparent
                    }
                },
            )
        }),
    );
    let on_top = matrix_row(
        matrix_label(ui, "windows-on-top", None),
        OVERLAYS
            .map(|kind| overlay_switch(ui, kind, |o| o.always_on_top, |o, v| o.always_on_top = v)),
    );
    // The size the window was dragged to ("Follow skin" until it is), and
    // the reset that puts it back where and as big as the skin says.
    let size = matrix_row(
        matrix_label(ui, "windows-size", Some("windows-reset-tip")),
        OVERLAYS.map(|kind| {
            let text = move || {
                let size = ui.settings.with(|s| kind.saved_size(s));
                match size {
                    Some([w, h]) => format!("{}×{}", w.round(), h.round()),
                    None => I18N.t_in(ui.locale.get(), "windows-size-skin"),
                }
            };
            widget(
                Stack::row(4.0)
                    .align(AlignSpec::Center)
                    .width(LengthSpec::Fill),
            )
            .children((
                widget(
                    Text::new(text())
                        .style(one_line(12.0, 500))
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
                    cx.dispatch_program_all(Msg::ResetWindow(kind));
                }),
            ))
            .into_any()
        }),
    );
    widget(Stack::column(SECTION_GAP))
        .children((
            section(ui, "", vec![header, show, lock, background, on_top, size]),
            widget(Stack::column(0.0).padding_xy(INSET, space::MD)).children((
                hint(ui.now_t("windows-layout-hint")),
                hint(ui.now_t("windows-input-na")),
            )),
        ))
        .into_any()
}
