use super::*;

/// A step number's circle.
const STEP_DOT: f32 = 22.0;

/// A numbered step: a numbered accent circle, then what to do.
pub(super) fn step(n: u8, body: impl IntoView) -> AnyView {
    widget(Stack::row(gap::CONTROL).align(AlignSpec::Center))
        .children((
            widget(crate::ui::kit::wash(
                crate::ui::kit::round(
                    Stack::row(0.0)
                        .width(LengthSpec::Px(STEP_DOT))
                        .height(LengthSpec::Px(STEP_DOT))
                        .align(AlignSpec::Center)
                        .justify(JustifySpec::Center)
                        .with_layout(|l| l.flex_shrink = Some(0.0)),
                    STEP_DOT,
                ),
                SemanticColorRole::Accent,
                0.12,
            ))
            .children(widget(
                Text::new(n.to_string())
                    .style(Type::Overline.style())
                    .color(SemanticColorRole::AccentOnSoft),
            )),
            widget(
                Stack::row(gap::CONTROL)
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
/// name, other capture methods folded under one line, and a sample line.
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
                widget(crate::ui::kit::cta(
                    Button::new(ui.now_t("output-obs-enable")).size(ControlSize::Small),
                ))
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
    let other = widget(Stack::column(gap::CONTROL).align(AlignSpec::Start)).children((
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
    widget(Stack::column(space::LG).padding_xy(inset::CARD, inset::ROW_Y))
        .children((
            steps,
            other,
            widget(Stack::row(0.0)).children(primary(ui, "output-sample", || Msg::PlaySample)),
        ))
        .into_any()
}

pub(super) fn output(ui: Ui) -> AnyView {
    let spout = |kind: WindowKind, title: &'static str| {
        let link: fn(&crate::ui::Status) -> &LinkState = match kind {
            WindowKind::Display => |s| &s.spout_display,
            WindowKind::PeerDisplay => |s| &s.spout_peer_display,
            _ => |s| &s.spout_history,
        };
        let read: fn(&Settings) -> &nanatype_settings::SpoutOutput = match kind {
            WindowKind::Display => |s| &s.output.display,
            WindowKind::PeerDisplay => |s| &s.output.peer_display,
            _ => |s| &s.output.history,
        };
        let write: fn(&mut Settings) -> &mut nanatype_settings::SpoutOutput = match kind {
            WindowKind::Display => |s| &mut s.output.display,
            WindowKind::PeerDisplay => |s| &mut s.output.peer_display,
            _ => |s| &mut s.output.history,
        };
        section(
            ui,
            title,
            vec![
                row(
                    ui,
                    if cfg!(target_os = "macos") {
                        "output-syphon"
                    } else {
                        "output-spout"
                    },
                    (kind == WindowKind::PeerDisplay).then_some("output-peer-hint"),
                    widget(Stack::row(gap::CONTROL).align(AlignSpec::Center))
                        .children((
                            link_badge(ui, link),
                            switch(
                                ui,
                                move |s| read(s).enabled,
                                move |s, v| write(s).enabled = v,
                            ),
                        ))
                        .into_any(),
                ),
                when_on(
                    ui,
                    move |s| read(s).enabled,
                    vec![
                        row(
                            ui,
                            "output-spout-name",
                            None,
                            widget(Stack::row(gap::TIGHT))
                                .children((
                                    text(
                                        ui,
                                        String::new(),
                                        220.0,
                                        move |s| read(s).name.clone(),
                                        move |s, v| write(s).name = v,
                                    ),
                                    widget(
                                        IconButton::new(ti::COPY, ui.now_t("output-spout-copy"))
                                            .size(ControlSize::Small)
                                            .with_tooltip(ui.now_t("output-spout-copy")),
                                    )
                                    .on_cx(
                                        move |_, _: &Activate, cx| {
                                            // Read when clicked: the name may have been edited since the
                                            // page was built.
                                            let current =
                                                read(&ui.settings.get_untracked()).name.clone();
                                            cx.dispatch_program_all(Msg::CopyText(current))
                                        },
                                    ),
                                ))
                                .into_any(),
                        ),
                        row(
                            ui,
                            "output-spout-size",
                            None,
                            select(
                                ui,
                                vec![
                                    ("window".into(), ui.now_t("output-spout-size-window")),
                                    ("1280x720".into(), "1280 × 720".into()),
                                    ("1920x1080".into(), "1920 × 1080".into()),
                                    ("2560x1440".into(), "2560 × 1440".into()),
                                    ("3840x2160".into(), "3840 × 2160".into()),
                                ],
                                move |s| {
                                    read(s)
                                        .size
                                        .map(|[w, h]| format!("{w}x{h}"))
                                        .unwrap_or_else(|| "window".into())
                                },
                                move |s, v| {
                                    let size = v.split_once('x').and_then(|(w, h)| {
                                        Some([w.parse().ok()?, h.parse().ok()?])
                                    });
                                    write(s).size = size;
                                },
                            ),
                        ),
                        row(
                            ui,
                            "output-spout-straight",
                            Some("output-spout-straight-hint"),
                            switch(
                                ui,
                                move |s| read(s).straight_alpha,
                                move |s, v| write(s).straight_alpha = v,
                            ),
                        ),
                    ],
                    "output-off-hint",
                ),
            ],
        )
    };
    widget(Stack::column(gap::SECTION))
        .children((
            section(ui, "output-obs-title", vec![obs_guide(ui)]),
            spout(WindowKind::Display, "output-display"),
            spout(WindowKind::PeerDisplay, "output-peer-display"),
            spout(WindowKind::History, "output-history"),
        ))
        .into_any()
}
