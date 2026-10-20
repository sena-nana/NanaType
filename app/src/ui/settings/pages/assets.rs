use super::*;

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

pub(super) fn assets(ui: Ui) -> AnyView {
    let dirs = dynamic(
        move || (ui.sticker_packs.get(), ui.locale.get()),
        move |(packs, _)| {
            use crate::stickers::Phase;
            let rows: Vec<AnyView> = packs
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
                                ui,
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
                        widget(Stack::row(space::SM).wrap(true))
                            .children(actions)
                            .into_any(),
                    )
                })
                .collect();
            widget(Stack::column(0.0)).children(rows).into_any()
        },
    );
    let phrases = dynamic(
        move || ui.settings.with(|s| s.assets.phrases.len()),
        move |n| {
            if *n == 0 {
                return crate::ui::feedback::empty_state(ui.now_t("empty-phrases"), None);
            }
            let rows: Vec<AnyView> = (0..*n)
                .map(|i| {
                    let p = ui.settings.with_untracked(|s| s.assets.phrases[i].clone());
                    let action = GlobalAction::Phrase(i);
                    // Two lines: name, hotkey, send and remove; then the
                    // content across the full width.
                    let first = widget(
                        Stack::row(gap::CONTROL)
                            .align(AlignSpec::Center)
                            .width(LengthSpec::Fill),
                    )
                    .children((
                        widget(Stack::row(0.0).width(LengthSpec::Px(200.0))).children(
                            widget(
                                TextInput::new(p.label.clone())
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
                            ui,
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
                            TextInput::new(p.markup.clone())
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
                    widget(Stack::column(space::XS))
                        .children((
                            widget(Stack::column(space::MD).padding_xy(inset::CARD, space::XL))
                                .children((first, content)),
                            hotkey_issue_line(ui, action),
                        ))
                        .into_any()
                })
                .collect();
            widget(Stack::column(0.0))
                .children(divided(rows))
                .into_any()
        },
    );
    widget(Stack::column(gap::SECTION))
        .children((
            section(
                ui,
                "assets-stickers",
                vec![
                    widget(Stack::column(0.0).padding_xy(inset::CARD, space::LG))
                        .children(hint(ui.now_t("assets-stickers-hint")))
                        .into_any(),
                    dirs.into_any(),
                    widget(Stack::row(0.0).padding_xy(inset::CARD, space::LG))
                        .children(button(ui, "assets-add-folder", || {
                            Msg::PickFile(FileTarget::StickerFolder)
                        }))
                        .into_any(),
                ],
            ),
            crate::ui::settings::telegram::panel(ui),
            section(
                ui,
                "assets-phrases",
                vec![
                    phrases.into_any(),
                    widget(Stack::row(0.0).padding_xy(inset::CARD, space::LG))
                        .children(button(ui, "assets-phrase-add", || {
                            edit(|s| {
                                s.assets.phrases.push(QuickPhrase {
                                    label: String::new(),
                                    markup: String::new(),
                                    send: false,
                                    hotkey: None,
                                })
                            })
                        }))
                        .into_any(),
                ],
            ),
        ))
        .into_any()
}
