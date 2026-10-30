mod common;
use nana_ui_devtools::agent::RuntimeAgentSession;
use nanatype::{
    app::speech::State,
    msg::Msg,
    ui::{Ui, input, settings},
};
use nanatype_i18n::Locale;
use nanatype_model::Document;
use nanatype_settings::{InputMode, Settings};
use nanatype_speech::{controller::Phase, model::ModelState};
use std::sync::Arc;

fn settle(session: &mut RuntimeAgentSession) {
    for _ in 0..4 {
        session.flush().unwrap();
    }
}
#[test]
fn speech_controls_fit_the_footer_and_interim_text_stays_out_of_the_draft() {
    for locale in [Locale::ZhCn, Locale::EnUs, Locale::JaJp] {
        for width in [720, 920, 1200] {
            for mode in [InputMode::Compose, InputMode::Mirror] {
                let mut settings = Settings::default();
                settings.general.input_mode = mode;
                let ui = Ui::new(Arc::new(settings), common::skin("nanatype.halo"), locale);
                ui.draft.set(Arc::new(Document::plain("confirmed")));
                ui.speech.set(Arc::new(State {
                    phase: Phase::Countdown,
                    remaining: 3,
                    partial: "尚未确认的文字 provisional text 未確定の文字".into(),
                    ..Default::default()
                }));
                let mut session =
                    common::session()
                        .themed(false)
                        .locale(locale)
                        .mount(width, 360, move || input::input_window(ui));
                settle(&mut session);
                let nodes = session.accessibility_dump();
                let send = nodes
                    .iter()
                    .find(|n| {
                        n.role == "button" && n.label.as_deref() == Some(&ui.now_t("tb-send"))
                    })
                    .unwrap();
                assert!(
                    send.bounds.x >= 0.0 && send.bounds.x + send.bounds.width <= width as f32 + 1.0,
                    "{locale:?} {width}: {:?}",
                    send.bounds
                );
                let mic = nodes.iter().find(|n| {
                    n.role == "button" && n.label.as_deref() == Some(&ui.now_t("settings-speech"))
                });
                assert!(mic.is_some());
                assert_eq!(ui.draft.get_untracked().plain_text(), "confirmed");
                let send_id = send.id;
                assert!(session.click_node(send_id).unwrap());
                assert!(
                    session
                        .document_mut()
                        .context_mut()
                        .take_program_messages()
                        .into_iter()
                        .filter_map(|m| m.downcast::<Msg>().ok())
                        .any(|m| matches!(*m, Msg::Core(nanatype_core::Cmd::Send)))
                );
                if let Ok(out) = std::env::var("NANATYPE_SPEECH_CAPTURE")
                    && locale == Locale::ZhCn
                    && width == 720
                    && mode == InputMode::Compose
                {
                    std::fs::create_dir_all(&out).unwrap();
                    let (size, rgba) = session.screenshot_rgba().unwrap();
                    image::save_buffer(
                        std::path::Path::new(&out).join("input.png"),
                        &rgba,
                        size.width,
                        size.height,
                        image::ColorType::Rgba8,
                    )
                    .unwrap();
                }
            }
        }
    }
}
#[test]
fn speech_settings_show_model_progress_and_edit_takeover_recovery_in_all_locales() {
    for locale in [Locale::ZhCn, Locale::EnUs, Locale::JaJp] {
        let ui = Ui::new(
            Arc::new(Settings::default()),
            common::skin("nanatype.halo"),
            locale,
        );
        ui.page.set(settings::Page::Speech);
        ui.speech.set(Arc::new(State {
            phase: Phase::Paused,
            model: ModelState::Working(42),
            ..Default::default()
        }));
        let mut session =
            common::session()
                .themed(false)
                .locale(locale)
                .mount(1000, 900, move || settings::settings_window(ui));
        settle(&mut session);
        if let Some(out) = std::env::var_os("NANATYPE_SPEECH_CAPTURE_DIR") {
            std::fs::create_dir_all(&out).unwrap();
            session
                .screenshot_png(
                    std::path::PathBuf::from(out).join(format!("settings-{locale:?}.png")),
                )
                .unwrap();
        }
        let nodes = session.accessibility_dump();
        for key in ["speech-resume", "speech-stop", "speech-model-cancel"] {
            assert!(
                nodes
                    .iter()
                    .any(|n| n.role == "button" && n.label.as_deref() == Some(&ui.now_t(key))),
                "{locale:?} {key}"
            );
        }
        assert!(
            !nodes.iter().any(|n| n.role == "button"
                && n.label.as_deref() == Some(&ui.now_t("speech-model-delete")))
        );
    }
}
