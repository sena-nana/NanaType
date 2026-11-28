use nana_ui::runtime::{TextInput, TextSubmitted};

use crate::msg::ObsMsg;

use super::super::*;
use super::spout::spout_plugin;

/// A step number's circle.
const STEP_DOT: f32 = 22.0;

/// A numbered step: a numbered accent circle, then what to do.
fn step(n: u8, body: impl IntoView) -> AnyView {
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::CONTROL)
            .align(AlignSpec::Center),
    )
    .children((
        widget(
            Stack::row(0.0)
                .width(LengthSpec::Px(STEP_DOT))
                .height(LengthSpec::Px(STEP_DOT))
                .align(AlignSpec::Center)
                .justify(JustifySpec::Center)
                .radius(RadiusTier::Full)
                .surface_mix(SemanticColorMix::alpha(SemanticColorRole::Accent, 0.12))
                .with_layout(|l| l.flex_shrink = Some(0.0)),
        )
        .children(widget(
            Text::new(n.to_string())
                .style(Type::Overline.style())
                .color(SemanticColorRole::AccentOnSoft),
        )),
        widget(
            Stack::row(0.0)
                .gap_step(gap::step::CONTROL)
                .align(AlignSpec::Center)
                .wrap(true)
                .grow(1.0)
                .shrink(1.0),
        )
        .children(body),
    ))
    .into_any()
}

/// [`obs_steps`] as the Output page's row. (The sample line is the page
/// header's.)
pub(super) fn obs_guide(ui: Ui) -> Row {
    Row::inset(
        widget(Stack::column(0.0).padding_step(inset::step::CARD, inset::step::ROW_Y))
            .children(obs_steps(ui, false)),
    )
}

/// Getting the display into OBS: enable output before selecting its live sender
/// name, other capture methods folded under one line. In the first-run
/// `guide` the steps sit closer, and a last one plays a sample line to see
/// it arrive.
pub(in crate::ui::settings::pages) fn obs_steps(ui: Ui, guide: bool) -> AnyView {
    let sample = guide;
    let body = |key: &str| {
        widget(
            Text::new(ui.now_t(key))
                .style(Type::Body.style())
                .color(SemanticColorRole::Text),
        )
        .into_any()
    };
    let sender = widget(crate::ui::kit::tag_action_frame()).children((
        widget(Text::new(String::new()).style(Type::Small.mono())).bind(move |t: &mut Text| {
            let name = ui.settings.with(|s| s.output.display.name.clone());
            if t.value != name {
                t.value = name;
            }
        }),
        widget(
            IconButton::new(ti::COPY, ui.now_t("output-spout-copy"))
                .size(ControlSize::Small)
                .with_tooltip(ui.now_t("output-spout-copy")),
        )
        .on_cx(move |_, _: &Activate, cx| {
            let name = ui
                .settings
                .with_untracked(|s| s.output.display.name.clone());
            cx.dispatch_program_all(Msg::CopyText(name))
        }),
    ));
    let enable = || {
        step(
            1,
            (
                body("output-obs-step-enable"),
                widget(
                    Button::new(ui.now_t("output-obs-enable"))
                        .size(ControlSize::Small)
                        .kind(ButtonKind::Primary),
                )
                .bind(move |b: &mut Button| {
                    let enabled = ui.settings.with(|s| s.output.display.enabled);
                    b.label = ui.now_t(if enabled {
                        "output-obs-enabled"
                    } else {
                        "output-obs-enable"
                    });
                    b.disabled = enabled;
                })
                .on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program_all(edit(|s| s.output.display.enabled = true));
                }),
                output_badge(ui, |s| s.get(WindowKind::Display)),
            ),
        )
    };
    let check = move || {
        sample.then(|| {
            step(
                5,
                (
                    body("output-obs-step-check"),
                    widget(
                        Button::new(ui.now_t("output-sample"))
                            .icon(ti::PLAYER_PLAY)
                            .size(ControlSize::Small),
                    )
                    .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(Msg::PlaySample)),
                ),
            )
        })
    };
    let steps: Vec<AnyView> = if cfg!(windows) {
        [
            Some(enable()),
            Some(step(2, (body("output-obs-step-plugin"), spout_plugin(ui)))),
            Some(step(3, (body("output-obs-step-source"), sender))),
            Some(step(4, body("output-obs-step-alpha"))),
            check(),
        ]
        .into_iter()
        .flatten()
        .collect()
    } else if cfg!(target_os = "macos") {
        [
            Some(enable()),
            Some(step(2, body("output-obs-step-syphon"))),
            Some(step(3, (body("output-obs-step-syphon-source"), sender))),
            Some(step(4, body("output-obs-step-alpha"))),
            check(),
        ]
        .into_iter()
        .flatten()
        .collect()
    } else {
        let _ = sender;
        vec![hint(ui.now_t("output-spout-unsupported"))]
    };
    let other_open = signal(!(cfg!(windows) || cfg!(target_os = "macos")));
    let other = widget(
        Stack::column(0.0)
            .gap_step(gap::step::CONTROL)
            .align(AlignSpec::Start),
    )
    .children((
        widget(
            Button::new(ui.now_t("output-obs-other"))
                .kind(ButtonKind::Text)
                .size(ControlSize::Small)
                .icon(ti::CHEVRON_RIGHT),
        )
        .bind(move |b: &mut Button| {
            let icon = if other_open.get() {
                ti::CHEVRON_DOWN
            } else {
                ti::CHEVRON_RIGHT
            };
            if b.icon != Some(icon) {
                b.icon = Some(icon);
            }
        })
        .on(move |_: &Activate| other_open.set(!other_open.get_untracked())),
        widget(Stack::column(0.0).with_layout(|l| {
            l.padding_left = Some(LengthSpec::Px(space::MD));
        }))
        .visible(move || other_open.get())
        .children(hint(ui.now_t(if cfg!(target_os = "macos") {
            "output-obs-mac"
        } else {
            "output-obs-window"
        }))),
    ));
    widget(Stack::column(0.0).gap_step(if guide {
        nana_ui_core::SpacingStep::Sm
    } else {
        nana_ui_core::SpacingStep::Lg
    }))
    .children((steps, other))
    .into_any()
}

/// The obs-websocket password: kept in the system's secret store, never in
/// the settings file; once saved it shows as saved, with a way to forget it.
pub(in crate::ui::settings::pages) fn obs_password_field(ui: Ui) -> AnyView {
    let draft = signal(String::new());
    let save = move || {
        let password = draft.get_untracked();
        draft.set(String::new());
        Msg::Obs(ObsMsg::Password(password))
    };
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::TIGHT)
            .align(AlignSpec::Center)
            .wrap(true),
    )
    .children((
        widget(Stack::row(0.0).width(LengthSpec::Px(field::SM))).children(
            widget(
                crate::ui::kit::text_input("")
                    .secure(true)
                    .placeholder(ui.now_t("obs-password-placeholder"))
                    .size(ControlSize::Small),
            )
            .bind(move |input: &mut TextInput| {
                let text = draft.get();
                if input.state.value.as_ref() != text.as_str() {
                    input.state.replace_value(text);
                }
            })
            .on(move |e: &TextChanged| draft.set(e.value.chars().take(256).collect()))
            .on_cx(move |_, _: &TextSubmitted, cx| cx.dispatch_program_all(save())),
        ),
        primary(ui, "obs-password-save", save),
        widget(
            Stack::row(0.0)
                .gap_step(gap::step::TIGHT)
                .align(AlignSpec::Center),
        )
        .visible(move || ui.obs.get().password_saved)
        .children((
            widget(
                Text::new(ui.now_t("obs-password-saved"))
                    .style(Type::Small.style())
                    .color(SemanticColorRole::Muted),
            ),
            icon_action(ui, "obs-password-clear", ti::ERASER, || {
                Msg::Obs(ObsMsg::Password(String::new()))
            }),
        )),
    ))
    .into_any()
}

/// Where the obs-websocket server is, and its password.
fn obs_link_fields(ui: Ui) -> Vec<Row> {
    vec![
        row(ui, "obs-address", None, obs_address(ui)),
        row(
            ui,
            "obs-password",
            Some("obs-password-hint"),
            obs_password_field(ui),
        ),
    ]
}

/// The obs-websocket address, as `host : port`.
pub(in crate::ui::settings::pages) fn obs_address(ui: Ui) -> AnyView {
    address(
        ui,
        (|s| s.output.obs.host.clone(), |s, v| s.output.obs.host = v),
        (|s| s.output.obs.port, |s, v| s.output.obs.port = v),
    )
}

/// The obs-websocket link: on, where, and the password.
pub(super) fn obs_link(ui: Ui) -> AnyView {
    toggled_section(
        ui,
        "output-obs-link",
        row_status(
            ui,
            "obs-link",
            Some("obs-link-hint"),
            link_badge(ui, |s| &s.obs),
            switch(
                ui,
                |s| s.output.obs.enabled,
                |s, v| s.output.obs.enabled = v,
            ),
        ),
        |s| s.output.obs.enabled,
        obs_link_fields(ui),
    )
}
