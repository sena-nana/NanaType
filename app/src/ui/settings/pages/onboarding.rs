use super::*;

/// How many steps the first-run guide has.
pub(super) const ONBOARD_STEPS: u8 = 4;

/// The first-run guide: one card marked by a 3px accent bar, four steps
/// (language and name, a skin with its live preview, the input mode, the
/// hotkeys and OBS) with Back / Next and a step indicator. Skip or the last
/// step's "Get started" closes it for good; About opens it again.
pub(super) fn onboarding(ui: Ui) -> AnyView {
    let step = ui.onboard_step;
    let finish = move || {
        step.set(0);
        edit(|s| s.general.onboarding_done = true)
    };
    let header = widget(
        Stack::row(space::MD)
            .align(AlignSpec::Center)
            .width(LengthSpec::Fill)
            .padding_xy(inset::CARD, 0.0),
    )
    .children((
        widget(
            IconGlyph::new(ti::SPARKLES)
                .size(16.0)
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
        Stack::row(space::MD)
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
        .on(move |_: &Activate| step.set(step.get_untracked().saturating_sub(1))),
        widget(Stack::row(0.0).grow(1.0)),
        // Next, and on the last step "Get started".
        widget(crate::ui::kit::cta(
            Button::new(ui.now_t("onboard-next")).size(ControlSize::Small),
        ))
        .bind(move |b: &mut Button| {
            let last = step.get() + 1 >= ONBOARD_STEPS;
            let label = ui.now_t(if last { "onboard-done" } else { "onboard-next" });
            if b.label != label {
                b.label = label;
            }
        })
        .on_cx(move |_, _: &Activate, cx| {
            crate::ui::motion::press(cx, ui.reduced_motion.get_untracked());
            let n = step.get_untracked();
            if n + 1 >= ONBOARD_STEPS {
                cx.dispatch_program_all(finish());
            } else {
                step.set(n + 1);
            }
        }),
    ));
    // The same card as every section, its welcome told by the sparkle and
    // the accent step dots rather than a frame of its own.
    widget(crate::ui::kit::card_frame(nana_ui_core::RadiusTier::Lg))
        .visible(move || !ui.settings.with(|s| s.general.onboarding_done))
        .children(
            widget(Stack::column(gap::CONTROL).padding_xy(0.0, inset::ROW_Y)).children((
                header,
                body,
                divider(),
                footer,
            )),
        )
        .into_any()
}

/// "Step 2 of 4" and a dot per step: the current one a wide accent pill,
/// the ones done accent dots, the ones ahead hairline dots.
pub(super) fn step_indicator(ui: Ui) -> AnyView {
    let step = ui.onboard_step;
    let label = move || {
        I18N.tr_in(
            ui.locale.get_untracked(),
            "onboard-step",
            &[
                ("step", u32::from(step.get() + 1).into()),
                ("total", u32::from(ONBOARD_STEPS).into()),
            ],
        )
    };
    let dots: Vec<AnyView> = (0..ONBOARD_STEPS)
        .map(|i| {
            widget(crate::ui::kit::round(
                Stack::row(0.0).height(LengthSpec::Px(space::SM)),
                space::SM,
            ))
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
    widget(Stack::row(space::MD).align(AlignSpec::Center))
        .children((
            widget(
                Text::new(label())
                    .style(Type::Caption.style())
                    .color(SemanticColorRole::Muted),
            )
            .bind(move |t: &mut Text| {
                let text = label();
                if t.value != text {
                    t.value = text;
                }
            }),
            widget(Stack::row(space::XS).align(AlignSpec::Center)).children(dots),
        ))
        .into_any()
}

/// One step's body, under the guide's header. Its rows carry no dividers of
/// their own: the footer's line is the step's only one, so none stacks on
/// it.
pub(super) fn onboard_step(ui: Ui, n: u8) -> AnyView {
    let lead = |key: &str| {
        widget(Stack::column(0.0).padding_xy(inset::CARD, 0.0))
            .children(Type::Lead.view(ui.now_t(key)))
            .into_any()
    };
    let body: Vec<AnyView> = match n {
        0 => vec![
            lead("onboard-welcome-body"),
            row(ui, "onboard-language", None, language_select(ui)),
            row(
                ui,
                "onboard-speaker",
                Some("general-speaker-hint"),
                speaker_field(ui),
            ),
        ],
        1 => vec![
            lead("onboard-skin-hint"),
            row(ui, "onboard-skin", None, skin_select(ui)),
            // The live preview of the skin just chosen.
            skin_preview(ui, signal(None)),
        ],
        2 => vec![
            lead("onboard-mode-body"),
            widget(
                Stack::row(space::XL)
                    .align(AlignSpec::Stretch)
                    .width(LengthSpec::Fill)
                    .padding_xy(inset::CARD, 0.0),
            )
            .children((
                mode_card(ui, InputMode::Compose),
                mode_card(ui, InputMode::Mirror),
            ))
            .into_any(),
        ],
        _ => vec![
            widget(Stack::column(space::LG).padding_xy(inset::CARD, 0.0))
                .children((subheading(ui, "onboard-hotkeys"), hotkeys_sentence(ui)))
                .into_any(),
            row(
                ui,
                "onboard-obs",
                Some("onboard-obs-hint"),
                button(ui, "onboard-obs", || Msg::SettingsPage(Page::Output)),
            ),
            widget(Stack::column(0.0).padding_xy(inset::CARD, 0.0))
                .children(hint(ui.now_t("onboard-reopen")))
                .into_any(),
        ],
    };
    widget(Stack::column(0.0)).children(body).into_any()
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
            crate::ui::kit::round(
                Stack::row(0.0)
                    .width(LengthSpec::Px(w))
                    .height(LengthSpec::Px(gap::TIGHT)),
                gap::TIGHT,
            )
            .surface(role),
        )
        .into_any()
    };
    let caret = || {
        widget(
            Stack::row(0.0)
                .width(LengthSpec::Px(2.0))
                .height(LengthSpec::Px(12.0))
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
                widget(Stack::row(space::XXS).align(AlignSpec::Center))
                    .children((
                        bar(lw, SemanticColorRole::BorderStrong),
                        (typing && i == last).then(caret),
                    ))
                    .into_any()
            })
            .collect();
        let frame = Stack::column(space::SM)
            .width(LengthSpec::Px(w))
            .padding_xy(space::MD, space::MD)
            .surface(SemanticColorRole::Surface)
            .outline(SemanticColorRole::BorderSoft, 1.0)
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
                            .width(LengthSpec::Px(2.0))
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
        Stack::row(space::MD)
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
/// does. The chosen one takes the selected fill and an accent outline.
pub(super) fn mode_card(ui: Ui, mode: InputMode) -> AnyView {
    let (name, about) = match mode {
        InputMode::Compose => ("mode-compose", "mode-compose-hint"),
        InputMode::Mirror => ("mode-mirror", "mode-mirror-hint"),
    };
    let chosen = move || ui.settings.with(|s| s.general.input_mode == mode);
    let item = widget(
        ListItem::new(ui.now_t(name))
            .style({
                let mut st = ListItem::new("").style;
                let l = Arc::make_mut(&mut st.layout);
                l.width = Some(LengthSpec::Fill);
                st.control_padding_x = None;
                l.padding_left = Some(LengthSpec::Px(space::LG));
                l.padding_right = Some(LengthSpec::Px(space::LG));
                l.padding_top = Some(LengthSpec::Px(space::LG));
                l.padding_bottom = Some(LengthSpec::Px(space::LG));
                st
            })
            .auto_height(true),
    )
    .bind(move |i: &mut ListItem| i.selected = chosen())
    .on_cx(move |_, _: &Activate, cx| {
        cx.dispatch_program_all(edit(move |s| s.general.input_mode = mode));
    })
    .content(
        widget(Stack::column(space::MD).width(LengthSpec::Fill)).children((
            mode_diagram(mode),
            widget(Text::new(ui.now_t(name)).style(Type::Strong.style())),
            hint(ui.now_t(about)),
        )),
    );
    widget(
        Stack::column(0.0)
            .width(LengthSpec::Px(0.0))
            .grow(1.0)
            .radius(nana_ui_core::RadiusTier::Md),
    )
    .bind(move |s: &mut Stack| {
        let (role, w) = if chosen() {
            (SemanticColorRole::Accent, 2.0)
        } else {
            (SemanticColorRole::BorderSoft, 1.0)
        };
        let taken = std::mem::replace(s, Stack::row(0.0));
        *s = taken.outline(role, w);
    })
    .children(item)
    .into_any()
}

/// "Press [F8] to summon the input and [F9] to continue": the sentence
/// around the hotkeys as keycaps, in the locale's own word order.
pub(super) fn hotkeys_sentence(ui: Ui) -> AnyView {
    let s = ui.settings.get_untracked();
    let chord = |h: Option<&nanatype_settings::Hotkey>| match h {
        Some(h) => crate::ui::kit::keycap::chord(&h.label(META_LABEL)),
        None => crate::ui::kit::keycap::keycap("—"),
    };
    let text = I18N.tr_in(
        ui.locale.get_untracked(),
        "onboard-hotkeys-body",
        &[("summon", "\u{1}".into()), ("continue", "\u{2}".into())],
    );
    let mut parts: Vec<AnyView> = Vec::new();
    let mut rest = text.as_str();
    while let Some(at) = rest.find(['\u{1}', '\u{2}']) {
        let (before, after) = rest.split_at(at);
        if !before.trim().is_empty() {
            parts.push(Type::Lead.view(before.trim().to_owned()));
        }
        parts.push(if after.starts_with('\u{1}') {
            chord(s.hotkeys.summon.as_ref())
        } else {
            chord(s.hotkeys.continue_.as_ref())
        });
        rest = &after[1..];
    }
    if !rest.trim().is_empty() {
        parts.push(Type::Lead.view(rest.trim().to_owned()));
    }
    widget(Stack::row(space::SM).align(AlignSpec::Center).wrap(true))
        .children(parts)
        .into_any()
}

/// A 3px bar pinned down the left edge of a relatively positioned parent.
pub(super) fn accent_bar_layout(l: &mut nana_ui_core::LayoutStyle) {
    l.position = PositionSpec::Absolute;
    l.offset_left = Some(LengthSpec::Px(0.0));
    l.offset_top = Some(LengthSpec::Px(0.0));
    l.offset_bottom = Some(LengthSpec::Px(0.0));
}
