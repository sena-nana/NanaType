use super::*;
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
    button(ui, "assets-add-folder", || {
        Msg::PickFile(FileTarget::StickerFolder)
    })
}

/// "Add phrase", which appends an empty quick phrase.
fn add_phrase(ui: Ui) -> AnyView {
    button(ui, "assets-phrase-add", || {
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

/// `rows` with `add` at the end of the last line, or the empty state with
/// `add` beside it when there are none.
fn list_or_empty(ui: Ui, rows: Vec<Row>, empty_key: &str, add: AnyView) -> Row {
    if rows.is_empty() {
        return empty_row(ui, empty_key, add);
    }
    Row::inset(widget(Stack::column(0.0)).children((divided(rows), divider(), kit::line((), add))))
}

pub(super) fn assets(ui: Ui) -> AnyView {
    let dirs = dynamic(
        move || (ui.sticker_packs.get(), ui.locale.get()),
        move |(packs, _)| {
            use crate::stickers::Phase;
            let rows: Vec<Row> = packs
                .iter()
                .map(|pack| {
                    let title = pack
                        .preset
                        .map(|p| ui.now_t(p.title_key()))
                        .unwrap_or_else(|| pack.title.clone());
                    let status = match &pack.phase {
                        Phase::NotDownloaded => ui.now_t("stickers-not-downloaded"),
                        Phase::Downloading(done, total) => ui.tr(
                            "stickers-downloading",
                            &[
                                ("done", (*done as u64).into()),
                                ("total", (*total as u64).into()),
                            ],
                        ),
                        Phase::Processing(done, total) => ui.tr(
                            "stickers-processing",
                            &[
                                ("done", (*done as u64).into()),
                                ("total", (*total as u64).into()),
                            ],
                        ),
                        Phase::Ready => {
                            ui.tr("stickers-ready", &[("count", (pack.count as u64).into())])
                        }
                        Phase::Failed(error) => {
                            ui.tr("stickers-failed", &[("message", error.clone().into())])
                        }
                    };
                    let details = [
                        status,
                        pack.author.clone(),
                        pack.license.clone(),
                        if pack.preset.is_none() {
                            pack.root.display().to_string()
                        } else {
                            String::new()
                        },
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
                        actions.push(button(ui, label, move || {
                            Msg::StickerRepository(root.clone())
                        }));
                    }
                    if !pack.phase.busy() && pack.phase != Phase::Ready {
                        let root = pack.root.clone();
                        let preset = pack.preset;
                        actions.push(button(
                            ui,
                            if pack.phase == Phase::NotDownloaded {
                                "stickers-download"
                            } else {
                                "stickers-retry"
                            },
                            move || {
                                preset
                                    .map(Msg::StickerDownload)
                                    .unwrap_or_else(|| Msg::StickerRetry(root.clone()))
                            },
                        ));
                    }
                    if pack.phase != Phase::NotDownloaded {
                        let root = pack.root.clone();
                        if pack.phase.busy() {
                            actions.push(button(ui, "common-cancel", move || {
                                Msg::StickerRemove(root.clone())
                            }));
                        } else {
                            actions.push(confirm(
                                *ui,
                                ui.now_t("assets-remove-folder-confirm"),
                                ui.now_t("assets-remove"),
                                |armed| remove_button(ui, armed),
                                move || Msg::StickerRemove(root.clone()),
                            ));
                        }
                    }
                    row_text(
                        title,
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
            list_or_empty(ui, rows, "empty-stickers", add_folder(ui))
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
                            |armed| remove_button(ui, armed),
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
            list_or_empty(ui, rows, "empty-phrases", add_phrase(ui))
        },
    );
    widget(Stack::column(0.0).gap_step(gap::step::SECTION))
        .children((
            section(ui, "assets-stickers", vec![Row::inset(dirs)]),
            crate::ui::settings::telegram::panel(ui),
            section(ui, "assets-phrases", vec![Row::inset(phrases)]),
        ))
        .into_any()
}
