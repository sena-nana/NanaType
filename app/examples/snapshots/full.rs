//! The default capture: every window and settings page.

use super::*;

/// Every window, page and state, in the order the review reads them.
pub fn capture(scene: &Scene, out: &std::path::Path, iw: u32, ih: u32) {
    douyin(scene, out);
    blivechat(scene, out);
    p2p(scene, out, iw, ih);
    if std::env::var_os("NANATYPE_P2P_SNAPSHOTS_ONLY").is_some() {
        return;
    }
    overlays(scene, out);
    settings_pages(scene, out);
    empty_input(scene, out, iw, ih);
    links(scene, out);
}

/// The Douyin settings page and Douyin chat mixed into the feed.
fn douyin(scene: &Scene, out: &std::path::Path) {
    let ui = scene.ui;
    let theme = &scene.theme;
    let before_dy_settings = ui.settings.get_untracked();
    let before_dy_chat = ui.chat.get_untracked();
    let before_dy_state = ui.douyin.get_untracked();
    let before_dy_page = ui.page.get_untracked();
    let mut dy_settings = (*before_dy_settings).clone();
    dy_settings.integrations.douyin.enabled = true;
    dy_settings.integrations.douyin.room = "123456789".into();
    ui.settings.set(Arc::new(dy_settings));
    ui.douyin.set(Arc::new(nanatype::app::douyin::State {
        account: nanatype_douyin::AccountState::SignedOut,
        connection: nanatype_douyin::ConnectionState::Connected {
            room: "123456789".into(),
        },
        warning: None,
    }));
    ui.platform_tab.set(nanatype::chat::Platform::Douyin);
    ui.page.set(settings::Page::Platforms);
    shot("dy-settings", out, 920, 1050, theme.clone(), |doc, id| {
        doc.context_mut()
            .mount_view_root(id, move || settings::settings_window(ui))
            .unwrap();
    });
    let mut mixed = (*before_dy_chat).clone();
    for (n, content) in [
        nanatype_douyin::Content::Text("抖音观众：你好！Hello from Douyin 👋".into()),
        nanatype_douyin::Content::Gift {
            name: "小心心".into(),
            count: 3,
            diamonds: Some(1),
        },
    ]
    .into_iter()
    .enumerate()
    {
        let mut message = ChatMessage::from_douyin(
            nanatype_douyin::ChatEvent {
                id: format!("dy-{n}"),
                author: nanatype_douyin::Author {
                    uid: "42".into(),
                    name: "抖音观众".into(),
                    avatar: String::new(),
                },
                content,
                at: nanatype_model::unix_millis() + n as u64,
            },
            None,
            None,
            9000 + n as u64,
        );
        message.room = "123456789".into();
        mixed.push(Arc::new(message));
    }
    ui.chat.set(Arc::new(mixed));
    shot("dy-chat", out, 620, 700, theme.clone(), |doc, id| {
        doc.context_mut()
            .mount_view_root(id, move || danmaku::chat_window(ui))
            .unwrap();
    });
    ui.chat.set(before_dy_chat);
    ui.settings.set(before_dy_settings);
    ui.douyin.set(before_dy_state);
    ui.page.set(before_dy_page);
}

/// The third-party chat page in both sources: a blivechat server joined by
/// room number, and 彗星号 with two plugin pages pushing to the relay.
fn blivechat(scene: &Scene, out: &std::path::Path) {
    use nanatype_blive::{BliveStatus, RelayStatus};
    use nanatype_settings::{BliveRoomKind, BlivechatSource};
    let ui = scene.ui;
    let theme = &scene.theme;
    let before_settings = ui.settings.get_untracked();
    let before_state = ui.blivechat.get_untracked();
    let before_status = ui.status.get_untracked();
    let before_page = ui.page.get_untracked();
    ui.platform_tab.set(nanatype::chat::Platform::Blivechat);
    ui.page.set(settings::Page::Platforms);
    for (name, source, relay) in [
        ("blive-settings", BlivechatSource::Server, RelayStatus::Off),
        (
            "comet-settings",
            BlivechatSource::Comet,
            RelayStatus::Listening { publishers: 1 },
        ),
    ] {
        let mut s = (*before_settings).clone();
        let b = &mut s.integrations.blivechat;
        b.enabled = true;
        b.source = source;
        b.room_kind = BliveRoomKind::RoomId;
        b.room_id = "21452505".into();
        b.comet_token = "3f9c0a7d5e214b6c8a0f1e2d3c4b5a69".into();
        let state = nanatype::app::blivechat::State {
            status: BliveStatus::Connected,
            relay,
            auth_saved: false,
        };
        let mut status = (*before_status).clone();
        (status.blivechat, status.blivechat_link) =
            link_text::blivechat(ui.locale.get_untracked(), &s.integrations.blivechat, &state);
        assert_consistent(&s, &status);
        ui.settings.set(Arc::new(s));
        ui.blivechat.set(Arc::new(state));
        ui.status.set(Arc::new(status));
        shot(name, out, 920, 1100, theme.clone(), |doc, id| {
            doc.context_mut()
                .mount_view_root(id, move || settings::settings_window(ui))
                .unwrap();
        });
    }
    ui.settings.set(before_settings);
    ui.blivechat.set(before_state);
    ui.status.set(before_status);
    ui.page.set(before_page);
}

/// The partner session: its input panel, both displays and the settings it adds.
fn p2p(scene: &Scene, out: &std::path::Path, iw: u32, ih: u32) {
    let (ui, w, h) = (scene.ui, scene.w, scene.h);
    let theme = &scene.theme;
    let skin = &scene.skin;
    let settings = &scene.settings;
    ui.peer_panel.set(true);
    shot("p2p-create", out, iw, ih, theme.clone(), |doc, id| {
        doc.context_mut()
            .mount_view_root(id, move || input::input_window(ui))
            .unwrap();
    });
    let mut peer_settings = settings.clone();
    peer_settings.general.speaker_name = "联机伙伴 / Partner".into();
    let mut peer_core = Core::new(peer_settings, skin.clone());
    peer_core.handle(
        Cmd::SendMarkup(
            "[color=#689fd7]我这边也能看到你的台词！[/color]\n两个人可以同时显示，也可以独立继续。"
                .into(),
        ),
        0,
    );
    peer_core.handle(Cmd::Skip, 1);
    ui.peer_display.set(Arc::new(peer_core.display().clone()));
    ui.peer.set(Arc::new(nanatype::p2p::SessionUi {
        active: true,
        host: true,
        invitation: "nanatype1:preview".into(),
        name: "联机伙伴".into(),
        status: nanatype_p2p::Status::Connected {
            route: "direct".into(),
            rtt_ms: 28,
        },
    }));
    shot("p2p-input", out, iw, ih, theme.clone(), |doc, id| {
        doc.context_mut()
            .mount_view_root(id, move || input::input_window(ui))
            .unwrap();
    });
    shot("p2p-local-display", out, w, h, theme.clone(), |doc, id| {
        doc.context_mut()
            .mount_view_root(id, move || dialogue::display_window(ui))
            .unwrap();
    });
    shot(
        "p2p-partner-display",
        out,
        w,
        h,
        theme.clone(),
        |doc, id| {
            doc.context_mut()
                .mount_view_root(id, move || dialogue::peer_window(ui))
                .unwrap();
        },
    );
    for page in [settings::Page::Windows, settings::Page::Output] {
        ui.page.set(page);
        shot(
            &format!("p2p-settings-{page:?}"),
            out,
            920,
            900,
            theme.clone(),
            |doc, id| {
                doc.context_mut()
                    .mount_view_root(id, move || settings::settings_window(ui))
                    .unwrap();
            },
        );
    }
    ui.page.set(settings::Page::Windows);
    shot(
        "p2p-settings-Windows-narrow",
        out,
        720,
        660,
        theme.clone(),
        |doc, id| {
            doc.context_mut()
                .mount_view_root(id, move || settings::settings_window(ui))
                .unwrap();
        },
    );
    let before_output = ui.settings.get_untracked();
    let mut enabled_output = (*before_output).clone();
    enabled_output.output.display.enabled = true;
    enabled_output.output.peer_display.enabled = true;
    enabled_output.output.history.enabled = true;
    ui.settings.set(Arc::new(enabled_output));
    let before_output_status = ui.status.get_untracked();
    let mut output_status = (*before_output_status).clone();
    output_status.spout_display = LinkState::Connecting;
    output_status.spout_peer_display = LinkState::Connecting;
    output_status.spout_history = LinkState::Connecting;
    ui.status.set(Arc::new(output_status));
    ui.page.set(settings::Page::Output);
    shot(
        "p2p-settings-Output-enabled",
        out,
        920,
        1400,
        theme.clone(),
        |doc, id| {
            doc.context_mut()
                .mount_view_root(id, move || settings::settings_window(ui))
                .unwrap();
        },
    );
    ui.settings.set(before_output);
    ui.status.set(before_output_status);
    ui.peer.set(Arc::new(nanatype::p2p::SessionUi::default()));
    ui.peer_panel.set(false);
}

/// The toolbar's sheets, the history window and the chat panel.
fn overlays(scene: &Scene, out: &std::path::Path) {
    let ui = scene.ui;
    let theme = &scene.theme;
    let skin = &scene.skin;
    // The toolbar's `?` sheet as its popover shows it.
    shot(
        "input-cheatsheet",
        out,
        620,
        250,
        theme.clone(),
        |doc, id| {
            doc.context_mut()
                .mount_view_root(id, move || {
                    nana_ui::runtime::view::widget(
                        nana_ui::runtime::Stack::column(0.0)
                            .padding(8.0)
                            .surface(nana_ui_core::SemanticColorRole::Surface),
                    )
                    .children(input::cheat_sheet(ui))
                })
                .unwrap();
        },
    );
    // The toolbar's sticker panel, listing the sticker folder.
    shot("input-stickers", out, 380, 200, theme.clone(), |doc, id| {
        doc.context_mut()
            .mount_view_root(id, move || {
                nana_ui::runtime::view::widget(
                    nana_ui::runtime::Stack::column(0.0)
                        .padding(8.0)
                        .surface(nana_ui_core::SemanticColorRole::Surface),
                )
                .children(input::sticker_panel(
                    ui,
                    nana_ui::runtime::view::signal(true),
                ))
            })
            .unwrap();
    });
    shot(
        "history",
        out,
        skin.history.width as u32,
        skin.history.height as u32,
        theme.clone(),
        |doc, id| {
            doc.context_mut()
                .mount_view_root(id, move || history::history_window(ui))
                .unwrap();
        },
    );
    // The newest rows alone: the quoted chat message and the reply that
    // quotes it and @-mentions the viewer, unclipped at the list's foot.
    // The box has cleared, so skins that hide the line still on it (the
    // echo_* logs, `hide_latest`) list the reply too.
    let all_rows = ui.history.get_untracked();
    ui.history.set(Arc::new(
        all_rows[all_rows.len().saturating_sub(3)..].to_vec(),
    ));
    let on_screen = ui.display.get_untracked();
    ui.display.set(Arc::new(nanatype_core::DisplayView::empty(
        nanatype_model::Entrance {
            kind: nanatype_model::EntranceKind::None,
            ms: 0,
        },
    )));
    shot(
        "history-reply",
        out,
        skin.history.width as u32,
        skin.history.height as u32,
        theme.clone(),
        |doc, id| {
            doc.context_mut()
                .mount_view_root(id, move || history::history_window(ui))
                .unwrap();
        },
    );
    ui.history.set(all_rows);
    ui.display.set(on_screen);
    shot("chat", out, 400, 640, theme.clone(), |doc, id| {
        doc.context_mut()
            .mount_view_root(id, move || danmaku::chat_window(ui))
            .unwrap();
    });
    let mut card_settings = (*ui.settings.get_untracked()).clone();
    card_settings.integrations.bilibili.panel_style = nanatype_settings::ChatPanelStyle::History;
    ui.settings.set(Arc::new(card_settings));
    shot("chat-cards", out, 400, 640, theme.clone(), |doc, id| {
        doc.context_mut()
            .mount_view_root(id, move || danmaku::chat_window(ui))
            .unwrap();
    });
}

/// Every settings page, the first-run guide, About's fonts and the skin list.
fn settings_pages(scene: &Scene, out: &std::path::Path) {
    let ui = scene.ui;
    let theme = &scene.theme;
    let settings = &scene.settings;
    ui.settings.set(Arc::new(settings.clone()));
    for page in settings::Page::ALL {
        ui.page.set(page);
        shot(
            &format!("settings-{page:?}"),
            out,
            920,
            660,
            theme.clone(),
            |doc, id| {
                doc.context_mut()
                    .mount_view_root(id, move || settings::settings_window(ui))
                    .unwrap();
            },
        );
    }
    // The first-run guide, one shot per step.
    ui.page.set(settings::Page::General);
    for n in 0..4u8 {
        ui.onboard_step.set(n);
        shot(
            &format!("settings-General-onboard-{}", n + 1),
            out,
            920,
            760,
            theme.clone(),
            |doc, id| {
                doc.context_mut()
                    .mount_view_root(id, move || settings::settings_window(ui))
                    .unwrap();
            },
        );
    }
    ui.onboard_step.set(0);

    // About's "Open-source fonts" panel, as it opens under its row.
    shot(
        "settings-About-fonts",
        out,
        760,
        380,
        theme.clone(),
        |doc, id| {
            doc.context_mut()
                .mount_view_root(id, move || {
                    nana_ui::runtime::view::widget(
                        nana_ui::runtime::Stack::column(0.0)
                            .surface(nana_ui_core::SemanticColorRole::Surface),
                    )
                    .children(settings::font_licence_panel(ui))
                })
                .unwrap();
        },
    );

    // Pages taller than the window, whole: the skin grid and the history
    // page's last row.
    for (page, h) in [
        (settings::Page::Skins, 1700),
        (settings::Page::History, 1100),
        (settings::Page::About, 1600),
    ] {
        ui.page.set(page);
        shot(
            &format!("settings-{page:?}-full"),
            out,
            920,
            h,
            theme.clone(),
            |doc, id| {
                doc.context_mut()
                    .mount_view_root(id, move || settings::settings_window(ui))
                    .unwrap();
            },
        );
    }
    // The skin library as a list (the view switch's other state), in the
    // window and whole.
    ui.page.set(settings::Page::Skins);
    ui.skins_grid.set(false);
    for (name, h) in [
        ("settings-Skins-list", 660),
        ("settings-Skins-list-full", 1900),
    ] {
        shot(name, out, 920, h, theme.clone(), |doc, id| {
            doc.context_mut()
                .mount_view_root(id, move || settings::settings_window(ui))
                .unwrap();
        });
    }
    ui.skins_grid.set(true);
}

/// The input window with nothing typed.
fn empty_input(scene: &Scene, out: &std::path::Path, iw: u32, ih: u32) {
    let ui = scene.ui;
    let theme = &scene.theme;
    // An empty draft shows the editor's placeholder.
    ui.draft.set(Arc::new(Document::new()));
    ui.editor_value.set(nana_ui_core::RichText::new(""));
    ui.reply.set(None);
    shot("input-empty", out, iw, ih, theme.clone(), |doc, id| {
        doc.context_mut()
            .mount_view_root(id, move || input::input_window(ui))
            .unwrap();
    });
    // The footer's status badges: a line waiting its turn, then mirror mode.
    let before_status = ui.status.get_untracked();
    let before_settings = ui.settings.get_untracked();
    let mut queued = (*before_status).clone();
    queued.queued = 2;
    ui.status.set(Arc::new(queued));
    shot("input-queued", out, iw, ih, theme.clone(), |doc, id| {
        doc.context_mut()
            .mount_view_root(id, move || input::input_window(ui))
            .unwrap();
    });
    ui.status.set(before_status);
    let mut mirror = (*before_settings).clone();
    mirror.general.input_mode = nanatype_settings::InputMode::Mirror;
    ui.settings.set(Arc::new(mirror));
    shot("input-mirror", out, iw, ih, theme.clone(), |doc, id| {
        doc.context_mut()
            .mount_view_root(id, move || input::input_window(ui))
            .unwrap();
    });
    ui.settings.set(before_settings);
}

/// The link pages with every link on, as the clients report it.
fn links(scene: &Scene, out: &std::path::Path) {
    let ui = scene.ui;
    let theme = &scene.theme;
    let settings = &scene.settings;
    let locale = scene.locale;
    // The same two pages with every link switched on, one in each state.
    // The states come from the clients' own reports, turned into text the
    // way the app does, so the pages read in the shot's language.
    let mut on = settings.clone();
    on.integrations.vts.enabled = true;
    on.integrations.osc.enabled = true;
    on.integrations.websocket.enabled = true;
    on.output.display.enabled = true;
    on.output.history.enabled = true;
    let status = reported(
        locale,
        &[
            BridgeStatus::VtsNeedsToken,
            BridgeStatus::Connecting(Sink::Osc),
            BridgeStatus::Connected(Sink::WebSocket),
            BridgeStatus::Clients(2),
        ],
        [
            WindowOutputStatus::Active {
                extent: [1920, 1080],
                native: true,
            },
            WindowOutputStatus::NativeUnavailable {
                reason: "no GPU".into(),
            },
        ],
        &ConnectionState::Error("connection refused".into()),
        &on.integrations.bilibili.room,
    );
    assert_consistent(&on, &status);
    ui.settings.set(Arc::new(on));
    ui.status.set(Arc::new(status));
    for page in [settings::Page::Integrations, settings::Page::Output] {
        ui.page.set(page);
        shot(
            &format!("settings-{page:?}-on"),
            out,
            920,
            1100,
            theme.clone(),
            |doc, id| {
                doc.context_mut()
                    .mount_view_root(id, move || settings::settings_window(ui))
                    .unwrap();
            },
        );
    }

    // VTube Studio authorised with a model's hotkeys listed (one mapped by
    // hand), and quick phrases with a hotkey.
    let mut on = (*ui.settings.get_untracked()).clone();
    on.integrations.bilibili.enabled = false;
    on.integrations.vts.token = Some("token".into());
    on.integrations
        .vts
        .expressions
        .insert("blush".into(), "hk-3".into());
    on.assets.phrases = vec![
        nanatype_settings::QuickPhrase {
            label: "开场".into(),
            markup: "大家好，我是[b]{speaker}[/b]！".into(),
            send: true,
            hotkey: Some(nanatype_settings::Hotkey {
                alt: true,
                ..nanatype_settings::Hotkey::ctrl("Digit1")
            }),
        },
        nanatype_settings::QuickPhrase {
            label: "感谢".into(),
            markup: "谢谢[wave]大家[/wave]～".into(),
            send: false,
            hotkey: None,
        },
    ];
    // The other links stay on, and say so.
    let mut status = reported(
        locale,
        &[
            BridgeStatus::Connected(Sink::Vts),
            BridgeStatus::Connected(Sink::Osc),
            BridgeStatus::Connected(Sink::WebSocket),
            BridgeStatus::Clients(1),
        ],
        [
            WindowOutputStatus::Active {
                extent: [1920, 1080],
                native: true,
            },
            WindowOutputStatus::Active {
                extent: [420, 560],
                native: true,
            },
        ],
        &ConnectionState::Off,
        &on.integrations.bilibili.room,
    );
    status.vts_hotkeys = vec![
        ("hk-1".into(), "Smile".into()),
        ("hk-2".into(), "Angry Face".into()),
        ("hk-3".into(), "Heart Eyes".into()),
    ];
    assert_consistent(&on, &status);
    ui.settings.set(Arc::new(on));
    ui.status.set(Arc::new(status));
    for page in [settings::Page::Integrations, settings::Page::Assets] {
        ui.page.set(page);
        shot(
            &format!("settings-{page:?}-filled"),
            out,
            920,
            1100,
            theme.clone(),
            |doc, id| {
                doc.context_mut()
                    .mount_view_root(id, move || settings::settings_window(ui))
                    .unwrap();
            },
        );
    }
}
