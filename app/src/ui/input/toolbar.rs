use super::cheat_sheet::cheat_sheet;
use super::panels::{
    alignment_panel, color_face, color_panel, font_panel, ruby_panel, size_panel, stroke_panel,
};
use super::*;
use crate::windows::TOOLBAR_H;

/// Toolbar glyphs: one size for every tool, centred in a 28px target.
pub(super) const GLYPH: f32 = 14.0;
/// Icon-button hit target.
pub(super) const TARGET: f32 = 28.0;
/// Input windows narrower than this fold typography and outline tools into "…".
const COMPACT_BELOW: f32 = 560.0;

/// The side of its button a tool's tooltip hangs on.
///
/// Never NanaUI's default, which follows the cursor: the tip goes above the
/// pointer, and flips below only when the flipped tip fits across as well as
/// down. At the window's top-right corner (the rail's first icon, the chat
/// pin) neither side fits across, so the tip stays above, and being clamped
/// into the window puts it right under the pointer. The pointer is then over
/// the tip, not the button: the tip closes, the button is hovered again, the
/// tip reopens — it blinks on every move. A tip on a side of the button never
/// covers the button, so it cannot take the pointer from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum Tip {
    /// Under the button (above it where the window has no room): bars at the
    /// top of the window.
    Below,
    /// Over the button: bars at the bottom of the window.
    Above,
    /// To the button's left (right where there is no room): the rail at the
    /// window's right edge.
    Left,
}

impl Tip {
    fn config(self) -> nana_ui::TooltipConfig {
        use nana_ui::TooltipPlacement as P;
        nana_ui::TooltipConfig {
            placement: match self {
                Tip::Below => P::Bottom,
                Tip::Above => P::Top,
                Tip::Left => P::Left,
            },
            ..Default::default()
        }
    }
}

/// A toolbar icon button: the shared glyph size in a 28px square, in the
/// toolbar ink, its tooltip on the `side` given.
pub(super) fn tool_button(
    icon: nana_ui::runtime::Icon,
    label: String,
    tip: String,
    side: Tip,
) -> IconButton {
    // Large is the size whose glyph is GLYPH; the square stays 28px.
    let mut b = IconButton::new(icon, label)
        .size(ControlSize::Large)
        .tooltip(tip, side.config());
    b.style.square = Some(SquareSize::IconButton);
    b
}

/// "Bold · Ctrl+B": a tooltip with the tool's shortcut.
fn with_shortcut(label: String, keys: Option<&str>) -> String {
    match keys {
        Some(k) => format!("{label} · {k}"),
        None => label,
    }
}

/// An icon button with a tooltip that sends `msg()`.
pub(super) fn tool(
    ui: Ui,
    icon: nana_ui::runtime::Icon,
    label: &'static str,
    side: Tip,
    msg: impl Fn() -> Msg + Send + 'static,
) -> AnyView {
    widget(tool_button(icon, ui.now_t(label), ui.now_t(label), side))
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
        Tip::Below,
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
pub(in crate::ui) fn pressed_tool(
    icon: nana_ui::runtime::Icon,
    label: String,
    side: Tip,
    on: impl Fn() -> bool + Send + 'static,
    msg: impl Fn() -> Msg + Send + 'static,
) -> nana_ui::runtime::view::El<IconButton> {
    widget(tool_button(icon, label.clone(), label, side))
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
        Tip::Below,
        move || ui.source_mode.get(),
        || Msg::Editor(EditorMsg::ToggleSource),
    )
    .into_any()
}

/// Shows the draft as the skin draws it, or as plain text.
fn skin_preview_toggle(ui: Ui) -> AnyView {
    let on = move || ui.settings.with(|s| s.general.input_skin_preview);
    pressed_tool(
        ti::EYE,
        ui.now_t("input-skin-preview"),
        Tip::Below,
        on,
        move || {
            let on = !ui.settings.with_untracked(|s| s.general.input_skin_preview);
            super::super::settings::controls::edit(move |s| s.general.input_skin_preview = on)
        },
    )
    .key("input-skin-preview-toggle")
    .into_any()
}

/// A menu trigger's face: a [`tool_button`] that opens and closes `open`.
fn trigger_button(
    icon: nana_ui::runtime::Icon,
    label: String,
    tip: Tip,
    open: Signal<bool>,
) -> nana_ui::runtime::view::El<IconButton> {
    opens(tool_button(icon, label.clone(), label, tip), open)
}

/// `button` as the trigger of the popover `open` keeps. Its tooltip is put
/// away while the popover is open: it would hang over the surface, and as
/// the topmost overlay it would take the Escape meant to close the popover.
pub(super) fn opens(
    button: IconButton,
    open: Signal<bool>,
) -> nana_ui::runtime::view::El<IconButton> {
    let tooltip = button.tooltip.clone();
    widget(button)
        .bind(move |b: &mut IconButton| {
            let tip = if open.get() { None } else { tooltip.clone() };
            if b.tooltip != tip {
                b.tooltip = tip;
            }
        })
        .on(move |_: &Activate| {
            let next = !open.get_untracked();
            open.set(next);
        })
}

/// A popover whose open state the app keeps, so items can close it.
pub(in crate::ui) fn popover(
    icon: nana_ui::runtime::Icon,
    label: String,
    tip: Tip,
    width: f32,
    open: Signal<bool>,
    body: impl IntoView,
) -> AnyView {
    let face = trigger_button(icon, label.clone(), tip, open).into_any();
    popover_with(face, label, width, open, body)
}

/// [`popover`] behind its own face, which fills the trigger.
fn popover_with(
    face: AnyView,
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
            .compact_trigger(true)
            .bare_trigger(true),
    )
    .trigger(face)
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
pub(super) fn menu(
    icon: nana_ui::runtime::Icon,
    label: String,
    tip: Tip,
    open: Signal<bool>,
    items: Vec<AnyView>,
) -> AnyView {
    let mut menu = ActionMenu::new().trigger(label.clone()).bare_trigger(true);
    menu.popover.compact_trigger = true;
    widget(menu)
        .trigger(trigger_button(icon, label, tip, open))
        .bind(move |m: &mut ActionMenu| m.popover.open = open.get())
        .on(move |e: &PopoverToggled| open.set(e.open))
        .on(move |_: &PopoverClosed| open.set(false))
        .children(items)
        .into_any()
}

/// How the text looks: weight and lines, then colour, size, font and
/// alignment, then outline, ruby and clearing. Pacing, effects and anything
/// inserted live in the sidebar.
pub(super) fn toolbar(ui: Ui) -> AnyView {
    let available_width = signal(920.0_f32);
    let key = move || {
        (
            ui.locale.get(),
            ui.chrome_dark.get(),
            ui.chrome_contrast.get(),
            available_width.get() < COMPACT_BELOW,
            ui.skin.with(|s| Arc::as_ptr(s) as usize),
            ui.settings.with(|s| s.hotkeys.input.clone()),
        )
    };
    dynamic(key, move |(_, _, _, compact, _, _)| {
        let compact = *compact;
        let [
            color_open,
            size_open,
            font_open,
            align_open,
            ruby_open,
            stroke_open,
            help_open,
            more_open,
        ] = std::array::from_fn(|_| signal(false));
        let t = |k: &str| ui.now_t(k);
        let divider = crate::ui::kit::vrule;
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
        ];
        let typography = vec![
            popover_with(
                color_face(ui, color_open),
                t("tb-color"),
                crate::ui::kit::popover::MD,
                color_open,
                color_panel(ui, color_open),
            ),
            popover(
                ti::TEXT_SIZE,
                t("tb-size"),
                Tip::Below,
                crate::ui::kit::popover::MD,
                size_open,
                size_panel(ui, size_open),
            ),
            popover(
                ti::TYPOGRAPHY,
                t("tb-font"),
                Tip::Below,
                crate::ui::kit::popover::LG,
                font_open,
                font_panel(ui, font_open),
            ),
            popover(
                ti::ALIGN_CENTER,
                t("tb-align"),
                Tip::Below,
                crate::ui::kit::popover::SM,
                align_open,
                alignment_panel(ui, align_open),
            ),
        ];
        let outline = vec![
            popover(
                ti::BORDER_OUTER,
                t("tb-stroke"),
                Tip::Below,
                crate::ui::kit::popover::MD,
                stroke_open,
                stroke_panel(ui, stroke_open),
            ),
            popover(
                ti::LANGUAGE_HIRAGANA,
                t("tb-ruby"),
                Tip::Below,
                crate::ui::kit::popover::MD,
                ruby_open,
                ruby_panel(ui, ruby_open),
            ),
        ];
        let clear = || {
            tool(
                ui,
                ti::CLEAR_FORMATTING,
                "tb-clear-format",
                Tip::Below,
                || act(Tool::ClearFormat),
            )
        };
        if compact {
            // The popovers of folded tools stay mounted, invisible and out of
            // the flow, so the "…" menu can open them in place.
            let open_item = |label: String, target: Signal<bool>| -> AnyView {
                widget(ActionMenuItem::new(label))
                    .on_cx(move |_, _: &Activate, _| {
                        more_open.set(false);
                        target.set(true);
                    })
                    .into_any()
            };
            let items = vec![
                open_item(t("tb-color"), color_open),
                open_item(t("tb-size"), size_open),
                open_item(t("tb-font"), font_open),
                open_item(t("tb-align"), align_open),
                widget(Divider::horizontal()).into_any(),
                open_item(t("tb-stroke"), stroke_open),
                open_item(t("tb-ruby"), ruby_open),
                item(t("tb-clear-format"), more_open, || act(Tool::ClearFormat)),
            ];
            bar.push(menu(ti::DOTS, t("tb-more"), Tip::Below, more_open, items));
            bar.push(
                widget(Stack::row(0.0).with_layout(|l| {
                    l.width = Some(LengthSpec::Px(0.0));
                    l.height = Some(LengthSpec::Px(0.0));
                    l.flex_shrink = Some(0.0);
                    l.overflow_x = OverflowSpec::Hidden;
                    l.overflow_y = OverflowSpec::Hidden;
                }))
                .children((typography, outline))
                .into_any(),
            );
        } else {
            bar.extend(typography);
            bar.push(divider());
            bar.extend(outline);
            bar.push(clear());
        }
        bar.extend([
            widget(Stack::spacer()).into_any(),
            skin_preview_toggle(ui),
            source_toggle(ui),
            popover(
                ti::HELP,
                t("tb-cheatsheet"),
                Tip::Below,
                crate::ui::kit::popover::SHEET,
                help_open,
                cheat_sheet(ui),
            ),
        ]);
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
            for width in [480, 560, 720, 920, 1200] {
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
                        let expected: &[&str] = if (width as f32) < COMPACT_BELOW {
                            &[
                                "tb-bold",
                                "tb-strike",
                                "tb-more",
                                "tb-source",
                                "tb-cheatsheet",
                            ]
                        } else {
                            &[
                                "tb-bold",
                                "tb-italic",
                                "tb-color",
                                "tb-size",
                                "tb-font",
                                "tb-align",
                                "tb-stroke",
                                "tb-ruby",
                                "tb-clear-format",
                                "tb-source",
                                "tb-cheatsheet",
                            ]
                        };
                        let controls = expected.iter().map(|key| {
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
                        if locale == Locale::ZhCn && mode == InputMode::Compose {
                            // Symmetric glyphs on menu triggers: their ink is
                            // centred in the trigger, as an icon button's is.
                            let (size, rgba) = session.screenshot_rgba().unwrap();
                            let px = |x: u32, y: u32| {
                                let i = ((y * size.width + x) * 4) as usize;
                                rgba[i..i + 3].iter().map(|&c| u32::from(c)).sum::<u32>() / 3
                            };
                            for key in ["tb-more", "tb-color", "tb-cheatsheet"]
                                .into_iter()
                                .filter(|k| expected.contains(k))
                            {
                                let label = ui.now_t(key);
                                let Some(node) = nodes
                                    .iter()
                                    .find(|node| node.label.as_deref() == Some(label.as_str()))
                                else {
                                    continue;
                                };
                                let b = &node.bounds;
                                let to_px = |v: f32| (v * scale).round() as u32;
                                let (x0, x1) = (to_px(b.x), to_px(b.x + b.width).min(size.width));
                                let (y0, y1) = (to_px(b.y), to_px(b.y + b.height).min(size.height));
                                let bg = px(x0, y0);
                                let ink: Vec<u32> = (x0..x1)
                                    .filter(|&x| (y0..y1).any(|y| px(x, y).abs_diff(bg) > 40))
                                    .collect();
                                let (first, last) = (ink[0], ink[ink.len() - 1] + 1);
                                let ink_center = (first + last) as f32 / 2.0 / scale;
                                assert!(
                                    (ink_center - (b.x + b.width / 2.0)).abs() <= 1.0,
                                    "{key} glyph off-centre: width={width}, {scale}x: \
                                     ink centre {ink_center}, {node:?}"
                                );
                            }
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
