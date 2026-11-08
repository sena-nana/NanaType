//! The session card must remain usable without changing the streamer's draft.

mod common;

use nana_ui::runtime::Stack;
use nana_ui::runtime::view::widget;
use nana_ui_core::LengthSpec;
use nana_ui_devtools::agent::RuntimeAgentSession;
use nanatype::{
    msg::{Msg, WindowMsg},
    p2p::SessionUi,
    ui::{Ui, p2p},
};
use nanatype_i18n::Locale;
use nanatype_p2p::Status;
use std::sync::Arc;

fn click(session: &mut RuntimeAgentSession, label: &str, width: f32) -> Msg {
    session.flush().unwrap();
    let node = session
        .accessibility_dump()
        .into_iter()
        .find(|node| node.role == "button" && node.label.as_deref() == Some(label))
        .unwrap_or_else(|| panic!("missing action {label}"));
    assert!(node.bounds.width > 0.0 && node.bounds.x >= 0.0);
    assert!(
        node.bounds.x + node.bounds.width <= width,
        "button {label} is outside the card: {:?}",
        node.bounds
    );
    assert!(session.click_node(node.id).unwrap());
    session
        .document_mut()
        .context_mut()
        .take_program_messages()
        .into_iter()
        .find_map(|message| message.downcast::<Msg>().ok())
        .map(|message| *message)
        .expect("action emitted")
}

#[test]
fn card_actions_fit_and_preserve_the_draft_in_each_locale() {
    for locale in [Locale::ZhCn, Locale::EnUs, Locale::JaJp] {
        for width in [400, 796] {
            let skin = common::skin("nanatype.halo");
            let ui = Ui::new(Arc::new(Default::default()), skin.clone(), locale);
            ui.peer_panel.set(true);
            let draft = nanatype_markup::parse("my [b]private draft[/b]").doc;
            ui.draft.set(Arc::new(draft.clone()));
            let mut session = common::session().themed(false).mount(width, 170, move || {
                widget(Stack::column(0.0).width(LengthSpec::Fill)).children(p2p::card(ui))
            });
            session.flush().unwrap();
            let title = session
                .accessibility_dump()
                .into_iter()
                .find(|node| {
                    node.label
                        .as_ref()
                        .is_some_and(|label| label.starts_with(&ui.now_t("peer-title")))
                })
                .expect("session title");
            assert!(title.bounds.width > 100.0, "status must remain visible");
            assert!(matches!(
                click(&mut session, &ui.now_t("peer-create"), width as f32),
                Msg::PeerStart {
                    invitation: None,
                    ..
                }
            ));
            assert!(matches!(
                click(&mut session, &ui.now_t("peer-join"), width as f32),
                Msg::PeerStart {
                    invitation: Some(_),
                    ..
                }
            ));
            ui.peer.set(Arc::new(SessionUi {
                active: true,
                host: true,
                invitation: "private-invitation".into(),
                name: "Partner".into(),
                status: Status::Connected {
                    route: "direct".into(),
                    rtt_ms: 42,
                },
                ..SessionUi::default()
            }));
            assert!(
                matches!(click(&mut session, &ui.now_t("peer-copy"), width as f32), Msg::CopyText(code) if code == "private-invitation")
            );
            assert!(matches!(
                click(&mut session, &ui.now_t("peer-window-toggle"), width as f32),
                Msg::Window(WindowMsg::Toggle(nanatype::windows::WindowKind::PARTNER))
            ));
            assert!(matches!(
                click(&mut session, &ui.now_t("peer-leave"), width as f32),
                Msg::PeerLeave
            ));
            assert_eq!(*ui.draft.get_untracked(), draft);
        }
    }
}

#[test]
fn each_partner_seat_has_its_own_output_switch() {
    for locale in [Locale::ZhCn, Locale::EnUs, Locale::JaJp] {
        let width = 400;
        let skin = common::skin("nanatype.halo");
        let ui = Ui::new(Arc::new(Default::default()), skin, locale);
        ui.peer_panel.set(true);
        ui.peer.set(Arc::new(SessionUi {
            active: true,
            host: true,
            status: Status::Connected {
                route: "direct".into(),
                rtt_ms: 12,
            },
            members: 3,
            seats: vec!["Alice".into(), String::new()],
            ..SessionUi::default()
        }));
        let mut session = common::session().themed(false).mount(width, 320, move || {
            widget(Stack::column(0.0).width(LengthSpec::Fill)).children(p2p::card(ui))
        });
        session.flush().unwrap();
        let out =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/p2p-card-tests");
        std::fs::create_dir_all(&out).unwrap();
        session
            .screenshot_png(out.join(format!("seats-{locale:?}.png")))
            .unwrap();
        let switches: Vec<_> = session
            .accessibility_dump()
            .into_iter()
            .filter(|node| node.role == "switch")
            .collect();
        assert_eq!(switches.len(), 2, "one switch per seat");
        for (seat, node) in switches.iter().enumerate() {
            assert!(
                node.bounds.x + node.bounds.width <= width as f32,
                "seat {seat}'s switch is outside the card: {:?}",
                node.bounds
            );
        }
        assert!(session.click_node(switches[1].id).unwrap());
        let Some(Msg::Edit(edit)) = session
            .document_mut()
            .context_mut()
            .take_program_messages()
            .into_iter()
            .find_map(|message| message.downcast::<Msg>().ok())
            .map(|message| *message)
        else {
            panic!("the switch edits the settings")
        };
        let mut settings = nanatype_settings::Settings::default();
        edit(&mut settings);
        assert!(!settings.output.partners[0].enabled);
        assert!(settings.output.partners[1].enabled);
    }
}
