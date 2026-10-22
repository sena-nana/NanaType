#![cfg(feature = "unofficial-chat")]

mod common;

use nana_ui_devtools::agent::RuntimeAgentSession;
use nanatype::{
    app::bilibili::State,
    msg::Msg,
    ui::{Ui, settings},
};
use nanatype_bilibili::{AccountState, ConnectionState, Profile, QrStatus, Room};
use nanatype_i18n::Locale;
use std::sync::Arc;

fn ui(locale: Locale) -> Ui {
    let skin = common::skin("nanatype.halo");
    let ui = Ui::new(
        Arc::new(nanatype_settings::Settings::default()),
        skin,
        locale,
    );
    ui.platform_tab.set(nanatype::chat::Platform::Bilibili);
    ui.page.set(settings::Page::Platforms);
    ui
}
fn session(ui: Ui, dark: bool) -> RuntimeAgentSession {
    let mut session = common::session()
        .themed(dark)
        .mount(1120, 900, move || settings::settings_window(ui));
    session.flush().unwrap();
    session
}
fn ready() -> State {
    State {
        account: AccountState::Ready(Profile {
            uid: 7,
            name: "测试主播".into(),
            avatar_url: String::new(),
            room_id: Some(100),
        }),
        connection: ConnectionState::Connected(Room {
            id: 100,
            owner: 7,
            title: "test room".into(),
            max_chars: 20,
        }),
        ..State::default()
    }
}
#[test]
fn account_states_render_in_all_locales_and_appearances() {
    for locale in [Locale::ZhCn, Locale::EnUs, Locale::JaJp] {
        for dark in [false, true] {
            for account in [
                AccountState::SignedOut,
                AccountState::Qr {
                    url: "https://passport.bilibili.com/fixture".into(),
                    status: QrStatus::Waiting,
                },
                AccountState::Qr {
                    url: "fixture".into(),
                    status: QrStatus::Expired,
                },
                ready().account,
            ] {
                let ui = ui(locale);
                ui.bilibili.set(Arc::new(State {
                    account: account.clone(),
                    ..State::default()
                }));
                let session = session(ui, dark);
                let nodes = session.accessibility_dump();
                assert!(
                    nodes
                        .iter()
                        .any(|n| n.label.as_deref() == Some(ui.now_t("bili-account").as_str()))
                );
                let expected = if matches!(account, AccountState::Ready(_)) {
                    ui.now_t("bili-logout")
                } else if matches!(account, AccountState::Qr { .. }) {
                    ui.now_t("bili-qr-refresh")
                } else {
                    ui.now_t("bili-login")
                };
                assert!(
                    nodes.iter().any(
                        |n| n.role == "button" && n.label.as_deref() == Some(expected.as_str())
                    ),
                    "missing {expected}: {nodes:?}"
                );
            }
        }
    }
}
#[test]
fn confirmation_is_a_real_modal_and_dispatches_only_after_confirmation() {
    let ui = ui(Locale::ZhCn);
    let mut state = ready();
    state.confirm = true;
    ui.bilibili.set(Arc::new(state));
    let mut session = session(ui, false);
    let nodes = session.accessibility_dump();
    let label = ui.now_t("bili-enable");
    let confirm = nodes
        .iter()
        .find(|n| n.role == "button" && n.label.as_deref() == Some(label.as_str()))
        .expect("visible confirmation action");
    assert!(!ui.bilibili.get_untracked().sending);
    assert!(session.click_node(confirm.id).unwrap());
    let messages = session.document_mut().context_mut().take_program_messages();
    assert!(messages.into_iter().any(|m| {
        m.downcast::<Msg>()
            .is_ok_and(|m| matches!(*m, Msg::BilibiliSendConfirm(true)))
    }));
}

#[test]
fn narrow_editor_keeps_account_send_and_sync_controls_visible() {
    for locale in [Locale::ZhCn, Locale::EnUs, Locale::JaJp] {
        let ui = ui(locale);
        let mut settings = (*ui.settings.get_untracked()).clone();
        settings.skin.overrides.region = Some([
            420.0 - ui.skin.get_untracked().dialogue.overscan * 2.0,
            200.0,
        ]);
        ui.settings.set(Arc::new(settings));
        ui.bilibili.set(Arc::new(ready()));
        let height = nanatype::windows::INPUT_SIZE.1;
        let mut session = common::session()
            .themed(false)
            .mount(420, height as u32, move || {
                nanatype::ui::input::input_window(ui)
            });
        // Client measurement updates the responsive footer on the following frame.
        for _ in 0..3 {
            session.flush().unwrap();
        }
        let nodes = session.accessibility_dump();
        for key in [
            "bili-account",
            "bili-send-switch",
            "tb-send",
            "bili-results",
        ] {
            let label = ui.now_t(key);
            let node = nodes
                .iter()
                .find(|n| n.label.as_deref() == Some(&label))
                .unwrap_or_else(|| panic!("missing {label}: {nodes:?}"));
            let b = &node.bounds;
            assert!(
                b.width > 0.0
                    && b.height > 0.0
                    && b.x >= 0.0
                    && b.y >= 0.0
                    && b.x + b.width <= 421.0
                    && b.y + b.height <= height + 1.0,
                "{locale:?}: {node:?}, height={height}"
            );
        }
        let label = ui.now_t("bili-send-switch");
        let trigger = nodes
            .iter()
            .find(|n| n.role == "menu" && n.label.as_deref() == Some(&label))
            .unwrap();
        session
            .click_xy(
                trigger.bounds.x + trigger.bounds.width / 2.0,
                trigger.bounds.y + trigger.bounds.height / 2.0,
            )
            .unwrap();
        let toggle = session
            .accessibility_dump()
            .into_iter()
            .find(|n| n.role == "switch" && n.label.as_deref() == Some(&label))
            .expect("the compact footer opens the real sync switch");
        assert!(toggle.bounds.x >= 0.0 && toggle.bounds.x + toggle.bounds.width <= 421.0);
        session
            .click_xy(
                toggle.bounds.x + toggle.bounds.width / 2.0,
                toggle.bounds.y + toggle.bounds.height / 2.0,
            )
            .unwrap();
        assert!(
            session
                .document_mut()
                .context_mut()
                .take_program_messages()
                .into_iter()
                .any(|m| m
                    .downcast::<Msg>()
                    .is_ok_and(|m| matches!(*m, Msg::BilibiliSendToggle)))
        );
    }
}
