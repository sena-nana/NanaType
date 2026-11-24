mod common;

use nana_ui::runtime::{StableNodeId, TextSelection};
use nana_ui_devtools::agent::RuntimeAgentSession;
use nanatype::{
    msg::{BliveFeedMsg, Msg},
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
                .any(|m| matches!(m, Msg::BliveFeed(BliveFeedMsg::Link(_)))),
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

#[test]
fn submitting_an_obs_link_clears_the_field_without_invalid_state() {
    let ui = on_tab(
        Locale::ZhCn,
        bilibili(BilibiliSource::Link),
        PlatformTab::Bilibili,
    );
    let save_label = ui.now_t("int-bili-link-save");
    let mut page = session(ui);

    let input = page
        .accessibility_dump()
        .into_iter()
        .find(|n| n.role == "text-input")
        .expect("OBS link text-input");
    let node_id = StableNodeId::new(input.id).unwrap();
    let input_x = input.bounds.x + input.bounds.width / 2.0;
    let input_y = input.bounds.y + input.bounds.height / 2.0;

    let first = "file:///C:/blive/loader.html?url=http%3A%2F%2Flocalhost%3A12450%2Froom%2F42";
    page.click_xy(input_x, input_y).unwrap();
    assert_eq!(
        page.document()
            .context()
            .world()
            .focused(page.document().document()),
        Some(node_id)
    );
    page.type_text(first).unwrap();
    let sent = click(&mut page, &save_label);
    page.flush().unwrap();

    let first_links: Vec<_> = sent
        .into_iter()
        .filter_map(|m| match m {
            Msg::BliveFeed(BliveFeedMsg::Link(link)) => Some(link),
            _ => None,
        })
        .collect();
    assert_eq!(first_links, vec![first.to_owned()]);
    {
        let world = page.document().context().world();
        let state = world.text_input(node_id).expect("text input state");
        assert_eq!(state.value, "");
        assert_eq!(state.selection, TextSelection::caret(0));
        assert!(state.selection.is_valid_for(state.value));
    }

    let second = "http://127.0.0.1:12450/room/99";
    page.click_xy(input_x, input_y).unwrap();
    assert_eq!(
        page.document()
            .context()
            .world()
            .focused(page.document().document()),
        Some(node_id)
    );
    page.type_text(second).unwrap();
    let sent = click(&mut page, &save_label);
    page.flush().unwrap();

    let second_links: Vec<_> = sent
        .into_iter()
        .filter_map(|m| match m {
            Msg::BliveFeed(BliveFeedMsg::Link(link)) => Some(link),
            _ => None,
        })
        .collect();
    assert_eq!(second_links, vec![second.to_owned()]);
    {
        let world = page.document().context().world();
        let state = world.text_input(node_id).expect("text input state");
        assert_eq!(state.value, "");
        assert_eq!(state.selection, TextSelection::caret(0));
        assert!(state.selection.is_valid_for(state.value));
    }
}
