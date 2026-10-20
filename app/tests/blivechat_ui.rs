mod common;

use nana_ui_devtools::agent::RuntimeAgentSession;
use nanatype::{
    msg::Msg,
    ui::{Ui, settings},
};
use nanatype_i18n::Locale;
use nanatype_settings::{BlivechatSource, Settings};
use std::sync::Arc;

fn ui(locale: Locale, settings: Settings) -> Ui {
    let ui = Ui::new(Arc::new(settings), common::skin("nanatype.halo"), locale);
    ui.page.set(settings::Page::Blivechat);
    ui
}

fn session(ui: Ui) -> RuntimeAgentSession {
    let mut session = common::session()
        .themed(false)
        .mount(1120, 1100, move || settings::settings_window(ui));
    session.flush().unwrap();
    session
}

fn has_button(session: &mut RuntimeAgentSession, label: &str) -> bool {
    session
        .accessibility_dump()
        .iter()
        .any(|n| n.role == "button" && n.label.as_deref() == Some(label))
}

fn enabled(source: BlivechatSource) -> Settings {
    let mut s = Settings::default();
    s.integrations.blivechat.enabled = true;
    s.integrations.blivechat.source = source;
    s.integrations.blivechat.comet_token = "0123abcd".into();
    s
}

#[test]
fn the_page_folds_away_while_off_and_names_both_sources_in_every_locale() {
    for locale in Locale::ALL {
        let ui = ui(locale, Settings::default());
        let mut off = session(ui);
        for key in ["int-blive-auth-save", "int-comet-copy"] {
            let label = ui.now_t(key);
            assert_ne!(label, key, "missing translation");
            assert!(!has_button(&mut off, &label), "{key} shows while off");
        }

        let ui = self::ui(locale, enabled(BlivechatSource::Server));
        let mut server = session(ui);
        assert!(has_button(&mut server, &ui.now_t("int-blive-auth-save")));
        assert!(!has_button(&mut server, &ui.now_t("int-comet-copy")));
        let nodes = server.accessibility_dump();
        for key in ["int-blive-source-server", "int-blive-source-comet"] {
            let label = ui.now_t(key);
            assert!(
                nodes
                    .iter()
                    .any(|n| n.label.as_deref() == Some(label.as_str())),
                "missing source {key} in {locale:?}"
            );
        }
    }
}

#[test]
fn the_comet_section_only_shows_and_copies_the_address() {
    let ui = ui(Locale::ZhCn, enabled(BlivechatSource::Comet));
    let mut session = session(ui);
    let nodes = session.accessibility_dump();
    let address = "ws://127.0.0.1:12451/comet?token=0123abcd";
    assert!(
        nodes
            .iter()
            .any(|n| n.label.as_deref() == Some(address) || n.value.as_deref() == Some(address)),
        "the address is shown"
    );
    assert!(!has_button(&mut session, &ui.now_t("int-blive-auth-save")));
    let label = ui.now_t("int-comet-copy");
    let copy = nodes
        .iter()
        .find(|n| n.role == "button" && n.label.as_deref() == Some(label.as_str()))
        .expect("a copy button");
    assert!(session.click_node(copy.id).unwrap());
    let messages = session.document_mut().context_mut().take_program_messages();
    assert!(messages.into_iter().any(|m| {
        m.downcast::<Msg>()
            .is_ok_and(|m| matches!(*m, Msg::CopyText(ref text) if text == address))
    }));
    let buttons = session
        .accessibility_dump()
        .into_iter()
        .filter(|n| n.role == "button")
        .filter_map(|n| n.label)
        .collect::<Vec<_>>();
    for word in ["插件", "端口", "index.html"] {
        assert!(
            buttons.iter().all(|b| !b.contains(word)),
            "no plugin or port controls: {buttons:?}"
        );
    }
}
