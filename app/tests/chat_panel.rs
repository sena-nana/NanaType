mod common;

use std::sync::Arc;

use nana_ui::runtime::StableNodeId;
use nana_ui_core::PointerEventsSpec;
use nana_ui_devtools::agent::{AccessibilityDumpNode, RuntimeAgentSession};
use nanatype::{
    chat::{ChatKind, ChatMessage},
    msg::Msg,
    ui::{Ui, danmaku},
};
use nanatype_i18n::Locale;
use nanatype_model::{ChatUser, Document};
use nanatype_settings::{ChatPanelStyle, Settings};

fn message(seq: u64) -> Arc<ChatMessage> {
    Arc::new(ChatMessage {
        platform: nanatype::chat::Platform::Bilibili,
        epoch: 0,
        room: String::new(),
        seq,
        id: seq.to_string(),
        kind: ChatKind::Text,
        user: ChatUser {
            uid: seq.to_string(),
            name: format!("观众 {seq} very long viewer name"),
            avatar: None,
        },
        guard: nanatype_bilibili::Guard::None,
        owner: false,
        admin: false,
        medal_level: 0,
        medal_name: String::new(),
        doc: Document::plain("第一行：hello world\n第二行：正文完整换行。"),
        at: 1000 + seq,
    })
}

fn panel(
    skin_id: &str,
    locale: Locale,
    style: ChatPanelStyle,
    width: u32,
    count: u64,
) -> (Ui, RuntimeAgentSession) {
    let skin = common::skin(skin_id);
    let mut settings = Settings::default();
    settings.integrations.bilibili.panel_style = style;
    // History output policies must not hide the operator's feed.
    settings.history.visible_items = 1;
    settings.history.fade_after_s = Some(0);
    settings.history.show_time = true;
    let ui = Ui::new(Arc::new(settings), skin.clone(), locale);
    ui.chat_width.set(width as f32);
    let mut status = (*ui.status.get_untracked()).clone();
    status.bilibili = "Bilibili · 21452505".into();
    ui.status.set(Arc::new(status));
    ui.chat.set(Arc::new((1..=count).map(message).collect()));
    let mut session = common::session()
        .themed(false)
        .mount(width, 450, move || danmaku::chat_window(ui));
    session.flush().unwrap();
    (ui, session)
}

fn button(session: &RuntimeAgentSession, label: &str) -> AccessibilityDumpNode {
    session
        .accessibility_dump()
        .into_iter()
        .find(|n| n.role == "button" && n.label.as_deref() == Some(label))
        .unwrap_or_else(|| panic!("missing {label}"))
}

fn action_style(
    session: &RuntimeAgentSession,
    node: &AccessibilityDumpNode,
) -> (f32, PointerEventsSpec) {
    let layout = &session
        .document()
        .context()
        .world()
        .node_style(StableNodeId::new(node.id).unwrap())
        .unwrap()
        .layout;
    (
        layout.opacity.unwrap_or(1.0),
        layout.pointer_events.unwrap_or_default(),
    )
}

fn has_label(session: &RuntimeAgentSession, label: &str) -> bool {
    session
        .accessibility_dump()
        .iter()
        .any(|n| n.label.as_deref() == Some(label))
}

fn focused(session: &RuntimeAgentSession, node: &AccessibilityDumpNode) -> bool {
    session
        .accessibility_dump()
        .iter()
        .any(|n| n.id == node.id && n.focused)
}

fn take_mentions(session: &mut RuntimeAgentSession) -> Vec<(u64, bool)> {
    session
        .document_mut()
        .context_mut()
        .take_program_messages()
        .into_iter()
        .filter_map(|m| match *m.downcast::<Msg>().ok()? {
            Msg::ChatMention { seq, reply } => Some((seq, reply)),
            _ => None,
        })
        .collect()
}

fn edit_style(ui: Ui, session: &mut RuntimeAgentSession, style: ChatPanelStyle) {
    if ui.settings.with(|s| s.integrations.bilibili.panel_style) == style {
        return;
    }
    let label = ui.now_t(match style {
        ChatPanelStyle::Compact => "chat-style-compact",
        ChatPanelStyle::History => "chat-style-history",
    });
    let option = session
        .accessibility_dump()
        .into_iter()
        .find(|n| n.label.as_deref() == Some(label.as_str()))
        .unwrap();
    assert!(session.click_node(option.id).unwrap());
    for message in session.document_mut().context_mut().take_program_messages() {
        if let Ok(message) = message.downcast::<Msg>()
            && let Msg::Edit(edit) = *message
        {
            let mut settings = (*ui.settings.get_untracked()).clone();
            edit(&mut settings);
            ui.settings.set(Arc::new(settings));
        }
    }
    assert_eq!(
        ui.settings.with(|s| s.integrations.bilibili.panel_style),
        style
    );
    session.flush().unwrap();
}

#[test]
fn capsules_show_on_hover_and_keyboard_focus_without_reflow() {
    for style in [ChatPanelStyle::Compact, ChatPanelStyle::History] {
        let (ui, mut session) = panel("nanatype.halo", Locale::ZhCn, style, 300, 2);
        let reply = button(&session, &ui.now_t("chat-reply"));
        let mention = button(&session, &ui.now_t("chat-at-action"));
        assert_eq!(
            action_style(&session, &reply),
            (0.0, PointerEventsSpec::None)
        );
        assert!(
            (reply.bounds.height
                - if style == ChatPanelStyle::Compact {
                    22.0
                } else {
                    26.0
                })
            .abs()
                < 0.1
        );
        assert!(
            session
                .accessibility_dump()
                .iter()
                .filter(|n| n.label.as_deref().is_some_and(|s| s.starts_with("观众 ")))
                .count()
                >= 2
        );
        session
            .hover_xy(reply.bounds.x + 2.0, reply.bounds.y + 2.0)
            .unwrap();

        assert_eq!(
            action_style(&session, &reply),
            (1.0, PointerEventsSpec::Auto)
        );
        assert_eq!(
            button(&session, &ui.now_t("chat-reply")).bounds,
            reply.bounds
        );
        session
            .hover_xy(mention.bounds.x + 2.0, mention.bounds.y + 2.0)
            .unwrap();
        assert_eq!(action_style(&session, &reply).0, 1.0);
        session.hover_xy(1.0, 1.0).unwrap();
        assert_eq!(action_style(&session, &reply).0, 0.0);
        ui.chat_target.set(Some(1));
        session.flush().unwrap();
        assert_eq!(
            action_style(&session, &reply).0,
            0.0,
            "selected reply target does not keep actions visible"
        );
        // Tab must reach an otherwise invisible capsule.
        for _ in 0..24 {
            session.key_press("Tab", "Tab", Default::default()).unwrap();
            if focused(&session, &reply) {
                break;
            }
        }
        assert!(focused(&session, &reply));
        assert_eq!(action_style(&session, &reply).0, 1.0);
        session
            .key_press("Enter", "Enter", Default::default())
            .unwrap();
        assert_eq!(take_mentions(&mut session), [(1, true)]);
        session
            .hover_xy(mention.bounds.x + 2.0, mention.bounds.y + 2.0)
            .unwrap();
        session
            .click_xy(mention.bounds.x + 2.0, mention.bounds.y + 2.0)
            .unwrap();
        assert_eq!(take_mentions(&mut session), [(1, false)]);
    }
}

#[test]
fn both_presentations_fit_narrow_panels_across_skins_and_languages() {
    for skin in ["nanatype.halo", "nanatype.night", "nanatype.echo_vanilla"] {
        for locale in [Locale::ZhCn, Locale::EnUs, Locale::JaJp] {
            for width in [300, 400] {
                let (ui, mut session) = panel(skin, locale, ChatPanelStyle::Compact, width, 2);
                for style in [ChatPanelStyle::Compact, ChatPanelStyle::History] {
                    edit_style(ui, &mut session, style);
                    for node in session.accessibility_dump() {
                        if node.label.as_ref().is_some_and(|s| !s.is_empty()) {
                            assert!(
                                node.bounds.x >= -1.0
                                    && node.bounds.x + node.bounds.width <= width as f32 + 1.0,
                                "{skin} {locale:?} {width} {style:?}: {:?} {:?}",
                                node.label,
                                node.bounds
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn changing_style_retains_reading_anchor_reply_and_draft() {
    let (ui, mut session) = panel(
        "nanatype.halo",
        Locale::ZhCn,
        ChatPanelStyle::Compact,
        400,
        40,
    );
    ui.draft.set(Arc::new(Document::plain("保留草稿")));
    ui.chat_target.set(Some(30));
    let quote = message(30).quote(Locale::ZhCn);
    ui.reply.set(Some(quote.clone()));
    session.scroll_by(180.0, 180.0, 0.0, 700.0).unwrap();
    session.flush().unwrap();
    assert!(has_label(&session, &ui.now_t("chat-latest")));
    let anchor = session
        .accessibility_dump()
        .into_iter()
        .find(|n| {
            n.label.as_deref().is_some_and(|s| s.starts_with("观众 "))
                && n.bounds.y >= 100.0
                && n.bounds.y < 300.0
        })
        .unwrap();
    let row_anchor = session
        .accessibility_dump()
        .into_iter()
        .find(|n| {
            let st = session
                .document()
                .context()
                .world()
                .node_style(StableNodeId::new(n.id).unwrap())
                .unwrap();
            st.layout.border_radius == Some(6.0)
                && st.layout.border_width == Some(1.0)
                && n.bounds.y + n.bounds.height > 76.0
                && n.bounds.y < 400.0
        })
        .unwrap();
    for style in [ChatPanelStyle::History, ChatPanelStyle::Compact] {
        edit_style(ui, &mut session, style);
        let same = session
            .accessibility_dump()
            .into_iter()
            .find(|n| n.label == anchor.label)
            .expect("reading row survives style change");
        let retained = session
            .accessibility_dump()
            .into_iter()
            .find(|n| n.id == row_anchor.id)
            .unwrap();
        assert!(
            (retained.bounds.y - row_anchor.bounds.y).abs() <= 1.0,
            "first visible row retains its viewport position: before {:?}, after {:?}",
            row_anchor.bounds,
            retained.bounds
        );
        assert!(
            same.bounds.y > 40.0 && same.bounds.y < 450.0,
            "anchor stays visible: {:?}",
            same.bounds
        );
        assert_eq!(ui.chat_target.get_untracked(), Some(30));
        assert_eq!(ui.reply.get_untracked(), Some(quote.clone()));
        assert_eq!(ui.draft.get_untracked().plain_text(), "保留草稿");
        assert!(has_label(&session, &ui.now_t("chat-latest")));
    }
    let latest = button(&session, &ui.now_t("chat-latest"));
    assert!(session.click_node(latest.id).unwrap());
    edit_style(ui, &mut session, ChatPanelStyle::History);
    let mut rows = (*ui.chat.get_untracked()).clone();
    rows.push(message(41));
    ui.chat.set(Arc::new(rows));
    session.flush().unwrap();
    assert!(!has_label(&session, &ui.now_t("chat-latest")));
    let newest = session
        .accessibility_dump()
        .into_iter()
        .find(|n| n.label.as_deref() == Some("观众 41 very long viewer name"))
        .unwrap();
    assert!(
        newest.bounds.y >= 76.0 && newest.bounds.y < 450.0,
        "new messages follow after returning to latest"
    );
}

#[test]
fn switching_skins_does_not_restyle_either_operator_layout() {
    for style in [ChatPanelStyle::Compact, ChatPanelStyle::History] {
        let (ui, mut session) = panel("nanatype.halo", Locale::ZhCn, style, 400, 2);
        let mut messages = (*ui.chat.get_untracked()).clone();
        Arc::make_mut(&mut messages[0]).doc =
            nanatype_markup::parse(r#"正文 [at=7 name="观众"]"#).doc;
        ui.chat.set(Arc::new(messages));
        ui.chat_target.set(Some(1));
        for dark in [false, true, false] {
            ui.chrome_dark.set(dark);
            session
                .document_mut()
                .context_mut()
                .set_theme_tokens(nanatype::skin_theme::compile(dark))
                .unwrap();
            session.flush().unwrap();
            let world = session.document().context().world();
            let body = session
                .accessibility_dump()
                .iter()
                .filter_map(|n| world.rich_text(StableNodeId::new(n.id).unwrap()))
                .find(|text| text.text().starts_with("正文"))
                .unwrap();
            let label = body.objects()[0].1.label_style().unwrap();
            let expected = nanatype::ui::rich::MentionLook::for_ui(dark);
            assert_eq!((label.fill, label.text), (expected.fill, expected.text));
            assert_skin_independent(ui, &mut session);
        }
    }
}

fn styles(session: &RuntimeAgentSession) -> Vec<String> {
    let world = session.document().context().world();
    session
        .accessibility_dump()
        .iter()
        .map(|n| {
            let id = StableNodeId::new(n.id).unwrap();
            let style = world.node_style(id).unwrap();
            format!(
                "{:?} {:?} {:?} {:?} {:?} {:?}",
                n.label,
                n.bounds,
                style.layout,
                style.foreground,
                style.background,
                world.rich_text(id)
            )
        })
        .collect()
}

#[test]
fn general_settings_keep_their_style_when_the_skin_changes() {
    use nanatype::ui::settings;
    let mut config = Settings::default();
    config.general.onboarding_done = true;
    let ui = Ui::new(
        Arc::new(config),
        common::skin("nanatype.halo"),
        Locale::ZhCn,
    );
    let mut session = common::session()
        .themed(false)
        .mount(920, 660, move || settings::settings_window(ui));
    session.flush().unwrap();
    assert!(has_label(&session, &ui.now_t("general-appearance")));
    for dark in [false, true] {
        ui.chrome_dark.set(dark);
        session
            .document_mut()
            .context_mut()
            .set_theme_tokens(nanatype::skin_theme::compile(dark))
            .unwrap();
        session.flush().unwrap();
        assert_skin_independent(ui, &mut session);
    }
    ui.page.set(settings::Page::Skins);
    session.flush().unwrap();
    assert!(!has_label(&session, &ui.now_t("general-appearance")));
}

fn assert_skin_independent(ui: Ui, session: &mut RuntimeAgentSession) {
    let registry = common::registry();
    let before = styles(session);
    for skin in [
        "nanatype.night",
        "nanatype.pixel",
        "nanatype.sakura",
        "nanatype.halo",
    ] {
        ui.skin.set(registry.load(skin).unwrap().skin);
        session.flush().unwrap();
        assert_eq!(
            styles(session),
            before,
            "{skin}, dark={}",
            ui.chrome_dark.get_untracked()
        );
    }
}
