use nana_ui::runtime::{FocusWithinChanged, NumberChanged, NumberInput};

use crate::msg::ObsMsg;

use super::super::*;
use super::capture::obs_source_row;
use super::{Read, Write};

/// `w × h`, as the size hints spell it.
pub(super) fn size_text([w, h]: [u32; 2]) -> String {
    format!("{w} × {h}")
}

/// The preset that follows the window (`SpoutOutput.size` stays `None`).
const FOLLOW_WINDOW: &str = "window";
/// Shown in the fields, and kept as the other side, while following the window.
const SIZE_BASE: [u32; 2] = [1280, 720];

/// Presets that already fit 1080p. Larger ones would be scaled down on save.
const SIZE_PRESETS: [([u32; 2], &str); 6] = [
    ([1280, 720], "16:9 · 720p"),
    ([1920, 1080], "16:9 · 1080p"),
    ([720, 1280], "9:16 · 720p"),
    ([1080, 1920], "9:16 · 1080p"),
    ([1080, 1080], "1:1 · 1080p"),
    ([1440, 1080], "4:3 · 1080p"),
];

fn preset_key([width, height]: [u32; 2]) -> String {
    format!("{width}x{height}")
}

fn shown_size(saved: Option<[u32; 2]>) -> [u32; 2] {
    saved.unwrap_or(SIZE_BASE)
}

/// The fixed size after editing `axis` (0 width, 1 height). The other side
/// stays as shown, and the pair is fitted into 1080p. A value that is not a
/// size leaves the shown pair unchanged.
pub(super) fn commit_side(saved: Option<[u32; 2]>, axis: usize, value: f64) -> [u32; 2] {
    let shown = shown_size(saved);
    let mut size = shown;
    if let Some(side) = size.get_mut(axis) {
        let next = value.round();
        if next.is_finite() && next >= 0.0 {
            *side = next as u32;
        }
    }
    nanatype_settings::fit_output_size(f64::from(size[0]), f64::from(size[1])).unwrap_or(shown)
}

/// `"window"` follows the window. A known preset stores that size.
pub(super) fn chosen_size(value: &str) -> Option<Option<[u32; 2]>> {
    if value == FOLLOW_WINDOW {
        return Some(None);
    }
    let (width, height) = value.split_once('x')?;
    let size = [width.parse::<u32>().ok()?, height.parse::<u32>().ok()?];
    SIZE_PRESETS
        .iter()
        .any(|(preset, _)| *preset == size)
        .then_some(Some(size))
}

/// Follow-window, a matching preset, or empty when the size is custom.
fn preset_choice(saved: Option<[u32; 2]>) -> String {
    match saved {
        None => FOLLOW_WINDOW.to_owned(),
        Some(size) if SIZE_PRESETS.iter().any(|(preset, _)| *preset == size) => preset_key(size),
        Some(_) => String::new(),
    }
}

fn size_options(ui: Ui) -> Vec<(String, String)> {
    let mut options = vec![(
        FOLLOW_WINDOW.to_owned(),
        I18N.t_in(ui.locale.get(), "output-spout-size-window"),
    )];
    options.extend(
        SIZE_PRESETS
            .iter()
            .map(|(size, name)| (preset_key(*size), (*name).to_owned())),
    );
    options
}

/// One side. An outside change replaces the field when unfocused; a draft
/// being typed stays.
fn side_field(ui: Ui, axis: usize, label_key: &'static str, read: Read, write: Write) -> AnyView {
    let initial = shown_size(ui.settings.with_untracked(|s| read(s).size))[axis];
    let focused = signal(false);
    widget(
        Stack::row(0.0)
            .width(LengthSpec::Px(field::XS))
            .with_layout(|l| l.flex_shrink = Some(0.0)),
    )
    .children(
        widget({
            let mut input = crate::ui::kit::number_input(f64::from(initial))
                .range(
                    f64::from(nanatype_settings::OUTPUT_MIN),
                    f64::from(nanatype_settings::OUTPUT_MAX_LONG),
                )
                .label(ui.now_t(label_key))
                .size(ControlSize::Small);
            crate::ui::tnum(Arc::make_mut(&mut input.style.layout));
            input
        })
        .on(move |e: &FocusWithinChanged| {
            focused.set_if_changed(e.focused);
        })
        .bind(move |input: &mut NumberInput| {
            let value = shown_size(ui.settings.with(|s| read(s).size))[axis];
            if !focused.get() {
                input.assign(f64::from(value));
            }
        })
        .on_cx(move |_, e: &NumberChanged, cx| {
            let value = e.value;
            cx.dispatch_program_all(edit(move |s| {
                write(s).size = Some(commit_side(read(s).size, axis, value));
            }));
        }),
    )
    .into_any()
}

/// Width × height, then the presets.
fn size_editor(ui: Ui, read: Read, write: Write) -> AnyView {
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::TIGHT)
            .align(AlignSpec::Center)
            .wrap(true),
    )
    .children((
        side_field(ui, 0, "output-size-width", read, write),
        widget(Type::Caption.text("×")),
        side_field(ui, 1, "output-size-height", read, write),
        live_select(
            ui,
            move || size_options(ui),
            move |s| preset_choice(read(s).size),
            move |s, value| {
                if let Some(size) = chosen_size(value) {
                    write(s).size = size;
                }
            },
        ),
    ))
    .into_any()
}

/// A row with the card's hairline above it, for a column built by hand.
fn below_divider(row: Row) -> AnyView {
    widget(Stack::column(0.0))
        .children((divider(), row))
        .into_any()
}

/// Downloads Off World Live's Spout2 plugin installer and starts it, with
/// progress on the button and what went wrong beside it.
pub(super) fn spout_plugin(ui: Ui) -> AnyView {
    use crate::services::spout_plugin::State;
    let label = move |state: &State| -> String {
        let locale = ui.locale.get();
        match state {
            State::Idle => I18N.t_in(locale, "output-obs-plugin-download"),
            State::Downloading { done, total } => I18N.tr_in(
                locale,
                "output-obs-plugin-downloading",
                &[("percent", (done * 100 / (*total).max(1)).to_string().into())],
            ),
            State::Launched => I18N.t_in(locale, "output-obs-plugin-again"),
            State::Failed(_) => I18N.t_in(locale, "output-obs-plugin-retry"),
        }
    };
    let note = move |state: &State| -> String {
        let locale = ui.locale.get();
        match state {
            State::Launched => I18N.t_in(locale, "output-obs-plugin-launched"),
            State::Failed(detail) => I18N.tr_in(
                locale,
                "output-obs-plugin-failed",
                &[("detail", detail.as_str().into())],
            ),
            _ => String::new(),
        }
    };
    (
        widget(
            Button::new(ui.now_t("output-obs-plugin-download"))
                .icon(ti::DOWNLOAD)
                .size(ControlSize::Small),
        )
        .bind(move |b: &mut Button| {
            let state = ui.spout_plugin.get();
            let text = label(&state);
            if b.label != text {
                b.label = text;
            }
            b.disabled = matches!(*state, State::Downloading { .. });
        })
        .on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program_all(Msg::Obs(ObsMsg::InstallSpoutPlugin));
        }),
        widget(
            Text::new(String::new())
                .style(Type::Small.style())
                .color(SemanticColorRole::Muted),
        )
        .visible(move || !note(&ui.spout_plugin.get()).is_empty())
        .bind(move |t: &mut Text| {
            let text = note(&ui.spout_plugin.get());
            if t.value != text {
                t.value = text;
            }
        }),
    )
        .into_any()
}

pub(super) fn spout_rows(
    ui: Ui,
    kind: WindowKind,
    title: &'static str,
    when: Option<fn(&Settings) -> bool>,
) -> [(Row, Shown); 2] {
    let read: Read = match kind {
        WindowKind::Display => |s| &s.output.display,
        WindowKind::Danmaku => |s| &s.output.danmaku,
        _ => |s| &s.output.history,
    };
    let write: Write = match kind {
        WindowKind::Display => |s| &mut s.output.display,
        WindowKind::Danmaku => |s| &mut s.output.danmaku,
        _ => |s| &mut s.output.history,
    };
    // The sender's name is fixed: shown only so the user knows which one
    // to pick in OBS, with a copy button.
    let name = row(
        ui,
        "output-spout-name",
        Some(if cfg!(target_os = "macos") {
            "output-spout-name-hint-syphon"
        } else {
            "output-spout-name-hint"
        }),
        widget(crate::ui::kit::tag_action_frame())
            .children((
                widget(
                    Text::new(read(&ui.settings.get_untracked()).name.clone())
                        .style(Type::Small.mono()),
                )
                .bind(move |t: &mut Text| {
                    let name = ui.settings.with(|s| read(s).name.clone());
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
                    let current = read(&ui.settings.get_untracked()).name.clone();
                    cx.dispatch_program_all(Msg::CopyText(current))
                }),
            ))
            .into_any(),
    );
    // Only while OBS is linked; it carries its own hairline so a hidden
    // row leaves none behind.
    let source = widget(Stack::column(0.0))
        .visible(move || ui.settings.with(|s| s.output.obs.enabled))
        .children(below_divider(obs_source_row(ui, kind, read, write)))
        .into_any();
    let size = row_live_hint(
        ui,
        "output-spout-size",
        move || {
            ui.settings
                .with(|s| s.output.obs.enabled && !read(s).obs_source.is_empty())
                .then(|| I18N.t_in(ui.locale.get(), "output-spout-size-fallback"))
        },
        size_editor(ui, read, write),
    );
    let straight = row_tip(
        ui,
        "output-spout-straight",
        "output-spout-straight-hint",
        switch(
            ui,
            move |s| read(s).straight_alpha,
            move |s, v| write(s).straight_alpha = v,
        ),
    );
    let header = row_status(
        ui,
        title,
        None,
        output_badge(ui, move |s| s.get(kind)),
        switch(
            ui,
            move |s| read(s).enabled,
            move |s, v| write(s).enabled = v,
        ),
    );
    [
        match when {
            Some(pred) => shown_if(header, move || ui.settings.with(|s| pred(s))),
            None => (header, None),
        },
        shown_if(
            Row::inset(widget(Stack::column(0.0)).children((
                name,
                source,
                below_divider(size),
                below_divider(straight),
            ))),
            move || {
                ui.settings
                    .with(|s| when.is_none_or(|pred| pred(s)) && read(s).enabled)
            },
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::{SIZE_PRESETS, chosen_size, commit_side};

    #[test]
    fn editing_a_side_while_following_the_window_starts_from_the_base() {
        assert_eq!(commit_side(None, 1, 480.0), [1280, 480]);
        assert_eq!(commit_side(Some([1920, 1080]), 0, 1600.0), [1600, 1080]);
    }

    #[test]
    fn editing_a_side_fits_the_pair_into_1080p() {
        assert_eq!(commit_side(Some([1920, 1080]), 1, 1920.0), [1080, 1080]);
    }

    #[test]
    fn a_side_that_is_not_a_size_keeps_the_shown_pair() {
        assert_eq!(commit_side(Some([1280, 720]), 1, 0.0), [1280, 720]);
    }

    #[test]
    fn choosing_the_window_clears_the_size_and_a_preset_stores_it() {
        assert_eq!(chosen_size("window"), Some(None));
        assert_eq!(chosen_size("1080x1920"), Some(Some([1080, 1920])));
        assert_eq!(chosen_size("3840x2160"), None);
    }

    #[test]
    fn presets_already_fit_1080p() {
        for ([width, height], _) in SIZE_PRESETS {
            assert_eq!(
                nanatype_settings::fit_output_size(f64::from(width), f64::from(height)),
                Some([width, height])
            );
        }
    }
}
