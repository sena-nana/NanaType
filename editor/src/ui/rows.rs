//! The dense rows every workspace's forms are built from: a fixed label
//! column beside its control, in groups under a small caption. One grammar
//! for the inspector, the sound table, the debugger and the package forms.
use super::kit::{self, Tone, Type, gap};
use nana_ui::runtime::view::{AnyView, IntoView, widget};
use nana_ui::runtime::{Stack, Text};
use nana_ui_core::{AlignSpec, LengthSpec, SemanticColorRole};

/// The label column of a form in the centre or the inspector.
pub(crate) const LABEL_W: f32 = 132.0;
/// A form in the centre, at most this wide: past it a field's label and its
/// value drift apart.
pub(crate) const FORM_W: f32 = 760.0;

/// `label` in a column `width` wide, `control` taking the rest.
pub(crate) fn row_w(width: f32, label: String, control: impl IntoView) -> AnyView {
    widget(
        Stack::row(0.)
            .gap_step(gap::step::TIGHT)
            .align(AlignSpec::Center)
            .width(LengthSpec::Fill),
    )
    .children((
        widget(
            Stack::row(0.)
                .width(LengthSpec::Px(width))
                .shrink(0.)
                .align(AlignSpec::Center),
        )
        .children(widget(
            Type::Caption
                .text(label)
                .color(SemanticColorRole::Muted)
                .truncating(),
        )),
        widget(
            Stack::row(0.)
                .grow(1.)
                .shrink(1.)
                .min_width(LengthSpec::Px(0.))
                .gap_step(gap::step::TIGHT)
                .align(AlignSpec::Center),
        )
        .children(control),
    ))
    .into_any()
}

pub(crate) fn row(label: String, control: impl IntoView) -> AnyView {
    row_w(LABEL_W, label, control)
}

/// A read-only value beside its label, wrapping where it must.
pub(crate) fn fact(label: String, value: impl Into<String>) -> AnyView {
    row(label, widget(super::views::readable(value)))
}

/// The same, in the code face: a path, a hash, a fingerprint.
pub(crate) fn fact_mono(label: String, value: impl Into<String>) -> AnyView {
    let mut style = Type::Small.mono();
    kit::wrap_anywhere(&mut style);
    row(label, widget(Text::new(value.into()).style(style)))
}

/// Rows under a small caption, with the group's tools at the caption's end.
pub(crate) fn group(title: String, tools: impl IntoView, rows: impl IntoView) -> AnyView {
    widget(
        Stack::column(0.)
            .gap_step(gap::step::TIGHT)
            .width(LengthSpec::Fill),
    )
    .children((
        widget(
            Stack::row(0.)
                .gap_step(gap::step::TIGHT)
                .align(AlignSpec::Center)
                .width(LengthSpec::Fill),
        )
        .children((
            widget(
                Stack::row(0.)
                    .grow(1.)
                    .shrink(1.)
                    .min_width(LengthSpec::Px(0.)),
            )
            .children(widget(Type::Overline.text(title))),
            tools,
        )),
        rows,
    ))
    .into_any()
}

/// A form's groups in the centre: one column from the pane's edge, a rule's
/// width apart, capped at [`FORM_W`].
pub(crate) fn form(key: &'static str, groups: impl IntoView) -> AnyView {
    widget(
        Stack::column(0.)
            .gap_step(gap::step::GROUP)
            .width(LengthSpec::Fill)
            .max_width(FORM_W)
            .padding_xy(gap::GROUP, gap::CONTROL),
    )
    .key(key)
    .children(groups)
    .into_any()
}

/// A line of a list: a status mark, what it is, and its tools at the end.
pub(crate) fn item(tone: Option<Tone>, body: impl IntoView, tools: impl IntoView) -> AnyView {
    widget(
        Stack::row(0.)
            .gap_step(gap::step::TIGHT)
            .align(AlignSpec::Center)
            .width(LengthSpec::Fill),
    )
    .children((
        tone.map(kit::status_dot),
        widget(
            Stack::column(0.)
                .grow(1.)
                .shrink(1.)
                .min_width(LengthSpec::Px(0.)),
        )
        .children(body),
        tools,
    ))
    .into_any()
}
