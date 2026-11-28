use super::hotkeys::{hotkey_control, hotkey_issue_line};
use super::*;
use crate::msg::StickerMsg;
use crate::ui::kit;

/// A trash button that arms a [`confirm`].
pub(super) fn remove_button(ui: Ui, armed: Signal<bool>) -> AnyView {
    widget(
        IconButton::new(ti::TRASH, ui.now_t("assets-remove"))
            .size(ControlSize::Small)
            .with_tooltip(ui.now_t("assets-remove")),
    )
    .on(move |_: &Activate| armed.set(true))
    .into_any()
}

/// "Add folder…", which picks a sticker folder.
fn add_folder(ui: Ui) -> AnyView {
    tool(ui, ti::PLUS, "assets-add-folder", || {
        Msg::PickFile(FileTarget::StickerFolder)
    })
}

/// "Add phrase", which appends an empty quick phrase.
fn add_phrase(ui: Ui) -> AnyView {
    tool(ui, ti::PLUS, "assets-phrase-add", || {
        edit(|s| {
            s.assets.phrases.push(QuickPhrase {
                label: String::new(),
                markup: String::new(),
                send: false,
                hotkey: None,
            })
        })
    })
}

/// How tall stickers are drawn, in line heights.
fn sticker_size(ui: Ui) -> Row {
    let (min, max) = nanatype_settings::STICKER_SCALE;
    row(
        ui,
        "assets-sticker-size",
        Some("assets-sticker-size-hint"),
        slider(
            ui,
            (f64::from(min), f64::from(max), 0.1),
            "×",
            |s| f64::from(s.assets.sticker_scale),
            |s, v| {
                // Whole tenths, so the reading does not drift off the step.
                let v = ((v * 10.0).round() / 10.0) as f32;
                let (min, max) = nanatype_settings::STICKER_SCALE;
                s.assets.sticker_scale = v.clamp(min, max);
            },
        ),
    )
}

/// `rows`, or the empty state when there are none.
fn list_or_empty(ui: Ui, rows: Vec<Row>, empty_key: &str) -> Row {
    if rows.is_empty() {
        return block(hint(ui.now_t(empty_key)));
    }
    Row::inset(widget(Stack::column(0.0)).children(divided(rows)))
}

pub(super) fn assets(ui: Ui) -> AnyView {
    let dirs = dynamic(
        move || (ui.sticker_packs.get(), ui.locale.get()),
        move |(packs, _)| {
            use crate::stickers::Phase;
            // The collab presets live on the Packages page; this list is the
            // folders the user brought.
            let rows: Vec<Row> = packs
                .iter()
                .filter(|pack| pack.preset.is_none())
                .map(|pack| {
                    let details = [
                        crate::ui::plugins::sticker_status(ui, pack),
                        pack.author.clone(),
                        pack.license.clone(),
                        pack.root.display().to_string(),
                    ]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · ");
                    let mut actions = Vec::new();
                    if pack
                        .repository
                        .as_deref()
                        .is_some_and(|url| url.starts_with("https://"))
                    {
                        let root = pack.root.clone();
                        let label = if pack
                            .repository
                            .as_deref()
                            .is_some_and(|u| u.starts_with("https://t.me/"))
                        {
                            "telegram-source"
                        } else {
                            "stickers-repository"
                        };
                        actions.push(icon_action(ui, label, ti::EXTERNAL_LINK, move || {
                            Msg::Sticker(StickerMsg::Repository(root.clone()))
                        }));
                    }
                    if matches!(pack.phase, Phase::Failed(_)) {
                        let root = pack.root.clone();
                        actions.push(icon_action(ui, "stickers-retry", ti::REFRESH, move || {
                            Msg::Sticker(StickerMsg::Retry(root.clone()))
                        }));
                    }
                    if pack.phase != Phase::NotDownloaded {
                        let root = pack.root.clone();
                        if pack.phase.busy() {
                            actions.push(icon_action(ui, "common-cancel", ti::X, move || {
                                Msg::Sticker(StickerMsg::Remove(root.clone()))
                            }));
                        } else {
                            actions.push(confirm(
                                *ui,
                                ui.now_t("assets-remove-folder-confirm"),
                                ui.now_t("assets-remove"),
                                move |armed| remove_button(ui, armed),
                                move || Msg::Sticker(StickerMsg::Remove(root.clone())),
                            ));
                        }
                    }
                    row_text(
                        pack.title.clone(),
                        Some(details),
                        widget(
                            Stack::row(0.0)
                                .gap_step(nana_ui_core::SpacingStep::Sm)
                                .wrap(true),
                        )
                        .children(actions)
                        .into_any(),
                    )
                })
                .collect();
            list_or_empty(ui, rows, "empty-stickers")
        },
    );
    let phrases = dynamic(
        move || ui.settings.with(|s| s.assets.phrases.len()),
        move |n| {
            let rows: Vec<Row> = (0..*n)
                .map(|i| {
                    let p = ui.settings.with_untracked(|s| s.assets.phrases[i].clone());
                    let action = Shortcut::Global(GlobalAction::Phrase(i));
                    // Two lines: name, hotkey, send and remove; then the
                    // content across the full width.
                    let first = widget(
                        Stack::row(0.0)
                            .gap_step(gap::step::CONTROL)
                            .align(AlignSpec::Center)
                            .width(LengthSpec::Fill),
                    )
                    .children((
                        widget(Stack::row(0.0).width(LengthSpec::Px(field::MD))).children(
                            widget(
                                kit::text_input(p.label.clone())
                                    .size(ControlSize::Small)
                                    .placeholder(ui.now_t("assets-phrase-label")),
                            )
                            .on_cx(move |_, e: &TextChanged, cx| {
                                let v = e.value.to_string();
                                cx.dispatch_program_all(edit(move |s| {
                                    if let Some(p) = s.assets.phrases.get_mut(i) {
                                        p.label = v;
                                    }
                                }))
                            }),
                        ),
                        hotkey_control(ui, action),
                        widget(Stack::row(0.0).grow(1.0)),
                        widget(
                            Text::new(ui.now_t("assets-phrase-send"))
                                .style(Type::Small.style())
                                .color(SemanticColorRole::Muted),
                        ),
                        widget(Switch::new("", p.send))
                            .bind(move |w: &mut Switch| {
                                w.checked = ui
                                    .settings
                                    .with(|s| s.assets.phrases.get(i).is_some_and(|p| p.send))
                            })
                            .on_cx(move |_, e: &ToggleChanged, cx| {
                                let v = e.checked;
                                cx.dispatch_program_all(edit(move |s| {
                                    if let Some(p) = s.assets.phrases.get_mut(i) {
                                        p.send = v;
                                    }
                                }))
                            }),
                        confirm(
                            *ui,
                            ui.now_t("assets-phrase-remove-confirm"),
                            ui.now_t("assets-remove"),
                            move |armed| remove_button(ui, armed),
                            move || {
                                edit(move |s| {
                                    if i < s.assets.phrases.len() {
                                        s.assets.phrases.remove(i);
                                    }
                                })
                            },
                        ),
                    ));
                    // Markup source, in the mono role.
                    let content = widget(
                        Stack::row(0.0)
                            .width(LengthSpec::Fill)
                            .with_layout(crate::ui::mono),
                    )
                    .children(
                        widget(
                            kit::text_input(p.markup.clone())
                                .size(ControlSize::Small)
                                .placeholder(ui.now_t("assets-phrase-markup")),
                        )
                        .on_cx(move |_, e: &TextChanged, cx| {
                            let v = e.value.to_string();
                            cx.dispatch_program_all(edit(move |s| {
                                if let Some(p) = s.assets.phrases.get_mut(i) {
                                    p.markup = v;
                                }
                            }))
                        }),
                    );
                    Row::inset(
                        widget(Stack::column(0.0).gap_step(nana_ui_core::SpacingStep::Xs))
                            .children((
                                widget(
                                    Stack::column(0.0)
                                        .gap_step(nana_ui_core::SpacingStep::Md)
                                        .padding_step(
                                            inset::step::CARD,
                                            nana_ui_core::SpacingStep::Xl,
                                        ),
                                )
                                .children((first, content)),
                                hotkey_issue_line(ui, action),
                            )),
                    )
                })
                .collect();
            list_or_empty(ui, rows, "empty-phrases")
        },
    );
    widget(Stack::column(0.0).gap_step(gap::step::SECTION))
        .children((
            section_tools(
                ui,
                "assets-stickers",
                vec![add_folder(ui)],
                vec![(sticker_size(ui), None), (Row::inset(dirs), None)],
            ),
            crate::ui::settings::telegram::panel(ui),
            section_tools(
                ui,
                "assets-phrases",
                vec![add_phrase(ui)],
                vec![(Row::inset(phrases), None)],
            ),
        ))
        .into_any()
}
