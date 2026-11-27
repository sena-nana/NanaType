use super::toolbar::{GLYPH, TARGET, Tip, opens, tool_button};
use super::*;

/// A colour swatch in the colour and outline panels.
const SWATCH: f32 = 26.0;
/// A picture swatch (a sticker, an image) in its panel.
pub(super) const PICTURE_SWATCH: f32 = 52.0;

const SWATCHES: [&str; 12] = [
    "#e8505b", "#f08a3c", "#f7c948", "#7bc96f", "#2fb5a6", "#3d9be9", "#5b6be8", "#a066e0",
    "#e86a92", "#ffffff", "#8a8f98", "#1b1f27",
];

/// A small button painted with a colour.
fn swatch_button(label: String, color: [f32; 4]) -> Button {
    let mut st = NodeStyle::default();
    {
        let l = Arc::make_mut(&mut st.layout);
        l.width = Some(LengthSpec::Px(SWATCH));
        l.height = Some(LengthSpec::Px(SWATCH));
        l.min_width = Some(LengthSpec::Px(SWATCH));
        l.paint_colors.background = Some(nana_ui_core::PaintColor::srgb(color));
        l.border_width = Some(1.0);
        l.border_style = Some(nana_ui_core::BorderStyle::Solid);
    }
    st.radius = Some(nana_ui_core::RadiusTier::Xs);
    st.border = Some(SemanticColorRole::BorderStrong);
    {
        let l = Arc::make_mut(&mut st.layout);
        l.padding_left = Some(LengthSpec::Px(0.0));
        l.padding_right = Some(LengthSpec::Px(0.0));
    }
    Button::new("")
        .accessible_name(label)
        .kind(ButtonKind::Text)
        .style(st)
}

/// A button showing an image (sticker picker).
pub(super) fn image_button(label: String, url: String) -> Button {
    let mut st = NodeStyle::default();
    {
        let l = Arc::make_mut(&mut st.layout);
        l.width = Some(LengthSpec::Px(PICTURE_SWATCH));
        l.height = Some(LengthSpec::Px(PICTURE_SWATCH));
        l.paint.background_layers = vec![nana_ui_core::BackgroundImage::url_with_fit(
            url,
            nana_ui_core::BackgroundImageFit::Contain,
        )];
    }
    st.radius = Some(nana_ui_core::RadiusTier::Sm);
    Button::new("")
        .accessible_name(label)
        .kind(ButtonKind::Text)
        .style(st)
}

/// A small caption over a group in a toolbar panel.
pub(super) fn caption(text: String) -> AnyView {
    widget(
        Text::new(text)
            .style(Type::Micro.style())
            .color(SemanticColorRole::Muted),
    )
    .into_any()
}

/// The colour tool's face: the glyph over a bar in the selection's colour.
pub(super) fn color_face(ui: Ui, open: Signal<bool>) -> AnyView {
    const BAR_H: f32 = 3.0;
    let label = ui.now_t("tb-color");
    let glyph = opens(
        tool_button(ti::LETTER_A, label.clone(), label, Tip::Below).icon_size(GLYPH - BAR_H),
        open,
    );
    widget(Stack::row(0.0).with_layout(|l| {
        l.position = PositionSpec::Relative;
        l.width = Some(LengthSpec::Px(TARGET));
        l.height = Some(LengthSpec::Px(TARGET));
        l.flex_shrink = Some(0.0);
    }))
    .children((
        glyph,
        widget(Stack::row(0.0).radius(nana_ui_core::RadiusTier::Xs)).bind(move |s: &mut Stack| {
            let attrs = ui.sel_attrs.get();
            let colors = crate::ui::kit::theme::palette(ui.chrome());
            let color = if attrs.mixed.contains(RichAttrMask::COLOR) {
                colors.faint.as_rgba_array()
            } else {
                match attrs.style.paint.color {
                    Some(nana_ui_core::PaintColor::Srgb { rgba }) => rgba,
                    _ => colors.text.as_rgba_array(),
                }
            };
            let l = s.layout_mut();
            // Under the glyph, which the button centres.
            l.position = PositionSpec::Absolute;
            l.offset_left = Some(LengthSpec::Px((TARGET - (GLYPH - 2.0)) / 2.0));
            l.offset_top = Some(LengthSpec::Px((TARGET + GLYPH - BAR_H) / 2.0));
            l.pointer_events = Some(PointerEventsSpec::None);
            l.width = Some(LengthSpec::Px(GLYPH - 2.0));
            l.height = Some(LengthSpec::Px(BAR_H));
            l.flex_shrink = Some(0.0);
            l.paint_colors.background = Some(nana_ui_core::PaintColor::srgb(color));
        }),
    ))
    .into_any()
}

pub(super) fn color_panel(ui: Ui, open: Signal<bool>) -> AnyView {
    let rows: Vec<AnyView> = SWATCHES
        .chunks(6)
        .map(|chunk| {
            let buttons: Vec<AnyView> = chunk
                .iter()
                .map(|hex| {
                    let rgba: Rgba8 = hex.parse().expect("swatch");
                    let color = c(rgba);
                    widget(swatch_button(hex.to_string(), color))
                        .on_cx(move |_, _: &Activate, cx| {
                            open.set(false);
                            cx.dispatch_program_all(act(Tool::Color(rgba)));
                        })
                        .into_any()
                })
                .collect();
            widget(Stack::row(0.0).gap_step(gap::step::TIGHT))
                .children(buttons)
                .into_any()
        })
        .collect();
    let picked = signal([0.9_f32, 0.3, 0.4, 1.0]);
    widget(
        Stack::column(0.0)
            .gap_step(gap::step::CONTROL)
            .padding_step(gap::step::TIGHT, gap::step::TIGHT),
    )
    .children((
        rows,
        widget(ColorField::new([0.9, 0.3, 0.4, 1.0]).label(ui.now_t("tb-color"))) // design-tokens: data
            .on(move |e: &ColorChanged| picked.set(e.value)),
        widget(Stack::row(0.0).gap_step(gap::step::TIGHT)).children((widget(
            Button::new(ui.now_t("common-apply"))
                .size(ControlSize::Small)
                .kind(ButtonKind::Primary),
        )
        .on_cx(move |_, _: &Activate, cx| {
            let [r, g, b, a] = picked.get_untracked();
            let to = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            let rgba = Rgba8::rgba(to(r), to(g), to(b), to(a)); // design-tokens: data
            open.set(false);
            cx.dispatch_program_all(act(Tool::Color(rgba)));
        }),)),
    ))
    .into_any()
}

fn typography_current(ui: Ui, value: &str) -> String {
    I18N.tr_in(
        ui.locale.get_untracked(),
        "tb-current-value",
        &[("value", value.into())],
    )
}

fn panel_action(
    label: String,
    open: Signal<bool>,
    action: impl Fn() -> Option<Tool> + Send + 'static,
) -> AnyView {
    widget(Button::new(label).size(ControlSize::Small))
        .on_cx(move |_, _: &Activate, cx| {
            if let Some(tool) = action() {
                open.set(false);
                cx.dispatch_program_all(act(tool));
            }
        })
        .into_any()
}

pub(super) fn size_panel(ui: Ui, open: Signal<bool>) -> AnyView {
    dynamic(
        move || open.get(),
        move |_| {
            let attrs = ui.sel_attrs.get_untracked();
            let base = ui.skin.with_untracked(|s| s.dialogue.text.size);
            let em = attrs
                .style
                .shape
                .size_em
                .unwrap_or_else(|| attrs.style.shape.size_px.map_or(1.0, |px| px / base));
            let relative = signal(f64::from(em.clamp(0.5, 3.0)));
            let pixels = signal(f64::from(
                attrs
                    .style
                    .shape
                    .size_px
                    .unwrap_or(em * base)
                    .clamp(4.0, 400.0),
            ));
            let current = if attrs.mixed.contains(RichAttrMask::SIZE) {
                ui.now_t("tb-mixed")
            } else if let Some(em) = attrs.style.shape.size_em {
                format!("{em}×")
            } else if let Some(px) = attrs.style.shape.size_px {
                format!("{px}px")
            } else {
                ui.now_t("tb-inherit")
            };
            let presets = [0.75_f32, 1.25, 1.5, 2.0]
                .into_iter()
                .map(|v| {
                    panel_action(format!("{v}×"), open, move || {
                        Some(Tool::Size(Some(Size::Em(v))))
                    })
                })
                .collect::<Vec<_>>();
            widget(
                Stack::column(0.0)
                    .gap_step(gap::step::CONTROL)
                    .padding_step(gap::step::TIGHT, gap::step::TIGHT),
            )
            .children((
                widget(Text::new(typography_current(ui, &current))).key("size-current"),
                widget(Stack::row(0.0).gap_step(gap::step::TIGHT)).children(presets),
                widget(Text::new(format!("{} (×)", ui.now_t("tb-size-relative")))),
                widget(
                    NumberInput::continuous(relative.get_untracked())
                        .range(0.5, 3.0)
                        .step(0.05)
                        .label(ui.now_t("tb-size-relative")),
                )
                .key("size-relative")
                .on(move |e: &NumberChanged| relative.set(e.value)),
                panel_action(ui.now_t("tb-apply-relative"), open, move || {
                    Some(Tool::Size(Some(Size::Em(relative.get_untracked() as f32))))
                }),
                widget(Text::new(format!("{} (px)", ui.now_t("tb-size-pixels")))),
                widget(
                    NumberInput::continuous(pixels.get_untracked())
                        .range(4.0, 400.0)
                        .step(1.0)
                        .label(ui.now_t("tb-size-pixels")),
                )
                .key("size-pixels")
                .on(move |e: &NumberChanged| pixels.set(e.value)),
                panel_action(ui.now_t("tb-apply-pixels"), open, move || {
                    Some(Tool::Size(Some(Size::Px(pixels.get_untracked() as f32))))
                }),
                panel_action(ui.now_t("tb-inherit"), open, || Some(Tool::Size(None))),
            ))
            .into_any()
        },
    )
    .into_any()
}

pub(super) fn font_panel(ui: Ui, open: Signal<bool>) -> AnyView {
    dynamic(
        move || open.get(),
        move |_| {
            use nanatype_model::FontSlot;
            let attrs = ui.sel_attrs.get_untracked();
            let current = if attrs.mixed.contains(RichAttrMask::FAMILY) {
                ui.now_t("tb-mixed")
            } else if let Some(alias) = attrs.style.shape.family_alias.as_deref() {
                ui.now_t(&format!("tb-font-{alias}"))
            } else {
                attrs
                    .style
                    .shape
                    .family
                    .as_ref()
                    .map_or_else(|| ui.now_t("tb-inherit"), |f| f.to_string())
            };
            let initial = if attrs.style.shape.family_alias.is_none() {
                attrs.style.shape.family.as_deref().unwrap_or("")
            } else {
                ""
            };
            let family = signal(initial.to_owned());
            let selected = move || {
                let name = family.get_untracked();
                (!name.trim().is_empty())
                    .then(|| Tool::Font(Some(FontSlot::Family(name.trim().into()))))
            };
            let choices = [
                ("tb-inherit", None),
                ("tb-font-body", Some(FontSlot::Body)),
                ("tb-font-name", Some(FontSlot::Name)),
                ("tb-font-accent", Some(FontSlot::Accent)),
            ]
            .into_iter()
            .map(|(key, font)| {
                panel_action(ui.now_t(key), open, move || Some(Tool::Font(font.clone())))
            })
            .collect::<Vec<_>>();
            widget(
                Stack::column(0.0)
                    .gap_step(gap::step::CONTROL)
                    .padding_step(gap::step::TIGHT, gap::step::TIGHT),
            )
            .children((
                widget(Text::new(typography_current(ui, &current))).key("font-current"),
                choices,
                widget(kit::text_input(initial).placeholder(ui.now_t("tb-font-family")))
                    .key("font-family")
                    .on(move |e: &TextChanged| family.set(e.value.to_string()))
                    .on_cx(move |_, _: &TextSubmitted, cx| {
                        if let Some(tool) = selected() {
                            open.set(false);
                            cx.dispatch_program_all(act(tool));
                        }
                    }),
                panel_action(ui.now_t("common-apply"), open, selected),
            ))
            .into_any()
        },
    )
    .into_any()
}

pub(super) fn alignment_panel(ui: Ui, open: Signal<bool>) -> AnyView {
    use nanatype_model::ParagraphAlign;
    let choices = [
        ("tb-inherit", None),
        ("tb-align-start", Some(ParagraphAlign::Start)),
        ("tb-align-center", Some(ParagraphAlign::Center)),
        ("tb-align-end", Some(ParagraphAlign::End)),
    ]
    .into_iter()
    .map(|(key, align)| panel_action(ui.now_t(key), open, move || Some(Tool::Align(align))))
    .collect::<Vec<_>>();
    widget(
        Stack::column(0.0)
            .gap_step(gap::step::CONTROL)
            .padding_step(gap::step::TIGHT, gap::step::TIGHT),
    )
    .children((
        widget(Text::new(""))
            .key("alignment-current")
            .bind(move |t: &mut Text| {
                let attrs = ui.sel_attrs.get();
                let key = if attrs.paragraph_alignment_mixed {
                    "tb-mixed"
                } else {
                    match attrs
                        .paragraph_alignment
                        .and_then(super::super::rich::align_from_rich)
                    {
                        None => "tb-inherit",
                        Some(ParagraphAlign::Start) => "tb-align-start",
                        Some(ParagraphAlign::Center) => "tb-align-center",
                        Some(ParagraphAlign::End) => "tb-align-end",
                    }
                };
                t.value = typography_current(ui, &ui.now_t(key));
            }),
        choices,
    ))
    .into_any()
}

pub(super) fn ruby_panel(ui: Ui, open: Signal<bool>) -> AnyView {
    let reading = signal(String::new());
    widget(
        Stack::column(0.0)
            .gap_step(gap::step::CONTROL)
            .padding_step(gap::step::TIGHT, gap::step::TIGHT),
    )
    .children((
        widget(kit::text_input("").placeholder(ui.now_t("tb-ruby-placeholder")))
            .on(move |e: &TextChanged| reading.set(e.value.to_string()))
            .on_cx(move |_, _: &TextSubmitted, cx| {
                let r = reading.get_untracked();
                if !r.trim().is_empty() {
                    open.set(false);
                    cx.dispatch_program_all(act(Tool::Ruby(r)));
                }
            }),
        widget(Stack::row(0.0).gap_step(gap::step::TIGHT)).children((
            widget(
                Button::new(ui.now_t("tb-ruby-apply"))
                    .size(ControlSize::Small)
                    .kind(ButtonKind::Primary),
            )
            .on_cx(move |_, _: &Activate, cx| {
                let r = reading.get_untracked();
                if !r.trim().is_empty() {
                    open.set(false);
                    cx.dispatch_program_all(act(Tool::Ruby(r)));
                }
            }),
            widget(Button::new(ui.now_t("tb-ruby-remove")).size(ControlSize::Small)).on_cx(
                move |_, _: &Activate, cx| {
                    open.set(false);
                    cx.dispatch_program_all(act(Tool::Ruby(String::new())));
                },
            ),
        )),
    ))
    .into_any()
}

/// Outline and shadow: colour swatches for the outline, named looks for the
/// shadow, each with a way to take it off.
pub(super) fn stroke_panel(ui: Ui, open: Signal<bool>) -> AnyView {
    let accent = ui.skin.get_untracked().chrome.accent;
    let stroke = move |color: Rgba8| {
        Tool::Stroke(Some(Stroke {
            color,
            width_px: 2.0,
        }))
    };
    // A button that applies `tool` and closes the panel.
    let press = move |button: Button, tool: Tool| -> AnyView {
        widget(button)
            .on_cx(move |_, _: &Activate, cx| {
                open.set(false);
                cx.dispatch_program_all(act(tool.clone()));
            })
            .into_any()
    };
    let swatches: Vec<AnyView> = [
        ("tb-stroke-white", Rgba8::rgba(255, 255, 255, 255)), // design-tokens: data
        ("tb-stroke-black", Rgba8::rgba(0, 0, 0, 255)),       // design-tokens: data
        ("tb-stroke-accent", accent),
    ]
    .into_iter()
    .map(|(key, color)| press(swatch_button(ui.now_t(key), c(color)), stroke(color)))
    .collect();
    let small = |key: &str| Button::new(ui.now_t(key)).size(ControlSize::Small);
    let drop = Shadow {
        color: Rgba8::rgba(0, 0, 0, 0x99), // design-tokens: data
        dx: 2.0,
        dy: 2.0,
        blur: 3.0,
    };
    let glow = Shadow {
        color: accent.with_alpha(220),
        dx: 0.0,
        dy: 0.0,
        blur: 8.0,
    };
    widget(
        Stack::column(0.0)
            .gap_step(gap::step::CONTROL)
            .padding_step(gap::step::TIGHT, gap::step::TIGHT),
    )
    .children((
        caption(ui.now_t("tb-stroke")),
        widget(
            Stack::row(0.0)
                .gap_step(gap::step::TIGHT)
                .align(AlignSpec::Center),
        )
        .children((swatches, press(small("tb-stroke-none"), Tool::Stroke(None)))),
        caption(ui.now_t("tb-shadow")),
        widget(
            Stack::row(0.0)
                .gap_step(gap::step::TIGHT)
                .align(AlignSpec::Center),
        )
        .children((
            press(small("tb-shadow-drop"), Tool::Shadow(Some(drop))),
            press(
                small("tb-shadow-glow").icon(ti::SPARKLES),
                Tool::Shadow(Some(glow)),
            ),
            press(small("tb-shadow-none"), Tool::Shadow(None)),
        )),
    ))
    .into_any()
}
