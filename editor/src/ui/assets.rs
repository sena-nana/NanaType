use super::{
    Ui,
    controls::{confirm, hint},
    kit,
};
use crate::{msg::Msg, session::Action};
use nana_ui::runtime::view::{AnyView, IntoView, dynamic, signal, widget};
use nana_ui::runtime::{Activate, Button, Stack, TextChanged};
use nana_ui::{ButtonKind, ControlSize};
use nana_ui_core::{
    BackgroundImage, BackgroundImageFit, LengthSpec, PointerEventsSpec, PositionSpec,
    SemanticColorRole,
};
pub fn operations(ui: Ui) -> AnyView {
    let target = ui.file_target;
    widget(Stack::column(0.).gap_step(kit::gap::step::CONTROL))
        .children((
            widget(
                Button::new(ui.now_t("pkgedit-reload-directory"))
                    .kind(ButtonKind::Ghost)
                    .size(ControlSize::Small),
            )
            .visible(move || {
                ui.package_editor.with(|v| {
                    v.as_ref().is_some_and(|v| {
                        matches!(
                            v.origin,
                            nanatype_theme::editor::DraftOrigin::Directory { .. }
                        )
                    })
                })
            })
            .on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program_all(Msg::PackageEditor(Action::ReloadDirectory))
            }),
            widget(
                kit::text_input(target.get_untracked())
                    .placeholder(ui.now_t("pkgedit-file-target"))
                    .size(ControlSize::Small),
            )
            .key("editor-file-target")
            .bind(move |a: &mut nana_ui::runtime::TextInput| {
                super::views::sync_text(&mut a.state, target.get())
            })
            .on(move |e: &TextChanged| target.set(e.value.to_string())),
            widget(
                Button::new(ui.now_t("pkgedit-new-file"))
                    .kind(ButtonKind::Ghost)
                    .size(ControlSize::Small),
            )
            .key("editor-new-file")
            .on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program_all(Msg::PackageEditor(Action::CreateFile(
                    target.get_untracked(),
                )))
            }),
            dynamic(
                move || {
                    ui.package_editor.with(|v| {
                        v.as_ref()
                            .map(|v| (v.selected.clone(), v.version, v.report.references.clone()))
                    })
                },
                move |state| {
                    let Some((Some(path), _, refs)) = state else {
                        return widget(Stack::column(0.)).into_any();
                    };
                    let from = path.clone();
                    let refs = refs
                        .iter()
                        .filter(|r| r.target == *path)
                        .map(|r| {
                            format!(
                                "{}: {} {}",
                                r.file,
                                r.path.join("."),
                                if r.automatic {
                                    ui.now_t("pkgedit-reference-auto")
                                } else {
                                    ui.now_t("pkgedit-reference-manual")
                                }
                            )
                        })
                        .collect::<Vec<_>>();
                    let detail = if refs.is_empty() {
                        ui.now_t("pkgedit-unused-asset")
                    } else {
                        refs.join("\n")
                    };
                    widget(Stack::column(0.).gap_step(kit::gap::step::CONTROL))
                        .children((
                            hint(detail.clone()),
                            confirm(
                                *ui,
                                format!("{}\n{detail}", ui.now_t("pkgedit-rename-question")),
                                ui.now_t("pkgedit-rename"),
                                move |armed| {
                                    widget(
                                        Button::new(ui.now_t("pkgedit-rename"))
                                            .kind(ButtonKind::Ghost)
                                            .size(ControlSize::Small),
                                    )
                                    .on(move |_: &Activate| armed.set(true))
                                    .into_any()
                                },
                                move || {
                                    Msg::PackageEditor(Action::RenameFile {
                                        from: from.clone(),
                                        to: target.get_untracked(),
                                    })
                                },
                            ),
                            widget(
                                Button::new(ui.now_t("pkgedit-replace"))
                                    .kind(ButtonKind::Ghost)
                                    .size(ControlSize::Small),
                            )
                            .on_cx(|_, _: &Activate, cx| {
                                cx.dispatch_program_all(Msg::PackageEditor(Action::ReplaceFile))
                            }),
                        ))
                        .into_any()
                },
            ),
        ))
        .into_any()
}
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
                children.push(hint(format!(
                    "{} ms · {} Hz · {} ch",
                    pcm.duration_ms(),
                    pcm.rate,
                    pcm.channels
                )));
                children.push(
                    widget(Stack::row(0.).wrap(true))
                        .children((
                            widget(Button::new(ui.now_t("pkgedit-play"))).on_cx(
                                |_, _: &Activate, cx| {
                                    cx.dispatch_program_all(Msg::PackageEditor(Action::PlayAsset))
                                },
                            ),
                            widget(Button::new(ui.now_t("pkgedit-static"))).on_cx(
                                |_, _: &Activate, cx| {
                                    cx.dispatch_program_all(Msg::PackageEditor(Action::StopAudio))
                                },
                            ),
                            widget(
                                kit::text_input((ui.audio_gain.get_untracked() * 100.).to_string())
                                    .placeholder("0–100%")
                                    .size(ControlSize::Small),
                            )
                            .on(move |e: &TextChanged| {
                                if let Ok(v) = e.value.parse::<f32>()
                                    && v.is_finite()
                                {
                                    ui.audio_gain.set((v / 100.).clamp(0., 1.));
                                }
                            }),
                        ))
                        .into_any(),
                );
            }
            widget(Stack::column(0.).gap_step(kit::gap::step::CONTROL))
                .children(children)
                .into_any()
        },
    )
    .into_any()
}
