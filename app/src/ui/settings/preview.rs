//! Isolated, unsaved skin editing. Only Save dispatches an application message.
use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use nana_ui::runtime::view::{AnyView, IntoView, Signal, dynamic, on_mount, signal, widget};
use nana_ui::runtime::{
    Activate, Button, ColorChanged, ColorField, Icon, IconButton, NumberChanged, NumberInput,
    RangeChanged, RangeField, Select, SelectChanged, SelectOption, SizeChanged, Stack, Text,
    TextChanged, TextInput, TextInputState,
};
use nana_ui::{ButtonKind, ControlSize};
use nana_ui_core::{AlignSpec, LengthSpec, SemanticColorRole};
use nanatype_model::Rgba8;
use nanatype_settings::Settings;
use nanatype_theme::Skin;

use super::controls::{Type, hint, section};
use crate::msg::Msg;
use crate::ui::Ui;
use crate::ui::kit::{self, gap};
use nana_ui::icons_tabler as ti;
pub use nanatype_frame::ui::preview::{
    Draft, InvalidProfile, Pane, StageAssets, Surface, default_size, pair_extent, sample, stage,
    stage_pair, stage_pair_placeholder, stage_placeholder, stage_view,
};

/// The overscan of skin `id` as the catalogue loaded it.
fn overscan(ui: Ui, id: &str) -> Option<f32> {
    ui.skins.with_untracked(|rows| {
        rows.iter()
            .find(|r| r.id == id)
            .and_then(|r| r.skin.as_ref())
            .map(|s| s.dialogue.overscan)
    })
}

/// [`Draft::load`] with the overscan of the catalogue's skin `id`.
pub fn load_draft(settings: &Settings, id: &str, ui: Ui) -> Draft {
    Draft::load(settings, id, *ui, overscan(ui, id))
}

pub(super) fn draft(ui: Ui, id: &str) -> Draft {
    nanatype_frame::ui::preview::draft(*ui, id, overscan(ui, id))
}

pub(super) fn change(ui: Ui, id: &str, f: impl FnOnce(&mut Draft)) {
    nanatype_frame::ui::preview::change(*ui, id, overscan(ui, id), f)
}

/// The stage and the fields side by side: each column's basis and the
/// least it narrows to before they stack.
const COLUMN_W: f32 = 300.0;
const COLUMN_MIN_W: f32 = 280.0;
/// Two fields share a row, `HALF_W` percent each.
const HALF_W: f32 = 48.0;
/// The body size (px) and line height (× size) the profile accepts.
const SIZE_RANGE: (f64, f64) = (8.0, 200.0);
const LINE_RANGE: (f64, f64) = (1.0, 3.0);
const LINE_STEP: f64 = 0.05;
/// The line height's value beside its slider.
const READOUT_W: f32 = 56.0;

// Prepares asked for while a frame was built. The settings window sends
// them after that frame is presented: registering a skin's fonts takes the
// process-wide text engine, and the frame still being laid out needs it too.
thread_local! {
    static DEFERRED: RefCell<Vec<(String, bool)>> = const { RefCell::new(Vec::new()) };
}

/// Remember a prepare for after the current frame. A repeat for the same
/// skin keeps the latest `play`.
fn defer_request(id: String, play: bool) {
    DEFERRED.with(|pending| {
        let mut pending = pending.borrow_mut();
        if let Some(slot) = pending.iter_mut().find(|(asked, _)| *asked == id) {
            slot.1 = play;
        } else {
            pending.push((id, play));
        }
    });
}

/// Prepares recorded since the last presented settings frame, oldest first.
pub fn take_deferred_requests() -> Vec<(String, bool)> {
    DEFERRED.with(|pending| std::mem::take(&mut *pending.borrow_mut()))
}

/// Both skin catalogue layouts preview a skin on selection; the preview
/// asks for its stage once it shows (see [`view`]).
pub(super) fn select(previewing: Signal<Option<String>>, row: &crate::ui::SkinRow) {
    if row.error.is_none() {
        previewing.set(Some(row.id.clone()));
    }
}

/// Width and height: typed, since they run into the thousands.
#[derive(Clone, Copy)]
enum Field {
    Width,
    Height,
}
impl Field {
    fn value(self, p: &mut Pane) -> &mut String {
        match self {
            Self::Width => &mut p.width,
            Self::Height => &mut p.height,
        }
    }
    fn key(self) -> &'static str {
        match self {
            Self::Width => "skin-edit-width",
            Self::Height => "skin-edit-height",
        }
    }
}

/// A caption over `control`, `width` wide.
fn captioned(ui: Ui, key: &str, width: LengthSpec, control: impl IntoView) -> AnyView {
    widget(
        Stack::column(0.0)
            .gap_step(gap::step::TIGHT)
            .width(width)
            .with_layout(|l| {
                l.min_width = Some(LengthSpec::Px(0.0));
            }),
    )
    .children((hint(ui.now_t(key)), control))
    .into_any()
}

fn surface_key(base: &str, history: bool) -> String {
    format!("{base}-{}", if history { "history" } else { "display" })
}

fn field(ui: Ui, id: &str, history: bool, field: Field, placeholder: String) -> AnyView {
    let read_id = id.to_owned();
    let write_id = id.to_owned();
    let read = move || field.value(draft(ui, &read_id).pane_mut(history)).clone();
    captioned(
        ui,
        field.key(),
        LengthSpec::Percent(HALF_W),
        widget(
            kit::text_input(read())
                .placeholder(placeholder)
                .size(ControlSize::Small),
        )
        .key(surface_key(field.key(), history))
        .bind(move |input: &mut TextInput| {
            let text = read();
            if input.state.value.as_str() != text {
                input.state = TextInputState::new(text);
            }
        })
        .on(move |e: &TextChanged| {
            change(ui, &write_id, |d| {
                *field.value(d.pane_mut(history)) = e.value.to_string()
            });
        }),
    )
}

/// `value` as a draft field: empty when it is the skin's own `default`, so
/// the field keeps following the skin.
fn unless_default(value: f32, default: f32) -> String {
    if (value - default).abs() < 1e-4 {
        String::new()
    } else {
        value.to_string()
    }
}

/// The body size: a number field, the skin's size while the draft has none.
fn size_field(ui: Ui, id: &str, history: bool, default: f32) -> AnyView {
    let read_id = id.to_owned();
    let write_id = id.to_owned();
    let read = move || {
        draft(ui, &read_id)
            .pane(history)
            .size
            .trim()
            .parse::<f32>()
            .unwrap_or(default)
    };
    // The value last shown, so only outside changes (reset, discard)
    // replace what is being typed.
    let shown = Arc::new(AtomicU32::new(read().to_bits()));
    captioned(
        ui,
        "skin-edit-size",
        LengthSpec::Percent(HALF_W),
        widget(
            kit::number_input(f64::from(read()))
                .range(SIZE_RANGE.0, SIZE_RANGE.1)
                .step(1.0)
                .label(ui.now_t("skin-edit-size"))
                .size(ControlSize::Small),
        )
        .key(surface_key("skin-edit-size", history))
        .bind(move |input: &mut NumberInput| {
            let v = read();
            if shown.swap(v.to_bits(), Ordering::Relaxed) != v.to_bits() {
                input.assign(f64::from(v));
            }
        })
        .on(move |e: &NumberChanged| {
            change(ui, &write_id, |d| {
                d.pane_mut(history).size = unless_default(e.value as f32, default)
            });
        }),
    )
}

/// The line height: a slider with its value beside it.
fn line_slider(ui: Ui, id: &str, history: bool, default: f32) -> AnyView {
    let read_id = id.to_owned();
    let write_id = id.to_owned();
    let (min, max) = LINE_RANGE;
    let value = move || {
        draft(ui, &read_id)
            .pane(history)
            .line_height
            .trim()
            .parse::<f64>()
            .unwrap_or(f64::from(default))
            .clamp(min, max)
    };
    let readout = value.clone();
    captioned(
        ui,
        "skin-edit-line",
        LengthSpec::Fill,
        widget(
            Stack::row(0.0)
                .gap_step(gap::step::CONTROL)
                .width(LengthSpec::Fill)
                .align(AlignSpec::Center),
        )
        .children((
            widget(Stack::row(0.0).grow(1.0).shrink(1.0)).children(
                widget(
                    RangeField::new(value(), min, max, LINE_STEP)
                        .show_value(false)
                        .show_label(false),
                )
                .key(surface_key("skin-edit-line", history))
                .bind(move |r: &mut RangeField| {
                    let v = value();
                    if (r.value - v).abs() > f64::EPSILON {
                        r.value = v;
                    }
                })
                .on(move |e: &RangeChanged| {
                    let v = ((e.value / LINE_STEP).round() * LINE_STEP).clamp(min, max);
                    change(ui, &write_id, |d| {
                        d.pane_mut(history).line_height = unless_default(v as f32, default)
                    });
                }),
            ),
            widget(
                Text::new(String::new())
                    .style({
                        let mut st = Type::Small.style();
                        let l = Arc::make_mut(&mut st.layout);
                        l.width = Some(LengthSpec::Px(READOUT_W));
                        l.white_space_nowrap = true;
                        crate::ui::tnum(l);
                        st
                    })
                    .color(SemanticColorRole::Muted),
            )
            .bind(move |t: &mut Text| {
                let s = format!("{:.2} ×", readout());
                if t.value != s {
                    t.value = s;
                }
            }),
        )),
    )
}

/// The text colour: a picker, the skin's colour while the draft has none.
fn color_field(ui: Ui, id: &str, history: bool, default: Rgba8) -> AnyView {
    let read_id = id.to_owned();
    let write_id = id.to_owned();
    let rgba = move || {
        draft(ui, &read_id)
            .pane(history)
            .color
            .trim()
            .parse::<Rgba8>()
            .unwrap_or(default)
    };
    captioned(
        ui,
        "skin-edit-color",
        LengthSpec::Percent(HALF_W),
        widget(
            ColorField::new(crate::ui::c(rgba()))
                .label(ui.now_t("skin-edit-color"))
                .size(ControlSize::Small),
        )
        .key(surface_key("skin-edit-color", history))
        .bind(move |f: &mut ColorField| {
            let want = crate::ui::c(rgba());
            if f.value != want && !f.opened {
                *f = f.clone().value(want);
            }
        })
        .on(move |e: &ColorChanged| {
            let [r, g, b, _] = e.value;
            let to = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            let c = Rgba8::rgba(to(r), to(g), to(b), 255); // design-tokens: data
            change(ui, &write_id, |d| {
                d.pane_mut(history).color = if c == default {
                    String::new()
                } else {
                    c.to_hex()
                }
            });
        }),
    )
}

/// The body font: the skin's own, the built-in fonts and common system ones.
fn font_select(ui: Ui, id: &str, history: bool) -> AnyView {
    let mut fonts = vec![SelectOption::new("", ui.now_t("common-skin-default"))];
    let mut families: Vec<String> = nanatype_theme::BuiltinFonts::embedded()
        .fonts()
        .iter()
        .filter_map(|f| f.family())
        .collect();
    families.extend(
        [
            "Microsoft YaHei UI",
            "SimSun",
            "Yu Gothic UI",
            "Meiryo UI",
            "PingFang SC",
            "Noto Sans SC",
            "Noto Sans JP",
            "Arial",
            "Consolas",
        ]
        .map(str::to_owned),
    );
    families.sort();
    families.dedup();
    fonts.extend(
        families
            .into_iter()
            .map(|f| SelectOption::new(f.clone(), f)),
    );
    let read_id = id.to_owned();
    let write_id = id.to_owned();
    captioned(
        ui,
        "skin-edit-font",
        LengthSpec::Fill,
        widget(
            Select::new(Some(String::new()))
                .options(fonts)
                .label(ui.now_t("skin-edit-font"))
                .size(ControlSize::Small),
        )
        .key(surface_key("skin-edit-font", history))
        .bind(move |s: &mut Select| {
            let font: Arc<str> = draft(ui, &read_id).pane(history).font.trim().into();
            // A family saved before the list offered it still shows.
            if !s.options.iter().any(|o| o.value == font) {
                s.options
                    .push(SelectOption::new(font.to_string(), font.to_string()));
            }
            if s.value.as_ref() != Some(&font) {
                s.value = Some(font);
            }
        })
        .on(move |e: &SelectChanged| {
            change(ui, &write_id, |d| {
                d.pane_mut(history).font = e.value.to_string()
            });
        }),
    )
}

/// A small icon action labelled, and tooltipped, by `key`.
fn action(ui: Ui, icon: Icon, key: &str) -> IconButton {
    IconButton::new(icon, ui.now_t(key))
        .size(ControlSize::Small)
        .with_tooltip(ui.now_t(key))
}

/// Font, size, colour, line height and window size for one surface.
fn pane_fields(ui: Ui, id: &str, base: &Skin, history: bool) -> AnyView {
    let text = if history {
        base.history.text.clone()
    } else {
        base.dialogue.text.clone()
    };
    let dimensions = default_size(base, history);
    let typed = [
        (Field::Width, dimensions[0].to_string()),
        (Field::Height, dimensions[1].to_string()),
    ]
    .into_iter()
    .map(|(field_, placeholder)| field(ui, id, history, field_, placeholder))
    .collect::<Vec<_>>();
    widget(
        Stack::column(0.0)
            .gap_step(gap::step::CONTROL)
            .width(LengthSpec::Fill),
    )
    .children((
        font_select(ui, id, history),
        widget(
            Stack::row(0.0)
                .gap_step(gap::step::CONTROL)
                .wrap(true)
                .width(LengthSpec::Fill),
        )
        .children((
            size_field(ui, id, history, text.size),
            color_field(ui, id, history, text.color),
        )),
        line_slider(ui, id, history, text.line_height),
        widget(
            Stack::row(0.0)
                .gap_step(gap::step::CONTROL)
                .wrap(true)
                .width(LengthSpec::Fill),
        )
        .children(typed),
    ))
    .into_any()
}

fn font_missing(font: &str, fallback_font: bool) -> bool {
    if font.trim().is_empty() {
        fallback_font
    } else {
        !nana_ui::shaped_face_families(font, "Aa字")
            .iter()
            .any(|f| f.eq_ignore_ascii_case(font.trim()))
    }
}

fn controls(ui: Ui, id: &str, base: Arc<Skin>, fallback_font: bool) -> AnyView {
    let reset_id = id.to_owned();
    let discard_id = id.to_owned();
    let save_id = id.to_owned();
    let valid_id = id.to_owned();
    let valid_base = base.clone();
    let save_base = base.clone();
    let params = super::skin_params::view(ui, id, base.clone());
    let display = section(
        ui,
        "windows-display",
        vec![kit::block(pane_fields(ui, id, &base, false))],
    );
    let history = section(
        ui,
        "windows-history",
        vec![kit::block(pane_fields(ui, id, &base, true))],
    );
    let status_id = id.to_owned();
    let pick_id = id.to_owned();
    let status = dynamic(
        move || {
            (
                draft(ui, &status_id),
                ui.settings
                    .with(|s| s.skin.personalizations.get(&status_id).cloned()),
            )
        },
        move |(d, saved)| {
            let Ok(profile) = d.profile(&base) else {
                return hint(ui.now_t("skin-edit-invalid"));
            };
            let mut note = ui.now_t(if saved.as_ref() == Some(&profile) {
                "skin-edit-saved"
            } else {
                "skin-edit-draft-hint"
            });
            // A built-in font the draft names is registered by the sample;
            // register it here too, whichever of them runs first.
            for font in [&d.display.font, &d.history.font] {
                ui.skin_preview.with_untracked(|p| {
                    if let Some(p) = p.as_ref().filter(|p| p.id == pick_id) {
                        p.assets.pick_family(font);
                    }
                });
            }
            if font_missing(&d.display.font, fallback_font)
                || font_missing(&d.history.font, fallback_font)
            {
                note = format!("{}\n{note}", ui.now_t("skin-edit-font-missing"));
            }
            hint(note)
        },
    );
    // Keep controls mounted while the sample rerenders, preserving IME/selection.
    widget(
        Stack::column(0.0)
            .gap_step(gap::step::CONTROL)
            .width(LengthSpec::Fill),
    )
    .children((
        params,
        display,
        history,
        // Where the draft stands on the left, what can be done with it on
        // the right. Reset restores both surfaces.
        widget(
            Stack::row(0.0)
                .gap_step(gap::step::CONTROL)
                .align(AlignSpec::Center)
                .width(LengthSpec::Fill)
                .wrap(true),
        )
        .children((
            widget(kit::grow_column(gap::HAIR)).children(status),
            widget(
                Stack::row(0.0)
                    .gap_step(gap::step::CONTROL)
                    .align(AlignSpec::Center)
                    .with_layout(|l| l.flex_shrink = Some(0.0)),
            )
            .children((
                widget(action(ui, ti::RESTORE, "skin-edit-reset")).on(move |_: &Activate| {
                    change(ui, &reset_id, |d| {
                        d.display = Pane::default();
                        d.history = Pane::default();
                        d.params = Default::default();
                    })
                }),
                widget(action(ui, ti::ARROW_BACK_UP, "skin-edit-discard")).on(
                    move |_: &Activate| {
                        ui.skin_drafts.update(|m| {
                            m.remove(&discard_id);
                        });
                    },
                ),
                widget(
                    Button::new(ui.now_t("skin-edit-save"))
                        .icon(ti::DEVICE_FLOPPY)
                        .size(ControlSize::Small)
                        .kind(ButtonKind::Primary),
                )
                .key("skin-edit-save")
                .bind(move |b: &mut Button| {
                    b.disabled = draft(ui, &valid_id).profile(&valid_base).is_err()
                })
                .on_cx(move |_, _: &Activate, cx| {
                    if let Ok(profile) = draft(ui, &save_id).profile(&save_base) {
                        cx.dispatch_program_all(Msg::SaveSkinPersonalization {
                            id: save_id.clone(),
                            profile,
                        });
                    }
                }),
            )),
        )),
    ))
    .into_any()
}

pub fn view(ui: Ui, previewing: Signal<Option<String>>) -> AnyView {
    let width = signal(0.0_f32);
    dynamic(
        move || {
            (
                previewing
                    .get()
                    .unwrap_or_else(|| ui.settings.with(|s| s.skin.id.clone())),
                ui.locale.get(),
                ui.skins.with(|s| Arc::as_ptr(s) as usize),
                ui.skin_preview
                    .with(|p| p.as_ref().map(|p| Arc::as_ptr(p) as usize)),
            )
        },
        move |(id, locale, _, _)| {
            let id = id.clone();
            let row = ui
                .skins
                .with_untracked(|rows| rows.iter().find(|r| r.id == id).cloned());
            let Some(row) = row.filter(|r| r.skin.is_some() && r.error.is_none()) else {
                return hint(ui.now_t("skin-edit-unavailable"));
            };
            // The page shows at once, on the placeholder until a stage is
            // ready. Building the dialogue and its controls waits for that,
            // which arrives after this frame has been presented.
            let prepared = ui
                .skin_preview
                .with_untracked(|p| p.clone().filter(|p| p.shows(&row, locale.tag())));
            // This mount is the same flush that is about to lay the step out,
            // and the prepare registers fonts on the text engine that layout
            // needs. The settings window sends it once this frame has been
            // presented.
            let ask = id.clone();
            // Effects start with the preview. Reduced motion and a skin with
            // nothing animated still leave the sample still.
            on_mount(move |_| defer_request(ask, true));
            // One prepare draws both windows. The fields for each stay mounted.
            let sample = match prepared.as_ref() {
                Some(p) => stage_pair(*ui, p.assets.clone(), row.image_urls.clone(), width),
                None => {
                    let skin = row.skin.as_ref().expect("checked above");
                    stage_pair_placeholder(*ui, skin, width)
                }
            };
            let controls =
                prepared.map(|p| controls(ui, &id, p.assets.base.clone(), row.fallback_font));
            let apply_id = id.clone();
            let apply_read = id.clone();
            widget(
                Stack::column(0.0)
                    .gap_step(gap::step::GROUP)
                    .width(LengthSpec::Fill),
            )
            .key("skin-preview")
            .children((
                widget(
                    Stack::row(0.0)
                        .gap_step(gap::step::CONTROL)
                        .align(AlignSpec::Center)
                        .width(LengthSpec::Fill),
                )
                .children((
                    kit::spacer(),
                    widget(action(ui, ti::CHECK, "common-apply"))
                        .key("apply-preview")
                        .visible(move || ui.settings.with(|s| s.skin.id != apply_read))
                        .on_cx(move |_, _: &Activate, cx| {
                            let id = apply_id.clone();
                            cx.dispatch_program_all(super::controls::edit(move |s| s.skin.id = id));
                        }),
                )),
                widget(
                    Stack::row(0.0)
                        .gap_step(gap::step::GROUP)
                        .wrap(true)
                        .align(AlignSpec::Start)
                        .width(LengthSpec::Fill),
                )
                .children((
                    widget(
                        Stack::column(0.0)
                            .width(LengthSpec::Px(COLUMN_W))
                            .grow(1.0)
                            .shrink(1.0)
                            .with_layout(|l| l.min_width = Some(LengthSpec::Px(COLUMN_MIN_W))),
                    )
                    .key("skin-preview-stage-column")
                    .on(move |e: &SizeChanged| {
                        width.set_if_changed(e.width);
                    })
                    .children(
                        widget(
                            Stack::column(0.0)
                                .gap_step(gap::step::GROUP)
                                .width(LengthSpec::Fill),
                        )
                        .children(sample),
                    ),
                    widget(
                        Stack::column(0.0)
                            .width(LengthSpec::Px(COLUMN_W))
                            .grow(1.0)
                            .shrink(1.0)
                            .with_layout(|l| l.min_width = Some(LengthSpec::Px(COLUMN_MIN_W))),
                    )
                    .key("skin-preview-controls-column")
                    .children(controls),
                )),
            ))
            .into_any()
        },
    )
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nana_ui::runtime::{DocumentId, RuntimeDocument};
    use nana_ui_devtools::agent::{AgentSession, RuntimeAgentSession};
    use nanatype_settings::SurfacePersonalization;

    fn fixture() -> (Ui, Arc<Skin>) {
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        let skin = registry.load("nanatype.halo").unwrap().skin;
        let ui = Ui::new(
            Arc::new(Settings::default()),
            skin.clone(),
            nanatype_i18n::Locale::ZhCn,
        );
        ui.skins.set(Arc::new(crate::ui::skin_rows(
            &registry,
            nanatype_i18n::Locale::ZhCn,
        )));
        (ui, skin)
    }

    fn click(session: &mut RuntimeAgentSession, name: &str) {
        session.flush().unwrap();
        let b = session
            .accessibility_dump()
            .into_iter()
            .find(|n| n.role == "button" && n.label.as_deref() == Some(name))
            .unwrap()
            .bounds;
        session
            .click_xy(b.x + b.width / 2.0, b.y + b.height / 2.0)
            .unwrap();
        session.flush().unwrap();
    }

    #[test]
    fn drafts_keep_each_skin_and_surface_separate_and_validate_both_before_save() {
        let (ui, base) = fixture();
        let original = ui.settings.get_untracked();
        change(ui, "nanatype.halo", |d| {
            d.display.size = "48".into();
            d.history.line_height = "2".into();
        });
        change(ui, "nanatype.night", |d| d.display.size = "32".into());
        let p = draft(ui, "nanatype.halo").profile(&base).unwrap();
        assert_eq!(p.display.text_size, Some(48.0));
        assert_eq!(p.history.text_size, None);
        assert_eq!(p.history.line_height, Some(2.0));
        assert_eq!(draft(ui, "nanatype.night").display.size, "32");
        assert!(Arc::ptr_eq(&ui.settings.get_untracked(), &original));
        for bad in ["NaN", "inf", "201", "7", "abc"] {
            change(ui, "nanatype.halo", |d| d.history.size = bad.into());
            assert!(draft(ui, "nanatype.halo").profile(&base).is_err(), "{bad}");
        }
        change(ui, "nanatype.halo", |d| {
            d.history.size.clear();
            d.display.width = "300".into();
        });
        let p = draft(ui, "nanatype.halo").profile(&base).unwrap();
        assert_eq!(p.display.size.unwrap()[0], 300.0);
        assert_eq!(
            p.display.size.unwrap()[1],
            base.dialogue.region[1] + 2.0 * base.dialogue.overscan
        );
    }

    #[test]
    fn native_controls_edit_only_the_draft_and_save_the_browsed_skin_without_activation() {
        let (ui, base) = fixture();
        let active = ui.settings.get_untracked();
        let source = ui.source.get_untracked();
        let history = ui.history.get_untracked();
        let id = "nanatype.night";
        let doc_id = DocumentId::new(1).unwrap();
        let mut doc = RuntimeDocument::new(doc_id);
        let render_base = base.clone();
        doc.context_mut()
            .mount_view_root(doc_id, move || controls(ui, id, render_base, false))
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, 700, 1400).unwrap();
        session.flush().unwrap();
        let input = session
            .accessibility_dump()
            .into_iter()
            .find(|n| {
                n.agent_path
                    .as_deref()
                    .is_some_and(|id| id.ends_with("skin-edit-size-display"))
            })
            .expect("body size field");
        session.click_node(input.id).unwrap();
        session.key_press("End", "End", Default::default()).unwrap();
        for _ in 0..4 {
            session
                .key_press("Backspace", "Backspace", Default::default())
                .unwrap();
        }
        session.type_text("42").unwrap();
        // A number field commits on Enter, not per keystroke.
        assert_eq!(draft(ui, id).display.size, "");
        session
            .key_press("Enter", "Enter", Default::default())
            .unwrap();
        assert_eq!(draft(ui, id).display.size, "42");
        assert_eq!(ui.settings.get_untracked(), active);
        assert_eq!(ui.source.get_untracked(), source);
        assert!(Arc::ptr_eq(&ui.history.get_untracked(), &history));
        assert!(
            session
                .document_mut()
                .context_mut()
                .take_program_messages()
                .is_empty()
        );
        change(ui, id, |d| d.history.size = "201".into());
        session.flush().unwrap();
        assert!(
            session
                .accessibility_dump()
                .iter()
                .any(
                    |n| n.label.as_deref() == Some(ui.now_t("skin-edit-save").as_str())
                        && n.disabled
                )
        );
        click(&mut session, &ui.now_t("skin-edit-save"));
        assert!(
            session
                .document_mut()
                .context_mut()
                .take_program_messages()
                .is_empty()
        );
        change(ui, id, |d| d.history.size.clear());
        session.flush().unwrap();
        click(&mut session, &ui.now_t("skin-edit-save"));
        let messages = session.document_mut().context_mut().take_program_messages();
        let saved = messages
            .into_iter()
            .find_map(|m| m.downcast::<Msg>().ok())
            .unwrap();
        let Msg::SaveSkinPersonalization {
            id: saved_id,
            profile,
        } = *saved
        else {
            panic!("save message");
        };
        assert_eq!(saved_id, id);
        assert_eq!(profile.display.text_size, Some(42.0));
        assert_eq!(ui.settings.get_untracked(), active);
        click(&mut session, &ui.now_t("skin-edit-reset"));
        let restored = draft(ui, id).profile(&base).unwrap();
        assert_eq!(restored.display, SurfacePersonalization::default());
        assert_eq!(restored.history, SurfacePersonalization::default());
        change(ui, id, |d| d.history.size = "72".into());
        click(&mut session, &ui.now_t("skin-edit-discard"));
        assert!(!ui.skin_drafts.with_untracked(|m| m.contains_key(id)));
    }

    fn commit_size(session: &mut RuntimeAgentSession, key: &str, text: &str) {
        session.flush().unwrap();
        let input = session
            .accessibility_dump()
            .into_iter()
            .find(|n| n.agent_path.as_deref().is_some_and(|id| id.ends_with(key)))
            .unwrap_or_else(|| panic!("missing {key}"));
        session.click_node(input.id).unwrap();
        session.key_press("End", "End", Default::default()).unwrap();
        for _ in 0..4 {
            session
                .key_press("Backspace", "Backspace", Default::default())
                .unwrap();
        }
        session.type_text(text).unwrap();
        session
            .key_press("Enter", "Enter", Default::default())
            .unwrap();
    }

    #[test]
    fn each_surface_has_its_own_font_picker_and_typed_values_follow_the_skin() {
        let (ui, base) = fixture();
        let id = "nanatype.halo";
        let doc_id = DocumentId::new(7).unwrap();
        let mut doc = RuntimeDocument::new(doc_id);
        let render_base = base.clone();
        doc.context_mut()
            .mount_view_root(doc_id, move || controls(ui, id, render_base, false))
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, 700, 1400).unwrap();
        session.flush().unwrap();
        let nodes = session.accessibility_dump();
        let fonts: Vec<_> = nodes
            .iter()
            .filter(|n| {
                n.agent_path.as_deref().is_some_and(|p| {
                    p.rsplit('/')
                        .next()
                        .is_some_and(|k| k.starts_with("skin-edit-font-"))
                })
            })
            .collect();
        assert_eq!(fonts.len(), 2, "{fonts:?}");
        for key in ["windows-display", "windows-history"] {
            assert!(
                nodes
                    .iter()
                    .any(|n| n.label.as_deref() == Some(ui.now_t(key).as_str())),
                "{key}"
            );
        }
        // The text buttons are icon buttons now, still named by their text.
        for key in ["skin-edit-reset", "skin-edit-discard", "skin-edit-save"] {
            assert!(
                nodes
                    .iter()
                    .any(|n| n.role == "button"
                        && n.label.as_deref() == Some(ui.now_t(key).as_str())),
                "{key}"
            );
        }

        commit_size(&mut session, "skin-edit-size-display", "40");
        assert_eq!(draft(ui, id).display.size, "40");
        let default = base.dialogue.text.size;
        commit_size(&mut session, "skin-edit-size-display", &default.to_string());
        assert_eq!(draft(ui, id).display.size, "");

        // The font list keeps a family typed before it was offered.
        change(ui, id, |d| d.display.font = "Some Old Family".into());
        session.flush().unwrap();
        assert!(
            session
                .accessibility_dump()
                .iter()
                .any(|n| n.label.as_deref() == Some("Some Old Family")
                    || n.value.as_deref() == Some("Some Old Family")),
        );
    }

    /// A standalone skin with one param of each kind and a preset.
    fn param_skin() -> Arc<Skin> {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(nanatype_theme::MANIFEST),
            r##"
[meta]
id = "test.params"
[params.fur]
type = "color"
default = "#fff7ec"
name = { zh-CN = "毛色" }
[params.size]
type = "number"
default = 28
min = 16
max = 40
step = 2
[params.align]
type = "enum"
default = "start"
options = [{ id = "start" }, { id = "center" }]
[params.shadow]
type = "bool"
default = false
[presets.night]
name = { zh-CN = "夜晚" }
values = { fur = "#2b2730", size = 32 }
[dialogue.box]
color = { param = "fur" }
[dialogue.text]
size = { param = "size" }
align = { param = "align" }
"##,
        )
        .unwrap();
        let files: Arc<dyn nanatype_theme::SkinFiles> =
            Arc::new(nanatype_theme::DirFiles::new(dir.path()));
        let loaded = nanatype_theme::load(
            files,
            nanatype_theme::SkinOrigin::Directory(dir.path().into()),
            &(),
            env!("CARGO_PKG_VERSION"),
        )
        .unwrap();
        loaded.skin
    }

    #[test]
    fn param_controls_edit_the_draft_keep_valid_values_and_save_them() {
        let (ui, _) = fixture();
        let base = param_skin();
        let id = "test.params";
        let doc_id = DocumentId::new(5).unwrap();
        let mut doc = RuntimeDocument::new(doc_id);
        let render_base = base.clone();
        doc.context_mut()
            .mount_view_root(doc_id, move || controls(ui, id, render_base, false))
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, 700, 1800).unwrap();
        session.flush().unwrap();
        let fur = session
            .accessibility_dump()
            .into_iter()
            .find(|n| {
                n.agent_path
                    .as_deref()
                    .is_some_and(|p| p.contains("skin-param-fur"))
            })
            .expect("hex field for the colour param");
        session.click_node(fur.id).unwrap();
        session.type_text("zz").unwrap();
        session.flush().unwrap();
        // Unparsable text stays in the field; the sample keeps the last
        // valid colour and the draft can still be saved.
        let d = draft(ui, id);
        assert!(d.params.raw.contains_key("fur"));
        assert!(!d.params.values.contains_key("fur"));
        assert!(d.profile(&base).unwrap().params.is_empty());
        assert!(session.accessibility_dump().iter().any(|n| {
            n.label.as_deref() == Some(ui.now_t("skin-params-invalid-color").as_str())
        }));

        change(ui, id, |d| {
            let _ = d.params.input(&base, "fur", "#102030");
        });
        let p = draft(ui, id).profile(&base).unwrap();
        assert_eq!(
            p.params.get("fur"),
            Some(&nanatype_model::ParamValue::Text("#102030".into()))
        );

        // A preset chip applies its values; matching values light it up.
        click(&mut session, "夜晚");
        let d = draft(ui, id);
        assert_eq!(d.params.matching_preset(&base), Some("night"));
        assert!(d.params.raw.is_empty());
        click(&mut session, &ui.now_t("skin-edit-save"));
        let saved = session
            .document_mut()
            .context_mut()
            .take_program_messages()
            .into_iter()
            .find_map(|m| m.downcast::<Msg>().ok())
            .unwrap();
        let Msg::SaveSkinPersonalization { profile, .. } = *saved else {
            panic!("save message");
        };
        // Only values that differ from the defaults are kept.
        assert_eq!(profile.params.len(), 2);
        assert_eq!(
            profile.params.get("size"),
            Some(&nanatype_model::ParamValue::Number(32.0))
        );

        click(&mut session, &ui.now_t("skin-params-default"));
        assert!(draft(ui, id).params.values.is_empty());
        change(ui, id, |d| {
            d.params
                .set("shadow", nanatype_model::ParamValue::Bool(true))
        });
        click(&mut session, &ui.now_t("skin-edit-reset"));
        assert!(draft(ui, id).params.values.is_empty());
    }

    #[test]
    fn sample_does_not_write_to_live_settings_history_or_editors() {
        let (ui, base) = fixture();
        let settings = ui.settings.get_untracked();
        let history = ui.history.get_untracked();
        let source = ui.source.get_untracked();
        let draft = ui.draft.get_untracked();
        let mut edited = load_draft(&settings, &settings.skin.id, ui);
        edited.history.size = "56".into();
        let doc_id = DocumentId::new(2).unwrap();
        let mut doc = RuntimeDocument::new(doc_id);
        doc.context_mut()
            .mount_view_root(doc_id, move || {
                sample(
                    *ui,
                    &base,
                    "nanatype.night",
                    &edited,
                    Surface::History,
                    Arc::default(),
                )
                .unwrap()
                .0
            })
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, 300, 560).unwrap();
        session.flush().unwrap();
        assert_eq!(ui.settings.get_untracked(), settings);
        assert!(Arc::ptr_eq(&ui.history.get_untracked(), &history));
        assert!(Arc::ptr_eq(&ui.draft.get_untracked(), &draft));
        assert_eq!(ui.source.get_untracked(), source);
        assert!(
            session
                .document_mut()
                .context_mut()
                .take_program_messages()
                .is_empty()
        );
    }

    fn node_keys(session: &mut RuntimeAgentSession) -> Vec<String> {
        session.flush().unwrap();
        session
            .accessibility_dump()
            .into_iter()
            .filter_map(|n| n.agent_path)
            .filter_map(|p| p.rsplit('/').next().map(str::to_owned))
            .collect()
    }

    #[test]
    fn the_page_shows_before_its_stage_and_a_repeated_selection_keeps_it() {
        let (ui, _) = fixture();
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        let id = "nanatype.sakura";
        let doc_id = DocumentId::new(6).unwrap();
        let mut doc = RuntimeDocument::new(doc_id);
        let previewing = signal(Some(id.to_owned()));
        doc.context_mut()
            .mount_view_root(doc_id, move || view(ui, previewing))
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, 720, 1400).unwrap();
        let keys = node_keys(&mut session);
        assert!(keys.iter().any(|k| k == "skin-preview-loading"), "{keys:?}");
        assert!(
            !keys.iter().any(|k| k == "skin-preview-display"),
            "{keys:?}"
        );
        assert!(
            !keys.iter().any(|k| k == "skin-preview-history"),
            "{keys:?}"
        );
        assert!(keys.iter().any(|k| k == "apply-preview"));
        assert!(!keys.iter().any(|k| k == "play-effects"), "{keys:?}");
        assert!(ui.skin_preview.get_untracked().is_none());
        // Mount records the prepare; nothing loads until the presented frame
        // sends it. The dialogue is still absent. Effects are requested so
        // the sample can play without a button.
        assert_eq!(take_deferred_requests(), vec![(id.to_owned(), true)]);

        crate::ui::prepare_skin_preview(ui, &registry, id, false).unwrap();
        let keys = node_keys(&mut session);
        assert!(
            keys.iter().any(|k| k == "skin-preview-display-box"),
            "{keys:?}"
        );
        assert!(
            keys.iter().any(|k| k == "skin-preview-history-box"),
            "{keys:?}"
        );
        assert!(keys.iter().any(|k| k == "skin-preview-display"), "{keys:?}");
        assert!(keys.iter().any(|k| k == "skin-preview-history"), "{keys:?}");
        assert!(!keys.iter().any(|k| k == "skin-preview-loading"));
        let stage = ui.skin_preview.get_untracked().unwrap();
        let rows = ui.skins.get_untracked();
        // Selecting it again, with or without effects, keeps the stage and
        // the catalogue as they are.
        crate::ui::prepare_skin_preview(ui, &registry, id, false).unwrap();
        crate::ui::prepare_skin_preview(ui, &registry, id, true).unwrap();
        assert!(Arc::ptr_eq(
            &ui.skin_preview.get_untracked().unwrap(),
            &stage
        ));
        assert!(Arc::ptr_eq(&ui.skins.get_untracked(), &rows));
    }

    #[test]
    fn tall_and_maximum_previews_scale_the_entire_logical_canvas() {
        let (ui, _) = fixture();
        let id = "nanatype.halo";
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        crate::ui::prepare_skin_preview(ui, &registry, id, false).unwrap();
        let doc_id = DocumentId::new(4).unwrap();
        let mut doc = RuntimeDocument::new(doc_id);
        doc.context_mut()
            .mount_view_root(doc_id, move || view(ui, signal(None)))
            .unwrap();
        let skin = registry.load(id).unwrap().skin;
        let mut session = RuntimeAgentSession::new(doc, 720, 1400).unwrap();
        for history in [false, true] {
            for [w, h] in [
                [300.0, 1200.0],
                [800.0, 2400.0],
                [160.0, 8192.0],
                [8192.0, 8192.0],
                [8192.0, 96.0],
                [300.0, 240.0],
            ] {
                change(ui, id, |d| {
                    let (pane, other) = if history {
                        (&mut d.history, &mut d.display)
                    } else {
                        (&mut d.display, &mut d.history)
                    };
                    pane.width = w.to_string();
                    pane.height = h.to_string();
                    pane.size = "200".into();
                    *other = Pane::default();
                });
                session.flush().unwrap();
                let nodes = session.accessibility_dump();
                let bounds = |key| {
                    nodes
                        .iter()
                        .find(|n| {
                            n.agent_path
                                .as_deref()
                                .is_some_and(|p| p.rsplit('/').next() == Some(key))
                        })
                        .unwrap_or_else(|| panic!("missing {key}"))
                        .bounds
                        .clone()
                };
                let column = bounds("skin-preview-stage-column");
                let content_w = (column.width - 24.0).max(1.0);
                let display_logical = if history {
                    default_size(&skin, false)
                } else {
                    [w, h]
                };
                let history_logical = if history {
                    [w, h]
                } else {
                    default_size(&skin, true)
                };
                let display_expected = (content_w / display_logical[0])
                    .min(340.0 / display_logical[1])
                    .min(1.0);
                let history_expected = content_w / history_logical[0].max(1.0);
                let cases = [
                    (
                        "display",
                        bounds("skin-preview-display-box"),
                        bounds("skin-preview-canvas"),
                        bounds("skin-preview-display"),
                        display_logical,
                        display_expected,
                        "skin-preview-display-metrics",
                    ),
                    (
                        "history",
                        bounds("skin-preview-history-box"),
                        bounds("skin-preview-history-canvas"),
                        bounds("skin-preview-history"),
                        history_logical,
                        history_expected,
                        "skin-preview-history-metrics",
                    ),
                ];
                for (name, stage, canvas, content, logical, expected, metrics) in cases {
                    let scale = content.width / logical[0];
                    assert!(
                        (content.height - logical[1] * scale).abs() < 1.0,
                        "{history} {name} {w}x{h}: scale {scale} {content:?}"
                    );
                    assert!(
                        (scale - expected).abs() < 0.02,
                        "{history} {name} {w}x{h}: scale {scale} expected {expected}"
                    );
                    for actual in [&stage, &canvas, &content] {
                        assert!(
                            actual.x >= column.x
                                && actual.x + actual.width <= column.x + column.width + 0.5,
                            "{history} {name} {w}x{h}: {actual:?}"
                        );
                    }
                    assert!((stage.x - canvas.x).abs() < 0.5 && (stage.y - canvas.y).abs() < 0.5);
                    let label = bounds(metrics);
                    assert!(
                        label.y + label.height <= canvas.y + 1.0,
                        "{history} {name} metrics cover the render: {label:?} {canvas:?}"
                    );
                    assert!(
                        (stage.width - canvas.width).abs() < 0.5
                            && (stage.height - canvas.height).abs() < 0.5
                    );
                    assert!(
                        nodes.iter().any(|n| n
                            .agent_path
                            .as_deref()
                            .is_some_and(|p| p.ends_with(metrics))),
                        "{history} {name} missing {metrics}"
                    );
                    let caption = format!(
                        "{} × {} px · {:.0}%",
                        logical[0].round(),
                        logical[1].round(),
                        expected * 100.0
                    );
                    assert!(
                        nodes.iter().any(|n| {
                            n.label.as_deref() == Some(caption.as_str())
                                || n.value.as_deref() == Some(caption.as_str())
                        }),
                        "{history} {name} missing {caption}"
                    );
                }
                assert!(
                    (bounds("skin-preview-history-box").width - content_w).abs() < 1.5,
                    "history does not fill the column"
                );
                assert!(bounds("skin-preview-display-box").width <= display_logical[0] + 0.5);
            }
        }
    }

    #[test]
    fn both_surfaces_edit_their_own_pane_without_switching() {
        let (ui, _) = fixture();
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        let id = "nanatype.halo";
        crate::ui::prepare_skin_preview(ui, &registry, id, false).unwrap();
        let doc_id = DocumentId::new(8).unwrap();
        let mut doc = RuntimeDocument::new(doc_id);
        doc.context_mut()
            .mount_view_root(doc_id, move || view(ui, signal(None)))
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, 900, 2400).unwrap();
        session.flush().unwrap();
        let dump = session.accessibility_dump();
        let display = ui.now_t("windows-display");
        let history_label = ui.now_t("windows-history");
        assert!(
            dump.iter().all(|n| {
                n.role != "button"
                    || !matches!(
                        n.label.as_deref(),
                        Some(label) if label == display || label == history_label
                    )
            }),
            "the surfaces are not a switch"
        );
        for key in [
            "skin-edit-size-display",
            "skin-edit-size-history",
            "skin-edit-font-display",
            "skin-edit-font-history",
        ] {
            assert!(
                dump.iter()
                    .any(|n| n.agent_path.as_deref().is_some_and(|p| p.ends_with(key))),
                "{key}"
            );
        }
        assert!(
            dump.iter().any(|n| {
                n.agent_path
                    .as_deref()
                    .is_some_and(|p| p.ends_with("skin-preview-display"))
            }),
            "display render is shown"
        );
        let has = |key: &str| {
            let label = ui.now_t(key);
            dump.iter()
                .any(|n| n.label.as_deref() == Some(label.as_str()))
        };
        for key in [
            "skin-edit-long-name",
            "skin-edit-history-viewer-name",
            "skin-edit-history-fan-name",
            "skin-edit-history-captain-name",
            "history-role-host",
            "history-role-viewer",
        ] {
            assert!(has(key), "{key} is shown");
        }
        commit_size(&mut session, "skin-edit-size-history", "42");
        let edited = draft(ui, id);
        assert_eq!(edited.history.size, "42");
        assert_eq!(edited.display.size, "");
        commit_size(&mut session, "skin-edit-size-display", "36");
        let edited = draft(ui, id);
        assert_eq!(edited.display.size, "36");
        assert_eq!(edited.history.size, "42");
    }

    #[test]
    fn minimum_history_keeps_room_for_messages_and_restores_its_title_when_enlarged() {
        let (ui, _) = fixture();
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        let base = registry.load("nanatype.celestial").unwrap().skin;
        let edited = load_draft(&ui.settings.get_untracked(), "nanatype.celestial", ui);
        let doc_id = DocumentId::new(3).unwrap();
        let mut doc = RuntimeDocument::new(doc_id);
        doc.context_mut()
            .mount_view_root(doc_id, move || {
                sample(
                    *ui,
                    &base,
                    "nanatype.celestial",
                    &edited,
                    Surface::History,
                    Arc::default(),
                )
                .unwrap()
                .0
            })
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, 160, 96).unwrap();
        session.flush().unwrap();
        let nodes = session.accessibility_dump();
        assert!(!nodes.iter().any(|n| n.label.as_deref() == Some("HISTORY")));
        for node in &nodes {
            if node.label.as_ref().is_some_and(|s| !s.is_empty()) {
                assert!(
                    node.bounds.x >= -1.0 && node.bounds.x + node.bounds.width <= 161.0,
                    "minimum history overflows: {:?}",
                    node
                );
            }
        }
        session.set_viewport(300, 560, 1.0).unwrap();
        session.flush().unwrap();
        assert!(
            session
                .accessibility_dump()
                .iter()
                .any(|n| n.label.as_deref() == Some("HISTORY"))
        );
    }
    #[test]
    fn an_unsaved_skin_preview_preserves_legacy_window_geometry_and_region() {
        let (ui, base) = fixture();
        let mut settings = (*ui.settings.get_untracked()).clone();
        settings.overlays.display.size = Some([900.0, 240.0]);
        settings.overlays.history.size = Some([300.0, 560.0]);
        let p = load_draft(&settings, "other.skin", ui)
            .profile(&base)
            .unwrap();
        assert_eq!(p.display.size, settings.overlays.display.size);
        assert_eq!(p.history.size, settings.overlays.history.size);
        settings.overlays.display.size = None;
        settings.skin.overrides.region = Some([420.0, 180.0]);
        let p = load_draft(&settings, "other.skin", ui)
            .profile(&base)
            .unwrap();
        assert_eq!(
            p.display.size,
            Some([
                420.0 + 2.0 * base.dialogue.overscan,
                180.0 + 2.0 * base.dialogue.overscan
            ])
        );
    }
}
