//! Shared operator controls; interim text stays outside the streamed draft.
use crate::msg::Msg;
use crate::ui::{
    Ui,
    kit::{Type, gap},
    settings::controls::*,
};
use nana_ui::runtime::view::{AnyView, IntoView, widget};
use nana_ui::runtime::{Stack, Text};
use nana_ui_core::LengthSpec;
use nanatype_settings::SpeechMode;
use std::sync::Arc;

pub(crate) fn mode(ui: Ui) -> AnyView {
    segmented(
        ui,
        select_keys(
            ui,
            &[
                ("manual", "speech-mode-manual"),
                ("auto", "speech-mode-auto"),
            ],
        ),
        |s| {
            if s.speech.mode == SpeechMode::Auto {
                "auto".into()
            } else {
                "manual".into()
            }
        },
        |s, v| {
            s.speech.mode = if v == "auto" {
                SpeechMode::Auto
            } else {
                SpeechMode::Manual
            }
        },
    )
}
/// Whether there is a status to show: listening, or an error to read.
pub(crate) fn has_status(ui: Ui) -> bool {
    ui.speech.with(|s| s.phase.active() || s.error.is_some())
}
/// The input window's status bar: [`status_text`] on the bar inset, shown
/// while [`has_status`] holds.
pub(crate) fn status(ui: Ui) -> AnyView {
    widget(
        Stack::column(0.0)
            .gap_step(gap::step::TIGHT)
            .padding_step(
                crate::ui::kit::inset::step::BAR_X,
                crate::ui::kit::inset::step::BAR_Y,
            )
            .width(LengthSpec::Fill),
    )
    .visible(move || has_status(ui))
    .children(status_text(ui))
    .into_any()
}
/// The phase and the interim text, as one caption without its own inset.
pub(crate) fn status_text(ui: Ui) -> AnyView {
    widget(Text::new("").style(Type::Caption.style()))
        .bind(move |text: &mut Text| {
            let state = ui.speech.get();
            let mut value = state.status(ui);
            if !state.partial.is_empty() {
                value.push_str(" · ");
                value.extend(state.partial.chars().take(120));
            }
            if text.value != value {
                text.value = value;
            }
            let l = Arc::make_mut(&mut text.style.layout);
            l.max_height = Some(LengthSpec::Px(crate::ui::kit::INPUT_STATUS_MAX_H));
            l.overflow_y = nana_ui_core::OverflowSpec::Hidden;
        })
        .into_any()
}
pub(crate) fn actions(ui: Ui) -> AnyView {
    nana_ui::runtime::view::dynamic(
        move || (ui.locale.get(), ui.speech.with(|s| s.phase)),
        move |(_, phase)| {
            let active = phase.active();
            widget(Stack::row(0.0).gap_step(gap::step::CONTROL))
                .children((
                    button(
                        ui,
                        if active {
                            "speech-stop"
                        } else {
                            "speech-start"
                        },
                        || Msg::SpeechToggle,
                    ),
                    (active
                        && matches!(
                            phase,
                            nanatype_speech::controller::Phase::Paused
                                | nanatype_speech::controller::Phase::Error
                        ))
                    .then(|| button(ui, "speech-resume", || Msg::SpeechResume)),
                ))
                .into_any()
        },
    )
    .into_any()
}
