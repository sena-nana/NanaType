//! Small building blocks both products' forms use.

use nana_ui::runtime::view::{AnyView, IntoView, Signal, signal, widget};
use nana_ui::runtime::{Activate, Button, Stack};
use nana_ui::{ButtonKind, ControlSize};
use nana_ui_core::{AlignSpec, SemanticColorRole};

use super::Stage;
use super::kit::{Type, gap};

/// Standard, padded base64 (for `data:` URLs).
pub fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

pub fn label(text: String) -> AnyView {
    Type::Label.view(text)
}

pub fn hint(text: String) -> AnyView {
    Type::Caption.view(text)
}

/// A destructive action asked twice: `trigger` arms it, then the question
/// shows with Cancel and a danger button that sends `msg`.
pub fn confirm<M: Send + 'static>(
    ui: Stage,
    question: String,
    ok_label: String,
    trigger: impl FnOnce(Signal<bool>) -> AnyView,
    msg: impl Fn() -> M + Send + 'static,
) -> AnyView {
    let armed = signal(false);
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::CONTROL)
            .align(AlignSpec::Center),
    )
    .children((
        widget(Stack::row(0.0).align(AlignSpec::Center))
            .visible(move || !armed.get())
            .children(trigger(armed)),
        widget(
            Stack::row(0.0)
                .gap_step(gap::step::CONTROL)
                .align(AlignSpec::Center)
                .wrap(true),
        )
        .visible(move || armed.get())
        .children((
            widget(Type::Small.text(question).color(SemanticColorRole::Warning)),
            widget(Button::new(ui.now_t("common-cancel")).size(ControlSize::Small))
                .on(move |_: &Activate| armed.set(false)),
            widget(
                Button::new(ok_label)
                    .kind(ButtonKind::Danger)
                    .size(ControlSize::Small),
            )
            .on_cx(move |_, _: &Activate, cx| {
                armed.set(false);
                cx.dispatch_program_all(msg());
            }),
        )),
    ))
    .into_any()
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64_pads() {
        assert_eq!(super::base64(b"ab"), "YWI=");
        assert_eq!(super::base64(b"abc"), "YWJj");
    }
}
