//! The input window: a full-area editor or an editable skin preview.
//! Both layouts retain the same rich/source editors and draft. Toolbar
//! buttons send semantic [`Tool`] actions; the source toggle edits markup.

use std::sync::Arc;

use nana_ui::icons_tabler as ti;
use nana_ui::runtime::view::{
    AnyView, EntityRef, IntoView, Signal, WithRefs, dynamic, entity_ref, segmented_option, signal,
    widget, with_refs,
};
use nana_ui::runtime::{
    ActionMenu, ActionMenuItem, Activate, Button, ColorChanged, ColorField, Divider,
    FocusWithinChanged, IconButton, IconGlyph, NodeStyle, NumberChanged, NumberInput, Popover,
    PopoverClosed, PopoverToggled, RichAttrMask, RichSelectionAttrs, RichTextEditor, ScrollAxes,
    ScrollView, ScrollViewportChanged, SegmentedControl, SizeChanged, Stack, Text, TextArea,
    TextChanged, TextInput, TextSubmitted,
};
use nana_ui::theme::space;
use nana_ui::{ButtonKind, ControlSize, PopoverPlacement};
use nana_ui_core::{
    AlignSpec, JustifySpec, LengthSpec, OverflowSpec, PointerEventsSpec, PositionSpec,
    SemanticColorRole, SquareSize,
};
use nanatype_core::Cmd;
use nanatype_model::{
    AssetRef, Atom, AtomKind, ContinuousKind, EntranceKind, Inline, Marker, Pace, Rgba8, Shadow,
    Size, Stroke,
};
use nanatype_settings::{InputKey, InputMode, Settings};

use super::dialogue::{Source, dialogue_box};
use super::rich::{ChipKind, MarkerLook};
use super::{I18N, Ui, c};
use crate::msg::{FileTarget, MarkupEdit, Msg, Tool};
use crate::ui::kit::{Tone, Type, gap, icon, inset};
use crate::windows::TOOLBAR_H;

/// A colour swatch in the colour and outline panels.
const SWATCH: f32 = 26.0;
/// A picture swatch (a sticker, an image) in its panel.
const PICTURE_SWATCH: f32 = 52.0;
/// The kind's stripe on a marker chip in the cheat sheet.
const CHIP_STRIPE: f32 = 3.0;
/// How far inside the draft's box its focus ring sits.
const FOCUS_INSET: f32 = 2.0;
/// The cheat sheet's lead column: a marker chip, or a chord of keycaps.
const SHEET_MARKERS_W: f32 = 116.0;
const SHEET_KEYS_W: f32 = 136.0;
/// The quoted viewer's avatar on the reply bar.
const REPLY_AVATAR: f32 = 18.0;

pub mod completion;
mod stickers;
mod workspace;

use nanatype_frame::ui::editor_view::{self, EditorEvent};
pub use nanatype_frame::ui::editor_view::{marker_label, preview_editor};

fn act(tool: Tool) -> Msg {
    Msg::Tool(tool)
}

fn marker(m: Marker) -> Msg {
    Msg::Tool(Tool::Insert(Inline::Marker(m)))
}

/// Toolbar glyphs: one size for every tool, centred in a 28px target.
const GLYPH: f32 = 14.0;
/// Icon-button hit target.
const TARGET: f32 = 28.0;

/// A toolbar icon button: the shared glyph size in a 28px square, in the
/// toolbar ink.
fn tool_button(icon: nana_ui::runtime::Icon, label: String, tip: String) -> IconButton {
    // Large is the size whose glyph is GLYPH; the square stays 28px.
    let mut b = IconButton::new(icon, label)
        .size(ControlSize::Large)
        .with_tooltip(tip);
    b.style.square = Some(SquareSize::IconButton);
    b
}

#[cfg(feature = "unofficial-chat")]
pub(super) fn footer_icon(icon: nana_ui::runtime::Icon, label: String) -> IconButton {
    tool_button(icon, label.clone(), label)
}

/// "Bold · Ctrl+B": a tooltip with the tool's shortcut.
fn with_shortcut(label: String, keys: Option<&str>) -> String {
    match keys {
        Some(k) => format!("{label} · {k}"),
        None => label,
    }
}

/// An icon button with a tooltip that sends `msg()`.
fn tool(
    ui: Ui,
    icon: nana_ui::runtime::Icon,
    label: &'static str,
    msg: impl Fn() -> Msg + Send + 'static,
) -> AnyView {
    widget(tool_button(icon, ui.now_t(label), ui.now_t(label)))
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(msg()))
        .into_any()
}

/// A toolbar toggle (bold, italic…) shown pressed while the whole selection
/// has the attribute; its tooltip names its shortcut.
fn toggle(
    ui: Ui,
    icon: nana_ui::runtime::Icon,
    label: &'static str,
    keys: Option<&'static str>,
    tool_of: fn() -> Tool,
    pressed: fn(&RichSelectionAttrs) -> bool,
) -> AnyView {
    widget(tool_button(
        icon,
        ui.now_t(label),
        with_shortcut(ui.now_t(label), keys),
    ))
    .bind(move |b: &mut IconButton| {
        let on = !ui.source_mode.get() && ui.sel_attrs.with(pressed);
        if b.selected != on {
            b.selected = on;
        }
    })
    .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(act(tool_of())))
    .into_any()
}

fn is_bold(a: &RichSelectionAttrs) -> bool {
    !a.mixed.contains(RichAttrMask::WEIGHT) && a.style.shape.weight.is_some_and(|w| w >= 600)
}

fn is_italic(a: &RichSelectionAttrs) -> bool {
    !a.mixed.contains(RichAttrMask::ITALIC) && a.style.shape.italic == Some(true)
}

fn is_underline(a: &RichSelectionAttrs) -> bool {
    !a.mixed.contains(RichAttrMask::DECORATION)
        && a.style.paint.decoration.is_some_and(|d| d.underline)
}

fn is_strike(a: &RichSelectionAttrs) -> bool {
    !a.mixed.contains(RichAttrMask::DECORATION)
        && a.style.paint.decoration.is_some_and(|d| d.line_through)
}

/// A toolbar button shown pressed while `on` holds, sending `msg()`.
fn pressed_tool(
    icon: nana_ui::runtime::Icon,
    label: String,
    on: impl Fn() -> bool + Send + 'static,
    msg: impl Fn() -> Msg + Send + 'static,
) -> nana_ui::runtime::view::El<IconButton> {
    widget(tool_button(icon, label.clone(), label))
        .bind(move |b: &mut IconButton| {
            let on = on();
            if b.selected != on {
                b.selected = on;
            }
        })
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(msg()))
}

/// Swaps the editor for the draft's markup source.
fn source_toggle(ui: Ui) -> AnyView {
    pressed_tool(
        ti::CODE,
        ui.now_t("tb-source"),
        move || ui.source_mode.get(),
        || Msg::ToggleSource,
    )
    .into_any()
}

/// Shows the draft as the skin draws it, or as plain text.
fn skin_preview_toggle(ui: Ui) -> AnyView {
    let on = move || ui.settings.with(|s| s.general.input_skin_preview);
    pressed_tool(ti::EYE, ui.now_t("input-skin-preview"), on, move || {
        let on = !ui.settings.with_untracked(|s| s.general.input_skin_preview);
        super::settings::controls::edit(move |s| s.general.input_skin_preview = on)
    })
    .key("input-skin-preview-toggle")
    .into_any()
}

/// A menu trigger's face: the bare glyph, centred in the box left after the
/// trigger's own `space::LG` padding, so the hit target stays [`TARGET`].
fn trigger_face(icon: nana_ui::runtime::Icon) -> AnyView {
    let inner = TARGET - 2.0 * space::LG;
    widget(
        Stack::row(0.0)
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center)
            .with_layout(|l| {
                l.width = Some(LengthSpec::Px(inner));
                l.min_width = Some(LengthSpec::Px(inner));
                l.height = Some(LengthSpec::Px(GLYPH));
                l.flex_shrink = Some(0.0);
                l.overflow_x = OverflowSpec::Visible;
                l.overflow_y = OverflowSpec::Visible;
            }),
    )
    .children(widget(IconGlyph::new(icon).size(GLYPH).decorative()))
    .into_any()
}

/// A popover whose open state the app keeps, so items can close it.
fn popover(
    icon: nana_ui::runtime::Icon,
    label: String,
    width: f32,
    open: Signal<bool>,
    body: impl IntoView,
) -> AnyView {
    widget(
        Popover::new()
            .trigger(label)
            .placement(PopoverPlacement::Bottom)
            .width(width)
            .bare_trigger(true),
    )
    .trigger(trigger_face(icon))
    .bind(move |p: &mut Popover| p.open = open.get())
    .on(move |e: &PopoverToggled| open.set(e.open))
    .on(move |_: &PopoverClosed| open.set(false))
    .children(body)
    .into_any()
}

/// Menu item that sends `msg()` and closes its menu.
fn item(label: String, open: Signal<bool>, msg: impl Fn() -> Msg + Send + 'static) -> AnyView {
    widget(ActionMenuItem::new(label))
        .on_cx(move |_, _: &Activate, cx| {
            open.set(false);
            cx.dispatch_program_all(msg());
        })
        .into_any()
}

/// An action menu behind a toolbar glyph.
fn menu(
    icon: nana_ui::runtime::Icon,
    label: String,
    open: Signal<bool>,
    items: Vec<AnyView>,
) -> AnyView {
    widget(ActionMenu::new().trigger(label).bare_trigger(true))
        .trigger(trigger_face(icon))
        .bind(move |m: &mut ActionMenu| m.popover.open = open.get())
        .on(move |e: &PopoverToggled| open.set(e.open))
        .on(move |_: &PopoverClosed| open.set(false))
        .children(items)
        .into_any()
}

/// A small button painted with a colour.
fn swatch_button(label: String, color: [f32; 4]) -> Button {
    let mut st = NodeStyle::default();
    {
        let l = Arc::make_mut(&mut st.layout);
        l.width = Some(LengthSpec::Px(SWATCH));
        l.height = Some(LengthSpec::Px(SWATCH));
        l.min_width = Some(LengthSpec::Px(SWATCH));
        l.background = Some(color);
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
fn image_button(label: String, url: String) -> Button {
    let mut st = NodeStyle::default();
    {
        let l = Arc::make_mut(&mut st.layout);
        l.width = Some(LengthSpec::Px(PICTURE_SWATCH));
        l.height = Some(LengthSpec::Px(PICTURE_SWATCH));
        l.paint.background_image = Some(nana_ui_core::BackgroundImage::url_with_fit(
            url,
            nana_ui_core::BackgroundImageFit::Contain,
        ));
    }
    st.radius = Some(nana_ui_core::RadiusTier::Sm);
    Button::new("")
        .accessible_name(label)
        .kind(ButtonKind::Text)
        .style(st)
}

const SWATCHES: [&str; 12] = [
    "#e8505b", "#f08a3c", "#f7c948", "#7bc96f", "#2fb5a6", "#3d9be9", "#5b6be8", "#a066e0",
    "#e86a92", "#ffffff", "#8a8f98", "#1b1f27",
];

fn color_panel(ui: Ui, open: Signal<bool>) -> AnyView {
    let rows: Vec<AnyView> = SWATCHES
        .chunks(6)
        .map(|chunk| {
            let buttons: Vec<AnyView> = chunk
                .iter()
                .map(|hex| {
                    let rgba: Rgba8 = hex.parse().expect("swatch");
                    let color = super::c(rgba);
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
        widget(Stack::row(0.0).gap_step(gap::step::TIGHT)).children((widget(crate::ui::kit::cta(
            Button::new(ui.now_t("common-apply")).size(ControlSize::Small),
        ))
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

fn size_panel(ui: Ui, open: Signal<bool>) -> AnyView {
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

fn font_panel(ui: Ui, open: Signal<bool>) -> AnyView {
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
                widget(TextInput::new(initial).placeholder(ui.now_t("tb-font-family")))
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

fn alignment_panel(ui: Ui, open: Signal<bool>) -> AnyView {
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
                        .and_then(super::rich::align_from_rich)
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

fn ruby_panel(ui: Ui, open: Signal<bool>) -> AnyView {
    let reading = signal(String::new());
    widget(
        Stack::column(0.0)
            .gap_step(gap::step::CONTROL)
            .padding_step(gap::step::TIGHT, gap::step::TIGHT),
    )
    .children((
        widget(TextInput::new("").placeholder(ui.now_t("tb-ruby-placeholder")))
            .on(move |e: &TextChanged| reading.set(e.value.to_string()))
            .on_cx(move |_, _: &TextSubmitted, cx| {
                let r = reading.get_untracked();
                if !r.trim().is_empty() {
                    open.set(false);
                    cx.dispatch_program_all(act(Tool::Ruby(r)));
                }
            }),
        widget(Stack::row(0.0).gap_step(gap::step::TIGHT)).children((
            widget(crate::ui::kit::cta(
                Button::new(ui.now_t("tb-ruby-apply")).size(ControlSize::Small),
            ))
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

fn expression_panel(ui: Ui, open: Signal<bool>) -> AnyView {
    let name = signal(String::new());
    let skin = ui.skin.get_untracked();
    let variants: Vec<AnyView> = skin
        .dialogue
        .portrait
        .variants
        .keys()
        .map(|v| {
            let v = v.clone();
            let label = v.clone();
            widget(Button::new(label).size(ControlSize::Small))
                .on_cx(move |_, _: &Activate, cx| {
                    open.set(false);
                    cx.dispatch_program_all(marker(Marker::Expression {
                        name: v.clone(),
                        intensity: None,
                    }));
                })
                .into_any()
        })
        .collect();
    widget(
        Stack::column(0.0)
            .gap_step(gap::step::CONTROL)
            .padding_step(gap::step::TIGHT, gap::step::TIGHT),
    )
    .children((
        widget(Stack::row(0.0).gap_step(gap::step::TIGHT).wrap(true)).children(variants),
        widget(TextInput::new("").placeholder(ui.now_t("tb-expression-placeholder")))
            .on(move |e: &TextChanged| name.set(e.value.to_string()))
            .on_cx(move |_, _: &TextSubmitted, cx| {
                let n = name.get_untracked();
                if !n.trim().is_empty() {
                    open.set(false);
                    cx.dispatch_program_all(marker(Marker::Expression {
                        name: n.trim().to_owned(),
                        intensity: None,
                    }));
                }
            }),
    ))
    .into_any()
}

/// The toolbar's sticker picker; `open` is its popover's state.
pub fn sticker_panel(ui: Ui, open: Signal<bool>) -> AnyView {
    stickers::panel(ui, Some(open), None)
}

/// A small caption over a group in a toolbar panel.
fn caption(text: String) -> AnyView {
    widget(
        Text::new(text)
            .style(Type::Micro.style())
            .color(SemanticColorRole::Muted),
    )
    .into_any()
}

/// Outline and shadow: colour swatches for the outline, named looks for the
/// shadow, each with a way to take it off.
fn stroke_panel(ui: Ui, open: Signal<bool>) -> AnyView {
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

/// A marker as the editor shows it, for the cheat sheet: the kind's stripe
/// and glyph, then its markup, on the editor's chip fill.
fn marker_chip(
    look: &MarkerLook,
    kind: ChipKind,
    icon: nana_ui::runtime::Icon,
    markup: &str,
) -> AnyView {
    let ink = c(look.text);
    let mut glyph = IconGlyph::new(icon).size(icon::SM).decorative();
    glyph.style.foreground = None;
    Arc::make_mut(&mut glyph.style.layout).color = Some(ink);
    let mut st = Type::Overline.style();
    {
        let l = Arc::make_mut(&mut st.layout);
        l.color = Some(ink);
        l.white_space_nowrap = true;
    }
    let fill = c(look.fill);
    let hue = c(kind.hue());
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::TIGHT)
            .align(AlignSpec::Center)
            .radius(nana_ui_core::RadiusTier::Xs)
            .with_layout(move |l| {
                l.height = Some(LengthSpec::Px(crate::ui::kit::keycap::H));
                l.flex_shrink = Some(0.0);
                l.background = Some(fill);
                l.padding_right = Some(LengthSpec::Px(inset::CHIP_X));
                l.overflow_x = nana_ui_core::OverflowSpec::Hidden;
                l.overflow_y = nana_ui_core::OverflowSpec::Hidden;
            }),
    )
    .children((
        widget(Stack::row(0.0).with_layout(move |l| {
            l.width = Some(LengthSpec::Px(CHIP_STRIPE));
            l.height = Some(LengthSpec::Px(crate::ui::kit::keycap::H));
            l.flex_shrink = Some(0.0);
            l.background = Some(hue);
        })),
        widget(glyph),
        widget(Text::new(markup).decorative().style(st)),
    ))
    .into_any()
}

/// One line of the cheat sheet: a fixed-width lead (a chip or keycaps) and
/// what it does.
fn sheet_row(lead: AnyView, lead_w: f32, meaning: String) -> AnyView {
    let mut st = Type::Caption.style();
    Arc::make_mut(&mut st.layout).white_space_nowrap = true;
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::CONTROL)
            .align(AlignSpec::Center)
            .with_layout(|l| {
                l.height = Some(LengthSpec::Px(crate::ui::kit::keycap::H));
            }),
    )
    .children((
        widget(
            Stack::row(0.0)
                .align(AlignSpec::Center)
                .with_layout(move |l| {
                    l.width = Some(LengthSpec::Px(lead_w));
                    l.min_width = Some(LengthSpec::Px(lead_w));
                    l.flex_shrink = Some(0.0);
                }),
        )
        .children(lead),
        widget(Text::new(meaning).style(st).color(SemanticColorRole::Muted)),
    ))
    .into_any()
}

/// The `?` sheet: the markers the editor shows as chips with what each does,
/// and the input window's own keys.
pub fn cheat_sheet(ui: Ui) -> AnyView {
    let look = super::rich::MentionLook::for_ui(ui.chrome_dark.get_untracked()).markers;
    let t = |k: &str| ui.now_t(k);
    let markers: Vec<AnyView> = [
        (ChipKind::Wait, ti::CLOCK, "[wait=500]", "cheat-wait"),
        (
            ChipKind::Continue,
            ti::PLAYER_PLAY,
            "[pause]",
            "cheat-pause",
        ),
        (ChipKind::Page, ti::PAGE_BREAK, "[page]", "cheat-page"),
        (
            ChipKind::Expression,
            ti::MOOD_SMILE,
            "[expr=…]",
            "cheat-expr",
        ),
        (ChipKind::Sound, ti::MUSIC, "[se=…]", "cheat-se"),
        (
            ChipKind::Picture,
            ti::STICKER,
            "[sticker=…]",
            "cheat-sticker",
        ),
        (ChipKind::Mention, ti::AT, "[at=…]", "cheat-at"),
    ]
    .into_iter()
    .map(|(kind, icon, markup, key)| {
        sheet_row(
            marker_chip(&look, kind, icon, markup),
            SHEET_MARKERS_W,
            t(key),
        )
    })
    .collect();
    // The keys as bound in Settings › Hotkeys; unbound ones are left out.
    let input = ui.settings.with_untracked(|s| s.hotkeys.input.clone());
    let bound = |k: InputKey| k.get(&input).map(|h| h.label(super::META_LABEL));
    let row = |k: InputKey, meaning: &str| {
        bound(k).map(|h| sheet_row(super::kit::keycap::chord(&h), SHEET_KEYS_W, t(meaning)))
    };
    let recall = match (bound(InputKey::RecallPrev), bound(InputKey::RecallNext)) {
        (Some(prev), Some(next)) => Some(
            widget(
                Stack::row(0.0)
                    .gap_step(gap::step::TIGHT)
                    .align(AlignSpec::Center),
            )
            .children((
                super::kit::keycap::chord(&prev),
                widget(
                    Text::new("/")
                        .decorative()
                        .style(Type::Small.style())
                        .color(SemanticColorRole::Muted),
                ),
                super::kit::keycap::chord(&next),
            ))
            .into_any(),
        ),
        (one, other) => one.or(other).map(|h| super::kit::keycap::chord(&h)),
    }
    .map(|lead| sheet_row(lead, SHEET_KEYS_W, t("cheat-recall")));
    let keys: Vec<AnyView> = [
        row(InputKey::Send, "tb-send"),
        row(InputKey::Newline, "cheat-newline"),
        row(InputKey::Bold, "tb-bold"),
        row(InputKey::Italic, "tb-italic"),
        row(InputKey::Underline, "tb-underline"),
        recall,
        row(InputKey::Hide, "cheat-hide"),
        // The `@` / `/` list's own key; not rebindable.
        Some(sheet_row(
            super::kit::keycap::chord("Tab"),
            SHEET_KEYS_W,
            t("cheat-complete"),
        )),
    ]
    .into_iter()
    .flatten()
    .collect();
    let column = |title: String, rows: Vec<AnyView>| {
        widget(
            Stack::column(0.0)
                .gap_step(gap::step::TIGHT)
                .width(LengthSpec::Shrink),
        )
        .children((caption(title), rows))
        .into_any()
    };
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::SECTION)
            .padding_step(gap::step::TIGHT, gap::step::TIGHT),
    )
    .children((
        column(t("cheat-markers"), markers),
        column(t("cheat-shortcuts"), keys),
    ))
    .into_any()
}

/// A hairline between two groups of toolbar tools, 16px tall.
/// Input windows narrower than this fold their low-frequency tools into "…".
const COMPACT_BELOW: f32 = 820.0;

fn toolbar(ui: Ui) -> AnyView {
    let available_width = signal(920.0_f32);
    let key = move || {
        (
            ui.locale.get(),
            ui.chrome_dark.get(),
            available_width.get(),
            ui.skin.with(|s| Arc::as_ptr(s) as usize),
            ui.settings.with(|s| {
                (
                    s.general.input_mode,
                    s.hotkeys.input.clone(),
                    s.assets.phrases.len(),
                )
            }),
        )
    };
    dynamic(key, move |_| {
        let [
            color_open,
            size_open,
            font_open,
            align_open,
            ruby_open,
            fx_open,
            pace_open,
            insert_open,
            sticker_open,
            expr_open,
            phrase_open,
            stroke_open,
            help_open,
        ] = std::array::from_fn(|_| signal(false));
        let t = |k: &str| ui.now_t(k);
        let skin = ui.skin.get_untracked();
        let settings = ui.settings.get_untracked();
        let effects: Vec<AnyView> = [
            (ContinuousKind::Shake, "tb-effect-shake"),
            (ContinuousKind::Wave, "tb-effect-wave"),
            (ContinuousKind::Jump, "tb-effect-jump"),
            (ContinuousKind::Rainbow, "tb-effect-rainbow"),
            (ContinuousKind::Pulse, "tb-effect-pulse"),
            (ContinuousKind::Flicker, "tb-effect-flicker"),
        ]
        .into_iter()
        .map(|(kind, key)| item(t(key), fx_open, move || act(Tool::Effect(kind))))
        .chain([
            item(t("tb-emphasis"), fx_open, || act(Tool::Emphasis)),
            widget(Divider::horizontal()).into_any(),
            item(t("tb-entrance-fade"), fx_open, || {
                act(Tool::Entrance(EntranceKind::Fade))
            }),
            item(t("tb-entrance-pop"), fx_open, || {
                act(Tool::Entrance(EntranceKind::Pop))
            }),
            item(t("tb-entrance-slide"), fx_open, || {
                act(Tool::Entrance(EntranceKind::SlideUp))
            }),
        ])
        .collect();
        let pacing: Vec<AnyView> = [250u32, 500, 1000]
            .into_iter()
            .map(|ms| {
                let label = I18N.tr_in(
                    ui.locale.get_untracked(),
                    "tb-wait-ms",
                    &[("ms", ms.into())],
                );
                item(label, pace_open, move || marker(Marker::Wait { ms }))
            })
            .chain([
                widget(Divider::horizontal()).into_any(),
                item(t("tb-speed-slow"), pace_open, || {
                    act(Tool::Pace(Pace::Scale(2.0)))
                }),
                item(t("tb-speed-fast"), pace_open, || {
                    act(Tool::Pace(Pace::Scale(0.5)))
                }),
                item(t("tb-speed-instant"), pace_open, || {
                    act(Tool::Pace(Pace::Instant))
                }),
                item(t("tb-speed-reset"), pace_open, || {
                    act(Tool::Pace(Pace::Inherit))
                }),
                widget(Divider::horizontal()).into_any(),
                item(t("tb-pause"), pace_open, || {
                    marker(Marker::Pause { auto_ms: None })
                }),
                item(t("tb-pause-auto"), pace_open, || {
                    marker(Marker::Pause {
                        auto_ms: Some(1500),
                    })
                }),
                item(t("tb-page"), pace_open, || marker(Marker::Page)),
            ])
            .collect();
        let shake = skin.effects.screen_shake_px;
        let mut inserts: Vec<AnyView> = vec![
            widget(ActionMenuItem::new(t("tb-image")))
                .bind(move |item: &mut ActionMenuItem| {
                    item.disabled = super::sources::plain_text_only(ui)
                })
                .on_cx(move |_, _: &Activate, cx| {
                    insert_open.set(false);
                    cx.dispatch_program_all(Msg::PickFile(FileTarget::InsertImage));
                })
                .into_any(),
            item(t("tb-screenshake"), insert_open, move || {
                marker(Marker::ScreenShake {
                    amp: shake,
                    ms: 300,
                })
            }),
        ];
        let se: Vec<String> = skin.sounds.se.keys().cloned().collect();
        if !se.is_empty() {
            inserts.push(widget(Divider::horizontal()).into_any());
            for name in se {
                let label = format!("{} · {}", t("tb-sound"), name);
                inserts.push(item(label, insert_open, move || {
                    marker(Marker::Sound {
                        sound: name.clone(),
                        gain: 1.0,
                    })
                }));
            }
        }
        let phrases: Vec<AnyView> = if settings.assets.phrases.is_empty() {
            // The menu's empty state: ✦ and one line, then the way to add some.
            vec![
                widget(
                    ActionMenuItem::new(t("empty-phrases"))
                        .leading(ti::SPARKLES)
                        .disabled(true),
                )
                .into_any(),
                item(t("empty-open-assets"), phrase_open, || {
                    Msg::SettingsPage(super::settings::Page::Assets)
                }),
            ]
        } else {
            settings
                .assets
                .phrases
                .iter()
                .enumerate()
                .map(|(i, p)| item(p.label.clone(), phrase_open, move || Msg::InsertPhrase(i)))
                .collect()
        };
        // Below this width the low-frequency tools fold into a "…" menu so
        // Send and the mode switch always stay on screen.
        let input_width = available_width.get_untracked();
        let compact = input_width < COMPACT_BELOW;
        let narrow = input_width < 640.0;
        let [more_open] = std::array::from_fn(|_| signal(false));
        let open_item = |label: String, target: Signal<bool>| -> AnyView {
            widget(ActionMenuItem::new(label))
                .on_cx(move |_, _: &Activate, _| {
                    more_open.set(false);
                    target.set(true);
                })
                .into_any()
        };
        let divider = crate::ui::kit::vrule;
        // Text styling, then colour and size, then the outline and ruby,
        // then what plays (effects, pacing) and what goes in.
        let mut bar: Vec<AnyView> = vec![
            toggle(
                ui,
                ti::BOLD,
                "tb-bold",
                Some("Ctrl+B"),
                || Tool::Bold,
                is_bold,
            ),
            toggle(
                ui,
                ti::ITALIC,
                "tb-italic",
                Some("Ctrl+I"),
                || Tool::Italic,
                is_italic,
            ),
            toggle(
                ui,
                ti::UNDERLINE,
                "tb-underline",
                Some("Ctrl+U"),
                || Tool::Underline,
                is_underline,
            ),
            toggle(
                ui,
                ti::STRIKETHROUGH,
                "tb-strike",
                None,
                || Tool::Strike,
                is_strike,
            ),
            divider(),
            popover(
                ti::PALETTE,
                t("tb-color"),
                crate::ui::kit::popover::MD,
                color_open,
                color_panel(ui, color_open),
            ),
            popover(
                ti::TEXT_SIZE,
                t("tb-size"),
                crate::ui::kit::popover::MD,
                size_open,
                size_panel(ui, size_open),
            ),
            popover(
                ti::TYPOGRAPHY,
                t("tb-font"),
                crate::ui::kit::popover::LG,
                font_open,
                font_panel(ui, font_open),
            ),
            popover(
                ti::ALIGN_CENTER,
                t("tb-align"),
                crate::ui::kit::popover::SM,
                align_open,
                alignment_panel(ui, align_open),
            ),
        ];
        let stroke = popover(
            ti::BORDER_OUTER,
            t("tb-stroke"),
            crate::ui::kit::popover::MD,
            stroke_open,
            stroke_panel(ui, stroke_open),
        );
        let ruby = popover(
            ti::LANGUAGE_HIRAGANA,
            t("tb-ruby"),
            crate::ui::kit::popover::MD,
            ruby_open,
            ruby_panel(ui, ruby_open),
        );
        let sticker = popover(
            ti::STICKER,
            t("tb-sticker"),
            crate::ui::kit::popover::XL,
            sticker_open,
            sticker_panel(ui, sticker_open),
        );
        let expression = popover(
            ti::MOOD_SMILE,
            t("tb-expression"),
            crate::ui::kit::popover::LG,
            expr_open,
            expression_panel(ui, expr_open),
        );
        let help = popover(
            ti::HELP,
            t("tb-cheatsheet"),
            crate::ui::kit::popover::SHEET,
            help_open,
            cheat_sheet(ui),
        );
        let effects_menu = menu(ti::SPARKLES, t("tb-effects"), fx_open, effects);
        let pacing_menu = menu(ti::HOURGLASS, t("tb-pacing"), pace_open, pacing);
        // The popovers of folded tools stay mounted, invisible and out of the
        // flow, so the "…" menu can open them in place.
        let mut hidden: Vec<AnyView> = Vec::new();
        if narrow {
            hidden.extend(bar.drain(1..));
        }
        if compact {
            // Clearing formats folds too, so the menus fit beside Send in
            // every language.
            if narrow {
                hidden.extend([effects_menu, pacing_menu]);
            } else {
                bar.extend([divider(), effects_menu, pacing_menu]);
            }
            let mut items: Vec<AnyView> = vec![
                widget(ActionMenuItem::new(t("tb-clear-format")))
                    .on_cx(move |_, _: &Activate, cx| {
                        more_open.set(false);
                        cx.dispatch_program_all(act(Tool::ClearFormat));
                    })
                    .into_any(),
                open_item(t("tb-stroke"), stroke_open),
                open_item(t("tb-ruby"), ruby_open),
                open_item(t("tb-sticker"), sticker_open),
                open_item(t("tb-expression"), expr_open),
                widget(ActionMenuItem::new(t("tb-source")))
                    .on_cx(move |_, _: &Activate, cx| {
                        more_open.set(false);
                        cx.dispatch_program_all(Msg::ToggleSource);
                    })
                    .into_any(),
                widget(Divider::horizontal()).into_any(),
            ];
            if narrow {
                for (key, tool) in [
                    ("tb-bold", Tool::Bold),
                    ("tb-italic", Tool::Italic),
                    ("tb-underline", Tool::Underline),
                    ("tb-strike", Tool::Strike),
                ] {
                    items.push(item(t(key), more_open, move || act(tool.clone())));
                }
                items.extend([
                    open_item(t("tb-color"), color_open),
                    open_item(t("tb-size"), size_open),
                    open_item(t("tb-font"), font_open),
                    open_item(t("tb-align"), align_open),
                    open_item(t("tb-effects"), fx_open),
                    open_item(t("tb-pacing"), pace_open),
                    open_item(t("tb-cheatsheet"), help_open),
                ]);
            }
            items.extend(inserts);
            bar.push(menu(ti::DOTS, t("tb-more"), more_open, items));
            if narrow {
                hidden.push(help);
            } else {
                bar.push(help);
            }
            hidden.extend([stroke, ruby, sticker, expression]);
        } else {
            bar.extend([stroke, ruby, divider()]);
            bar.extend([effects_menu, pacing_menu, sticker, expression]);
            bar.push(menu(ti::TEXT_PLUS, t("tb-insert"), insert_open, inserts));
            bar.push(tool(ui, ti::CLEAR_FORMATTING, "tb-clear-format", || {
                act(Tool::ClearFormat)
            }));
            bar.push(help);
        }
        bar.push(widget(Stack::spacer()).into_any());
        if !hidden.is_empty() {
            bar.push(
                widget(Stack::row(0.0).with_layout(|l| {
                    l.width = Some(LengthSpec::Px(0.0));
                    l.height = Some(LengthSpec::Px(0.0));
                    l.flex_shrink = Some(0.0);
                    l.overflow_x = OverflowSpec::Hidden;
                    l.overflow_y = OverflowSpec::Hidden;
                }))
                .children(hidden)
                .into_any(),
            );
        }
        bar.push(menu(ti::MESSAGE, t("tb-phrases"), phrase_open, phrases));
        bar.push(skin_preview_toggle(ui));
        bar.push(source_toggle(ui));
        widget(
            Stack::bar(0.0)
                .gap_step(gap::step::HAIR)
                .height(LengthSpec::Px(TOOLBAR_H))
                .padding_step(inset::step::BAR_Y, nana_ui_core::SpacingStep::Sm)
                .align(AlignSpec::Center)
                .with_layout(|l| l.flex_shrink = Some(0.0)),
        )
        .key("input-toolbar")
        .on(move |e: &SizeChanged| {
            if available_width.get_untracked() != e.width {
                available_width.set(e.width);
            }
        })
        .children(bar)
        .into_any()
    })
    .into_any()
}

fn status_content(ui: Ui) -> AnyView {
    enum Line {
        Error(String),
        Queued(String),
        Empty,
    }
    // Notices show as toasts above this line.
    let key = move || {
        let status = ui.status.get();
        let diag = ui.diagnostics.with(|d| d.first().cloned());
        (diag, status.queued, ui.locale.get())
    };
    let line = dynamic(key, move |k| {
        let (diag, queued, locale) = k.clone();
        let line = if let Some(d) = diag {
            Line::Error(d)
        } else if queued > 0 {
            Line::Queued(I18N.tr_in(locale, "status-queued-chip", &[("position", queued.into())]))
        } else {
            Line::Empty
        };
        match line {
            Line::Error(text) => widget(
                Text::new(text)
                    .style(Type::Small.style())
                    .truncating()
                    .color(SemanticColorRole::Danger),
            )
            .into_any(),
            // Live and attention states take the highlight, not the accent.
            Line::Queued(text) => crate::ui::kit::badge(text, Tone::Accent),
            Line::Empty => widget(Stack::row(0.0)).into_any(),
        }
    })
    // A chip or an error arrives on the `enter` token.
    .transition(super::motion::enter());
    widget(Stack::bar(0.0).align(AlignSpec::Center).with_layout(|l| {
        l.width = Some(LengthSpec::Px(0.0));
        l.min_width = Some(LengthSpec::Px(0.0));
        l.flex_grow = Some(1.0);
    }))
    .key("input-status")
    .children((
        widget(Stack::row(0.0).with_layout(|l| {
            l.width = Some(LengthSpec::Px(0.0));
            l.flex_grow = Some(1.0);
            l.min_width = Some(LengthSpec::Px(0.0));
        }))
        .visible(move || ui.reply.with(Option::is_none))
        .children(line),
        widget(Stack::spacer())
            .visible(move || ui.reply.with(Option::is_some))
            .children(reply_bar(ui)),
    ))
    .into_any()
}

/// "Replying to": the quoted chat message the next line answers, with a
/// button that drops it.
fn reply_bar(ui: Ui) -> AnyView {
    dynamic(
        move || (ui.reply.get(), ui.chrome_dark.get()),
        move |(reply, _)| {
            let Some(reply) = reply else {
                return widget(Stack::row(0.0)).into_any();
            };
            let mut label = Type::Overline.style();
            {
                let l = Arc::make_mut(&mut label.layout);
                l.white_space_nowrap = true;
                l.flex_shrink = Some(1.0);
                l.min_width = Some(LengthSpec::Px(0.0));
                l.max_width = Some(LengthSpec::Percent(45.0));
            }
            // The accent, darkened where it would not read as text.
            label.foreground = Some(SemanticColorRole::AccentOnSoft);
            let mut body = Type::Caption.style();
            body.foreground = Some(SemanticColorRole::Muted);
            widget(
                Stack::row(0.0)
                    .gap_step(gap::step::TIGHT)
                    .align(AlignSpec::Center)
                    .width(LengthSpec::Fill),
            )
            .children((
                widget(
                    Text::new(I18N.tr_in(
                        ui.locale.get_untracked(),
                        "chat-replying",
                        &[("name", reply.user.name.as_str().into())],
                    ))
                    .style(label)
                    .truncating(),
                ),
                super::danmaku::asset_avatar(
                    *ui,
                    reply.user.avatar.as_ref().map(ToString::to_string),
                    &reply.user.name,
                    REPLY_AVATAR,
                    crate::ui::kit::theme::palette(ui.chrome_dark.get())
                        .accent
                        .as_rgba_array(),
                ),
                widget(Stack::row(0.0).with_layout(|l| {
                    l.flex_grow = Some(1.0);
                    l.flex_shrink = Some(1.0);
                    l.min_width = Some(LengthSpec::Px(0.0));
                }))
                .children(widget(
                    Text::new(reply.text.clone()).style(body).truncating(),
                )),
                widget(
                    IconButton::new(ti::X, ui.now_t("chat-reply-cancel"))
                        .size(ControlSize::Small)
                        .with_tooltip(ui.now_t("chat-reply-cancel")),
                )
                .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(Msg::ClearReply)),
            ))
            .into_any()
        },
    )
    .into_any()
}

/// Localized editor commands, in `TextEditAction::ALL` order.
pub fn editor_context_menu(
    ui: Ui,
    x: f32,
    y: f32,
    enabled: [bool; 6],
) -> nana_ui::runtime::ContextMenu {
    use nana_ui::runtime::{ContextMenu, ContextMenuItem, TextEditAction};
    let labels = [
        "edit-undo",
        "edit-redo",
        "edit-cut",
        "edit-copy",
        "edit-paste",
        "edit-select-all",
    ];
    let hints = if cfg!(target_os = "macos") {
        ["⌘Z", "⇧⌘Z", "⌘X", "⌘C", "⌘V", "⌘A"]
    } else {
        ["Ctrl+Z", "Ctrl+Y", "Ctrl+X", "Ctrl+C", "Ctrl+V", "Ctrl+A"]
    };
    ContextMenu::new(x, y).items(
        TextEditAction::ALL
            .into_iter()
            .enumerate()
            .map(|(i, action)| {
                ContextMenuItem::new(action.key(), ui.now_t(labels[i]))
                    .hint(hints[i])
                    .disabled(!enabled[i])
            }),
    )
}

/// The draft inside the dialogue box, its events sent to the session.
fn editor_area(
    ui: Ui,
    editor: EntityRef<RichTextEditor>,
    source: EntityRef<TextArea>,
    focused: Signal<bool>,
    inert: bool,
) -> AnyView {
    editor_view::editor_area(*ui, editor, source, focused, inert, |event| {
        Some(match event {
            EditorEvent::Context {
                source,
                x,
                y,
                keyboard,
            } => Msg::EditorContext {
                source,
                x,
                y,
                keyboard,
            },
            EditorEvent::Changed(rich) => Msg::EditorChanged(rich),
            EditorEvent::Selection(selection) => Msg::EditorSelection(selection),
            EditorEvent::Paste => Msg::EditorPaste,
            EditorEvent::Typed { text, selection } => {
                Msg::Markup(MarkupEdit::Typed { text, selection })
            }
        })
    })
}

/// While the draft has focus, a 2px ring in the focus accent at 40% just
/// inside the box's region, outside the skin's own border: the input window
/// shows where typing goes, and the display (which has no ring) stays clean.
fn focus_ring(focused: Signal<bool>) -> AnyView {
    widget(
        Stack::column(0.0)
            .outline(SemanticColorRole::FocusBorder, 2.0)
            .radius(nana_ui_core::RadiusTier::Sm)
            .with_layout(|l| {
                l.position = PositionSpec::Absolute;
                l.offset_left = Some(LengthSpec::Px(FOCUS_INSET));
                l.offset_top = Some(LengthSpec::Px(FOCUS_INSET));
                l.offset_right = Some(LengthSpec::Px(FOCUS_INSET));
                l.offset_bottom = Some(LengthSpec::Px(FOCUS_INSET));
                l.opacity = Some(0.4);
                l.pointer_events = Some(PointerEventsSpec::None);
            }),
    )
    .visible(move || focused.get())
    .into_any()
}

/// The input window. Returns the editor and the source field for key
/// handling, and the stack the host shows toasts in.
pub fn input_window(ui: Ui) -> WithRefs<impl IntoView, (EditorRef, SourceRef, ToastsRef)> {
    let editor = entity_ref::<RichTextEditor>();
    let source = entity_ref::<TextArea>();
    let toasts = entity_ref::<nana_ui::runtime::ToastStack>();
    let focused = signal(false);
    let viewport = signal((0.0_f32, 0.0_f32));
    let is_preview = move || ui.settings.with(|s| s.general.input_skin_preview);
    let canvas_size = move || {
        if !is_preview() {
            return viewport.get();
        }
        let skin = ui.skin.get();
        let s = ui.settings.get();
        crate::windows::size_of(crate::windows::WindowKind::Display, &skin, &s)
    };
    // Fit only the preview presentation. The editor still lays out at the
    // stream canvas size, so resizing the sidebar cannot change line breaks.
    let preview_scale = move || {
        if !is_preview() {
            return 1.0;
        }
        let (w, _) = canvas_size();
        ((viewport.get().0 - 32.0).max(1.0) / w.max(1.0)).min(1.0)
    };
    let preview = widget(ScrollView::new(ScrollAxes::Both).with_layout(|l| {
        l.width = Some(LengthSpec::Fill);
        l.height = Some(LengthSpec::Px(0.0));
        l.min_height = Some(LengthSpec::Px(0.0));
        l.min_width = Some(LengthSpec::Px(0.0));
        l.flex_grow = Some(1.0);
        l.flex_shrink = Some(1.0);
    }))
    .key("input-preview-scroll")
    .bind(move |s: &mut ScrollView| {
        s.scrollbars = Some(if is_preview() {
            nana_ui_core::ScrollbarVisibility::Always
        } else {
            nana_ui_core::ScrollbarVisibility::Hidden
        });
    })
    .on(move |e: &ScrollViewportChanged| {
        let size = (e.width, e.height);
        if viewport.get_untracked() != size {
            viewport.set(size);
        }
    })
    .children(
        widget(
            Stack::column(0.0)
                .align(AlignSpec::Center)
                .justify(JustifySpec::Center),
        )
        .bind(move |s: &mut Stack| {
            let (w, h) = canvas_size();
            let (vw, vh) = viewport.get();
            let scale = preview_scale();
            let pad = if is_preview() { 32.0 } else { 0.0 };
            *s = s.clone().surface(if is_preview() {
                SemanticColorRole::Subtle
            } else {
                SemanticColorRole::Background
            });
            let l = s.layout_mut();
            l.width = Some(LengthSpec::Px(vw.max(w * scale + pad)));
            l.height = Some(LengthSpec::Px(vh.max(h * scale + pad)));
            l.flex_shrink = Some(0.0);
        })
        .children(
            widget(Stack::column(0.0).with_layout(|l| {
                l.position = PositionSpec::Relative;
                l.flex_shrink = Some(0.0);
                // The unscaled child must not enlarge the scrollable area.
                l.overflow_x = OverflowSpec::Hidden;
                l.overflow_y = OverflowSpec::Hidden;
            }))
            .key("input-preview-fit")
            .bind(move |s: &mut Stack| {
                let (w, h) = canvas_size();
                let scale = preview_scale();
                let l = s.layout_mut();
                l.width = Some(LengthSpec::Px(w * scale));
                l.height = Some(LengthSpec::Px(h * scale));
            })
            .children(
                widget(Stack::column(0.0).with_layout(|l| {
                    l.position = PositionSpec::Absolute;
                    l.offset_left = Some(LengthSpec::Px(0.0));
                    l.offset_top = Some(LengthSpec::Px(0.0));
                    l.flex_shrink = Some(0.0);
                }))
                .key("input-preview-canvas")
                .bind(move |s: &mut Stack| {
                    let (w, h) = canvas_size();
                    let l = s.layout_mut();
                    l.width = Some(LengthSpec::Px(w));
                    l.height = Some(LengthSpec::Px(h));
                    l.transform = is_preview()
                        .then(|| super::surface::affine(0.0, 0.0, 0.0, preview_scale()));
                    l.transform_origin = Some(nana_ui_core::TransformOrigin {
                        x: LengthSpec::Px(0.0),
                        y: LengthSpec::Px(0.0),
                    });
                })
                .on(move |e: &FocusWithinChanged| {
                    if focused.get_untracked() != e.focused {
                        focused.set(e.focused);
                    }
                })
                .children((
                    dialogue_box(
                        *ui,
                        Source::Draft,
                        Some(editor_area(ui, editor, source, focused, false)),
                    ),
                    focus_ring(focused),
                )),
            ),
        ),
    );
    let invitation = signal(String::new());
    let relay = signal(String::new());
    let view = widget(
        Stack::fill_column(0.0)
            .surface(SemanticColorRole::Background)
            .with_layout(|l| {
                l.position = PositionSpec::Relative;
                l.overflow_x = OverflowSpec::Hidden;
                l.overflow_y = OverflowSpec::Hidden;
            }),
    )
    .key("input-window")
    .on(move |event: &SizeChanged| {
        if ui.input_width.get_untracked() != event.width {
            ui.input_width.set(event.width);
        }
        if ui.input_height.get_untracked() != event.height {
            ui.input_height.set(event.height);
        }
    })
    .children((
        workspace::split(
            ui,
            widget(Stack::fill_column(0.0).with_layout(|l| {
                l.min_width = Some(LengthSpec::Px(0.0));
            }))
            .children((
                toolbar(ui),
                crate::ui::kit::rule(),
                preview,
                super::plugins::mounted(ui, nanatype_plugins::Mount::Input),
            )),
        ),
        workspace::footer(ui, invitation, relay),
        completion::popup(ui),
        super::feedback::toast_stack(toasts, crate::windows::INPUT_ACTIONS_H + 4.0, 2),
    ));
    with_refs(view, (editor, source, toasts))
}
/// Read the split model after native pointer/keyboard routing, before layout.
pub(crate) fn changed_sidebar_width(ui: Ui, cx: &nana_ui::runtime::AppContext) -> Option<f32> {
    workspace::changed_width(ui, cx)
}

/// Selection-only navigation in NanaUI's plain TextArea emits no TextChanged.
/// Read its retained caret before a toolbar or another window inserts markup.
pub fn capture_source_selection(
    ui: Ui,
    cx: &nana_ui::runtime::AppContext,
    source: nana_ui::runtime::Entity<TextArea>,
) {
    if !ui.source_mode.get_untracked() {
        return;
    }
    let Some(state) = cx.world().text_input(source.stable_id()) else {
        return;
    };
    let mut current = ui.source.get_untracked();
    let selection = (state.selection.anchor, state.selection.focus);
    if current.text == state.value && current.selection != selection {
        current.selection = selection;
        ui.source.set(current);
    }
}

pub type EditorRef = EntityRef<RichTextEditor>;
pub type SourceRef = EntityRef<TextArea>;
pub type ToastsRef = EntityRef<nana_ui::runtime::ToastStack>;

#[cfg(test)]
mod tests {
    use super::*;
    use nana_ui::runtime::{DocumentId, RuntimeDocument};
    use nana_ui_devtools::agent::RuntimeAgentSession;
    use nanatype_i18n::Locale;

    #[test]
    fn toolbar_controls_share_the_compact_height_and_vertical_center() {
        let skin = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"))
            .load("nanatype.halo")
            .unwrap()
            .skin;
        for locale in [Locale::ZhCn, Locale::EnUs, Locale::JaJp] {
            for width in [720, 796, 920, 1100, 1200] {
                for mode in [InputMode::Compose, InputMode::Mirror] {
                    for scale in [1.0, 1.5, 2.0] {
                        let mut settings = Settings::default();
                        settings.general.input_mode = mode;
                        settings.skin.overrides.region = Some([width as f32, 200.0]);
                        let ui = Ui::new(Arc::new(settings), skin.clone(), locale);
                        let id = DocumentId::new(1).unwrap();
                        let mut document = RuntimeDocument::new(id);
                        document
                            .context_mut()
                            .set_theme_tokens(crate::ui::kit::theme::compile(false))
                            .unwrap();
                        document
                            .context_mut()
                            .mount_view_root(id, move || toolbar(ui))
                            .unwrap();
                        let mut session = RuntimeAgentSession::new_scaled(
                            document,
                            width,
                            TOOLBAR_H as u32,
                            scale,
                        )
                        .unwrap();
                        let nodes = session.accessibility_dump();
                        let controls = [
                            "tb-bold",
                            "tb-italic",
                            "tb-color",
                            "tb-size",
                            "tb-font",
                            "tb-align",
                            "tb-effects",
                            "tb-pacing",
                            "tb-phrases",
                            "tb-source",
                        ]
                        .map(|key| {
                            let label = ui.now_t(key);
                            nodes
                                .iter()
                                .find(|node| node.label.as_deref() == Some(label.as_str()))
                                .unwrap_or_else(|| panic!("missing toolbar control {label}"))
                        });
                        for node in controls {
                            let bounds = &node.bounds;
                            assert!(
                                (bounds.height - TARGET).abs() <= 1.0 / scale,
                                "{locale:?}, width={width}, {mode:?}, {scale}x: {node:?}"
                            );
                            assert!(
                                (bounds.y + bounds.height / 2.0 - TOOLBAR_H / 2.0).abs()
                                    <= 1.0 / scale,
                                "{locale:?}, width={width}, {mode:?}, {scale}x: {node:?}"
                            );
                            assert!(
                                bounds.width > 0.0
                                    && bounds.x >= 0.0
                                    && bounds.x + bounds.width <= width as f32 + 1.0 / scale,
                                "toolbar control clipped: {locale:?}, width={width}: {node:?}"
                            );
                        }
                        if locale == Locale::ZhCn
                            && mode == InputMode::Compose
                            && let Some(out) = std::env::var_os("NANATYPE_TOOLBAR_CAPTURE_DIR")
                        {
                            let out = std::path::PathBuf::from(out);
                            std::fs::create_dir_all(&out).unwrap();
                            session
                                .screenshot_png(out.join(format!("toolbar-{width}-{scale}x.png")))
                                .unwrap();
                        }
                    }
                }
            }
        }
    }
}
