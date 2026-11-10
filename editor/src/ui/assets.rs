use super::{Ui, controls::hint, kit};
use crate::{msg::Msg, session::Action};
use nana_ui::icons_tabler as ti;
use nana_ui::runtime::view::{AnyView, IntoView, dynamic, signal, widget};
use nana_ui::runtime::{Activate, Stack};

/// The audition volume's slider.
const VOLUME_W: f32 = 140.0;
use nana_ui_core::{
    BackgroundImage, BackgroundImageFit, LengthSpec, PointerEventsSpec, PositionSpec,
    SemanticColorRole,
};
/// The selected asset: an image on a checkerboard with its nine-slice
/// guides, a font's sample, or a sound's transport.
pub fn preview(ui: Ui) -> AnyView {
    dynamic(
        move || {
            ui.asset
                .with(|a| a.as_ref().map(|a| std::sync::Arc::as_ptr(a) as usize))
        },
        move |_| {
            let Some(asset) = ui.asset.get_untracked() else {
                return widget(Stack::column(0.)).into_any();
            };
            let mut children = Vec::new();
            if let Some(url) = asset.image.clone() {
                let width = signal(300_f32);
                let dimensions = asset.dimensions.unwrap_or([1, 1]);
                let slice = ui.package_editor.with_untracked(|v| {
                    v.as_ref().and_then(|v| {
                        v.report
                            .references
                            .iter()
                            .filter(|r| r.target == asset.path && r.automatic)
                            .find_map(|r| {
                                let mut path = r.path.clone();
                                path.pop();
                                path.push("slice".into());
                                let table = v
                                    .documents
                                    .get(&r.file)
                                    .and_then(|s| nanatype_theme::editor::parse_document(s).ok())?;
                                let values =
                                    nanatype_theme::editor::value_at(&table, &path)?.as_array()?;
                                let values = values
                                    .iter()
                                    .map(|v| {
                                        v.as_integer()
                                            .map(|i| i as f32)
                                            .or_else(|| v.as_float().map(|f| f as f32))
                                    })
                                    .collect::<Option<Vec<_>>>()?;
                                <[f32; 4]>::try_from(values).ok()
                            })
                    })
                });
                children.push(
                    widget(
                        Stack::column(0.)
                            .height(LengthSpec::Px(220.))
                            .width(LengthSpec::Fill)
                            .with_layout(move |l| {
                                l.position = PositionSpec::Relative;
                                l.paint.background_layers = vec![BackgroundImage::url(
                                    kit::checker_url(ui.chrome_dark.get_untracked()),
                                )];
                                l.paint.background_image = Some(BackgroundImage::url_with_fit(
                                    url,
                                    BackgroundImageFit::Contain,
                                ));
                            }),
                    )
                    .key("editor-asset-image")
                    .on(move |e: &nana_ui::runtime::SizeChanged| {
                        width.set_if_changed(e.width);
                    })
                    .children(dynamic(
                        move || width.get(),
                        move |width| {
                            let mut lines = Vec::new();
                            if let Some([top, right, bottom, left]) = slice {
                                let [iw, ih] = dimensions.map(|v| v as f32);
                                let scale = (*width / iw).min(220. / ih);
                                let (x, y) = ((*width - iw * scale) / 2., (220. - ih * scale) / 2.);
                                for (x, y, w, h) in [
                                    (x + left * scale, y, 1., ih * scale),
                                    (x + (iw - right) * scale, y, 1., ih * scale),
                                    (x, y + top * scale, iw * scale, 1.),
                                    (x, y + (ih - bottom) * scale, iw * scale, 1.),
                                ] {
                                    lines.push(
                                        widget(
                                            Stack::column(0.)
                                                .width(LengthSpec::Px(w))
                                                .height(LengthSpec::Px(h))
                                                .surface(SemanticColorRole::Accent)
                                                .with_layout(move |l| {
                                                    l.position = PositionSpec::Absolute;
                                                    l.offset_left = Some(LengthSpec::Px(x));
                                                    l.offset_top = Some(LengthSpec::Px(y));
                                                    l.pointer_events =
                                                        Some(PointerEventsSpec::None);
                                                }),
                                        )
                                        .into_any(),
                                    );
                                }
                            }
                            widget(Stack::column(0.)).children(lines).into_any()
                        },
                    ))
                    .into_any(),
                );
            }
            if let Some(family) = asset.font.as_ref().and_then(|f| f.family.clone()) {
                children.push(
                    widget(kit::Type::Body.text(ui.now_t("skin-edit-sample")))
                        .bind(move |t: &mut nana_ui::runtime::Text| {
                            let l = std::sync::Arc::make_mut(&mut t.style.layout);
                            l.font_family = Some(family.clone());
                            l.font_size = Some(24.);
                        })
                        .key("editor-asset-font")
                        .into_any(),
                );
            }
            if let Some(pcm) = &asset.pcm {
                children.push(player(
                    ui,
                    format!(
                        "{} ms · {} Hz · {} ch",
                        pcm.duration_ms(),
                        pcm.rate,
                        pcm.channels
                    ),
                ));
            }
            widget(Stack::column(0.).gap_step(kit::gap::step::CONTROL))
                .children(children)
                .into_any()
        },
    )
    .into_any()
}

/// A sound's transport: play, stop, the audition volume and what was decoded.
fn player(ui: Ui, facts: String) -> AnyView {
    use nana_ui::runtime::{RangeChanged, RangeField, RangeInput};
    let mut volume = RangeField::new(
        f64::from(ui.audio_gain.get_untracked()) * 100.,
        0.,
        100.,
        1.,
    )
    .size(nana_ui::ControlSize::Small)
    .show_label(false);
    {
        let l = std::sync::Arc::make_mut(&mut volume.style.layout);
        l.width = Some(LengthSpec::Px(VOLUME_W));
        l.flex_shrink = Some(1.);
    }
    widget(
        Stack::row(0.)
            .wrap(true)
            .gap_step(kit::gap::step::TIGHT)
            .align(nana_ui_core::AlignSpec::Center),
    )
    .key("editor-asset-player")
    .children((
        widget(super::shell::tool(
            ti::PLAYER_PLAY,
            ui.now_t("pkgedit-play"),
        ))
        .key("editor-asset-play")
        .on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program_all(Msg::PackageEditor(Action::PlayAsset))
        }),
        widget(super::shell::tool(
            ti::PLAYER_STOP,
            ui.now_t("pkgedit-stop"),
        ))
        .key("editor-asset-stop")
        .on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program_all(Msg::PackageEditor(Action::StopAudio))
        }),
        widget(volume.accessible_name(ui.now_t("pkgedit-volume")))
            .key("editor-asset-volume")
            .on(move |e: &RangeInput| ui.audio_gain.set((e.value / 100.) as f32))
            .on(move |e: &RangeChanged| ui.audio_gain.set((e.value / 100.) as f32)),
        hint(facts),
    ))
    .into_any()
}
