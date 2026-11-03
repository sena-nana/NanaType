//! Layout parts: cards, sections, rows, dividers and bars. A settings page,
//! the package list and the chat window's cards are all built from these, so
//! their insets, hairlines and radii are one decision.

use std::sync::Arc;

use nana_ui::runtime::view::{
    AnyView, IntoView, NodeRef, ViewBuilder, node_ref, on_mount, settings_row, widget,
};
use nana_ui::runtime::{
    AppContext, Divider, Entity, MutationQueue, SettingsRow, StableNodeId, Stack,
};
use nana_ui_core::{AlignSpec, JustifySpec, LengthSpec, RadiusTier, SemanticColorRole};

use super::tokens::{gap, inset};
use nana_ui::runtime::view::RaisedSurfacePainter;

/// One row of a section's card, already on the card's inset.
///
/// A card insets nothing itself (NanaUI's `group_row` adds a hairline, not
/// padding), so a bare button or caption would sit flush against its edge.
/// Only the row builders make one: [`row_text`], [`row_hint_fn`],
/// [`block`] for full-width content and [`line`] for anything else. A view
/// that already carries the card inset says so with [`Row::inset`].
pub struct Row(AnyView);

impl Row {
    /// `view` as a row as it is: it brings the card's inset itself.
    pub fn inset(view: impl IntoView) -> Self {
        Self(view.into_any())
    }
}

impl IntoView for Row {
    fn build(self, vb: &mut ViewBuilder<'_, '_, '_>) {
        self.0.build(vb)
    }

    fn into_any(self) -> AnyView {
        self.0
    }
}

/// Whether a section row is shown; `None` always shows it.
pub type Shown = Option<Box<dyn Fn() -> bool + Send + 'static>>;

/// A row that shows only while `cond` holds, for [`section_titled`].
pub fn shown_if(view: impl IntoView, cond: impl Fn() -> bool + Send + 'static) -> (Row, Shown) {
    (Row::inset(view), Some(Box::new(cond)))
}

/// A card's frame: the surface, a soft hairline and (on a light chrome) a
/// short lift, at `radius`. Children are laid out in a column. (No NanaUI
/// `CardKind` draws this: `Raised` has no hairline and lifts on the theme's
/// popover shadow in both modes, `Outlined` has no fill.)
pub fn card_frame(radius: RadiusTier) -> Stack {
    Stack::column(0.0)
        .radius(radius)
        .painter(RaisedSurfacePainter(radius))
}

/// A card holding free-form content: padded on the card inset, its
/// children `gap::CONTROL` apart.
pub fn panel() -> Stack {
    Stack::column(0.0)
        .gap_step(gap::step::CONTROL)
        .padding_step(inset::step::CARD, inset::step::CARD)
        .radius(RadiusTier::Lg)
        .painter(RaisedSurfacePainter(RadiusTier::Lg))
}

/// A titled group of rows (NanaUI's settings group): the title above, the
/// card below, an inset hairline between rows; a hidden row takes its
/// divider with it. `None` leaves the title out (the page heading already
/// names it).
pub fn section_titled<V: IntoView>(title: Option<String>, rows: Vec<(V, Shown)>) -> AnyView {
    use nana_ui::runtime::view::group_row;
    let rows = rows
        .into_iter()
        .map(|(view, shown)| match shown {
            Some(cond) => group_row(Row::inset(view)).visible(cond),
            None => group_row(Row::inset(view)),
        })
        .collect();
    nana_ui::runtime::view::settings_group(title, rows).into_any()
}

/// The inset hairline between two rows of a card.
pub fn divider() -> AnyView {
    widget(Divider::horizontal().inset(inset::CARD)).into_any()
}

/// A full-width hairline (under a bar, between panes).
pub fn rule() -> AnyView {
    widget(Divider::horizontal()).into_any()
}

/// A short vertical hairline between groups of a toolbar: an icon tall, in
/// the stronger border colour, centred in the row.
pub fn vrule() -> AnyView {
    let mut rule = Divider::vertical();
    rule.style.background = Some(SemanticColorRole::Border);
    let l = Arc::make_mut(&mut rule.style.layout);
    l.height = Some(LengthSpec::Px(super::tokens::icon::LG));
    l.align_self = Some(AlignSpec::Center);
    l.margin_left = Some(LengthSpec::Px(gap::TIGHT));
    l.margin_right = Some(LengthSpec::Px(gap::TIGHT));
    widget(rule).into_any()
}

/// Rows with a divider between each, for a group nested in one card row.
pub fn divided<V: IntoView>(rows: Vec<V>) -> Vec<AnyView> {
    rows.into_iter()
        .enumerate()
        .map(|(i, r)| {
            widget(Stack::column(0.0))
                .children(((i > 0).then(divider), Row::inset(r)))
                .into_any()
        })
        .collect()
}

/// A full-width block inside a card, inset like the rows.
pub fn block(view: impl IntoView) -> Row {
    Row::inset(
        widget(
            Stack::column(0.0)
                .gap_step(gap::step::CONTROL)
                .padding_step(inset::step::CARD, inset::step::ROW_Y),
        )
        .children(view),
    )
}

/// A row inside a card: `left` takes the room, `right` keeps its width.
/// For rows that are not a label and a control (a list entry, a header).
pub fn line(left: impl IntoView, right: impl IntoView) -> Row {
    Row::inset(
        widget(
            Stack::row(0.0)
                .gap_step(gap::step::GROUP)
                .padding_step(inset::step::CARD, inset::step::ROW_Y)
                .align(AlignSpec::Center)
                .width(LengthSpec::Fill),
        )
        .children((
            widget(crate::ui::kit::grow_column(gap::HAIR)).children(left),
            widget(
                Stack::row(0.0)
                    .gap_step(gap::step::CONTROL)
                    .align(AlignSpec::Center)
                    .justify(JustifySpec::End)
                    .with_layout(|l| l.flex_shrink = Some(0.0)),
            )
            .children(right),
        )),
    )
}

/// Label (and hint) on the left, control on the right; stacks when narrow.
/// A hint too long to read beside a control goes in a tip card under the
/// row instead (see [`with_tip`]).
pub fn row_text(label_text: String, hint_text: Option<String>, control: AnyView) -> Row {
    if let Some(text) = hint_text.as_deref().filter(|h| long_hint(h)) {
        let text = text.to_owned();
        return with_tip(row_text(label_text, None, control), text);
    }
    let hint: Option<Arc<str>> = hint_text.map(Into::into);
    inset_row(
        settings_row(label_text)
            .hint(hint)
            .control(control_slot(control))
            .into_any(),
    )
}

/// A hint at least this wide (a CJK character counts two) reads as a
/// paragraph: beside a control it wraps into a narrow column of lines.
const TIP_AT: usize = 72;

fn long_hint(text: &str) -> bool {
    let wide = |c: char| c >= '\u{2E80}';
    text.chars()
        .map(|c| if wide(c) { 2 } else { 1 })
        .sum::<usize>()
        >= TIP_AT
}

/// `row` with `text` in a tip card under it: what the row needs said, too
/// long for a hint beside its control.
pub fn with_tip(row: Row, text: String) -> Row {
    let tip = widget(
        Stack::column(0.0)
            .padding_step(inset::step::CARD, inset::step::ROW_Y)
            .with_layout(|l| l.padding_top = Some(LengthSpec::Px(0.0))),
    )
    .children(super::callout(super::Tone::Neutral, text));
    Row::inset(widget(Stack::column(0.0)).children((row, tip)))
}

/// A label and its control on one line, packed close: the label is cut
/// before the control is, and the row never stacks. For a narrow column of
/// settings, whose controls are [`super::field::DENSE`] wide.
pub fn dense_row(label_text: String, control: impl IntoView) -> AnyView {
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::CONTROL)
            .align(AlignSpec::Center)
            .justify(JustifySpec::SpaceBetween)
            .width(LengthSpec::Fill)
            .with_layout(|l| {
                l.padding_top = Some(LengthSpec::Px(gap::HAIR));
                l.padding_bottom = Some(LengthSpec::Px(gap::HAIR));
            }),
    )
    .children((
        widget(
            super::Type::Caption
                .text(label_text)
                .style(super::Type::Caption.one_line()),
        ),
        widget(
            Stack::row(0.0)
                .align(AlignSpec::Center)
                .justify(JustifySpec::End)
                .with_layout(|l| l.flex_shrink = Some(0.0)),
        )
        .children(control),
    ))
    .into_any()
}

/// [`row_text`] whose hint is re-read as the signals it reads change.
pub fn row_hint_fn(
    label_text: String,
    hint: impl Fn() -> Option<String> + Send + 'static,
    control: AnyView,
) -> Row {
    inset_row(
        settings_row(label_text)
            .hint(move || hint().map(Arc::<str>::from))
            .control(control_slot(control))
            .into_any(),
    )
}

/// The row's control, in a slot that spaces it from its neighbours. The row
/// names the slot it is given for assistive technology; the control inside
/// it is what a screen reader lands on, so it takes the row's label too.
fn control_slot(control: AnyView) -> AnyView {
    let slot = node_ref();
    on_mount(move |cx| name_slot_controls(cx, slot));
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::CONTROL)
            .align(AlignSpec::Center),
    )
    .node_ref(slot)
    .children(control)
    .into_any()
}

/// Let the row's label name every unnamed control under `slot`: a switch, a
/// slider beside its readout, a field inside its width box.
fn name_slot_controls(cx: &mut AppContext, slot: NodeRef) {
    let Some(slot) = slot.get_untracked() else {
        return;
    };
    let world = cx.world();
    let label = world.labelled_by(slot).or_else(|| {
        let row = world.node(slot)?.parent?;
        cx.read(Entity::<SettingsRow>::from_stable_id(row), |r| r.label_slot)
            .ok()
            .flatten()
    });
    if let Some(label) = label {
        name_controls(cx, slot, label);
    }
}

/// Once mounted, let the first text under `caption` name every unnamed
/// control under `controls`: a caption written above or beside its field.
pub fn caption_names_controls(caption: NodeRef, controls: NodeRef) {
    on_mount(move |cx| {
        let label = caption.get_untracked().and_then(|c| {
            let world = cx.world();
            let mut stack = vec![c];
            while let Some(id) = stack.pop() {
                if world.text(id).is_some_and(|t| !t.is_empty()) {
                    return Some(id);
                }
                stack.extend(world.node(id)?.children.into_iter().rev());
            }
            None
        });
        if let (Some(scope), Some(label)) = (controls.get_untracked(), label) {
            name_controls(cx, scope, label);
        }
    });
}

/// Let `label` name every control under `scope` that has no name of its own
/// (a switch, a slider, a field, a select or a segmented choice).
pub fn name_controls(cx: &mut AppContext, scope: StableNodeId, label: StableNodeId) {
    use nana_ui::runtime::AccessibilityRole as R;
    let world = cx.world();
    let mut queue = MutationQueue::new();
    let mut stack = vec![scope];
    while let Some(id) = stack.pop() {
        let Some(node) = world.node(id) else {
            continue;
        };
        let unnamed = world.accessibility(id).is_some_and(|a| {
            matches!(
                a.role,
                R::Switch | R::Slider | R::TextInput | R::ComboBox | R::RadioGroup | R::Checkbox
            ) && a.label.as_deref().is_none_or(str::is_empty)
        });
        if id != scope && unnamed && world.labelled_by(id).is_none() {
            queue.set_labelled_by(id, Some(label));
        }
        stack.extend(node.children);
    }
    if !queue.is_empty() {
        let _ = cx.commit_mutations(queue);
    }
}

fn inset_row(row: AnyView) -> Row {
    Row::inset(widget(Stack::column(0.0).padding_xy(inset::CARD, 0.0)).children(row))
}

/// A column that takes the room a row leaves it (and gives it back when the
/// row is narrow), its height following its content.
pub fn grow_column(gap: f32) -> Stack {
    Stack::column(gap)
        .grow(1.0)
        .shrink(1.0)
        .with_layout(|l| l.min_width = Some(LengthSpec::Px(0.0)))
}

/// A flexible gap that pushes what follows to the end of a row.
pub fn spacer() -> AnyView {
    widget(Stack::row(0.0).grow(1.0).shrink(1.0)).into_any()
}

/// A column of page sections, `gap::SECTION` apart.
pub fn sections(children: impl IntoView) -> AnyView {
    widget(Stack::column(0.0).gap_step(gap::step::SECTION))
        .children(children)
        .into_any()
}
