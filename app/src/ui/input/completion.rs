//! The `@` / `/` completion list, floating over the foot of the compose
//! area while it is open. The editor keeps focus: its keys choose a row
//! (see `host::completion_key`), and a click fills one in.
use super::*;
use crate::completion::Trigger;
use crate::msg::Complete;
use crate::ui::kit::{card_frame, gap, icon, popover};
use nana_ui::runtime::{IconGlyph, ListItem};
use nana_ui_core::RadiusTier;

pub(super) fn popup(ui: Ui) -> AnyView {
    let rows = dynamic(
        move || {
            ui.completion.with(|c| {
                c.as_ref()
                    .map(|v| (v.trigger, Arc::as_ptr(&v.items) as *const () as usize))
            })
        },
        move |key| {
            let Some(view) = ui.completion.get_untracked() else {
                return Vec::new();
            };
            let glyph = match key.map(|(t, _)| t) {
                Some(Trigger::Mention) => ti::AT,
                _ => ti::SLASH,
            };
            view.items
                .iter()
                .enumerate()
                .map(|(i, item)| row(ui, i, &item.label, &item.detail, glyph))
                .collect()
        },
    );
    widget(
        card_frame(RadiusTier::Md)
            .gap_step(gap::step::HAIR)
            .padding_step(gap::step::TIGHT, gap::step::TIGHT)
            .with_layout(|l| {
                l.position = PositionSpec::Absolute;
                l.offset_left = Some(LengthSpec::Px(gap::CONTROL));
                l.offset_bottom = Some(LengthSpec::Px(gap::CONTROL));
                l.width = Some(LengthSpec::Px(popover::XL));
                l.max_width = Some(LengthSpec::Percent(90.0));
            }),
    )
    .key("input-completion")
    .visible(move || ui.completion.with(Option::is_some))
    .children(rows)
    .into_any()
}

fn row(ui: Ui, i: usize, label: &str, detail: &str, glyph: nana_ui::Icon) -> AnyView {
    let mut item = ListItem::new(label).size(ControlSize::Small);
    item.detail = detail.to_owned();
    widget(item)
        .bind(move |item: &mut ListItem| {
            let selected = ui
                .completion
                .with(|c| c.as_ref().is_some_and(|v| v.selected == i));
            if item.selected != selected {
                item.selected = selected;
            }
        })
        .leading(
            widget(IconGlyph::new(glyph).size(icon::MD)).bind(|g: &mut IconGlyph| {
                g.style.foreground = Some(SemanticColorRole::Muted);
            }),
        )
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(Msg::Complete(Complete::Accept(Some(i))))
        })
        .into_any()
}
