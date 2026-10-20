//! Small parts every window shares: tags, pills, status dots, callouts,
//! empty states, the call-to-action button and the transparency checker.

use nana_ui::icons_tabler as ti;
use nana_ui::runtime::view::{AnyView, IntoView, widget};
use nana_ui::runtime::{Activate, Button, Icon, IconGlyph, Stack, Text};
use nana_ui::{ButtonKind, ControlSize};
use nana_ui_core::{
    AlignSpec, JustifySpec, LengthSpec, RadiusTier, SemanticColorMix, SemanticColorRole,
};

use super::paint::CtaPainter;
use super::tokens::{DOT, PILL_H, Tone, Type, gap, icon, inset};
use crate::msg::Msg;

/// A quiet tag: a subtle chip with a soft hairline at the control radius,
/// its text small. For a marker, a capability, a sender name.
pub fn tag(text: impl Into<String>) -> AnyView {
    tag_styled(Type::Small.text(text).color(SemanticColorRole::Muted))
}

/// [`tag`] in the mono face, for a literal value (an address, a code).
pub fn mono_tag(text: impl Into<String>) -> AnyView {
    tag_styled(
        Text::new(text.into())
            .style(Type::Small.mono())
            .color(SemanticColorRole::Muted),
    )
}

fn tag_styled(text: Text) -> AnyView {
    widget(tag_frame()).children(widget(text)).into_any()
}

/// A tag's frame, for a tag whose content is bound in place.
pub fn tag_frame() -> Stack {
    Stack::row(gap::TIGHT)
        .padding_xy(inset::CHIP_X, inset::CHIP_Y)
        .align(AlignSpec::Center)
        .surface(SemanticColorRole::Subtle)
        .outline(SemanticColorRole::BorderSoft, 1.0)
        .radius(RadiusTier::Sm)
        .with_layout(|l| l.flex_shrink = Some(0.0))
}

/// `stack` filled with `role` at `alpha` over what is behind it: a tint
/// that follows the theme.
pub fn wash(stack: Stack, role: SemanticColorRole, alpha: f32) -> Stack {
    let st = stack
        .node_style()
        .surface_mix(SemanticColorMix::alpha(role, alpha));
    stack.style(st)
}

/// A round frame (a pill, a dot, an avatar) `height` px tall.
pub fn round(stack: Stack, height: f32) -> Stack {
    stack.radius_px(height / 2.0)
}

/// A tinted pill in `tone`'s colour with an optional leading dot: the
/// app's status mark (connected, sealed, queued).
pub fn pill(text: impl Into<String>, tone: Tone, dot: bool) -> AnyView {
    widget(pill_frame(tone))
        .children((
            dot.then(|| status_dot(tone)),
            widget(Type::Micro.text(text).color(tone.ink())),
        ))
        .into_any()
}

/// A pill's frame in `tone`.
fn pill_frame(tone: Tone) -> Stack {
    let frame = round(
        Stack::row(gap::TIGHT)
            .height(LengthSpec::Px(PILL_H))
            .padding_xy(inset::CHIP_X, 0.0)
            .align(AlignSpec::Center)
            .with_layout(|l| l.flex_shrink = Some(0.0)),
        PILL_H,
    );
    wash(frame, tone.role(), 0.12)
}

/// A status dot in `tone`'s colour.
pub fn status_dot(tone: Tone) -> AnyView {
    widget(dot_frame().surface(tone.role())).into_any()
}

/// A dot's frame, for a dot whose colour is bound in place.
pub fn dot_frame() -> Stack {
    round(
        Stack::row(0.0)
            .width(LengthSpec::Px(DOT))
            .height(LengthSpec::Px(DOT))
            .with_layout(|l| l.flex_shrink = Some(0.0)),
        DOT,
    )
}

/// A one-line note in a tinted box: what went wrong, what to know.
pub fn callout(tone: Tone, text: impl Into<String>) -> AnyView {
    let glyph = match tone {
        Tone::Danger => ti::ALERT_CIRCLE,
        Tone::Warning => ti::ALERT_TRIANGLE,
        Tone::Success => ti::CIRCLE_CHECK,
        _ => ti::INFO_CIRCLE,
    };
    widget(wash(
        Stack::row(gap::CONTROL)
            .padding_xy(inset::PANEL, inset::BAR_Y)
            .align(AlignSpec::Start)
            .radius(RadiusTier::Sm),
        tone.role(),
        0.10,
    ))
    .children((
        widget(
            IconGlyph::new(glyph)
                .size(icon::MD)
                .role(tone.role())
                .decorative(),
        ),
        widget(Type::Body.text(text).width(LengthSpec::Fill)),
    ))
    .into_any()
}

/// An icon on a tinted square: a list entry's lead mark.
pub fn icon_tile(glyph: Icon, size: f32, tone: Tone) -> AnyView {
    widget(wash(
        Stack::row(0.0)
            .width(LengthSpec::Px(size))
            .height(LengthSpec::Px(size))
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center)
            .radius(RadiusTier::Sm)
            .with_layout(|l| l.flex_shrink = Some(0.0)),
        tone.role(),
        0.12,
    ))
    .children(widget(
        IconGlyph::new(glyph)
            .size(size / 2.0)
            .role(tone.role())
            .decorative(),
    ))
    .into_any()
}

/// An empty list: a ✦ in the accent colour, one muted line and, when
/// there is somewhere to go, a quiet text button.
pub fn empty_state(
    text: String,
    action: Option<(String, Box<dyn Fn() -> Msg + Send + 'static>)>,
) -> AnyView {
    let action = action.map(|(label, msg)| {
        widget(
            Button::new(label)
                .kind(ButtonKind::Text)
                .size(ControlSize::Small),
        )
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(msg()))
        .into_any()
    });
    widget(
        Stack::row(gap::CONTROL)
            .padding_xy(inset::CARD, inset::ROW_Y)
            .align(AlignSpec::Center)
            .wrap(true),
    )
    .children((
        widget(
            IconGlyph::new(ti::SPARKLES)
                .size(icon::MD)
                .role(SemanticColorRole::Accent),
        ),
        widget(Type::Lead.text(text)),
        action,
    ))
    .into_any()
}

/// `button` as the view's call to action: [`CtaPainter`] over a solid accent
/// fill, its label in the on-accent colour. One per view.
pub fn cta(button: Button) -> Button {
    let mut st = button.style.clone();
    // The painter draws the fill, so the node has none of its own (a
    // disabled button shows the plain primary tint instead).
    st.foreground = Some(SemanticColorRole::AccentText);
    st.background = None;
    st.border = None;
    st.interaction.disabled.background = Some(SemanticColorRole::AccentSoft);
    st.painter = Some(CtaPainter.into());
    button.style(st)
}

/// A small button that sends `msg`.
pub fn button(label: String, msg: impl Fn() -> Msg + Send + 'static) -> AnyView {
    widget(Button::new(label).size(ControlSize::Small))
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(msg()))
        .into_any()
}

/// A small call-to-action button that sends `msg`.
pub fn cta_button(label: String, msg: impl Fn() -> Msg + Send + 'static) -> AnyView {
    widget(cta(Button::new(label).size(ControlSize::Small)))
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(msg()))
        .into_any()
}

/// A row of items `gap::CONTROL` apart, centred.
pub fn hstack(children: impl IntoView) -> AnyView {
    widget(Stack::row(gap::CONTROL).align(AlignSpec::Center))
        .children(children)
        .into_any()
}

/// A 10px transparency checkerboard tile as a data URL, in the chrome's
/// subtle and hover tones.
pub fn checker_url(dark: bool) -> String {
    fn tile(dark: bool) -> String {
        let p = super::theme::palette(dark);
        let q = |c: nana_ui::theme::SemanticColor| {
            let c = crate::skin_theme::rgba8(c);
            [c.r, c.g, c.b]
        };
        let (a, b) = (q(p.hover), q(p.subtle));
        let img = image::RgbImage::from_fn(20, 20, |x, y| {
            image::Rgb(if (x / 10 + y / 10) % 2 == 0 { a } else { b })
        });
        let mut png = Vec::new();
        let _ = img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png);
        format!(
            "data:image/png;base64,{}",
            crate::ui::settings::base64(&png)
        )
    }
    static LIGHT: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| tile(false));
    static DARK: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| tile(true));
    if dark { DARK.clone() } else { LIGHT.clone() }
}
