use super::controls::{block, row, row_text, row_tip, section_with, shown_if, text};
use crate::ui::kit::{self, Type, field};
use crate::{
    msg::Msg,
    stickers::telegram::{Command, Phase},
    ui::Ui,
};
use nana_ui::{
    ControlSize,
    runtime::view::{AnyView, IntoView, Signal, dynamic, signal, widget},
    runtime::{Activate, Button, Stack, Text, TextChanged, TextInput},
};
use nana_ui_core::LengthSpec;

fn action(ui: Ui, key: &str, command: impl Fn() -> Command + Send + 'static) -> AnyView {
    let connect = key == "telegram-connect";
    widget(Button::new(ui.now_t(key)).size(ControlSize::Small))
        .key(key.to_owned())
        .bind(move |b: &mut Button| {
            let state = ui.telegram.get();
            b.disabled =
                state.busy || (connect && matches!(state.phase, Phase::Code | Phase::Password));
        })
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(Msg::Telegram(command())))
        .into_any()
}

/// A login or search field, with what acts on it beside it.
fn field(
    ui: Ui,
    key: &str,
    value: Signal<String>,
    secure: bool,
    actions: impl IntoView,
) -> AnyView {
    let input = widget(kit::text_input("").secure(secure).size(ControlSize::Small))
        .key(format!("{key}-input"))
        .bind(move |input: &mut TextInput| {
            let value = value.get();
            if input.state.value.as_ref() != value {
                input.state = nana_ui::runtime::TextInputState::new(value);
            }
            input.disabled = ui.telegram.get().busy;
        })
        .on(move |e: &TextChanged| value.set(e.value.chars().take(256).collect()));
    row(
        ui,
        key,
        None,
        kit::hstack((
            widget(Stack::row(0.0).width(LengthSpec::Px(field::MD))).children(input),
            actions,
        )),
    )
    .into_any()
}

/// Whether the session is in `phase`, tracked.
fn at(ui: Ui, phase: Phase) -> impl Fn() -> bool + Send + 'static {
    move || ui.telegram.get().phase == phase
}

/// What the session is doing, worded.
fn status_text(ui: Ui) -> AnyView {
    widget(Type::Caption.text(""))
        .key("telegram-status")
        .bind(move |t: &mut Text| {
            let state = ui.telegram.get();
            t.value = if let Some(error) = &state.error {
                ui.tr("stickers-failed", &[("message", error.clone().into())])
            } else if let Some((done, total)) = state.progress {
                ui.tr(
                    "stickers-downloading",
                    &[
                        ("done", (done as u64).into()),
                        ("total", (total as u64).into()),
                    ],
                )
            } else {
                ui.now_t(if state.busy {
                    "telegram-working"
                } else {
                    match state.phase {
                        Phase::Offline => "telegram-offline",
                        Phase::Code => "telegram-code-sent",
                        Phase::Password => "telegram-password-required",
                        Phase::Ready => "telegram-ready",
                    }
                })
            };
        })
        .into_any()
}

pub(super) fn panel(ui: Ui) -> AnyView {
    let api_hash = signal(String::new());
    let phone = signal(String::new());
    let code = signal(String::new());
    let password = signal(String::new());
    let query = signal(String::new());
    let connect = action(ui, "telegram-connect", move || {
        let settings = ui.settings.get_untracked();
        let hash = api_hash.get_untracked();
        api_hash.set(String::new());
        let number = phone.get_untracked();
        phone.set(String::new());
        Command::Connect {
            api_id: settings.assets.telegram.api_id,
            api_hash: hash,
            phone: number,
            proxy: settings.assets.telegram.proxy.clone(),
        }
    });
    let cancel = widget(Button::new(ui.now_t("common-cancel")).size(ControlSize::Small))
        .visible(move || {
            let state = ui.telegram.get();
            state.busy && state.progress.is_some()
        })
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(Msg::Telegram(Command::Cancel)));
    let logout = widget(Button::new(ui.now_t("telegram-logout")).size(ControlSize::Small))
        .visible(move || {
            let state = ui.telegram.get();
            state.phase != Phase::Offline || state.error.is_some()
        })
        .bind(move |b: &mut Button| b.disabled = ui.telegram.get().busy)
        .key("telegram-logout")
        .on_cx(move |_, _: &Activate, cx| {
            for secret in [api_hash, phone, code, password] {
                secret.set(String::new());
            }
            cx.dispatch_program_all(Msg::Telegram(Command::Logout))
        });
    let results = dynamic(
        move || ui.telegram.get().results.clone(),
        move |packs| {
            let rows: Vec<_> = packs
                .iter()
                .map(|pack| {
                    let name = pack.name.clone();
                    row_text(
                        pack.title.clone(),
                        Some(format!(
                            "{} · {}",
                            pack.name,
                            ui.tr("telegram-count", &[("count", (pack.count as u64).into())])
                        )),
                        action(ui, "stickers-download", move || {
                            Command::Download(name.clone())
                        }),
                    )
                })
                .collect();
            widget(Stack::column(0.0)).children(rows).into_any()
        },
    );
    let no_results = move || {
        let state = ui.telegram.get();
        state.searched && state.results.is_empty() && !state.busy && state.error.is_none()
    };
    section_with(
        ui,
        "telegram-stickers",
        vec![
            (
                row_tip(
                    ui,
                    "telegram-api-id",
                    "telegram-hint",
                    text(
                        ui,
                        String::new(),
                        field::MD,
                        |s| {
                            if s.assets.telegram.api_id > 0 {
                                s.assets.telegram.api_id.to_string()
                            } else {
                                String::new()
                            }
                        },
                        |s, value| s.assets.telegram.api_id = value.parse().unwrap_or(0),
                    ),
                ),
                None,
            ),
            shown_if(
                field(ui, "telegram-api-hash", api_hash, true, ()),
                at(ui, Phase::Offline),
            ),
            shown_if(
                field(ui, "telegram-phone", phone, true, ()),
                at(ui, Phase::Offline),
            ),
            (
                row_tip(
                    ui,
                    "telegram-proxy",
                    "telegram-proxy-hint",
                    text(
                        ui,
                        "socks5://127.0.0.1:7890".into(),
                        field::MD,
                        |s| s.assets.telegram.proxy.clone(),
                        |s, value| s.assets.telegram.proxy = value,
                    ),
                ),
                None,
            ),
            shown_if(
                field(
                    ui,
                    "telegram-code",
                    code,
                    true,
                    action(ui, "telegram-verify", move || {
                        let value = code.get_untracked();
                        code.set(String::new());
                        Command::Code(value)
                    }),
                ),
                at(ui, Phase::Code),
            ),
            shown_if(
                field(
                    ui,
                    "telegram-password",
                    password,
                    true,
                    action(ui, "telegram-verify", move || {
                        let value = password.get_untracked();
                        password.set(String::new());
                        Command::Password(value)
                    }),
                ),
                at(ui, Phase::Password),
            ),
            // Where the session stands, and what can be done about it.
            (kit::line(status_text(ui), (connect, cancel, logout)), None),
            shown_if(
                field(
                    ui,
                    "telegram-query",
                    query,
                    false,
                    (
                        action(ui, "telegram-search", move || {
                            Command::Search(query.get_untracked())
                        }),
                        action(ui, "telegram-download", move || {
                            Command::Download(query.get_untracked())
                        }),
                    ),
                ),
                at(ui, Phase::Ready),
            ),
            shown_if(results.into_any(), move || {
                at(ui, Phase::Ready)() && !ui.telegram.get().results.is_empty()
            }),
            shown_if(
                block(widget(Type::Caption.text(ui.now_t("telegram-no-results")))),
                move || at(ui, Phase::Ready)() && no_results(),
            ),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stickers::telegram::{Pack, State};
    use nana_ui::runtime::{DocumentId, RuntimeDocument};
    use nana_ui_devtools::agent::RuntimeAgentSession;
    use std::sync::Arc;

    #[test]
    fn login_states_search_and_download_controls_render_and_dispatch() {
        let out = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target/telegram-acceptance");
        std::fs::create_dir_all(&out).unwrap();
        for (index, phase) in [Phase::Offline, Phase::Code, Phase::Password, Phase::Ready]
            .into_iter()
            .enumerate()
        {
            let skin = nanatype_theme::SkinRegistry::new(None, "test")
                .load("nanatype.vanilla")
                .unwrap()
                .skin;
            let mut settings = nanatype_settings::Settings::default();
            settings.assets.telegram.api_id = 12345;
            let ui = Ui::new(Arc::new(settings), skin, nanatype_i18n::Locale::ZhCn);
            ui.telegram.set(Arc::new(State {
                phase,
                results: vec![Pack {
                    name: "TestPack".into(),
                    title: "测试贴纸包".into(),
                    count: 3,
                }],
                ..Default::default()
            }));
            let id = DocumentId::new(1).unwrap();
            let mut document = RuntimeDocument::new(id);
            document
                .context_mut()
                .set_theme_tokens(crate::ui::kit::theme::compile(index % 2 == 1))
                .unwrap();
            document
                .context_mut()
                .mount_view_root(id, move || panel(ui))
                .unwrap();
            let mut session = RuntimeAgentSession::new(document, 840, 1000).unwrap();
            session
                .screenshot_png(out.join(format!("phase-{index}.png")))
                .unwrap();
            let nodes = session.accessibility_dump();
            assert!(
                nodes
                    .iter()
                    .any(|n| n.label.as_deref() == Some("Telegram 贴纸"))
            );
            let expected = match phase {
                Phase::Offline => "手机号（含 + 国家代码）",
                Phase::Code => "Telegram 验证码",
                Phase::Password => "两步验证密码",
                Phase::Ready => "测试贴纸包",
            };
            assert!(
                nodes.iter().any(|n| n.label.as_deref() == Some(expected)),
                "missing {expected}"
            );
            for node in nodes
                .iter()
                .filter(|n| n.role == "button" && n.bounds.width > 0.0)
            {
                assert!(
                    node.bounds.x >= 0.0 && node.bounds.x + node.bounds.width <= 840.5,
                    "button outside panel: {node:?}"
                );
            }
            let click = |session: &mut RuntimeAgentSession, key: &str| {
                let node = session
                    .accessibility_dump()
                    .into_iter()
                    .find(|n| {
                        n.agent_path
                            .as_deref()
                            .is_some_and(|p| p.rsplit('/').next() == Some(key))
                    })
                    .unwrap();
                session
                    .click_xy(
                        node.bounds.x + node.bounds.width / 2.0,
                        node.bounds.y + node.bounds.height / 2.0,
                    )
                    .unwrap();
            };
            let type_into = |session: &mut RuntimeAgentSession, key: &str, value: &str| {
                click(session, key);
                session.type_text(value).unwrap();
            };
            match phase {
                Phase::Offline => {
                    type_into(&mut session, "telegram-api-hash-input", &"a".repeat(32));
                    type_into(&mut session, "telegram-phone-input", "+12345678");
                    click(&mut session, "telegram-connect");
                }
                Phase::Code => {
                    type_into(&mut session, "telegram-code-input", "12345");
                    click(&mut session, "telegram-verify");
                }
                Phase::Password => {
                    type_into(&mut session, "telegram-password-input", "test-password");
                    click(&mut session, "telegram-verify");
                }
                Phase::Ready => {
                    type_into(&mut session, "telegram-query-input", "cats");
                    click(&mut session, "telegram-search");
                    click(&mut session, "stickers-download");
                }
            }
            let commands: Vec<_> = session
                .document_mut()
                .context_mut()
                .take_program_messages()
                .into_iter()
                .filter_map(|m| m.downcast::<Msg>().ok())
                .collect();
            assert!(commands.iter().any(|m| match (&**m, phase) {
                (
                    Msg::Telegram(Command::Connect {
                        api_id: 12345,
                        api_hash,
                        phone,
                        ..
                    }),
                    Phase::Offline,
                ) => api_hash == &"a".repeat(32) && phone == "+12345678",
                (Msg::Telegram(Command::Code(code)), Phase::Code) => code == "12345",
                (Msg::Telegram(Command::Password(password)), Phase::Password) =>
                    password == "test-password",
                (Msg::Telegram(Command::Search(query)), Phase::Ready) => query == "cats",
                _ => false,
            }));
            if phase == Phase::Ready {
                assert!(commands.iter().any(
                    |m| matches!(&**m, Msg::Telegram(Command::Download(name)) if name == "TestPack")
                ));
            }
        }
    }
}
