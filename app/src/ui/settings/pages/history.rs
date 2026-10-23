use super::*;

pub(super) fn history(ui: Ui) -> AnyView {
    widget(Stack::column(0.0).gap_step(gap::step::SECTION))
        .children((
            section(
                ui,
                "",
                vec![
                    row(
                        ui,
                        "history-sync",
                        None,
                        select(
                            ui,
                            select_keys(
                                ui,
                                &[
                                    ("submit", "history-sync-submit"),
                                    ("reveal", "history-sync-reveal"),
                                    ("complete", "history-sync-complete"),
                                ],
                            ),
                            |s| match s.history.sync {
                                HistorySync::OnSubmit => "submit".into(),
                                HistorySync::Reveal => "reveal".into(),
                                HistorySync::OnComplete => "complete".into(),
                            },
                            |s, v| {
                                s.history.sync = match v {
                                    "submit" => HistorySync::OnSubmit,
                                    "complete" => HistorySync::OnComplete,
                                    _ => HistorySync::Reveal,
                                }
                            },
                        ),
                    ),
                    row_tip(
                        ui,
                        "history-hide-latest",
                        "history-hide-latest-hint",
                        select(
                            ui,
                            select_keys(
                                ui,
                                &[
                                    ("skin", "history-hide-latest-skin"),
                                    ("always", "history-hide-latest-always"),
                                    ("never", "history-hide-latest-never"),
                                ],
                            ),
                            |s| match s.history.hide_latest {
                                HideLatest::Skin => "skin".into(),
                                HideLatest::Always => "always".into(),
                                HideLatest::Never => "never".into(),
                            },
                            |s, v| {
                                s.history.hide_latest = match v {
                                    "always" => HideLatest::Always,
                                    "never" => HideLatest::Never,
                                    _ => HideLatest::Skin,
                                }
                            },
                        ),
                    ),
                    row(
                        ui,
                        "history-visible",
                        None,
                        number(
                            ui,
                            (1.0, 200.0),
                            |s| f64::from(s.history.visible_items),
                            |s, v| s.history.visible_items = v as u32,
                        ),
                    ),
                    row(
                        ui,
                        "history-max",
                        None,
                        number(
                            ui,
                            (1.0, 5000.0),
                            |s| f64::from(s.history.max_items),
                            |s, v| s.history.max_items = v as u32,
                        ),
                    ),
                    row(
                        ui,
                        "history-fade",
                        None,
                        slider_with(
                            ui,
                            (0.0, 300.0, 5.0),
                            "s",
                            off_at_zero,
                            |s| f64::from(s.history.fade_after_s.unwrap_or(0)),
                            |s, v| s.history.fade_after_s = (v > 0.0).then_some(v as u32),
                        ),
                    ),
                    toggle_row!(ui, "history-avatar", history.show_avatar),
                    toggle_row!(ui, "history-name", history.show_name),
                    toggle_row!(ui, "history-time", history.show_time),
                    row(
                        ui,
                        "history-session",
                        None,
                        confirm(
                            *ui,
                            ui.now_t("history-clear-confirm"),
                            ui.now_t("history-clear"),
                            |armed| {
                                widget(
                                    Button::new(ui.now_t("history-clear")).size(ControlSize::Small),
                                )
                                .on(move |_: &Activate| armed.set(true))
                                .into_any()
                            },
                            || Msg::Core(nanatype_core::Cmd::ClearHistory),
                        ),
                    ),
                ],
            ),
            // The filters only mean something while chat joins the history.
            section_with(
                ui,
                "history-chat",
                vec![
                    (
                        toggle_row!(ui, "history-chat-show", history.chat.enabled),
                        None,
                    ),
                    when_on(
                        ui,
                        |s| s.history.chat.enabled,
                        vec![
                            row(
                                ui,
                                "chat-filter-kinds",
                                None,
                                chip_toggles(
                                    ui,
                                    &[
                                        (
                                            "chat-filter-text",
                                            |s| s.history.chat.filter.text,
                                            |s, v| s.history.chat.filter.text = v,
                                        ),
                                        (
                                            "chat-filter-super-chat",
                                            |s| s.history.chat.filter.super_chat,
                                            |s, v| s.history.chat.filter.super_chat = v,
                                        ),
                                        (
                                            "chat-filter-gift",
                                            |s| s.history.chat.filter.gift,
                                            |s, v| s.history.chat.filter.gift = v,
                                        ),
                                        (
                                            "chat-filter-member",
                                            |s| s.history.chat.filter.member,
                                            |s, v| s.history.chat.filter.member = v,
                                        ),
                                    ],
                                ),
                            ),
                            row(
                                ui,
                                "chat-min-gift",
                                None,
                                number(
                                    ui,
                                    (0.0, 10000.0),
                                    |s| f64::from(s.history.chat.filter.min_gift_coin) / 1000.0,
                                    |s, v| {
                                        s.history.chat.filter.min_gift_coin =
                                            (v * 1000.0).round() as u32
                                    },
                                ),
                            ),
                        ],
                    ),
                    (
                        toggle_row!(ui, "history-show-quote", history.show_quote),
                        None,
                    ),
                ],
            ),
        ))
        .into_any()
}

/// "Windows →": a quiet button that opens another settings page (the page
/// header's, where the window itself and its capture are set).
pub(super) fn page_link(ui: Ui, page: Page) -> AnyView {
    widget(
        Button::new(format!("{} →", ui.now_t(page.label_key())))
            .kind(ButtonKind::Ghost)
            .size(ControlSize::Small),
    )
    .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(Msg::SettingsPage(page)))
    .into_any()
}
