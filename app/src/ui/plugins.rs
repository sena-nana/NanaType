//! Host-owned widgets and paths generated from a validated, retained value tree.
use crate::{
    msg::{FileTarget, Msg},
    ui::Ui,
};
use nana_ui::runtime::view::{AnyView, IntoView, dynamic, widget};
use nana_ui::runtime::{
    Activate, Button, Checkbox, LayoutBox, PaintContext, PaintPath, Painter, Stack, Text,
    ToggleChanged,
};
use nana_ui_core::{LengthSpec, OverflowSpec};
use nanatype_plugins::{DrawCommand, Mount, Node};
use std::hash::{Hash, Hasher};

struct Canvas {
    draw: Vec<DrawCommand>,
    key: u64,
}
impl Painter for Canvas {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        let bounds = cx.bounds();
        cx.save();
        cx.translate(bounds.x, bounds.y);
        for command in &self.draw {
            let mut path = PaintPath::new();
            let (color, stroke) = match command {
                DrawCommand::Rect {
                    rect,
                    color,
                    radius,
                } => {
                    path.rounded_rect(
                        LayoutBox {
                            x: rect[0],
                            y: rect[1],
                            width: rect[2],
                            height: rect[3],
                        },
                        [*radius; 4],
                    );
                    (*color, 0.)
                }
                DrawCommand::Path {
                    points,
                    color,
                    closed,
                    width,
                } => {
                    if let Some(first) = points.first() {
                        path.move_to(first[0], first[1]);
                        for p in &points[1..] {
                            path.line_to(p[0], p[1]);
                        }
                    }
                    if *closed {
                        path.close();
                    }
                    (*color, *width)
                }
            };
            let color = color.map(|c| f32::from(c) / 255.);
            if stroke > 0. {
                cx.stroke_path(&path, stroke, color);
            } else {
                cx.fill_path(&path, color);
            }
        }
        cx.restore();
    }
    fn paint_key(&self) -> u64 {
        self.key
    }
}

fn node(package: &str, n: &Node) -> AnyView {
    match n {
        Node::Column { id, children } | Node::Row { id, children } => {
            let stack = if matches!(n, Node::Row { .. }) {
                Stack::row(6.)
            } else {
                Stack::column(6.)
            };
            widget(stack)
                .key(format!("plugin:{package}:{id}"))
                .children(
                    children
                        .iter()
                        .map(|child| node(package, child))
                        .collect::<Vec<_>>(),
                )
                .into_any()
        }
        Node::Text { id, text } => widget(Text::new(text.clone()))
            .key(format!("plugin:{package}:{id}"))
            .into_any(),
        Node::Button { id, text, command } => {
            let package = package.to_owned();
            let command = command.clone();
            widget(Button::new(text.clone()))
                .key(format!("plugin:{package}:{id}"))
                .on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program_all(Msg::PluginInput {
                        id: package.clone(),
                        event: serde_json::json!({"type":"command","id":command}),
                    });
                })
                .into_any()
        }
        Node::Canvas {
            id,
            width,
            height,
            draw,
        } => {
            let mut hash = std::collections::hash_map::DefaultHasher::new();
            format!("{draw:?}").hash(&mut hash);
            widget(
                Stack::column(0.)
                    .width(LengthSpec::Px(*width))
                    .height(LengthSpec::Px(*height))
                    .painter(Canvas {
                        draw: draw.clone(),
                        key: hash.finish(),
                    })
                    .with_layout(|l| {
                        l.overflow_x = OverflowSpec::Hidden;
                        l.overflow_y = OverflowSpec::Hidden;
                    }),
            )
            .key(format!("plugin:{package}:{id}"))
            .into_any()
        }
    }
}

pub fn mounted(ui: Ui, slot: Mount) -> AnyView {
    dynamic(
        move || ui.plugin_views.get(),
        move |updates| {
            let mut packages: Vec<_> = updates.iter().collect();
            packages.sort_by_key(|(id, _)| *id);
            let views = packages
                .into_iter()
                .flat_map(|(id, update)| {
                    update
                        .views
                        .iter()
                        .filter(move |view| view.slot == slot)
                        .map(move |view| node(id, &view.root))
                })
                .collect::<Vec<_>>();
            widget(Stack::column(0.)).children(views).into_any()
        },
    )
    .into_any()
}

pub fn settings(ui: Ui) -> AnyView {
    let toolbar = widget(Stack::row(8.).wrap(true)).children((
        widget(Button::new(ui.now_t("packages-import"))).on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program_all(Msg::PickFile(FileTarget::ImportSkin))
        }),
        widget(Button::new(ui.now_t("packages-refresh")))
            .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(Msg::RefreshPackages)),
    ));
    let packages = dynamic(
        move || (ui.packages.get(), ui.settings.get(), ui.plugin_errors.get()),
        move |(rows, settings, errors)| {
            let rows = rows
                .iter()
                .map(|row| {
                    let package = &row.manifest;
                    let id = package.meta.id.clone();
                    let name = nanatype_theme::localized(
                        &package.meta.name,
                        ui.locale.get_untracked().tag(),
                        &id,
                    );
                    let enable_id = id.clone();
                    let export_id = id.clone();
                    let remove_id = id.clone();
                    let functional = package
                        .capabilities
                        .contains(&nanatype_theme::PackageCapability::Lua);
                    let enabled = settings.enabled_plugins.contains(&id);
                    let toggle = functional.then(|| {
                        widget(Button::new(ui.now_t("packages-review-enable")))
                            .key(format!("package-review:{id}"))
                            .on_cx(move |_, _: &Activate, cx| {
                                cx.dispatch_program_all(Msg::RequestPluginActivation(
                                    enable_id.clone(),
                                ));
                            })
                            .into_any()
                    });
                    let disable_id = id.clone();
                    let integrity = integrity_label(ui, &row.integrity);
                    let dependencies = (!package.dependencies.is_empty()).then(|| {
                        widget(readable(format!(
                            "{}: {}",
                            ui.now_t("packages-dependencies"),
                            package
                                .dependencies
                                .iter()
                                .map(|(id, version)| format!("{id}@{version}"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        )))
                    });
                    let error = errors.get(&id).map(|e| widget(Text::new(e.clone())));
                    let capabilities = package
                        .capabilities
                        .iter()
                        .map(|cap| {
                            ui.now_t(match cap {
                                nanatype_theme::PackageCapability::Skin => "packages-kind-skin",
                                nanatype_theme::PackageCapability::Resources => {
                                    "packages-kind-resources"
                                }
                                nanatype_theme::PackageCapability::Lua => "packages-kind-function",
                            })
                        })
                        .collect::<Vec<_>>()
                        .join(" · ");
                    widget(Stack::column(6.).padding_xy(12., 12.))
                        .key(id.clone())
                        .children((
                            widget(readable(format!("{} · {}", name, package.meta.version))),
                            widget(Text::new(id.clone())),
                            widget(Text::new(capabilities)),
                            widget(readable(format!(
                                "{}: {} · {}: {}",
                                ui.now_t("package-author"),
                                provided(ui, &package.meta.author),
                                ui.now_t("package-created-at"),
                                provided(ui, &package.created_at)
                            ))),
                            widget(readable(format!(
                                "{}: {} · {}: {}",
                                ui.now_t("package-license"),
                                provided(ui, &package.meta.license),
                                ui.now_t("package-licensed-to"),
                                provided(ui, &package.licensed_to)
                            ))),
                            widget(Text::new(integrity)),
                            dependencies,
                            error,
                            widget(Stack::row(8.).wrap(true)).children((
                                toggle,
                                (functional && enabled).then(|| {
                                    widget(Button::new(ui.now_t("packages-disable"))).on_cx(
                                        move |_, _: &Activate, cx| {
                                            cx.dispatch_program_all(Msg::DisablePlugin(
                                                disable_id.clone(),
                                            ))
                                        },
                                    )
                                }),
                                widget(Button::new(ui.now_t("skins-export"))).on_cx(
                                    move |_, _: &Activate, cx| {
                                        cx.dispatch_program_all(Msg::ExportSkin(export_id.clone()))
                                    },
                                ),
                                (row.origin != nanatype_theme::SkinOrigin::Builtin).then(|| {
                                    widget(Button::new(ui.now_t("packages-remove"))).on_cx(
                                        move |_, _: &Activate, cx| {
                                            cx.dispatch_program_all(Msg::RemovePackage(
                                                remove_id.clone(),
                                            ))
                                        },
                                    )
                                }),
                            )),
                        ))
                        .into_any()
                })
                .collect::<Vec<_>>();
            widget(Stack::column(6.)).children(rows).into_any()
        },
    );
    let commands = dynamic(
        move || ui.plugin_views.get(),
        move |updates| {
            let mut buttons = Vec::new();
            let mut packages: Vec<_> = updates.iter().collect();
            packages.sort_by_key(|(id, _)| *id);
            for (id, update) in packages {
                for command in &update.commands {
                    let id = id.clone();
                    let command_id = command.id.clone();
                    buttons.push(
                        widget(Button::new(command.label.clone()))
                            .on_cx(move |_, _: &Activate, cx| {
                                cx.dispatch_program_all(Msg::PluginCommand {
                                    id: id.clone(),
                                    command: command_id.clone(),
                                })
                            })
                            .into_any(),
                    );
                }
            }
            widget(Stack::column(6.)).children(buttons).into_any()
        },
    );
    widget(Stack::column(12.))
        .children((
            toolbar,
            widget(Text::new(ui.now_t("packages-access"))),
            activation(ui),
            packages,
            commands,
            mounted(ui, Mount::Panel),
        ))
        .into_any()
}

fn provided(ui: Ui, text: &str) -> String {
    if text.is_empty() {
        ui.now_t("package-unspecified")
    } else {
        text.into()
    }
}
fn readable(value: impl Into<String>) -> Text {
    let mut text = Text::new(value);
    let layout = std::sync::Arc::make_mut(&mut text.style.layout);
    layout.overflow_wrap = Some(nana_ui_core::OverflowWrapSpec::Anywhere);
    layout.white_space = nana_ui_core::WhiteSpaceSpec::PreWrap;
    text
}
pub fn integrity_label(
    ui: Ui,
    status: &Result<Option<nanatype_theme::SealStatus>, String>,
) -> String {
    match status {
        Ok(Some(s)) => format!(
            "{} · {} · {} {}",
            ui.now_t("package-signature-valid"),
            fingerprint(&s.fingerprint),
            s.protected.len(),
            ui.now_t("package-protected-files")
        ),
        Ok(None) => ui.now_t("package-unsigned"),
        Err(error) => format!("{}: {error}", ui.now_t("package-integrity-failed")),
    }
}
fn fingerprint(hash: &str) -> String {
    hash.as_bytes()
        .chunks(8)
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}
/// Every declared read with its owner, format, size, source, license and signature coverage.
fn reads(ui: Ui, data: &nanatype_theme::DataAccess) -> String {
    let rows: Vec<_> = data
        .disclosures()
        .map(|d| {
            format!(
                "• {} · {} · {} v{} · {} B\n  {}: {}\n  {}: {}\n  {}",
                d.id,
                d.owner,
                d.format.name(),
                d.format_version,
                d.bytes,
                ui.now_t("package-data-source"),
                d.source,
                ui.now_t("package-data-license"),
                d.license,
                ui.now_t(if d.protected {
                    "package-data-protected"
                } else {
                    "package-data-unprotected"
                }),
            )
        })
        .collect();
    if rows.is_empty() {
        ui.now_t("packages-consent-reads-none")
    } else {
        format!(
            "{}\n{}",
            ui.now_t("packages-consent-reads"),
            rows.join("\n")
        )
    }
}
pub fn activation(ui: Ui) -> AnyView {
    let show_source = nana_ui::runtime::view::signal(false);
    dynamic(
        move || (ui.plugin_activation.get(), show_source.get()),
        move |(review, show)| {
            let Some(review) = review else {
                return widget(Stack::column(0.)).into_any();
            };
            let source = if *show {
                review.source.clone()
            } else {
                review.source.chars().take(16000).collect::<String>()
            };
            widget(Stack::column(8.).padding_xy(16., 16.))
                .key("plugin-activation-review")
                .children((
                    widget(Text::new(ui.now_t("packages-consent-title"))),
                    widget(readable(format!(
                        "{} · {}\n{}: {}\n{}: {}\n{}: {}\n{}: {}",
                        review.id,
                        review.manifest.meta.version,
                        ui.now_t("package-author"),
                        provided(ui, &review.manifest.meta.author),
                        ui.now_t("package-created-at"),
                        provided(ui, &review.manifest.created_at),
                        ui.now_t("package-licensed-to"),
                        provided(ui, &review.manifest.licensed_to),
                        ui.now_t("package-license"),
                        provided(ui, &review.manifest.meta.license)
                    ))),
                    widget(Text::new(integrity_label(
                        ui,
                        &Ok(review.integrity.clone()),
                    ))),
                    widget(Text::new(ui.now_t("packages-consent-risk"))),
                    widget(Text::new(ui.now_t("packages-consent-data"))),
                    widget(readable(reads(ui, &review.data))).key("plugin-consent-reads"),
                    widget(Stack::column(4.)).children((
                        widget(readable(format!(
                            "{}: {}",
                            ui.now_t("packages-location"),
                            review.location
                        ))),
                        widget(readable(format!(
                            "{}: {}",
                            ui.now_t("packages-script-entry"),
                            review.manifest.entry().unwrap_or("")
                        ))),
                        widget(Text::new(ui.now_t(
                            if review.integrity.as_ref().is_some_and(|seal| {
                                review
                                    .manifest
                                    .entry()
                                    .is_some_and(|entry| seal.protected.contains(entry))
                            }) {
                                "packages-entry-protected"
                            } else {
                                "packages-entry-unprotected"
                            },
                        ))),
                        widget(readable(format!(
                            "{}: {}",
                            ui.now_t("packages-content-digest"),
                            fingerprint(&review.digest)
                        ))),
                    )),
                    widget(Button::new(ui.now_t("packages-review-source")))
                        .on(move |_: &Activate| show_source.set(true)),
                    widget(
                        nana_ui::runtime::TextArea::new(source)
                            .read_only(true)
                            .height(160.),
                    )
                    .key("plugin-source-code"),
                    widget(Text::new(ui.now_t("packages-source-preview-limit"))),
                    widget(Checkbox::new(ui.now_t("packages-source-trusted"), false))
                        .key("plugin-source-trusted")
                        .bind(move |s: &mut Checkbox| s.checked = ui.plugin_source_trusted.get())
                        .on(move |e: &ToggleChanged| ui.plugin_source_trusted.set(e.checked)),
                    widget(Stack::row(8.).wrap(true)).children((
                        widget(Button::new(ui.now_t("packages-consent-confirm")))
                            .key("plugin-consent-confirm")
                            .bind(move |button: &mut Button| {
                                button.disabled = !ui.plugin_source_trusted.get()
                            })
                            .on_cx(|_, _: &Activate, cx| {
                                cx.dispatch_program_all(Msg::ConfirmPluginActivation)
                            }),
                        widget(Button::new(ui.now_t("packages-consent-cancel"))).on_cx(
                            |_, _: &Activate, cx| {
                                cx.dispatch_program_all(Msg::CancelPluginActivation)
                            },
                        ),
                    )),
                ))
                .into_any()
        },
    )
    .into_any()
}
