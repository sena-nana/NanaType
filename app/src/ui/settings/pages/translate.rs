use super::*;
use crate::msg::TranslateMsg;
use crate::ui::kit;
use nana_ui::runtime::{TextInput, TextSubmitted};
use nanatype_settings::TranslateProvider;
use nanatype_translate::lang;

fn openai(s: &Settings) -> bool {
    s.translate.enabled && s.translate.provider == TranslateProvider::Openai
}

fn deepl(s: &Settings) -> bool {
    s.translate.enabled && s.translate.provider == TranslateProvider::Deepl
}

/// Translated subtitles: the switch and its badge, the provider and its
/// key, the language, which lines and where they show, and a test.
pub(super) fn translate(ui: Ui) -> AnyView {
    let master = row_status(
        ui,
        "translate-enabled",
        Some("translate-enabled-hint"),
        link_badge(ui, |s| &s.translate),
        switch(ui, |s| s.translate.enabled, |s, v| s.translate.enabled = v),
    );
    let provider = row(
        ui,
        "translate-provider",
        None,
        segmented(
            ui,
            select_keys(
                ui,
                &[
                    ("openai", "translate-provider-openai"),
                    ("deepl", "translate-provider-deepl"),
                ],
            ),
            |s| s.translate.provider.key().into(),
            |s, v| {
                s.translate.provider = TranslateProvider::from_key(v).unwrap_or_default();
            },
        ),
    );
    let openai_rows = vec![
        row(
            ui,
            "translate-base-url",
            Some("translate-base-url-hint"),
            code_text(
                ui,
                "https://api.openai.com/v1".into(),
                field::LG,
                |s| s.translate.base_url.clone(),
                |s, v| s.translate.base_url = v,
            ),
        ),
        row(
            ui,
            "translate-model",
            None,
            code_text(
                ui,
                "gpt-4o-mini".into(),
                field::MD,
                |s| s.translate.model.clone(),
                |s, v| s.translate.model = v,
            ),
        ),
        row(
            ui,
            "translate-key",
            Some("translate-key-openai-hint"),
            key_field(ui, TranslateProvider::Openai),
        ),
    ];
    let deepl_rows = vec![row(
        ui,
        "translate-key",
        Some("translate-key-deepl-hint"),
        key_field(ui, TranslateProvider::Deepl),
    )];
    let rows = vec![
        row(
            ui,
            "translate-target",
            None,
            live_select(
                ui,
                move || target_options(ui),
                |s| s.translate.target.clone(),
                |s, v| s.translate.target = v.to_owned(),
            ),
        ),
        row(
            ui,
            "translate-target-custom",
            Some("translate-target-custom-hint"),
            code_text(
                ui,
                "pt-BR".into(),
                field::SM,
                |_| String::new(),
                |s, v| {
                    if lang::valid_code(&v) {
                        s.translate.target = v.trim().to_owned();
                    }
                },
            ),
        ),
        toggle_row!(ui, "translate-own", translate.own),
        toggle_row!(ui, "translate-partners", translate.partners),
        toggle_row!(ui, "translate-chat", translate.chat),
        toggle_row!(ui, "translate-display", translate.display),
        toggle_row!(ui, "translate-history", translate.history),
        row(
            ui,
            "translate-proxy",
            Some("translate-proxy-hint"),
            code_text(
                ui,
                "http://127.0.0.1:7890".into(),
                field::MD,
                |s| s.translate.proxy.clone(),
                |s, v| s.translate.proxy = v,
            ),
        ),
        test_row(ui),
    ];
    section_with(
        ui,
        "",
        vec![
            (Row::inset(master), None),
            when_on(ui, |s| s.translate.enabled, vec![provider]),
            when_on(ui, openai, openai_rows),
            when_on(ui, deepl, deepl_rows),
            when_on(ui, |s| s.translate.enabled, rows),
        ],
    )
}

/// The listed languages by their own names; a typed code stays listed.
fn target_options(ui: Ui) -> Vec<(String, String)> {
    let current = ui.settings.with(|s| s.translate.target.clone());
    let mut options: Vec<(String, String)> = lang::TARGETS
        .iter()
        .map(|l| (l.code.to_owned(), format!("{} · {}", l.native, l.code)))
        .collect();
    if lang::find(&current).is_none() {
        options.push((current.clone(), current));
    }
    options
}

/// A provider's API key: kept in the system's secret store, never in the
/// settings file; once saved it shows as saved, with a way to forget it.
fn key_field(ui: Ui, provider: TranslateProvider) -> AnyView {
    let draft = signal(String::new());
    let save = move || {
        let key = draft.get_untracked();
        draft.set(String::new());
        Msg::Translate(TranslateMsg::Key(provider, key))
    };
    let saved = move || {
        ui.translate.with(|st| match provider {
            TranslateProvider::Openai => st.openai_key,
            TranslateProvider::Deepl => st.deepl_key,
        })
    };
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::TIGHT)
            .align(AlignSpec::Center)
            .wrap(true),
    )
    .children((
        widget(Stack::row(0.0).width(LengthSpec::Px(field::SM))).children(
            widget(
                kit::text_input("")
                    .secure(true)
                    .placeholder(ui.now_t("translate-key-placeholder"))
                    .size(ControlSize::Small),
            )
            .bind(move |input: &mut TextInput| {
                let text = draft.get();
                if input.state.value.as_ref() != text.as_str() {
                    input.state.value = text.into();
                }
            })
            .on(move |e: &TextChanged| draft.set(e.value.chars().take(512).collect()))
            .on_cx(move |_, _: &TextSubmitted, cx| cx.dispatch_program_all(save())),
        ),
        primary(ui, "translate-key-save", save),
        widget(
            Stack::row(0.0)
                .gap_step(gap::step::TIGHT)
                .align(AlignSpec::Center),
        )
        .visible(saved)
        .children((
            widget(
                Text::new(ui.now_t("translate-key-saved"))
                    .style(Type::Small.style())
                    .color(SemanticColorRole::Muted),
            ),
            button(ui, "translate-key-clear", move || {
                Msg::Translate(TranslateMsg::Key(provider, String::new()))
            }),
        )),
    ))
    .into_any()
}

/// The test button and what it last gave.
fn test_row(ui: Ui) -> Row {
    let result = move || {
        let locale = ui.locale.get();
        ui.translate.with(|st| {
            if st.testing {
                return I18N.t_in(locale, "translate-testing");
            }
            match &st.test {
                Some(Ok((sample, text))) => format!("{sample} → {text}"),
                Some(Err(reason)) => reason.clone(),
                None => I18N.t_in(locale, "translate-test-hint"),
            }
        })
    };
    kit::line(
        widget(
            Text::new(result())
                .style(Type::Small.style())
                .color(SemanticColorRole::Muted),
        )
        .bind(move |t: &mut Text| {
            let text = result();
            if t.value != text {
                t.value = text;
            }
            let failed = ui
                .translate
                .with(|st| matches!(st.test, Some(Err(_))) && !st.testing);
            t.style.foreground = Some(if failed {
                SemanticColorRole::Danger
            } else {
                SemanticColorRole::Muted
            });
        }),
        button(ui, "translate-test", || Msg::Translate(TranslateMsg::Test)),
    )
}
