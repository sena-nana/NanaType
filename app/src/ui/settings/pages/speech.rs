use super::*;
use crate::ui::kit;
use nanatype_settings::SpeechBackend;
use nanatype_speech::model::ModelState;

pub(super) fn settings(ui: Ui) -> AnyView {
    widget(Stack::column(0.0).gap_step(gap::step::SECTION))
        .children((
            section_with(
                ui,
                "",
                vec![
                    (
                        block((
                            crate::ui::speech::actions(ui),
                            hint(ui.now_t("speech-background-hint")),
                        )),
                        None,
                    ),
                    shown_if(block(crate::ui::speech::status_text(ui)), move || {
                        crate::ui::speech::has_status(ui)
                    }),
                    (
                        row(
                            ui,
                            "speech-mode",
                            Some("speech-mode-hint"),
                            crate::ui::speech::mode(ui),
                        ),
                        None,
                    ),
                    (
                        row(
                            ui,
                            "speech-silence",
                            Some("speech-silence-hint"),
                            number(
                                ui,
                                (1.0, 10.0),
                                |s| f64::from(s.speech.silence_seconds),
                                |s, v| s.speech.silence_seconds = v.round() as u32,
                            ),
                        ),
                        None,
                    ),
                ],
            ),
            section(
                ui,
                "speech-recognition",
                vec![
                    row(
                        ui,
                        "speech-backend",
                        Some("speech-backend-hint"),
                        select(
                            ui,
                            select_keys(
                                ui,
                                &[
                                    ("auto", "speech-backend-auto"),
                                    ("system", "speech-backend-system"),
                                    ("local", "speech-backend-local"),
                                ],
                            ),
                            |s| {
                                match s.speech.backend {
                                    SpeechBackend::Auto => "auto",
                                    SpeechBackend::System => "system",
                                    SpeechBackend::Local => "local",
                                }
                                .into()
                            },
                            |s, v| {
                                s.speech.backend = match v {
                                    "system" => SpeechBackend::System,
                                    "local" => SpeechBackend::Local,
                                    _ => SpeechBackend::Auto,
                                };
                                if s.speech.backend == SpeechBackend::System
                                    && s.speech.language.as_deref() == Some("auto")
                                {
                                    s.speech.language = None;
                                }
                            },
                        ),
                    ),
                    row(
                        ui,
                        "speech-device",
                        None,
                        live_select(
                            ui,
                            move || {
                                let mut options =
                                    vec![(String::new(), ui.now_t("speech-device-default"))];
                                options.extend(ui.speech.with(|s| {
                                    s.inventory
                                        .devices
                                        .iter()
                                        .map(|n| (n.clone(), n.clone()))
                                        .collect::<Vec<_>>()
                                }));
                                options
                            },
                            |s| s.speech.device.clone().unwrap_or_default(),
                            |s, v| s.speech.device = (!v.is_empty()).then(|| v.into()),
                        ),
                    ),
                    row(
                        ui,
                        "speech-language",
                        Some("speech-language-hint"),
                        live_select(
                            ui,
                            move || {
                                let backend = ui.settings.with(|s| s.speech.backend);
                                let mut options =
                                    vec![(String::new(), ui.now_t("speech-language-default"))];
                                // Keep the three interface languages at the front, even when the system
                                // recognizer isn't installed; Auto can use the local model for these.
                                options.extend(
                                    [("zh-CN", "中文"), ("en-US", "English"), ("ja-JP", "日本語")]
                                        .map(|(v, l)| (v.to_string(), l.to_string())),
                                );
                                if backend != SpeechBackend::System {
                                    options.push(("auto".into(), ui.now_t("speech-language-auto")));
                                    options.extend(nanatype_speech::local_languages());
                                }
                                if backend != SpeechBackend::Local {
                                    let languages =
                                        ui.speech.with(|s| s.inventory.system_languages.clone());
                                    for tag in languages {
                                        if !options.iter().any(|(v, _)| *v == tag) {
                                            options.push((tag.clone(), tag));
                                        }
                                    }
                                }
                                options
                            },
                            |s| s.speech.language.clone().unwrap_or_default(),
                            |s, v| s.speech.language = (!v.is_empty()).then(|| v.into()),
                        ),
                    ),
                    kit::line((), button(ui, "speech-refresh", || Msg::SpeechRefresh)),
                ],
            ),
            dynamic(
                move || (ui.locale.get(), ui.speech.with(|s| s.model.clone())),
                move |(_, state)| {
                    let status = match state {
                        ModelState::Missing => ui.now_t("speech-model-missing"),
                        ModelState::Ready => ui.now_t("speech-model-ready"),
                        ModelState::Working(percent) => ui.tr(
                            "speech-model-progress",
                            &[("percent", (*percent as u32).into())],
                        ),
                        ModelState::Error(e) => format!(
                            "{}: {}",
                            ui.now_t("speech-model-error"),
                            if e == "speech-model-invalid" {
                                ui.now_t("speech-model-invalid")
                            } else {
                                e.clone()
                            }
                        ),
                    };
                    let working = matches!(state, ModelState::Working(_));
                    let ready = matches!(state, ModelState::Ready);
                    section(
                        ui,
                        "speech-model",
                        vec![kit::line(
                            (Type::Body.view(status), hint(ui.now_t("speech-model-hint"))),
                            (
                                (!working && !ready).then(|| {
                                    button(ui, "speech-model-download", || Msg::SpeechModelDownload)
                                }),
                                (!working).then(|| {
                                    button(ui, "speech-model-import", || {
                                        Msg::PickFile(FileTarget::SpeechModel)
                                    })
                                }),
                                working.then(|| {
                                    button(ui, "speech-model-cancel", || Msg::SpeechModelCancel)
                                }),
                                (!working && ready).then(|| {
                                    button(ui, "speech-model-delete", || Msg::SpeechModelDelete)
                                }),
                            ),
                        )],
                    )
                },
            ),
        ))
        .into_any()
}
