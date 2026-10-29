//! The quick input bar: the summon hotkey's one plain-text line, with no
//! toolbar or panels. Enter sends the line as typed (never as markup); Esc,
//! or leaving the bar, hides it.

use std::sync::Arc;

use nana_ui::ControlSize;
use nana_ui::icons_tabler as ti;
use nana_ui::runtime::view::{EntityRef, IntoView, WithRefs, entity_ref, widget, with_refs};
use nana_ui::runtime::{IconGlyph, Stack, TextChanged, TextInput, TextSubmitted};
use nana_ui_core::{AlignSpec, SemanticColorRole};

use super::Ui;
use crate::msg::Msg;
use crate::ui::kit::{self, gap, icon, inset};

pub type QuickRef = EntityRef<TextInput>;

/// The bar. Returns its field, for the host's Esc key and focus.
pub fn quick_window(ui: Ui) -> WithRefs<impl IntoView, QuickRef> {
    let field = entity_ref::<TextInput>();
    let mut input = kit::text_input("")
        .placeholder(ui.now_t("quick-placeholder"))
        .size(ControlSize::Large);
    // Flush with the bar: the window itself is the field.
    input.style.border = None;
    input.style.background = None;
    input.style.interaction = Default::default();
    Arc::make_mut(&mut input.style.layout).flex_grow = Some(1.0);
    let view = widget(
        Stack::fill_row(0.0)
            .gap_step(gap::step::CONTROL)
            .padding_step(inset::step::BAR_X, inset::step::BAR_Y)
            .align(AlignSpec::Center)
            .surface(SemanticColorRole::Background),
    )
    .key("quick-window")
    .children((
        widget(IconGlyph::new(ti::MESSAGE).size(icon::LG).decorative()).bind(
            |g: &mut IconGlyph| {
                g.style.foreground = Some(SemanticColorRole::Muted);
            },
        ),
        widget(input)
            .entity_ref(field)
            .key("quick-field")
            .bind(move |input: &mut TextInput| {
                let text = ui.quick_text.get();
                if input.state.value.as_ref() != text.as_str() {
                    input.state.replace_value(text);
                }
            })
            .on(move |e: &TextChanged| ui.quick_text.set(e.value.to_string()))
            .on_cx(move |_, _: &TextSubmitted, cx| {
                cx.dispatch_program_all(Msg::QuickSend(ui.quick_text.get_untracked()));
            }),
    ));
    with_refs(view, field)
}
