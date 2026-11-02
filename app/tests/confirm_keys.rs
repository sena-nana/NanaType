//! The keyboard in the settings window's confirmations: Esc backs out of
//! one, Enter goes ahead.

mod common;

use std::sync::Arc;

use nana_ui::runtime::StableNodeId;
use nana_ui_devtools::agent::{AccessibilityDumpNode, RuntimeAgentSession};
use nana_ui_platform::InputModifiers;
use nanatype::app::backup::{Pending, View};
use nanatype::msg::{BackupMsg, HistoryMsg, Msg};
use nanatype::ui::{
    Ui,
    settings::{self, Page},
};
use nanatype_i18n::Locale;
use nanatype_settings::Settings;

struct Fixture {
    ui: Ui,
    session: RuntimeAgentSession,
}

impl Fixture {
    fn new(page: Page) -> Self {
        let skin = common::skin(nanatype_theme::DEFAULT_SKIN);
        let mut settings = Settings::default();
        settings.general.onboarding_done = true;
        let ui = Ui::new(Arc::new(settings), skin, Locale::EnUs);
        ui.page.set(page);
        let mut session = common::session()
            .themed(false)
            .mount(720, 1600, move || settings::settings_window(ui));
        session.flush().unwrap();
        Self { ui, session }
    }

    fn buttons(&self, key: &str) -> Vec<AccessibilityDumpNode> {
        let label = self.ui.now_t(key);
        self.session
            .accessibility_dump()
            .into_iter()
            .filter(|n| n.role == "button" && n.label.as_deref() == Some(label.as_str()))
            .filter(|n| n.bounds.width > 0.0)
            .collect()
    }

    fn key(&mut self, key: &str) {
        self.session
            .key_press(key, key, InputModifiers::default())
            .unwrap();
        self.session.flush().unwrap();
    }

    fn messages(&mut self) -> Vec<Msg> {
        self.session
            .document_mut()
            .context_mut()
            .take_program_messages()
            .into_iter()
            .filter_map(|m| m.downcast::<Msg>().ok().map(|m| *m))
            .collect()
    }
}

fn pending() -> Pending {
    Pending {
        sections: vec!["general"],
        from_schema: 1,
        version: "1.0.0".into(),
        avatars: 0,
        positions: false,
        integrations: false,
    }
}

#[test]
fn the_import_confirmation_closes_on_esc_and_imports_on_enter() {
    let mut f = Fixture::new(Page::General);
    for (key, confirmed) in [("Escape", false), ("Enter", true)] {
        f.ui.backup.set(Arc::new(View {
            pending: Some(pending()),
            ..View::default()
        }));
        f.session.flush().unwrap();
        f.messages();
        f.key(key);
        let messages = f.messages();
        assert!(
            messages
                .iter()
                .any(|m| matches!(m, Msg::Backup(BackupMsg::Confirm(c)) if *c == confirmed)),
            "{key}: {:?}",
            messages
                .iter()
                .map(|m| match m {
                    Msg::Backup(BackupMsg::Confirm(c)) => format!("confirm {c}"),
                    _ => "other".to_owned(),
                })
                .collect::<Vec<_>>()
        );
        f.ui.backup.set(Arc::new(View::default()));
        f.session.flush().unwrap();
    }
}

fn armed(f: &Fixture) -> bool {
    !f.buttons("common-cancel").is_empty()
}

fn focused(f: &Fixture) -> Option<u64> {
    let doc = f.session.document();
    doc.context()
        .world()
        .focused(doc.document())
        .map(|id| id.get())
}

/// Arm the inline confirmation behind `trigger` (clear history, delete a
/// saved session, reset). They are not dialogs: arming swaps the trigger for
/// the question, Cancel and the confirming button, in the page's tab order.
fn arm(f: &mut Fixture, trigger: &str) {
    // As a keyboard user: the trigger has the focus, Enter arms it.
    let first = f.buttons(trigger)[0].clone();
    let doc = f.session.document_mut();
    let document = doc.document();
    doc.context_mut()
        .focus_node(document, StableNodeId::new(first.id).unwrap())
        .unwrap();
    f.key("Enter");
    assert!(armed(f), "{trigger}: armed");
}

/// Put the focus on the visible button labelled `key`.
fn focus(f: &mut Fixture, key: &str) {
    let target = f.buttons(key).last().expect("the button").id;
    let doc = f.session.document_mut();
    let document = doc.document();
    doc.context_mut()
        .focus_node(document, StableNodeId::new(target).unwrap())
        .unwrap();
    assert_eq!(focused(f), Some(target));
}

/// Enter on the confirming button goes ahead; on Cancel it backs out.
fn inline(page: Page, trigger: &str, confirm: &str, expect: impl Fn(&Msg) -> bool) {
    let mut f = Fixture::new(page);
    if page == Page::History {
        let mut view = nanatype::archive::View {
            sessions: vec![nanatype::archive::Session {
                name: "2026-11-02_213005".into(),
                lines: 0,
            }],
            ..Default::default()
        };
        view.open("2026-11-02_213005".into(), Vec::new());
        f.ui.archive.set(Arc::new(view));
        f.session.flush().unwrap();
    }
    arm(&mut f, trigger);
    focus(&mut f, "common-cancel");
    f.messages();
    f.key("Enter");
    assert!(!f.messages().iter().any(&expect), "{trigger}: Cancel");
    assert!(!armed(&f), "{trigger}: Cancel disarms");
    arm(&mut f, trigger);
    focus(&mut f, confirm);
    f.key("Enter");
    assert!(f.messages().iter().any(&expect), "{trigger}: confirmed");
}

#[test]
fn clearing_the_history_confirms_from_the_keyboard() {
    inline(Page::History, "history-clear", "history-clear", |m| {
        matches!(m, Msg::Core(nanatype_core::Cmd::ClearHistory))
    });
}

#[test]
fn deleting_a_saved_session_confirms_from_the_keyboard() {
    inline(
        Page::History,
        "history-saved-delete",
        "history-saved-delete",
        |m| matches!(m, Msg::History(HistoryMsg::Delete(_))),
    );
}

#[test]
fn resetting_confirms_from_the_keyboard() {
    inline(
        Page::General,
        "backup-reset-button",
        "backup-reset-button",
        |m| matches!(m, Msg::Backup(BackupMsg::Reset)),
    );
}

/// Arming hides the focused trigger, so the armed confirmation takes the
/// focus: Esc right after arming backs out (and the trigger has the focus
/// again), Enter goes ahead without first moving to the confirming button.
#[test]
fn an_armed_confirmation_backs_out_on_esc_and_confirms_on_enter() {
    for (page, trigger, expect) in [
        (
            Page::General,
            "backup-reset-button",
            (|m: &Msg| matches!(m, Msg::Backup(BackupMsg::Reset))) as fn(&Msg) -> bool,
        ),
        (Page::History, "history-clear", |m| {
            matches!(m, Msg::Core(nanatype_core::Cmd::ClearHistory))
        }),
    ] {
        let mut f = Fixture::new(page);
        arm(&mut f, trigger);
        f.key("Escape");
        assert!(!armed(&f), "{trigger}: Esc");
        let back = f.buttons(trigger)[0].id;
        assert_eq!(
            focused(&f),
            Some(back),
            "{trigger}: focus back on the trigger"
        );
        arm(&mut f, trigger);
        f.messages();
        f.key("Enter");
        assert!(f.messages().iter().any(expect), "{trigger}: Enter");
    }
}
