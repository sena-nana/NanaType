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
    ui.platform_tab.set(nanatype::chat::Platform::Blivechat);
    ui.page.set(settings::Page::Platforms);
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
    s
}

#[test]
fn the_page_folds_away_while_off_and_names_every_source_in_every_locale() {
    for locale in Locale::ALL {
        let ui = ui(locale, Settings::default());
        let mut off = session(ui);
        let label = ui.now_t("int-blive-auth-save");
        assert!(
            !has_button(&mut off, &label),
            "the auth code shows while off"
        );
        let comet = ui.now_t("int-comet-port");
        assert_ne!(comet, "int-comet-port", "missing translation");
        assert!(
            !off.accessibility_dump()
                .iter()
                .any(|n| n.label.as_deref() == Some(comet.as_str())),
            "the 彗星号 port shows while off"
        );

        let ui = self::ui(locale, enabled(BlivechatSource::Server));
        let mut server = session(ui);
        assert!(has_button(&mut server, &ui.now_t("int-blive-auth-save")));
        let nodes = server.accessibility_dump();
        for key in [
            "int-blive-source-server",
            "int-blive-source-comet",
            "int-blive-source-laplace",
        ] {
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
fn the_comet_section_only_asks_for_the_port() {
    for locale in Locale::ALL {
        let ui = ui(locale, enabled(BlivechatSource::Comet));
        let mut session = session(ui);
        let nodes = session.accessibility_dump();
        let label = ui.now_t("int-comet-port");
        assert!(
            nodes
                .iter()
                .any(|n| n.label.as_deref() == Some(label.as_str())),
            "the port row shows in {locale:?}"
        );
        assert!(
            nodes.iter().any(|n| n.value.as_deref() == Some("19400")),
            "the client's own port by default"
        );
        assert!(!has_button(&mut session, &ui.now_t("int-blive-auth-save")));
        assert!(!has_button(
            &mut session,
            &ui.now_t("int-laplace-link-save")
        ));
    }
}

#[test]
fn the_laplace_section_takes_a_pasted_link() {
    for locale in Locale::ALL {
        let ui = ui(locale, enabled(BlivechatSource::Laplace));
        let mut session = session(ui);
        let save = ui.now_t("int-laplace-link-save");
        assert_ne!(save, "int-laplace-link-save", "missing translation");
        assert!(has_button(&mut session, &save));
        let label = ui.now_t("int-laplace-link");
        assert!(
            session
                .accessibility_dump()
                .iter()
                .any(|n| n.label.as_deref() == Some(label.as_str())),
            "the link row shows in {locale:?}"
        );
        let guide = ui.now_t("int-laplace-guide");
        let nodes = session.accessibility_dump();
        let button = nodes
            .iter()
            .find(|n| n.role == "button" && n.label.as_deref() == Some(guide.as_str()))
            .expect("a guide button");
        assert!(session.click_node(button.id).unwrap());
        let messages = session.document_mut().context_mut().take_program_messages();
        assert!(
            messages.into_iter().any(|m| m
                .downcast::<Msg>()
                .is_ok_and(|m| matches!(*m, Msg::OpenUrl(url) if url.ends_with("laplace.md")))),
            "the guide opens in {locale:?}"
        );
    }
}
