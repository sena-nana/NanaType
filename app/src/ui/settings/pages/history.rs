use super::*;
use crate::archive;
use crate::msg::{HistoryMsg, WindowMsg};
use crate::ui::kit;
use nana_ui::runtime::RichTextView;

/// The viewer's scrolling height.
const VIEWER_H: f32 = 280.0;

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
                        Some("history-fade-hint"),
                        slider(
                            ui,
                            (0.0, 300.0, 5.0),
                            "s",
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
                            move |armed| {
                                widget(
                                    IconButton::new(ti::ERASER, ui.now_t("history-clear"))
                                        .size(ControlSize::Small)
                                        .with_tooltip(ui.now_t("history-clear")),
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
                                number_unit(
                                    ui,
                                    (0.0, 10000.0),
                                    "unit-yuan",
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
            archive_settings(ui),
            saved_sessions(ui),
        ))
        .into_any()
}

/// Whether sent lines are saved to disk, viewer chat with them, and for how
/// long. Retention applies even while saving is off, to what is already there.
fn archive_settings(ui: Ui) -> AnyView {
    section_with(
        ui,
        "history-archive",
        vec![
            (
                toggle_row!(ui, "history-archive-enabled", history.archive.enabled),
                None,
            ),
            when_on(
                ui,
                |s| s.history.archive.enabled,
                vec![toggle_row!(
                    ui,
                    "history-archive-chat",
                    history.archive.chat
                )],
            ),
            (
                row(
                    ui,
                    "history-archive-keep",
                    Some("history-archive-keep-hint"),
                    number_unit(
                        ui,
                        (0.0, 3650.0),
                        "unit-days",
                        |s| f64::from(s.history.archive.keep_days),
                        |s, v| s.history.archive.keep_days = v as u32,
                    ),
                ),
                None,
            ),
        ],
    )
}

/// The saved sessions, newest first; the open one shows its lines below it.
fn saved_sessions(ui: Ui) -> AnyView {
    let body = dynamic(
        move || (ui.archive.get(), ui.locale.get()),
        move |(view, _)| {
            if view.sessions.is_empty() {
                return block(hint(ui.now_t("history-saved-empty")));
            }
            let mut rows = Vec::new();
            for session in &view.sessions {
                let open = view.open.as_deref() == Some(session.name.as_str());
                let name = session.name.clone();
                rows.push(row_text(
                    session.title(),
                    Some(ui.tr(
                        "history-saved-lines",
                        &[("count", (session.lines as u64).into())],
                    )),
                    button(
                        ui,
                        if open {
                            "history-saved-close"
                        } else {
                            "history-saved-view"
                        },
                        move || Msg::History(HistoryMsg::Open((!open).then(|| name.clone()))),
                    ),
                ));
                if open {
                    rows.push(viewer(ui, view, &session.name));
                }
            }
            Row::inset(widget(Stack::column(0.0)).children(divided(rows)))
        },
    );
    section_tools(
        ui,
        "history-saved",
        vec![tool(ui, ti::FOLDER, "history-saved-folder", || {
            Msg::History(HistoryMsg::OpenFolder)
        })],
        vec![(Row::inset(body), None)],
    )
}

/// Render an archived line's original markup when it has one. Plain chat and
/// legacy records keep their text fallback; typed records retain inline images
/// and rich text in `markup`.
fn saved_body(ui: Ui, record: &archive::Record) -> AnyView {
    let Some(markup) = record.markup.as_deref() else {
        return Type::Body.view(record.text.clone());
    };
    let doc = nanatype_markup::parse(markup).doc;
    let skin = ui.skin.get_untracked();
    let mut base = crate::ui::dialogue::Base::history(
        &skin,
        nanatype_model::Entrance {
            kind: nanatype_model::EntranceKind::None,
            ms: 0,
        },
        ui.reduced_motion.get_untracked(),
    );
    base.sticker_scale = ui.settings.with_untracked(|s| s.assets.sticker_scale);
    let look = base.look();
    let urls = ui.asset_urls.get_untracked();
    let rich = crate::ui::rich::to_rich(
        &doc,
        &crate::ui::rich::Env {
            look: &look,
            urls: &urls,
            editor: false,
            locale: ui.locale.get_untracked(),
        },
    );
    widget(
        RichTextView::new(rich)
            .style(base.node_style())
            .width(LengthSpec::Fill),
    )
    .into_any()
}

/// One session's lines (time · speaker · text) in a scrolling panel, the
/// newest page first with earlier pages a click away, then its exports and
/// deletion.
fn viewer(ui: Ui, view: &archive::View, name: &str) -> Row {
    let hidden = view.hidden();
    let lines: Vec<AnyView> = view
        .listed()
        .iter()
        .map(|r| {
            widget(
                Stack::row(0.0)
                    .gap_step(gap::step::TIGHT)
                    .align(AlignSpec::Start)
                    .width(LengthSpec::Fill),
            )
            .children((
                widget(Stack::row(0.0).gap_step(gap::step::TIGHT).with_layout(|l| {
                    l.flex_shrink = Some(0.0);
                    crate::ui::tnum(l);
                }))
                .children((
                    Type::Caption.view(crate::ui::history::local_time(r.at, true)),
                    Type::Small.view(r.speaker.clone()),
                )),
                widget(kit::grow_column(0.0)).children((
                    saved_body(ui, r),
                    r.translation.clone().map(|t| {
                        widget(
                            Text::new(t)
                                .style(Type::Small.style())
                                .color(SemanticColorRole::Muted),
                        )
                    }),
                )),
            ))
            .into_any()
        })
        .collect();
    let note = (hidden > 0).then(|| {
        hint(ui.tr(
            "history-saved-truncated",
            &[
                ("shown", (view.listed().len() as u64).into()),
                ("count", (view.records.len() as u64).into()),
            ],
        ))
    });
    // Heads the list, so the earlier lines appear right under it.
    let earlier = (hidden > 0).then(|| {
        widget(
            Button::new(ui.tr(
                "history-saved-earlier",
                &[("count", (hidden.min(archive::View::PAGE) as u64).into())],
            ))
            .kind(ButtonKind::Ghost)
            .size(ControlSize::Small),
        )
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(Msg::History(HistoryMsg::Earlier)))
        .into_any()
    });
    let name = name.to_owned();
    let actions = widget(
        Stack::row(0.0)
            .gap_step(gap::step::TIGHT)
            .align(AlignSpec::Center)
            .justify(JustifySpec::End)
            .wrap(true)
            .width(LengthSpec::Fill),
    )
    .children((
        button(ui, "history-saved-export-txt", || {
            Msg::History(HistoryMsg::Export(archive::Format::Txt))
        }),
        button(ui, "history-saved-export-srt", || {
            Msg::History(HistoryMsg::Export(archive::Format::Srt))
        }),
        confirm(
            *ui,
            ui.now_t("history-saved-delete-confirm"),
            ui.now_t("history-saved-delete"),
            move |armed| {
                widget(
                    IconButton::new(ti::TRASH, ui.now_t("history-saved-delete"))
                        .size(ControlSize::Small)
                        .with_tooltip(ui.now_t("history-saved-delete")),
                )
                .on(move |_: &Activate| armed.set(true))
                .into_any()
            },
            move || Msg::History(HistoryMsg::Delete(name.clone())),
        ),
    ));
    block((
        note,
        widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|l| {
            l.max_height = Some(LengthSpec::Px(VIEWER_H));
        }))
        .children(widget(Stack::column(0.0).gap_step(gap::step::HAIR)).children((earlier, lines))),
        actions,
    ))
}

/// "Windows →": a quiet button that opens another settings page (the page
/// header's, where the window itself and its capture are set).
pub(super) fn page_link(ui: Ui, page: Page) -> AnyView {
    widget(
        Button::new(format!("{} →", ui.now_t(page.label_key())))
            .kind(ButtonKind::Ghost)
            .size(ControlSize::Small),
    )
    .on_cx(move |_, _: &Activate, cx| {
        cx.dispatch_program_all(Msg::Window(WindowMsg::SettingsPage(page)))
    })
    .into_any()
}
