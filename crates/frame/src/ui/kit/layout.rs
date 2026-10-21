//! Layout parts: cards, sections, rows, dividers and bars. A settings page,
//! the package list and the chat window's cards are all built from these, so
//! their insets, hairlines and radii are one decision.

use std::sync::Arc;

use nana_ui::runtime::view::{AnyView, IntoView, settings_row, widget};
use nana_ui::runtime::{Divider, Stack};
use nana_ui_core::{AlignSpec, JustifySpec, LengthSpec, RadiusTier, SemanticColorRole};

use super::tokens::{gap, inset};
use nana_ui::runtime::view::RaisedSurfacePainter;

/// Whether a section row is shown; `None` always shows it.
pub type Shown = Option<Box<dyn Fn() -> bool + Send + 'static>>;

/// A row that shows only while `cond` holds, for [`section_with`].
pub fn shown_if(view: AnyView, cond: impl Fn() -> bool + Send + 'static) -> (AnyView, Shown) {
    (view, Some(Box::new(cond)))
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
pub fn section_titled(title: Option<String>, rows: Vec<(AnyView, Shown)>) -> AnyView {
    use nana_ui::runtime::view::group_row;
    let rows = rows
        .into_iter()
        .map(|(view, shown)| match shown {
            Some(cond) => group_row(view).visible(cond),
            None => group_row(view),
        })
        .collect();
    nana_ui::runtime::view::settings_group(title, rows)
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
pub fn divided(rows: Vec<AnyView>) -> Vec<AnyView> {
    rows.into_iter()
        .enumerate()
        .map(|(i, r)| {
            widget(Stack::column(0.0))
                .children(((i > 0).then(divider), r))
                .into_any()
        })
        .collect()
}

/// A full-width block inside a card, inset like the rows.
pub fn block(view: impl IntoView) -> AnyView {
    widget(
        Stack::column(0.0)
            .gap_step(gap::step::CONTROL)
            .padding_step(inset::step::CARD, inset::step::ROW_Y),
    )
    .children(view)
    .into_any()
}

/// A row inside a card: `left` takes the room, `right` keeps its width.
/// For rows that are not a label and a control (a list entry, a header).
pub fn line(left: impl IntoView, right: impl IntoView) -> AnyView {
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
    ))
    .into_any()
}

/// Label (and hint) on the left, control on the right; stacks when narrow.
pub fn row_text(label_text: String, hint_text: Option<String>, control: AnyView) -> AnyView {
    let hint: Option<Arc<str>> = hint_text.map(Into::into);
    inset_row(
        settings_row(label_text)
            .hint(hint)
            .control(control_slot(control))
            .into_any(),
    )
}

/// [`row_text`] whose hint is re-read as the signals it reads change.
pub fn row_hint_fn(
    label_text: String,
    hint: impl Fn() -> Option<String> + Send + 'static,
    control: AnyView,
) -> AnyView {
    inset_row(
        settings_row(label_text)
            .hint(move || hint().map(Arc::<str>::from))
            .control(control_slot(control))
            .into_any(),
    )
}

fn control_slot(control: AnyView) -> AnyView {
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::CONTROL)
            .align(AlignSpec::Center),
    )
    .children(control)
    .into_any()
}

fn inset_row(row: AnyView) -> AnyView {
    widget(Stack::column(0.0).padding_xy(inset::CARD, 0.0))
        .children(row)
        .into_any()
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
