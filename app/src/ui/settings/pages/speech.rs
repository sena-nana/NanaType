use super::*;
use crate::msg::{SpeechMsg, TtsMsg};
use crate::ui::kit;
use nanatype_settings::SpeechBackend;
use nanatype_speech::model::ModelState;
use nanatype_tts::Voice;

pub(super) fn settings(ui: Ui) -> AnyView {
    widget(Stack::column(0.0).gap_step(gap::step::SECTION))
        .children((
            section_with(
                ui,
                "",
                vec![
                    (
                        row(
                            ui,
                            "speech-input",
                            Some("speech-background-hint"),
                            listen_buttons(ui),
                        ),
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
                            number_unit(
                                ui,
                                (1.0, 10.0),
                                "unit-seconds",
                                |s| f64::from(s.speech.silence_seconds),
                                |s, v| s.speech.silence_seconds = v.round() as u32,
                            ),
                        ),
                        None,
                    ),
                ],
            ),
            section_tools(
                ui,
                "speech-recognition",
                vec![tool(ui, ti::REFRESH, "speech-refresh", || {
                    Msg::Speech(SpeechMsg::Refresh)
                })],
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
                ]
                .into_iter()
                .map(|r| (r, None))
                .collect(),
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
                        vec![kit::row_marked(
                            status,
                            ui.now_t("speech-model-hint"),
                            controls(
                                vec![
                                    (!working && !ready).then(|| {
                                        icon_action(
                                            ui,
                                            "speech-model-download",
                                            ti::CLOUD_DOWNLOAD,
                                            || Msg::Speech(SpeechMsg::ModelDownload),
                                        )
                                    }),
                                    (!working).then(|| {
                                        icon_action(ui, "speech-model-import", ti::DOWNLOAD, || {
                                            Msg::PickFile(FileTarget::SpeechModel)
                                        })
                                    }),
                                    working.then(|| {
                                        icon_action(ui, "speech-model-cancel", ti::X, || {
                                            Msg::Speech(SpeechMsg::ModelCancel)
                                        })
                                    }),
                                    (!working && ready).then(|| {
                                        icon_action(ui, "speech-model-delete", ti::TRASH, || {
                                            Msg::Speech(SpeechMsg::ModelDelete)
                                        })
                                    }),
                                ]
                                .into_iter()
                                .flatten()
                                .collect(),
                            ),
                        )],
                    )
                },
            ),
            tts(ui),
        ))
        .into_any()
}

/// Reading sent lines aloud: the switch, then the voices (a default one and
/// each character's), pace and loudness, and what else is read.
fn tts(ui: Ui) -> AnyView {
    // Only the interface language's voices at first: there can be hundreds.
    let filter = signal(primary(ui.locale.get_untracked().tag()).to_owned());
    let mut rows = vec![
        row(ui, "tts-language", None, language_filter(ui, filter)),
        row(
            ui,
            "tts-voice",
            Some("tts-voice-hint"),
            live_select(
                ui,
                move || voice_options(ui, filter, "tts-voice-system", |s| s.tts.voice.clone()),
                |s| s.tts.voice.clone().unwrap_or_default(),
                |s, v| s.tts.voice = (!v.is_empty()).then(|| v.into()),
            ),
        ),
        Row::inset(dynamic(
            move || {
                ui.settings.with(|s| {
                    s.general
                        .characters
                        .iter()
                        .map(|c| c.name.clone())
                        .collect::<Vec<_>>()
                })
            },
            move |names| {
                let rows: Vec<Row> = names
                    .iter()
                    .enumerate()
                    .map(|(i, name)| character_voice(ui, filter, i, name))
                    .collect();
                Row::inset(widget(Stack::column(0.0)).children(divided(rows)))
            },
        )),
        row(
            ui,
            "tts-rate",
            None,
            slider(
                ui,
                (0.5, 2.0, 0.05),
                "×",
                |s| f64::from(s.tts.rate),
                |s, v| s.tts.rate = v as f32,
            ),
        ),
        row(
            ui,
            "tts-volume",
            None,
            percent(
                ui,
                |s| f64::from(s.tts.volume),
                |s, v| s.tts.volume = v as f32,
            ),
        ),
        toggle_row!(ui, "tts-chat", tts.chat),
        toggle_row!(ui, "tts-partners", tts.partners),
        toggle_row!(ui, "tts-pace", tts.pace_typing),
    ];
    if !nanatype_tts::Tts::available() {
        rows.insert(0, block(hint(ui.now_t("tts-unavailable"))));
    }
    let enabled = |s: &Settings| s.tts.enabled;
    section_tools(
        ui,
        "tts",
        vec![
            info_tip(ui, "tts-hint"),
            tool(ui, ti::REFRESH, "tts-refresh", || Msg::Tts(TtsMsg::Refresh)),
            widget(Stack::row(0.0))
                .visible(move || ui.settings.with(|s| enabled(s)))
                .children(tool(ui, ti::PLAYER_PLAY, "tts-test", || {
                    Msg::Tts(TtsMsg::Test(None))
                }))
                .into_any(),
        ],
        vec![
            (toggle_row!(ui, "tts-enabled", tts.enabled), None),
            when_on(ui, enabled, rows),
        ],
    )
}

/// `ja` of `ja-JP`.
fn primary(tag: &str) -> &str {
    tag.split(['-', '_']).next().unwrap_or(tag)
}

/// Which languages' voices the pickers list: one, or all of them.
fn language_filter(ui: Ui, filter: Signal<String>) -> AnyView {
    let build = move || -> Vec<SelectOption> {
        let mut languages: Vec<String> = ui
            .tts_voices
            .with(|v| v.iter().map(|v| primary(&v.language).to_owned()).collect());
        languages.sort();
        languages.dedup();
        std::iter::once(SelectOption::new("", ui.now_t("tts-language-all")))
            .chain(
                languages
                    .into_iter()
                    .map(|l| SelectOption::new(l.clone(), l)),
            )
            .collect()
    };
    widget(Stack::row(0.0).width(LengthSpec::Px(field::MD)))
        .children(
            widget(
                Select::new(Some(filter.get_untracked()))
                    .options(build())
                    .size(ControlSize::Small),
            )
            .bind(move |s: &mut Select| {
                let opts = build();
                if s.options.len() != opts.len()
                    || s.options.iter().zip(&opts).any(|(a, b)| a.value != b.value)
                {
                    s.options = opts;
                }
                let v: Arc<str> = filter.get().into();
                if s.value.as_ref() != Some(&v) {
                    s.value = Some(v);
                }
            })
            .on(move |e: &SelectChanged| filter.set(e.value.to_string())),
        )
        .into_any()
}

/// The voices of the filtered language, by language and name, after the
/// default; the chosen one stays listed under another filter.
fn voice_options(
    ui: Ui,
    filter: Signal<String>,
    default_key: &str,
    chosen: impl Fn(&Settings) -> Option<String>,
) -> Vec<(String, String)> {
    let filter = filter.get();
    let chosen = ui.settings.with(|s| chosen(s));
    let label = |v: &Voice| format!("{} · {}", v.name, v.language);
    let mut options = vec![(String::new(), ui.now_t(default_key))];
    ui.tts_voices.with(|voices| {
        options.extend(
            voices
                .iter()
                .filter(|v| {
                    filter.is_empty()
                        || primary(&v.language) == filter
                        || chosen.as_deref() == Some(v.id.as_str())
                })
                .map(|v| (v.id.clone(), label(v))),
        );
        // A voice no longer installed still shows what is set.
        if let Some(id) = chosen
            && !voices.iter().any(|v| v.id == id)
        {
            options.push((id.clone(), id));
        }
    });
    options
}

/// Character `i`'s voice, with a sample of it.
fn character_voice(ui: Ui, filter: Signal<String>, i: usize, name: &str) -> Row {
    let label = if name.is_empty() {
        ui.tr("general-character-unnamed", &[("n", (i as u64 + 1).into())])
    } else {
        ui.tr("tts-character-voice", &[("name", name.to_owned().into())])
    };
    row_text(
        label,
        None,
        kit::hstack((
            live_select(
                ui,
                move || {
                    voice_options(ui, filter, "tts-voice-default", move |s| {
                        s.general.characters.get(i).and_then(|c| c.voice.clone())
                    })
                },
                move |s| {
                    s.general
                        .characters
                        .get(i)
                        .and_then(|c| c.voice.clone())
                        .unwrap_or_default()
                },
                move |s, v| {
                    if let Some(c) = s.general.characters.get_mut(i) {
                        c.voice = (!v.is_empty()).then(|| v.into());
                    }
                },
            ),
            button(ui, "tts-test", move || Msg::Tts(TtsMsg::Test(Some(i)))),
        ))
        .into_any(),
    )
}

/// Start or stop listening, and Resume while it is paused: bound in place,
/// so the row's control slot keeps its size as the phase changes.
fn listen_buttons(ui: Ui) -> AnyView {
    use nanatype_speech::controller::Phase;
    let phase = move || ui.speech.with(|s| s.phase);
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::CONTROL)
            .align(AlignSpec::Center),
    )
    .children((
        widget(Button::new(ui.now_t("speech-start")).size(ControlSize::Small))
            .bind(move |b: &mut Button| {
                let key = if phase().active() {
                    "speech-stop"
                } else {
                    "speech-start"
                };
                let label = super::I18N.t_in(ui.locale.get(), key);
                if b.label != label {
                    b.label = label;
                }
            })
            .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(Msg::Speech(SpeechMsg::Toggle))),
        widget(Stack::row(0.0))
            .visible(move || {
                let p = phase();
                p.active() && matches!(p, Phase::Paused | Phase::Error)
            })
            .children(button(ui, "speech-resume", || {
                Msg::Speech(SpeechMsg::Resume)
            })),
    ))
    .into_any()
}
