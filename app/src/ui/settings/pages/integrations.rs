use super::*;

/// Width of the hotkey column of the VTS mapping table.
pub(super) const VTS_SELECT_W: f32 = field::MD;

/// One marker's row: its chip, then the VTS hotkey it presses ("Auto" lets
/// the bridge match the hotkey of the same name).
pub(super) fn vts_map_row(ui: Ui, key: String, options: Vec<SelectOption>) -> AnyView {
    let current = {
        let key = key.clone();
        move || {
            ui.settings
                .with(|s| s.integrations.vts.expressions.get(&key).cloned())
                .unwrap_or_default()
        }
    };
    let start = current();
    let set_key = key.clone();
    widget(
        Stack::bar(0.0)
            .gap_step(nana_ui_core::SpacingStep::Xl)
            .padding_step(inset::step::CARD, inset::step::ROW_Y)
            .align(AlignSpec::Center),
    )
    .children((
        widget(
            Stack::row(0.0)
                .gap_step(nana_ui_core::SpacingStep::Sm)
                .align(AlignSpec::Center)
                .grow(1.0)
                .shrink(1.0),
        )
        .children((
            crate::ui::kit::mono_tag(format!("[expr={key}]")),
            widget(IconGlyph::new(ti::ARROW_RIGHT).size(icon::SM).decorative()),
        )),
        widget(Stack::row(0.0).width(LengthSpec::Px(VTS_SELECT_W))).children(
            widget(
                Select::new(Some(start))
                    .options(options)
                    .size(ControlSize::Small),
            )
            .bind(move |s: &mut Select| {
                let v: Arc<str> = current().into();
                if s.value.as_ref() != Some(&v) {
                    s.value = Some(v);
                }
            })
            .on_cx(move |_, e: &SelectChanged, cx| {
                let (k, id) = (set_key.clone(), e.value.to_string());
                cx.dispatch_program_all(edit(move |s| {
                    let map = &mut s.integrations.vts.expressions;
                    if id.is_empty() {
                        map.remove(&k);
                    } else {
                        map.insert(k, id);
                    }
                }))
            }),
        ),
    ))
    .into_any()
}

/// Expression markers mapped to VTube Studio hotkeys: access first, then a
/// two-column table of every marker the model's hotkeys (or a saved
/// mapping) give.
pub(super) fn vts_mapping(ui: Ui) -> AnyView {
    let authorized = move || {
        ui.settings.with(|s| s.integrations.vts.token.is_some())
            && ui.status.with(|s| s.vts != LinkState::NeedsAuth)
    };
    let header = widget(
        Stack::bar(0.0)
            .gap_step(nana_ui_core::SpacingStep::Xl)
            .padding_step(inset::step::CARD, nana_ui_core::SpacingStep::Xl)
            .align(AlignSpec::Center),
    )
    .children((
        widget(
            Stack::column(0.0)
                .gap_step(gap::step::HAIR)
                .grow(1.0)
                .shrink(1.0),
        )
        .children(subheading(ui, "int-vts-map")),
        widget(
            Stack::row(0.0)
                .gap_step(gap::step::TIGHT)
                .align(AlignSpec::Center),
        )
        .children((
            widget(
                StatusBadge::new(ui.now_t("int-vts-authorized"), StatusTone::Success).compact(true),
            )
            .visible(authorized),
            // With nothing listed, the empty state below carries the button.
            widget(Stack::row(0.0))
                .visible(move || {
                    !authorized()
                        && (ui.status.with(|s| !s.vts_hotkeys.is_empty())
                            || ui
                                .settings
                                .with(|s| !s.integrations.vts.expressions.is_empty()))
                })
                .children(button(ui, "int-vts-authorize", || Msg::VtsAuthorize)),
            widget(
                IconButton::new(ti::REFRESH, ui.now_t("int-vts-refresh"))
                    .size(ControlSize::Small)
                    .with_tooltip(ui.now_t("int-vts-refresh")),
            )
            .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(Msg::VtsRefresh)),
        )),
    ));
    let table = dynamic(
        move || {
            let (hotkeys, connected) = ui.status.with(|s| {
                (
                    s.vts_hotkeys.clone(),
                    matches!(s.vts, LinkState::Connected(_)),
                )
            });
            let mapped = ui.settings.with(|s| {
                s.integrations
                    .vts
                    .expressions
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<Vec<_>>()
            });
            (hotkeys, connected, mapped)
        },
        move |(hotkeys, connected, mapped)| {
            let mut markers: Vec<String> = Vec::new();
            let names = hotkeys.iter().map(|(_, n)| nanatype_bridge::mapping_key(n));
            for key in names.chain(mapped.iter().map(|(k, _)| k.clone())) {
                if !key.is_empty() && !markers.contains(&key) {
                    markers.push(key);
                }
            }
            if markers.is_empty() {
                return if *connected {
                    crate::ui::kit::empty_state(
                        ui.now_t("empty-vts-hotkeys"),
                        Some((ui.now_t("int-vts-refresh"), Box::new(|| Msg::VtsRefresh))),
                    )
                } else {
                    crate::ui::kit::empty_state(
                        ui.now_t("int-vts-map-empty"),
                        Some((
                            ui.now_t("int-vts-authorize"),
                            Box::new(|| Msg::VtsAuthorize),
                        )),
                    )
                };
            }
            let mut options = vec![SelectOption::new("", ui.now_t("int-vts-map-auto"))];
            options.extend(
                hotkeys
                    .iter()
                    .map(|(id, name)| SelectOption::new(id.clone(), name.clone())),
            );
            for (_, id) in mapped {
                if !hotkeys.iter().any(|(h, _)| h == id) {
                    options.push(SelectOption::new(id.clone(), id.clone()));
                }
            }
            let caption = |key: &str| {
                widget(
                    Text::new(ui.now_t(key))
                        .style(Type::Overline.style())
                        .color(SemanticColorRole::Muted),
                )
            };
            let head = widget(
                Stack::bar(0.0)
                    .gap_step(nana_ui_core::SpacingStep::Xl)
                    .padding_xy(inset::CARD, 0.0)
                    .align(AlignSpec::Center)
                    .with_layout(|l| l.padding_bottom = Some(LengthSpec::Px(space::XS))),
            )
            .children((
                widget(Stack::row(0.0).grow(1.0)).children(caption("int-vts-map-expression")),
                widget(Stack::row(0.0).width(LengthSpec::Px(VTS_SELECT_W)))
                    .children(caption("int-vts-map-hotkey")),
            ));
            let rows: Vec<AnyView> = markers
                .into_iter()
                .map(|key| vts_map_row(ui, key, options.clone()))
                .collect();
            widget(Stack::column(0.0).with_layout(|l| {
                l.padding_bottom = Some(LengthSpec::Px(space::SM));
            }))
            .children((head, divided(rows)))
            .into_any()
        },
    );
    widget(Stack::column(0.0))
        .children((header, table))
        .into_any()
}

pub(super) fn integrations(ui: Ui) -> AnyView {
    widget(Stack::column(0.0).gap_step(gap::step::SECTION))
        .children((
            toggled_section(
                ui,
                "",
                row(
                    ui,
                    "int-vts",
                    Some("int-vts-hint"),
                    crate::ui::kit::hstack((
                        link_badge(ui, |s| &s.vts),
                        switch(
                            ui,
                            |s| s.integrations.vts.enabled,
                            |s, v| s.integrations.vts.enabled = v,
                        ),
                    )),
                ),
                |s| s.integrations.vts.enabled,
                vec![
                    row(
                        ui,
                        "int-vts-port",
                        None,
                        port(
                            ui,
                            |s| s.integrations.vts.port,
                            |s, v| s.integrations.vts.port = v,
                        ),
                    ),
                    vts_mapping(ui),
                    toggle_row!(ui, "int-vts-lipsync", integrations.vts.lip_sync),
                    row(
                        ui,
                        "int-vts-mouth-param",
                        None,
                        text(
                            ui,
                            "MouthOpen".into(),
                            field::SM,
                            |s| s.integrations.vts.mouth_param.clone(),
                            |s, v| s.integrations.vts.mouth_param = v,
                        ),
                    ),
                    row(
                        ui,
                        "int-vts-mouth-amp",
                        None,
                        slider(
                            ui,
                            (0.1, 1.0, 0.05),
                            "",
                            |s| f64::from(s.integrations.vts.mouth_amp),
                            |s, v| s.integrations.vts.mouth_amp = v as f32,
                        ),
                    ),
                    row(
                        ui,
                        "int-vts-reset",
                        None,
                        select(
                            ui,
                            select_keys(
                                ui,
                                &[
                                    ("line_end", "int-vts-reset-line-end"),
                                    ("next_line", "int-vts-reset-next-line"),
                                    ("off", "int-vts-reset-off"),
                                ],
                            ),
                            |s| s.integrations.vts.reset_expressions.key().into(),
                            |s, v| {
                                s.integrations.vts.reset_expressions =
                                    VtsReset::from_key(v).unwrap_or_default()
                            },
                        ),
                    ),
                ],
            ),
            toggled_section(
                ui,
                "",
                row(
                    ui,
                    "int-osc",
                    Some("int-osc-hint"),
                    crate::ui::kit::hstack((
                        link_badge(ui, |s| &s.osc),
                        switch(
                            ui,
                            |s| s.integrations.osc.enabled,
                            |s, v| s.integrations.osc.enabled = v,
                        ),
                    )),
                ),
                |s| s.integrations.osc.enabled,
                vec![
                    row(
                        ui,
                        "int-osc-host",
                        None,
                        address(
                            ui,
                            (
                                |s| s.integrations.osc.host.clone(),
                                |s, v| s.integrations.osc.host = v,
                            ),
                            (
                                |s| s.integrations.osc.port,
                                |s, v| s.integrations.osc.port = v,
                            ),
                        ),
                    ),
                    row(
                        ui,
                        "int-osc-prefix",
                        None,
                        code_text(
                            ui,
                            "/nanatype".into(),
                            field::SM,
                            |s| s.integrations.osc.prefix.clone(),
                            |s, v| s.integrations.osc.prefix = v,
                        ),
                    ),
                ],
            ),
            toggled_section(
                ui,
                "",
                row(
                    ui,
                    "int-ws",
                    Some("int-ws-hint"),
                    crate::ui::kit::hstack((
                        link_badge(ui, |s| &s.ws),
                        switch(
                            ui,
                            |s| s.integrations.websocket.enabled,
                            |s, v| s.integrations.websocket.enabled = v,
                        ),
                    )),
                ),
                |s| s.integrations.websocket.enabled,
                vec![row(
                    ui,
                    "int-ws-port",
                    None,
                    port(
                        ui,
                        |s| s.integrations.websocket.port,
                        |s, v| {
                            // Below 1024 needs admin rights; keep the last good port.
                            if v >= 1024 {
                                s.integrations.websocket.port = v;
                            }
                        },
                    ),
                )],
            ),
        ))
        .into_any()
}
