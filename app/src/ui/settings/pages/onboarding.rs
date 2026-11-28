use super::hotkeys::{hotkey_control, hotkey_issue_caption};
use super::output::{
    display_source, obs_address, obs_password_field, sample_step, shared_steps, step as numbered,
    step_text,
};
use super::*;
use crate::msg::WindowMsg;
use crate::ui::settings::preview::{guide_save, guide_tools};
use nanatype_settings::StreamRoute;

/// The diagram's text caret and the dialogue box's live edge (width, height).
const CARET: (f32, f32) = (2.0, 12.0);

/// The step that picks a skin and adjusts it.
const SKIN_STEP: u8 = 1;

/// The step that records the common hotkeys; the one after it is the last.
const HOTKEY_STEP: u8 = 3;

/// Whether the display can go to OBS as a shared texture here (Spout2 on
/// Windows, Syphon on macOS); elsewhere only window capture is offered.
const SHARED: bool = cfg!(windows) || cfg!(target_os = "macos");

/// The side of an app card's glyph tile.
const APP_TILE: f32 = 34.0;

/// How many steps the first-run guide has.
pub(super) const ONBOARD_STEPS: u8 = nanatype_settings::ONBOARDING_STEPS;

/// Show step `n` and remember it, so a guide closed halfway reopens there.
fn go_to(ui: Ui, cx: &mut nana_ui::runtime::ViewContext<'_, Button>, n: u8) {
    ui.onboard_step.set(n);
    cx.dispatch_program_all(edit(move |s| s.general.onboarding_step = n));
}

/// The first-run guide: one card marked by a 3px accent bar, five steps
/// (language and name, a skin with its name plate and part scales and its
/// live preview, the input mode, the common hotkeys, and the streaming app
/// the display goes into) with Back / Next and a step indicator. Skip or
/// the last step's "Get started" closes it for good; About opens it again.
pub(super) fn onboarding(ui: Ui) -> AnyView {
    let step = ui.onboard_step;
    let finish = move || {
        ui.shader_preview.set(false);
        step.set(0);
        edit(|s| {
            s.general.onboarding_done = true;
            s.general.onboarding_step = 0;
        })
    };
    let header = widget(
        Stack::row(0.0)
            .gap_step(nana_ui_core::SpacingStep::Md)
            .align(AlignSpec::Center)
            .width(LengthSpec::Fill)
            .padding_xy(inset::CARD, 0.0),
    )
    .children((
        widget(
            IconGlyph::new(ti::SPARKLES)
                .size(icon::MD)
                .role(SemanticColorRole::Accent),
        ),
        widget(Text::new(ui.now_t("onboard-welcome")).style(Type::Title.style())),
        widget(Stack::row(0.0).grow(1.0)),
        step_indicator(ui),
        widget(
            Button::new(ui.now_t("onboard-skip"))
                .kind(ButtonKind::Text)
                .size(ControlSize::Small),
        )
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(finish())),
    ));
    // Each step rises in on the `enter` token.
    let body = dynamic(move || step.get(), move |n| onboard_step(ui, *n))
        .transition(crate::ui::motion::enter());
    let footer = widget(
        Stack::row(0.0)
            .gap_step(nana_ui_core::SpacingStep::Md)
            .align(AlignSpec::Center)
            .width(LengthSpec::Fill)
            .padding_xy(inset::CARD, 0.0),
    )
    .children((
        widget(
            Button::new(ui.now_t("onboard-back"))
                .icon(ti::CHEVRON_LEFT)
                .size(ControlSize::Small),
        )
        .visible(move || step.get() > 0)
        .on_cx(move |_, _: &Activate, cx| {
            let n = step.get_untracked();
            if n == SKIN_STEP {
                ui.shader_preview.set(false);
            }
            go_to(ui, cx, n.saturating_sub(1));
        }),
        widget(Stack::row(0.0).grow(1.0)),
        // Next, and on the last step "Get started".
        widget(
            Button::new(ui.now_t("onboard-next"))
                .size(ControlSize::Small)
                .kind(ButtonKind::Primary),
        )
        .bind(move |b: &mut Button| {
            let n = step.get();
            let last = n + 1 >= ONBOARD_STEPS;
            let label = ui.now_t(if last { "onboard-done" } else { "onboard-next" });
            if b.label != label {
                b.label = label;
            }
            // The skin step saves its fields on Next; it waits while they
            // do not make a valid profile (the step says why).
            let blocked = n == SKIN_STEP && guide_save(ui).is_err();
            if b.disabled != blocked {
                b.disabled = blocked;
            }
        })
        .on_cx(move |_, _: &Activate, cx| {
            crate::ui::motion::press(cx, ui.reduced_motion.get_untracked());
            let n = step.get_untracked();
            if n + 1 >= ONBOARD_STEPS {
                cx.dispatch_program_all(finish());
            } else {
                if n == SKIN_STEP {
                    let Ok(save) = guide_save(ui) else { return };
                    if let Some(msg) = save {
                        cx.dispatch_program_all(msg);
                    }
                    ui.shader_preview.set(false);
                }
                go_to(ui, cx, n + 1);
            }
        }),
    ));
    // The same card as every section, its welcome told by the sparkle and
    // the accent step dots rather than a frame of its own.
    widget(crate::ui::kit::card_frame(nana_ui_core::RadiusTier::Lg))
        .children(
            widget(
                Stack::column(0.0)
                    .gap_step(gap::step::CONTROL)
                    .padding_xy(0.0, inset::ROW_Y),
            )
            .children((header, body, divider(), footer)),
        )
        .into_any()
}

/// A dot per step: the current one a wide accent pill, the ones done accent
/// dots, the ones ahead hairline dots.
pub(super) fn step_indicator(ui: Ui) -> AnyView {
    let step = ui.onboard_step;
    let dots: Vec<AnyView> = (0..ONBOARD_STEPS)
        .map(|i| {
            widget(
                Stack::row(0.0)
                    .height(LengthSpec::Px(space::SM))
                    .radius(RadiusTier::Full),
            )
            .bind(move |s: &mut Stack| {
                let n = step.get();
                let role = if i <= n {
                    SemanticColorRole::Accent
                } else {
                    SemanticColorRole::BorderStrong
                };
                let taken = std::mem::replace(s, Stack::row(0.0));
                *s = taken
                    .width(LengthSpec::Px(if i == n { space::XXXL } else { space::SM }))
                    .surface(role);
            })
            .into_any()
        })
        .collect();
    widget(
        Stack::row(0.0)
            .gap_step(nana_ui_core::SpacingStep::Xs)
            .align(AlignSpec::Center),
    )
    .children(dots)
    .into_any()
}

/// One step's body, under the guide's header. Its rows carry no dividers of
/// their own: the footer's line is the step's only one, so none stacks on
/// it.
pub(super) fn onboard_step(ui: Ui, n: u8) -> AnyView {
    let body: Vec<Row> = match n {
        0 => vec![
            row(ui, "onboard-language", None, language_select(ui)),
            // The guide says it under the label: a first run reads it.
            row_text(
                ui.now_t("onboard-speaker"),
                Some(ui.now_t("general-speaker-hint")),
                speaker_field(ui),
            ),
        ],
        SKIN_STEP => [
            // Undo sits just before the picker, the choice it undoes.
            row(
                ui,
                "onboard-skin",
                None,
                widget(
                    Stack::row(0.0)
                        .gap_step(nana_ui_core::SpacingStep::Sm)
                        .align(AlignSpec::Center),
                )
                .children((guide_tools(ui), skin_select(ui)))
                .into_any(),
            ),
        ]
        .into_iter()
        // The name plate and part scales sit in the preview's display group.
        .chain([Row::inset(block(crate::ui::settings::preview::guide_view(
            ui,
        )))])
        .collect(),
        2 => vec![Row::inset(
            widget(
                Stack::row(0.0)
                    .gap_step(nana_ui_core::SpacingStep::Xl)
                    .align(AlignSpec::Stretch)
                    .width(LengthSpec::Fill)
                    .padding_xy(inset::CARD, 0.0),
            )
            .children((
                mode_card(ui, InputMode::Compose),
                mode_card(ui, InputMode::Mirror),
            )),
        )],
        HOTKEY_STEP => hotkeys_step(ui),
        _ => stream_step(ui),
    };
    widget(Stack::column(0.0)).children(body).into_any()
}

/// The hotkeys the last step offers to set: the two ways to start typing.
/// Continue, skip and the rest are on the Hotkeys page.
const COMMON_KEYS: [GlobalAction; 2] = [GlobalAction::Summon, GlobalAction::QuickInput];

/// The narrowest a hotkey cell gets before the grid drops to one column.
const KEY_CELL_MIN: f32 = 240.0;

/// A step's content on the card's content line, its parts `Md` apart.
fn guide_column(children: impl IntoView) -> Row {
    Row::inset(
        widget(
            Stack::column(0.0)
                .gap_step(nana_ui_core::SpacingStep::Md)
                .padding_xy(inset::CARD, 0.0),
        )
        .children(children),
    )
}

/// The common hotkeys, recorded in place, in a framed group that links to
/// the Hotkeys page for the rest.
fn hotkeys_step(ui: Ui) -> Vec<Row> {
    let more = widget(
        Button::new(ui.now_t("onboard-hotkeys-more"))
            .kind(ButtonKind::Text)
            .size(ControlSize::Small)
            .icon(ti::KEYBOARD),
    )
    .on_cx(|_, _: &Activate, cx| {
        cx.dispatch_program_all(Msg::Window(WindowMsg::SettingsPage(Page::Hotkeys)))
    })
    .into_any();
    vec![guide_column(group(
        ui,
        ti::KEYBOARD,
        "onboard-hotkeys",
        (None, Some(more)),
        None,
        hotkey_grid(ui),
    ))]
}

/// The last step: which app the stream runs in, as two cards; for OBS, how
/// it takes the picture (Spout2, or Syphon on macOS, recommended; window
/// capture still offered); then the steps for that route. Nothing is
/// chosen at first, and "Get started" never waits on it.
fn stream_step(ui: Ui) -> Vec<Row> {
    let route = move || ui.settings.with(|s| s.general.onboarding_route);
    let question = widget(Stack::column(0.0).gap_step(nana_ui_core::SpacingStep::Xxs)).children((
        subheading(ui, "onboard-stream"),
        hint(ui.now_t("onboard-stream-hint")),
    ));
    let apps = widget(
        Stack::row(0.0)
            .gap_step(nana_ui_core::SpacingStep::Xl)
            .align(AlignSpec::Stretch)
            .width(LengthSpec::Fill),
    )
    .children((app_card(ui, false), app_card(ui, true)));
    let methods = widget(
        Stack::column(0.0)
            .gap_step(nana_ui_core::SpacingStep::Sm)
            .width(LengthSpec::Fill),
    )
    .visible(move || {
        SHARED
            && matches!(
                route(),
                Some(StreamRoute::ObsShared | StreamRoute::ObsWindow)
            )
    })
    .children((
        subheading(ui, "onboard-method"),
        widget(
            Stack::row(0.0)
                .gap_step(nana_ui_core::SpacingStep::Md)
                .align(AlignSpec::Stretch)
                .width(LengthSpec::Fill),
        )
        .children((method_card(ui, true), method_card(ui, false))),
    ));
    let guide = dynamic(route, move |r| route_guide(ui, *r));
    vec![guide_column((question, apps, methods, guide))]
}

/// Answer the last step with `route`. The display's Spout2 (Syphon) sender
/// starts off and is apart from its window, so choosing it turns the sender
/// on instead of asking; the window is left as it is.
fn choose_route(s: &mut Settings, route: StreamRoute) {
    s.general.onboarding_route = Some(route);
    if route == StreamRoute::ObsShared {
        s.output.display.enabled = true;
    }
}

/// OBS or another app, as a card to pick. OBS starts on the shared texture
/// where there is one, and keeps window capture if that was chosen before.
fn app_card(ui: Ui, other: bool) -> AnyView {
    let (glyph, name, about) = if other {
        (
            ti::DEVICE_DESKTOP,
            "onboard-stream-other",
            "onboard-stream-other-hint",
        )
    } else {
        (
            ti::BROADCAST,
            "onboard-stream-obs",
            if cfg!(target_os = "macos") {
                "onboard-stream-obs-hint-mac"
            } else if SHARED {
                "onboard-stream-obs-hint"
            } else {
                "onboard-stream-obs-hint-window"
            },
        )
    };
    let chosen = move || {
        ui.settings.with(|s| match s.general.onboarding_route {
            Some(StreamRoute::OtherWindow) => other,
            Some(_) => !other,
            None => false,
        })
    };
    let pick = move || {
        edit(move |s| {
            let route = match s.general.onboarding_route {
                _ if other => StreamRoute::OtherWindow,
                Some(StreamRoute::ObsWindow) => StreamRoute::ObsWindow,
                _ if SHARED => StreamRoute::ObsShared,
                _ => StreamRoute::ObsWindow,
            };
            choose_route(s, route);
        })
    };
    let tile = widget({
        let tile = Stack::row(0.0)
            .width(LengthSpec::Px(APP_TILE))
            .height(LengthSpec::Px(APP_TILE))
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center)
            .radius(RadiusTier::Sm)
            .with_layout(|l| l.flex_shrink = Some(0.0));
        if other {
            tile.surface(SemanticColorRole::Subtle)
        } else {
            tile.surface_mix(SemanticColorMix::alpha(SemanticColorRole::Accent, 0.12))
        }
    })
    .children(widget(IconGlyph::new(glyph).size(icon::MD).role(
        if other {
            SemanticColorRole::Muted
        } else {
            SemanticColorRole::Accent
        },
    )));
    pick_card(
        ui.now_t(name),
        chosen,
        pick,
        widget(
            Stack::column(0.0)
                .gap_step(nana_ui_core::SpacingStep::Sm)
                .width(LengthSpec::Fill),
        )
        .children((
            widget(
                Stack::row(0.0)
                    .gap_step(nana_ui_core::SpacingStep::Md)
                    .align(AlignSpec::Center),
            )
            .children((
                tile,
                widget(Text::new(ui.now_t(name)).style(Type::Strong.style())),
            )),
            hint(ui.now_t(about)),
        ))
        .into_any(),
    )
}

/// How OBS takes the picture, as a card to pick: the shared texture,
/// marked recommended, or window capture.
fn method_card(ui: Ui, shared: bool) -> AnyView {
    let route = if shared {
        StreamRoute::ObsShared
    } else {
        StreamRoute::ObsWindow
    };
    let (name, about) = match (shared, cfg!(target_os = "macos")) {
        (true, true) => ("onboard-method-syphon", "onboard-method-shared-hint"),
        (true, false) => ("onboard-method-spout", "onboard-method-shared-hint"),
        (false, _) => ("onboard-method-window", "onboard-method-window-hint"),
    };
    let chosen = move || {
        ui.settings
            .with(|s| s.general.onboarding_route == Some(route))
    };
    let recommended = shared.then(|| {
        widget(StatusBadge::new(ui.now_t("onboard-recommended"), StatusTone::Info).compact(true))
    });
    pick_card(
        ui.now_t(name),
        chosen,
        move || edit(move |s| choose_route(s, route)),
        widget(
            Stack::column(0.0)
                .gap_step(nana_ui_core::SpacingStep::Xxs)
                .width(LengthSpec::Fill),
        )
        .children((
            widget(
                Stack::row(0.0)
                    .gap_step(nana_ui_core::SpacingStep::Sm)
                    .align(AlignSpec::Center),
            )
            .children((
                widget(Text::new(ui.now_t(name)).style(Type::Strong.style())),
                recommended,
            )),
            hint(ui.now_t(about)),
        ))
        .into_any(),
    )
}

/// The steps for the chosen route; nothing until one is chosen.
fn route_guide(ui: Ui, route: Option<StreamRoute>) -> Option<AnyView> {
    let mac = cfg!(target_os = "macos");
    Some(match route? {
        StreamRoute::ObsShared => {
            let shared = group(
                ui,
                ti::BROADCAST,
                if mac {
                    "onboard-guide-syphon"
                } else {
                    "onboard-guide-spout"
                },
                (None, Some(output_badge(ui, |s| s.get(WindowKind::Display)))),
                None,
                steps_column(shared_steps(ui)),
            );
            widget(Stack::column(0.0).gap_step(nana_ui_core::SpacingStep::Md))
                .children((shared, link_group(ui)))
                .into_any()
        }
        StreamRoute::ObsWindow => group(
            ui,
            ti::APP_WINDOW,
            "onboard-guide-window-obs",
            (None, None),
            None,
            window_steps(ui, false),
        ),
        StreamRoute::OtherWindow => group(
            ui,
            ti::APP_WINDOW,
            "onboard-guide-window-other",
            (None, None),
            None,
            window_steps(ui, true),
        ),
    })
}

/// Numbered steps, a little apart.
fn steps_column(steps: Vec<AnyView>) -> AnyView {
    widget(Stack::column(0.0).gap_step(nana_ui_core::SpacingStep::Sm))
        .children(steps)
        .into_any()
}

/// The optional obs-websocket link that sizes the Spout2 (Syphon) output
/// from its source's box in OBS. Window capture has no output to size, so
/// only the shared route shows it.
fn link_group(ui: Ui) -> AnyView {
    let optional =
        widget(StatusBadge::new(ui.now_t("onboard-optional"), StatusTone::Neutral).compact(true))
            .into_any();
    group(
        ui,
        ti::PLUG_CONNECTED,
        "onboard-obs-link",
        (Some(optional), Some(link_switch(ui))),
        Some("onboard-obs-link-hint"),
        link_fields(ui),
    )
}

/// Window capture, into OBS or another app: open the display window, add a
/// capture of it there, and play a sample line; then what keeps it showing.
/// Into OBS, the note offers the shared texture instead.
fn window_steps(ui: Ui, other: bool) -> AnyView {
    let open = widget(
        Button::new(ui.now_t("onboard-window-open"))
            .size(ControlSize::Small)
            .kind(ButtonKind::Primary),
    )
    .bind(move |b: &mut Button| {
        let shown = ui.settings.with(|s| s.overlays.display.visible);
        let label = ui.now_t(if shown {
            "onboard-window-opened"
        } else {
            "onboard-window-open"
        });
        if b.label != label {
            b.label = label;
        }
        b.disabled = shown;
    })
    .on_cx(|_, _: &Activate, cx| {
        cx.dispatch_program_all(Msg::Window(WindowMsg::Show(WindowKind::Display)))
    });
    let window = widget(crate::ui::kit::tag_frame()).children(
        widget(Text::new(String::new()).style(Type::Small.style())).bind(move |t: &mut Text| {
            let title = WindowKind::Display.title(ui.locale.get());
            if t.value != title {
                t.value = title;
            }
        }),
    );
    let mut steps = vec![numbered(1, (step_text(ui, "onboard-window-show"), open))];
    let note = if other {
        steps.push(numbered(
            2,
            (step_text(ui, "onboard-window-other-source"), window),
        ));
        steps.push(sample_step(ui, 3, "onboard-window-other-check"));
        hint(ui.now_t("onboard-window-other-note"))
    } else {
        let source = if cfg!(target_os = "macos") {
            "onboard-window-mac-source"
        } else {
            "onboard-window-obs-source"
        };
        steps.push(numbered(2, (step_text(ui, source), window)));
        let check = if cfg!(windows) {
            steps.push(numbered(3, step_text(ui, "onboard-window-obs-method")));
            4
        } else {
            3
        };
        steps.push(sample_step(ui, check, "output-obs-step-check"));
        let switch_back = SHARED.then(|| {
            widget(
                Button::new(ui.now_t(if cfg!(target_os = "macos") {
                    "onboard-use-syphon"
                } else {
                    "onboard-use-spout"
                }))
                .kind(ButtonKind::Text)
                .size(ControlSize::Small),
            )
            .on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program_all(edit(|s| choose_route(s, StreamRoute::ObsShared)))
            })
        });
        widget(
            Stack::row(0.0)
                .gap_step(nana_ui_core::SpacingStep::Xs)
                .align(AlignSpec::Center)
                .wrap(true),
        )
        .children((hint(ui.now_t("onboard-window-obs-note")), switch_back))
        .into_any()
    };
    widget(Stack::column(0.0).gap_step(nana_ui_core::SpacingStep::Sm))
        .children((steps, note))
        .into_any()
}

/// One of the last step's groups: a hairline frame around its title line
/// (a glyph, the title, a `tag` after it and `trailing` at the end), what
/// it is for, and its body.
fn group(
    ui: Ui,
    glyph: nana_ui::runtime::Icon,
    title_key: &str,
    (tag, trailing): (Option<AnyView>, Option<AnyView>),
    hint_key: Option<&str>,
    body: AnyView,
) -> AnyView {
    widget(
        Stack::column(0.0)
            .gap_step(nana_ui_core::SpacingStep::Sm)
            .width(LengthSpec::Fill)
            .padding_step(nana_ui_core::SpacingStep::Lg, nana_ui_core::SpacingStep::Md)
            .surface(SemanticColorRole::Background)
            .border(SemanticColorRole::Border, 1.0)
            .radius(RadiusTier::Md),
    )
    .children((
        widget(
            Stack::row(0.0)
                .gap_step(nana_ui_core::SpacingStep::Sm)
                .align(AlignSpec::Center)
                .width(LengthSpec::Fill),
        )
        .children((
            widget(
                IconGlyph::new(glyph)
                    .size(icon::MD)
                    .role(SemanticColorRole::Accent),
            ),
            subheading(ui, title_key),
            tag,
            widget(Stack::row(0.0).grow(1.0)),
            trailing,
        )),
        hint_key.map(|key| hint(ui.now_t(key))),
        body,
    ))
    .into_any()
}

/// The common hotkeys, two to a line where they fit: each a name and its
/// recorder, with what is wrong with it underneath.
fn hotkey_grid(ui: Ui) -> AnyView {
    let cells: Vec<AnyView> = COMMON_KEYS
        .into_iter()
        .map(|a| {
            let action = Shortcut::Global(a);
            widget(
                Stack::column(0.0)
                    .gap_step(nana_ui_core::SpacingStep::Xxs)
                    .width(LengthSpec::Px(0.0))
                    .grow(1.0)
                    .min_width(LengthSpec::Px(KEY_CELL_MIN)),
            )
            .children((
                field_line(ui.now_t(a.label_key()), hotkey_control(ui, action)),
                hotkey_issue_caption(ui, action),
            ))
            .into_any()
        })
        .collect();
    // Lines close together, columns well apart, so one cell's clear button
    // does not read as the next one's.
    widget(
        Stack::row(0.0)
            .gap_step(nana_ui_core::SpacingStep::Sm)
            .width(LengthSpec::Fill)
            .wrap(true)
            .with_layout(|l| l.column_gap = Some(LengthSpec::Px(space::XXXL))),
    )
    .children(cells)
    .into_any()
}

/// A label and its control on one line, the control at the end; it wraps
/// under the label when the line is too narrow.
fn field_line(label: String, control: AnyView) -> AnyView {
    widget(
        Stack::row(0.0)
            .gap_step(nana_ui_core::SpacingStep::Md)
            .align(AlignSpec::Center)
            .justify(JustifySpec::End)
            .width(LengthSpec::Fill)
            .wrap(true),
    )
    .children((
        widget(Stack::row(0.0).grow(1.0)).children(widget(
            Text::new(label)
                .style(Type::Body.style())
                .color(SemanticColorRole::Text),
        )),
        control,
    ))
    .into_any()
}

/// The obs-websocket link's state and switch, named for assistive tech as
/// the Output page's row names it.
fn link_switch(ui: Ui) -> AnyView {
    widget(
        Stack::row(0.0)
            .gap_step(nana_ui_core::SpacingStep::Sm)
            .align(AlignSpec::Center),
    )
    .children((
        link_badge(ui, |s| &s.obs),
        widget({
            let mut switch = Switch::new("", ui.settings.with_untracked(|s| s.output.obs.enabled));
            switch.accessible_name = Some(ui.now_t("obs-link").into());
            switch
        })
        .bind(move |s: &mut Switch| s.checked = ui.settings.with(|x| x.output.obs.enabled))
        .on_cx(|_, e: &ToggleChanged, cx| {
            let on = e.checked;
            cx.dispatch_program_all(edit(move |s| s.output.obs.enabled = on));
        }),
    ))
    .into_any()
}

/// While the link is on: where OBS is, its password, then the display's
/// source in OBS and what sizes it now.
fn link_fields(ui: Ui) -> AnyView {
    let (select, sizing, notice) = display_source(ui);
    widget(
        Stack::column(0.0)
            .gap_step(nana_ui_core::SpacingStep::Sm)
            .width(LengthSpec::Fill),
    )
    .visible(move || ui.settings.with(|s| s.output.obs.enabled))
    .children((
        field_line(ui.now_t("obs-address"), obs_address(ui)),
        field_line(ui.now_t("obs-password"), obs_password_field(ui)),
        field_line(ui.now_t("obs-source"), select),
        widget(
            Text::new(String::new())
                .style(Type::Caption.style())
                .color(SemanticColorRole::Muted),
        )
        .bind(move |t: &mut Text| {
            let text = sizing().unwrap_or_default();
            if t.value != text {
                t.value = text;
            }
        }),
        notice,
    ))
    .into_any()
}

/// Every skin that loads, by name, bound to the active skin: choosing one
/// applies it, and the preview below follows.
pub(super) fn skin_select(ui: Ui) -> AnyView {
    let options: Vec<(String, String)> = ui
        .skins
        .get_untracked()
        .iter()
        .filter(|r| r.error.is_none())
        .map(|r| (r.id.clone(), r.name.clone()))
        .collect();
    select(
        ui,
        options,
        |s| s.skin.id.clone(),
        |s, v| s.skin.id = v.to_owned(),
    )
}

/// A picture of an input mode: the input window's text on the left, the
/// dialogue box on the right, and what carries one to the other.
pub(super) fn mode_diagram(mode: InputMode) -> AnyView {
    let bar = |w: f32, role: SemanticColorRole| {
        widget(
            Stack::row(0.0)
                .width(LengthSpec::Px(w))
                .height(LengthSpec::Px(gap::TIGHT))
                .radius(RadiusTier::Full)
                .surface(role),
        )
        .into_any()
    };
    let caret = || {
        widget(
            Stack::row(0.0)
                .width(LengthSpec::Px(CARET.0))
                .height(LengthSpec::Px(CARET.1))
                .surface(SemanticColorRole::Accent),
        )
        .into_any()
    };
    // A small window: its lines, the last ending at a caret when typing.
    let window = |w: f32, lines: Vec<f32>, typing: bool, accent_edge: bool| {
        let last = lines.len().saturating_sub(1);
        let rows: Vec<AnyView> = lines
            .into_iter()
            .enumerate()
            .map(|(i, lw)| {
                widget(
                    Stack::row(0.0)
                        .gap_step(nana_ui_core::SpacingStep::Xxs)
                        .align(AlignSpec::Center),
                )
                .children((
                    bar(lw, SemanticColorRole::BorderStrong),
                    (typing && i == last).then(caret),
                ))
                .into_any()
            })
            .collect();
        let frame = Stack::column(0.0)
            .gap_step(nana_ui_core::SpacingStep::Sm)
            .width(LengthSpec::Px(w))
            .padding_step(nana_ui_core::SpacingStep::Md, nana_ui_core::SpacingStep::Md)
            .surface(SemanticColorRole::Surface)
            .border(SemanticColorRole::BorderSoft, 1.0)
            .radius(nana_ui_core::RadiusTier::Sm)
            .with_layout(|l| {
                l.position = PositionSpec::Relative;
                l.overflow_x = OverflowSpec::Hidden;
                l.overflow_y = OverflowSpec::Hidden;
            });
        widget(frame)
            .children((
                accent_edge.then(|| {
                    widget(
                        Stack::column(0.0)
                            .width(LengthSpec::Px(CARET.0))
                            .surface(SemanticColorRole::Accent)
                            .with_layout(accent_bar_layout),
                    )
                }),
                rows,
            ))
            .into_any()
    };
    let (link, shown, typing) = match mode {
        // Written in full, then sent: the box shows the whole line.
        InputMode::Compose => (ti::SEND, vec![40.0, 26.0], false),
        // Followed live: the box shows what is typed so far.
        InputMode::Mirror => (ti::ARROW_NARROW_RIGHT, vec![40.0, 14.0], true),
    };
    widget(
        Stack::row(0.0)
            .gap_step(nana_ui_core::SpacingStep::Md)
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center)
            .width(LengthSpec::Fill)
            .surface(SemanticColorRole::Subtle)
            .radius(nana_ui_core::RadiusTier::Md),
    )
    .children((
        window(60.0, vec![40.0, 14.0], true, false),
        widget(IconGlyph::new(link).size(icon::MD).role(match mode {
            InputMode::Compose => SemanticColorRole::Accent,
            InputMode::Mirror => SemanticColorRole::Highlight,
        })),
        window(76.0, shown, typing, true),
    ))
    .into_any()
}

/// One input mode as a card to pick: its picture, its name and what it
/// does.
pub(super) fn mode_card(ui: Ui, mode: InputMode) -> AnyView {
    let (name, about) = match mode {
        InputMode::Compose => ("mode-compose", "mode-compose-hint"),
        InputMode::Mirror => ("mode-mirror", "mode-mirror-hint"),
    };
    pick_card(
        ui.now_t(name),
        move || ui.settings.with(|s| s.general.input_mode == mode),
        move || edit(move |s| s.general.input_mode = mode),
        widget(
            Stack::column(0.0)
                .gap_step(nana_ui_core::SpacingStep::Md)
                .width(LengthSpec::Fill),
        )
        .children((
            mode_diagram(mode),
            widget(Text::new(ui.now_t(name)).style(Type::Strong.style())),
            hint(ui.now_t(about)),
        ))
        .into_any(),
    )
}

/// A card to pick, an equal share of its row: `content`, named `name` for
/// assistive tech; activating it sends `pick()`. The `chosen` one takes the
/// selected fill, an accent border and an accent ring.
fn pick_card(
    name: String,
    chosen: impl Fn() -> bool + Send + Sync + Copy + 'static,
    pick: impl Fn() -> Msg + Send + Sync + 'static,
    content: AnyView,
) -> AnyView {
    let base = {
        let mut st = ListItem::new("").style;
        let l = Arc::make_mut(&mut st.layout);
        // Half the row each; the row's stretch gives both the taller's height.
        l.width = Some(LengthSpec::Px(0.0));
        l.flex_grow = Some(1.0);
        // What `auto_height` clears, cleared here too: the bind below
        // replaces the whole style.
        l.min_height = None;
        st.control_padding_x = None;
        l.padding_left = Some(LengthSpec::Px(space::LG));
        l.padding_right = Some(LengthSpec::Px(space::LG));
        l.padding_top = Some(LengthSpec::Px(space::LG));
        l.padding_bottom = Some(LengthSpec::Px(space::LG));
        st.radius = Some(nana_ui_core::RadiusTier::Md);
        // The card's border says chosen or not; a selected row's hover and
        // press would grey it or flash it.
        st.interaction.selected_hovered.border = None;
        st.interaction.selected_pressed.border = None;
        st
    };
    // The fill, border and ring are one box's, so the fill never covers the
    // border's antialiased inner edge at a fractional scale. The border keeps
    // its width and only turns accent; the chosen card's extra weight is a
    // ring outside it, so switching moves nothing.
    let paint = move |on: bool| {
        let border = if on {
            SemanticColorRole::Accent
        } else {
            SemanticColorRole::BorderSoft
        };
        base.clone()
            .border(border, 1.0)
            .ring(SemanticColorRole::Accent, if on { 1.0 } else { 0.0 })
    };
    widget(ListItem::new(name).style(paint(chosen())).auto_height(true))
        .bind(move |i: &mut ListItem| {
            let on = chosen();
            i.selected = on;
            let style = paint(on);
            if i.style != style {
                i.style = style;
            }
        })
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(pick()))
        .content(content)
        .into_any()
}

/// A 3px bar pinned down the left edge of a relatively positioned parent.
pub(super) fn accent_bar_layout(l: &mut nana_ui_core::LayoutStyle) {
    l.position = PositionSpec::Absolute;
    l.offset_left = Some(LengthSpec::Px(0.0));
    l.offset_top = Some(LengthSpec::Px(0.0));
    l.offset_bottom = Some(LengthSpec::Px(0.0));
}
