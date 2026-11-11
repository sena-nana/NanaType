mod common;

use nana_ui_devtools::agent::RuntimeAgentSession;
use nanatype::{
    msg::{BliveMsg, Msg},
    ui::{Ui, settings, sources::PlatformTab},
};
use nanatype_i18n::Locale;
use nanatype_settings::{BilibiliSource, Settings};
use std::sync::Arc;

fn on_tab(locale: Locale, settings: Settings, tab: PlatformTab) -> Ui {
    let ui = Ui::new(Arc::new(settings), common::skin("nanatype.halo"), locale);
    ui.platform_tab.set(tab);
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

fn has_label(session: &mut RuntimeAgentSession, label: &str) -> bool {
    session
        .accessibility_dump()
        .iter()
        .any(|n| n.label.as_deref() == Some(label))
}

fn bilibili(source: BilibiliSource) -> Settings {
    let mut s = Settings::default();
    s.integrations.bilibili.enabled = true;
    s.integrations.bilibili.source = source;
    s
}

/// The program messages a click on the button labelled `label` sends.
fn click(session: &mut RuntimeAgentSession, label: &str) -> Vec<Msg> {
    let nodes = session.accessibility_dump();
    let button = nodes
        .iter()
        .find(|n| n.role == "button" && n.label.as_deref() == Some(label))
        .unwrap_or_else(|| panic!("a {label} button"));
    assert!(session.click_node(button.id).unwrap());
    session
        .document_mut()
        .context_mut()
        .take_program_messages()
        .into_iter()
        .filter_map(|m| m.downcast::<Msg>().ok().map(|m| *m))
        .collect()
}

#[test]
fn the_tabs_are_bilibili_twitch_and_the_other_platforms_in_every_locale() {
    for locale in Locale::ALL {
        let ui = on_tab(locale, Settings::default(), PlatformTab::Bilibili);
        let mut page = session(ui);
        for tab in PlatformTab::ALL {
            let label = ui.now_t(tab.label_key());
            assert_ne!(label, tab.label_key(), "missing translation");
            assert!(has_label(&mut page, &label), "{tab:?} in {locale:?}");
        }
    }
}

#[test]
fn the_bilibili_page_folds_away_while_off() {
    for locale in Locale::ALL {
        let ui = on_tab(locale, Settings::default(), PlatformTab::Bilibili);
        let mut off = session(ui);
        let switch = ui.now_t("int-bili");
        assert_ne!(switch, "int-bili", "missing translation");
        assert!(has_label(&mut off, &switch));
        assert!(!has_button(&mut off, &ui.now_t("int-bili-link-save")));
        assert!(!has_label(&mut off, &ui.now_t("int-comet-port")));
    }
}

#[test]
fn the_bilibili_page_takes_a_pasted_obs_link() {
    for locale in Locale::ALL {
        let ui = on_tab(
            locale,
            bilibili(BilibiliSource::Link),
            PlatformTab::Bilibili,
        );
        let mut page = session(ui);
        for key in [
            "int-bili-link",
            "int-bili-source-link",
            "int-bili-source-comet",
            "int-bili-link-none",
        ] {
            let label = ui.now_t(key);
            assert_ne!(label, key, "missing translation");
            assert!(has_label(&mut page, &label), "{key} in {locale:?}");
        }
        assert!(!has_label(&mut page, &ui.now_t("int-comet-port")));
        let sent = click(&mut page, &ui.now_t("int-bili-link-save"));
        assert!(
            sent.iter()
                .any(|m| matches!(m, Msg::Blive(BliveMsg::Link(_)))),
            "the link is sent in {locale:?}"
        );
        let sent = click(&mut page, &ui.now_t("int-laplace-guide"));
        assert!(
            sent.iter()
                .any(|m| matches!(m, Msg::OpenUrl(url) if url.ends_with("laplace.md"))),
            "the guide opens in {locale:?}"
        );
    }
}

#[test]
fn bilibili_by_comet_only_asks_for_the_port() {
    for locale in Locale::ALL {
        let ui = on_tab(
            locale,
            bilibili(BilibiliSource::Comet),
            PlatformTab::Bilibili,
        );
        let mut page = session(ui);
        assert!(has_label(&mut page, &ui.now_t("int-comet-port")));
        assert!(
            page.accessibility_dump()
                .iter()
                .any(|n| n.value.as_deref() == Some("19400")),
            "the client's own port by default"
        );
        assert!(!has_button(&mut page, &ui.now_t("int-bili-link-save")));
    }
}

#[test]
fn the_other_platforms_only_have_comet() {
    for locale in Locale::ALL {
        let ui = on_tab(locale, Settings::default(), PlatformTab::Other);
        let mut off = session(ui);
        let switch = ui.now_t("int-comet-other");
        assert_ne!(switch, "int-comet-other", "missing translation");
        assert!(has_label(&mut off, &switch));
        assert!(!has_label(&mut off, &ui.now_t("int-comet-port")));
        assert!(!has_button(&mut off, &ui.now_t("int-bili-link-save")));

        let mut s = Settings::default();
        s.integrations.comet.other_platforms = true;
        let ui = on_tab(locale, s, PlatformTab::Other);
        let mut on = session(ui);
        assert!(has_label(&mut on, &ui.now_t("int-comet-port")));
    }
}
