//! Small building blocks both products' forms use.

use nana_ui::runtime::view::{
    AnyView, IntoView, NodeRef, Signal, entity_ref, node_ref, on_mount, signal, when, widget,
};
use nana_ui::runtime::{Activate, Button, Stack};
use nana_ui::{ButtonKind, ControlSize};
use nana_ui_core::{AlignSpec, LengthSpec, SemanticColorRole};
use nana_ui_platform::KeyState;
use std::sync::{Arc, Mutex};

use super::Stage;
use super::kit::{Type, field, gap};

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
///
/// Arming hides the trigger the focus was on, so the danger button takes
/// it: Enter goes ahead, Esc on either button backs out. Backing out hands
/// the focus back to the trigger.
pub fn confirm<M: Send + 'static>(
    ui: Stage,
    question: String,
    ok_label: String,
    trigger: impl FnOnce(Signal<bool>) -> AnyView + Send + 'static,
    msg: impl Fn() -> M + Send + 'static,
) -> AnyView {
    let armed = signal(false);
    // Whether it was armed since it mounted, so the trigger takes the focus
    // back only from the question, not when the page opens.
    let returning = signal(false);
    let (trigger_ref, ok_ref) = (node_ref(), node_ref());
    // Both halves are built once and kept: the one not shown waits out of
    // the tree, so the other is in it (and can take the focus) at once.
    let trigger = Mutex::new(Some(trigger));
    let msg = Arc::new(Mutex::new(msg));
    let question_row = move || {
        let (cancel, ok) = (entity_ref::<Button>(), entity_ref::<Button>());
        on_mount(move |cx| {
            for button in [cancel.get(), ok.get()].into_iter().flatten() {
                let _ = cx.on_key(button, move |key| {
                    let esc = key.state == KeyState::Pressed && key.physical.0.as_ref() == "Escape";
                    if esc {
                        armed.set(false);
                    }
                    esc
                });
            }
        });
        let msg = msg.clone();
        // The question takes a line of its own, at most a field wide, and
        // wraps there, the buttons under it: on one line it pushed the
        // row's label into a column a character wide.
        widget(
            Stack::row(0.0)
                .gap_step(gap::step::CONTROL)
                .align(AlignSpec::Center)
                .wrap(true)
                .with_layout(|l| l.max_width = Some(LengthSpec::Px(field::MD))),
        )
        .children((
            widget(Stack::column(0.0).width(LengthSpec::Fill)).children(widget(
                Type::Small
                    .text(question.clone())
                    .color(SemanticColorRole::Warning),
            )),
            widget(Button::new(ui.now_t("common-cancel")).size(ControlSize::Small))
                .entity_ref(cancel)
                .on(move |_: &Activate| armed.set(false)),
            widget(
                Button::new(ok_label.clone())
                    .kind(ButtonKind::Danger)
                    .size(ControlSize::Small),
            )
            .entity_ref(ok)
            .node_ref(ok_ref)
            .on_cx(move |_, _: &Activate, cx| {
                armed.set(false);
                let msg = msg.lock().unwrap_or_else(|e| e.into_inner());
                cx.dispatch_program_all(msg());
            }),
        ))
    };
    widget(Stack::row(0.0).align(AlignSpec::Center))
        .children((
            when(armed, question_row)
                .otherwise(move || {
                    let trigger = trigger
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .take()
                        .map(|trigger| trigger(armed));
                    widget(Stack::row(0.0).align(AlignSpec::Center))
                        .node_ref(trigger_ref)
                        .children(trigger)
                })
                .keep_alive()
                // The block's column is full width by default: in a
                // settings row's control slot it took the whole row and
                // squeezed the label to a character a line.
                .css(nana_ui::runtime::view::css! { width: auto; }),
            // Nothing to see: each branch only moves the focus as it mounts.
            when(armed, move || {
                returning.set(true);
                focus_first(ok_ref);
                widget(Stack::row(0.0))
            })
            .otherwise(move || {
                if returning.get_untracked() {
                    focus_first(trigger_ref);
                }
                widget(Stack::row(0.0))
            })
            .visible(false),
        ))
        .into_any()
}

/// Once mounted, move the focus to the first focusable node at or under
/// `scope`.
fn focus_first(scope: NodeRef) {
    on_mount(move |cx| {
        let Some(root) = scope.get_untracked() else {
            return;
        };
        let world = cx.world();
        let Some(document) = world.node(root).map(|n| n.document) else {
            return;
        };
        let mut stack = vec![root];
        let mut target = None;
        while let Some(id) = stack.pop() {
            if world.interaction(id).is_some_and(|i| i.focusable) {
                target = Some(id);
                break;
            }
            if let Some(node) = world.node(id) {
                stack.extend(node.children.into_iter().rev());
            }
        }
        if let Some(target) = target {
            let _ = cx.focus_node(document, target);
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64_pads() {
        assert_eq!(super::base64(b"ab"), "YWI=");
        assert_eq!(super::base64(b"abc"), "YWJj");
    }
}
