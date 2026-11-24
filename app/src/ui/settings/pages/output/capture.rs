use crate::msg::ObsMsg;

use super::super::*;
use super::spout::size_text;
use super::{Read, Write};

/// The OBS source choices: none, then the program scene's inputs (capture
/// sources first), then the saved source when this scene lacks it.
fn source_options(ui: Ui, read: Read) -> Vec<(String, String)> {
    let locale = ui.locale.get();
    let saved = ui.settings.with(|s| read(s).obs_source.clone());
    let mut options = vec![(String::new(), I18N.t_in(locale, "obs-source-none"))];
    ui.obs.with(|state| {
        if let Some(scene) = state.scene.as_deref() {
            for item in crate::app::obs::candidates(scene) {
                options.push((item.source.clone(), item.source.clone()));
            }
        }
    });
    if !saved.is_empty() && !options.iter().any(|(v, _)| *v == saved) {
        let label = I18N.tr_in(
            locale,
            "obs-source-absent",
            &[("name", saved.clone().into())],
        );
        options.push((saved, label));
    }
    options
}

/// What sets the output's size now, as the source row's hint.
pub(super) fn sizing_hint(ui: Ui, read: Read) -> Option<String> {
    use crate::app::obs::Sizing;
    let locale = ui.locale.get();
    let state = ui.obs.get();
    let sizing = ui
        .settings
        .with(|s| crate::app::obs::sizing(s, &state, read(s)));
    let scene_arg = |scene: String| [("scene", scene.into())];
    Some(match sizing {
        Sizing::Manual => I18N.t_in(locale, "obs-size-pick"),
        Sizing::Offline => I18N.t_in(locale, "obs-size-offline"),
        Sizing::Missing { scene } => I18N.tr_in(locale, "obs-size-missing", &scene_arg(scene)),
        Sizing::NoBounds { scene } => I18N.tr_in(locale, "obs-size-no-bounds", &scene_arg(scene)),
        Sizing::Following { bounds, size } => {
            let boxed = [bounds[0].round() as u32, bounds[1].round() as u32];
            if boxed == size {
                I18N.tr_in(
                    locale,
                    "obs-size-following",
                    &[("size", size_text(size).into())],
                )
            } else {
                I18N.tr_in(
                    locale,
                    "obs-size-clamped",
                    &[
                        ("box", size_text(boxed).into()),
                        ("size", size_text(size).into()),
                    ],
                )
            }
        }
    })
}

/// Which OBS source this output is, and where its size comes from; a
/// source without a bounding box gets a notice card below the row, with a
/// button that turns one on.
pub(super) fn obs_source_row(ui: Ui, kind: WindowKind, read: Read, write: Write) -> Row {
    let row = row_live_hint(
        ui,
        "obs-source",
        move || sizing_hint(ui, read),
        source_select(ui, read, write),
    );
    let notice = widget(
        Stack::column(0.0)
            .padding_step(inset::step::CARD, inset::step::ROW_Y)
            .with_layout(|l| l.padding_top = Some(LengthSpec::Px(0.0))),
    )
    .children(bounds_notice(ui, kind, read));
    Row::inset(widget(Stack::column(0.0)).children((row, notice)))
}

/// The select of the OBS source an output is.
pub(super) fn source_select(ui: Ui, read: Read, write: Write) -> AnyView {
    live_select(
        ui,
        move || source_options(ui, read),
        move |s| read(s).obs_source.clone(),
        move |s, v| write(s).obs_source = v.to_owned(),
    )
}

/// While the output's source has no bounding box: a notice with a button
/// that turns one on.
pub(super) fn bounds_notice(ui: Ui, kind: WindowKind, read: Read) -> AnyView {
    let no_bounds = move || {
        let state = ui.obs.get();
        ui.settings.with(|s| {
            matches!(
                crate::app::obs::sizing(s, &state, read(s)),
                crate::app::obs::Sizing::NoBounds { .. }
            )
        })
    };
    widget(Stack::column(0.0))
        .visible(no_bounds)
        .children(
            // One line beside a taller button: centred, not top-aligned.
            nana_ui::runtime::view::callout(
                ti::ALERT_TRIANGLE,
                StatusTone::Warning,
                ui.now_t("obs-bounds-notice"),
            )
            .trailing(primary(ui, "obs-fit-bounds", move || {
                Msg::Obs(ObsMsg::FitBounds(kind))
            }))
            .css(nana_ui::runtime::view::css! { align-items: center; }),
        )
        .into_any()
}

/// The display's OBS source: its select, what sizes it now, and the notice
/// for a source without a bounding box.
pub(in crate::ui::settings::pages) fn display_source(
    ui: Ui,
) -> (AnyView, impl Fn() -> Option<String> + Send + Copy, AnyView) {
    let read: Read = |s| &s.output.display;
    (
        source_select(ui, read, |s| &mut s.output.display),
        move || sizing_hint(ui, read),
        bounds_notice(ui, WindowKind::Display, read),
    )
}
