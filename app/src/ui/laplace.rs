//! The LAPLACE page: chat from a LAPLACE Event Bridge.
use super::{
    Ui,
    settings::controls::{block, laplace_badge, primary, row, switch, toggled_section},
};
use crate::msg::{LaplaceMsg, Msg};
use crate::ui::kit::{self, Type, gap};
use nana_ui::ControlSize;
use nana_ui::runtime::view::{AnyView, IntoView, signal, widget};
use nana_ui::runtime::{Stack, Text, TextChanged, TextInput, TextSubmitted};
use nana_ui_core::{AlignSpec, LengthSpec, SemanticColorRole};

/// One card: the switch, then the bridge's link and what it connects to.
pub fn settings_page(ui: Ui) -> AnyView {
    // What it connects to, with the token never shown.
    let saved = widget(
        Text::new("")
            .style(Type::Caption.mono())
            .color(SemanticColorRole::Muted),
    )
    .bind(move |t: &mut Text| {
        let address = ui.settings.with(|s| s.integrations.laplace.url.clone());
        let v = if ui.laplace.get().token_saved {
            let sep = if address.contains('?') { '&' } else { '?' };
            format!("{address}{sep}token=••••")
        } else {
            address
        };
        if t.value != v {
            t.value = v;
        }
    });
    toggled_section(
        ui,
        "",
        row(
            ui,
            "int-laplace",
            None,
            kit::hstack((
                laplace_badge(ui),
                switch(
                    ui,
                    |s| s.integrations.laplace.enabled,
                    |s, v| s.integrations.laplace.enabled = v,
                ),
            )),
        ),
        |s| s.integrations.laplace.enabled,
        vec![
            row(
                ui,
                "int-laplace-link",
                Some("int-laplace-link-hint"),
                link_field(ui),
            ),
            // The address is long: on its own line, so the label keeps its width.
            block(saved),
        ],
    )
}

/// The bridge's link, pasted whole: its token goes to the system's secret
/// store and only the address to the settings file.
fn link_field(ui: Ui) -> AnyView {
    let draft = signal(String::new());
    let save = move || {
        let link = draft.get_untracked().trim().to_owned();
        draft.set(String::new());
        Msg::Laplace(LaplaceMsg::Link(link))
    };
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::TIGHT)
            .align(AlignSpec::Center),
    )
    .children((
        widget(Stack::row(0.0).width(LengthSpec::Px(crate::ui::kit::field::SM))).children(
            widget(
                kit::text_input("")
                    .secure(true)
                    .placeholder(ui.now_t("int-laplace-link-placeholder"))
                    .size(ControlSize::Small),
            )
            .bind(move |input: &mut TextInput| {
                let text = draft.get();
                if input.state.value.as_ref() != text.as_str() {
                    input.state.value = text.into();
                }
            })
            .on(move |e: &TextChanged| draft.set(e.value.chars().take(2048).collect()))
            .on_cx(move |_, _: &TextSubmitted, cx| cx.dispatch_program_all(save())),
        ),
        primary(ui, "int-laplace-link-save", save),
        kit::tool(
            nana_ui::icons_tabler::BOOK,
            ui.now_t("int-laplace-guide"),
            move || Msg::OpenUrl(guide(ui.locale.get_untracked())),
        ),
    ))
    .into_any()
}

/// How to set up a bridge for NanaType, in the interface's language.
fn guide(locale: nanatype_i18n::Locale) -> &'static str {
    use nanatype_i18n::Locale;
    match locale {
        Locale::JaJp => "https://github.com/sena-nana/NanaType/blob/main/docs/ja/laplace.md",
        Locale::EnUs => "https://github.com/sena-nana/NanaType/blob/main/docs/en/laplace.md",
        _ => "https://github.com/sena-nana/NanaType/blob/main/docs/laplace.md",
    }
}
