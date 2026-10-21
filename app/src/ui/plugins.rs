//! Host-owned widgets and paths generated from a validated, retained value tree.
use crate::ui::kit::{self, Tone, Type, gap, inset};
use crate::{
    msg::{FileTarget, Msg},
    ui::Ui,
};
use nana_ui::icons_tabler as ti;
use nana_ui::runtime::view::{AnyView, IntoView, dynamic, widget};
use nana_ui::runtime::{Activate, Button, Checkbox, Stack, Text, ToggleChanged};
use nana_ui::{ButtonKind, ControlSize};
use nana_ui_core::{AlignSpec, LengthSpec, RadiusTier, SemanticColorRole};
use nanatype_plugins::Mount;
use std::sync::Arc;

/// The script source shown for review before it is trusted.
const SOURCE_PREVIEW_H: f32 = 160.0;
/// A package's lead mark on its card.
const PACKAGE_TILE: f32 = 36.0;

pub use nanatype_frame::ui::plugin_view::{fingerprint, integrity_label};

/// The live plugins' views at `slot`; their events go back to the plugin.
pub fn mounted(ui: Ui, slot: Mount) -> AnyView {
    nanatype_frame::ui::plugin_view::mounted(*ui, slot, |id| {
        Arc::new(move |event| Msg::PluginInput {
            id: id.clone(),
            event,
        })
    })
}

/// The Packages page's header actions: import a package, reload them all.
pub fn actions(ui: Ui) -> AnyView {
    kit::hstack((
        kit::button(ui.now_t("packages-refresh"), || Msg::RefreshPackages),
        kit::cta_button(ui.now_t("packages-import"), || {
            Msg::PickFile(FileTarget::ImportSkin)
        }),
    ))
}

/// One installed package as a card: what it is and where it comes from on
/// top, its credits in the middle, what can be done with it at the foot.
fn package_card(
    ui: Ui,
    row: &crate::plugins::PackageRow,
    enabled: bool,
    error: Option<&String>,
) -> AnyView {
    use nanatype_theme::PackageCapability as Cap;
    let package = &row.manifest;
    let id = package.meta.id.clone();
    let name = nanatype_theme::localized(&package.meta.name, ui.locale.get_untracked().tag(), &id);
    let functional = package.capabilities.contains(&Cap::Lua);
    let builtin = row.origin == nanatype_theme::SkinOrigin::Builtin;
    let glyph = if functional {
        ti::CODE
    } else if package.capabilities.contains(&Cap::Skin) {
        ti::PALETTE
    } else {
        ti::PACKAGE
    };
    let integrity_tone = match &row.integrity {
        Ok(Some(_)) => Tone::Success,
        Ok(None) => Tone::Neutral,
        Err(_) => Tone::Danger,
    };
    let tags: Vec<AnyView> = package
        .capabilities
        .iter()
        .map(|cap| {
            kit::tag(ui.now_t(match cap {
                Cap::Skin => "packages-kind-skin",
                Cap::Resources => "packages-kind-resources",
                Cap::Lua => "packages-kind-function",
            }))
        })
        .collect();
    let dependencies = (!package.dependencies.is_empty()).then(|| {
        widget(caption(format!(
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
    let (enable_id, disable_id, export_id, remove_id) =
        (id.clone(), id.clone(), id.clone(), id.clone());
    let toggle = functional.then(|| {
        if enabled {
            widget(Button::new(ui.now_t("packages-disable")).size(ControlSize::Small))
                .on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program_all(Msg::DisablePlugin(disable_id.clone()))
                })
                .into_any()
        } else {
            widget(Button::new(ui.now_t("packages-review-enable")).size(ControlSize::Small))
                .key(format!("package-review:{id}"))
                .bind(move |button: &mut Button| button.disabled = ui.plugin_activation_busy.get())
                .on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program_all(Msg::RequestPluginActivation(enable_id.clone()));
                })
                .into_any()
        }
    });
    widget(kit::card_frame(RadiusTier::Md))
        .key(id.clone())
        .children((
            kit::line(
                widget(
                    Stack::row(0.0)
                        .gap_step(gap::step::GROUP)
                        .align(AlignSpec::Center),
                )
                .children((
                    kit::icon_tile(
                        glyph,
                        PACKAGE_TILE,
                        if enabled { Tone::Accent } else { Tone::Neutral },
                    ),
                    widget(crate::ui::kit::grow_column(gap::HAIR)).children((
                        widget(readable(name).style(Type::Strong.style())),
                        widget(
                            Text::new(format!("{id} · {}", package.meta.version))
                                .style(Type::Caption.mono())
                                .color(SemanticColorRole::Muted),
                        ),
                    )),
                )),
                kit::badge(integrity_label(*ui, &row.integrity), integrity_tone),
            ),
            widget(
                Stack::column(0.0)
                    .gap_step(gap::step::TIGHT)
                    .padding_xy(inset::CARD, 0.0)
                    .with_layout(|l| l.padding_bottom = Some(LengthSpec::Px(inset::ROW_Y))),
            )
            .children((
                widget(Stack::row(0.0).gap_step(gap::step::TIGHT).wrap(true)).children(tags),
                widget(caption(format!(
                    "{}: {} · {}: {}",
                    ui.now_t("package-author"),
                    provided(ui, &package.meta.author),
                    ui.now_t("package-created-at"),
                    provided(ui, &package.created_at)
                ))),
                widget(caption(format!(
                    "{}: {} · {}: {}",
                    ui.now_t("package-license"),
                    provided(ui, &package.meta.license),
                    ui.now_t("package-licensed-to"),
                    provided(ui, &package.licensed_to)
                ))),
                dependencies,
                error.map(|e| kit::callout(Tone::Danger, e.clone())),
            )),
            kit::rule(),
            widget(
                Stack::row(0.0)
                    .gap_step(gap::step::CONTROL)
                    .wrap(true)
                    .padding_step(inset::step::CARD, inset::step::BAR_Y)
                    .align(AlignSpec::Center)
                    .justify(nana_ui_core::JustifySpec::End),
            )
            .children((
                (!builtin).then(|| {
                    widget(
                        Button::new(ui.now_t("packages-remove"))
                            .kind(ButtonKind::Text)
                            .size(ControlSize::Small),
                    )
                    .on_cx(move |_, _: &Activate, cx| {
                        cx.dispatch_program_all(Msg::RemovePackage(remove_id.clone()))
                    })
                }),
                kit::spacer(),
                widget(Button::new(ui.now_t("skins-export")).size(ControlSize::Small)).on_cx(
                    move |_, _: &Activate, cx| {
                        cx.dispatch_program_all(Msg::ExportSkin(export_id.clone()))
                    },
                ),
                toggle,
            )),
        ))
        .into_any()
}

pub fn settings(ui: Ui) -> AnyView {
    let packages = dynamic(
        move || (ui.packages.get(), ui.settings.get(), ui.plugin_errors.get()),
        move |(rows, settings, errors)| {
            let cards = rows
                .iter()
                .map(|row| {
                    let id = &row.manifest.meta.id;
                    package_card(
                        ui,
                        row,
                        settings.enabled_plugins.contains(id),
                        errors.get(id),
                    )
                })
                .collect::<Vec<_>>();
            if cards.is_empty() {
                return kit::empty_state::<Msg>(ui.now_t("packages-none"), None);
            }
            widget(Stack::column(0.0).gap_step(gap::step::GROUP))
                .children(cards)
                .into_any()
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
                        widget(Button::new(command.label.clone()).size(ControlSize::Small))
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
            widget(Stack::row(0.0).gap_step(gap::step::CONTROL).wrap(true))
                .children(buttons)
                .into_any()
        },
    );
    // Sections with nothing in them take no room, so the page keeps one
    // rhythm whichever are showing.
    let reviewing = move || {
        ui.plugin_activation.with(Option::is_some)
            || ui.plugin_activation_busy.get()
            || ui.plugin_activation_error.with(Option::is_some)
    };
    let has_commands = move || {
        ui.plugin_views
            .with(|v| v.values().any(|u| !u.commands.is_empty()))
    };
    let has_panels = move || {
        ui.plugin_views.with(|v| {
            v.values()
                .any(|u| u.views.iter().any(|w| w.slot == Mount::Panel))
        })
    };
    kit::sections((
        widget(Stack::column(0.0))
            .visible(reviewing)
            .children(activation(ui)),
        widget(Stack::column(0.0).gap_step(gap::step::CONTROL)).children((
            overline(ui.now_t("packages-installed")),
            packages,
            kit::callout(Tone::Neutral, ui.now_t("packages-access")),
        )),
        widget(Stack::column(0.0))
            .visible(has_commands)
            .children(commands),
        widget(Stack::column(0.0))
            .visible(has_panels)
            .children(mounted(ui, Mount::Panel)),
    ))
}

/// A section's title, inset like the cards' contents.
fn overline(text: String) -> AnyView {
    widget(Stack::row(0.0).padding_xy(gap::TIGHT, 0.0))
        .children(Type::Overline.view(text))
        .into_any()
}

/// [`readable`] in the caption role.
fn caption(value: impl Into<String>) -> Text {
    readable(value)
        .style({
            let mut st = Type::Caption.style();
            let layout = std::sync::Arc::make_mut(&mut st.layout);
            layout.overflow_wrap = Some(nana_ui_core::OverflowWrapSpec::Anywhere);
            layout.white_space = nana_ui_core::WhiteSpaceSpec::PreWrap;
            st
        })
        .color(SemanticColorRole::Muted)
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
        move || {
            (
                ui.plugin_activation.get(),
                show_source.get(),
                ui.plugin_activation_busy.get(),
                ui.plugin_activation_error.get(),
            )
        },
        move |(review, show, busy, error)| {
            let Some(review) = review else {
                if *busy || error.is_some() {
                    return widget(kit::panel())
                        .key("plugin-activation-status")
                        .children((
                            widget(readable(
                                error
                                    .clone()
                                    .unwrap_or_else(|| ui.now_t("packages-consent-reviewing")),
                            ))
                            .key("plugin-activation-message"),
                            activation_cancel(ui),
                        ))
                        .into_any();
                }
                return widget(Stack::column(0.)).into_any();
            };
            let source = if *show {
                review.source.clone()
            } else {
                review.source.chars().take(16000).collect::<String>()
            };
            widget(kit::panel())
                .key("plugin-activation-review")
                .children((
                    Type::Title.view(ui.now_t("packages-consent-title")),
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
                    widget(readable(integrity_label(
                        *ui,
                        &Ok(review.integrity.clone()),
                    ))),
                    kit::callout(Tone::Warning, ui.now_t("packages-consent-risk")),
                    widget(Text::new(ui.now_t("packages-consent-data"))),
                    widget(readable(reads(ui, &review.data))).key("plugin-consent-reads"),
                    widget(Stack::column(0.0).gap_step(gap::step::TIGHT)).children((
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
                            .height(SOURCE_PREVIEW_H),
                    )
                    .key("plugin-source-code"),
                    widget(Stack::column(0.0).gap_step(gap::step::TIGHT)).children((
                        widget(Text::new(ui.now_t("packages-source-preview-limit"))),
                        busy.then(|| widget(readable(ui.now_t("packages-consent-confirming")))),
                    )),
                    widget(Checkbox::new(ui.now_t("packages-source-trusted"), false))
                        .key("plugin-source-trusted")
                        .bind(move |s: &mut Checkbox| {
                            s.checked = ui.plugin_source_trusted.get();
                            s.disabled = ui.plugin_activation_busy.get();
                        })
                        .on(move |e: &ToggleChanged| ui.plugin_source_trusted.set(e.checked)),
                    widget(Stack::row(0.0).gap_step(gap::step::CONTROL).wrap(true)).children((
                        widget(kit::cta(Button::new(ui.now_t("packages-consent-confirm"))))
                            .key("plugin-consent-confirm")
                            .bind(move |button: &mut Button| {
                                button.disabled = !ui.plugin_source_trusted.get()
                                    || ui.plugin_activation_busy.get()
                            })
                            .on_cx(|_, _: &Activate, cx| {
                                cx.dispatch_program_all(Msg::ConfirmPluginActivation)
                            }),
                        activation_cancel(ui),
                    )),
                ))
                .into_any()
        },
    )
    .into_any()
}

fn activation_cancel(ui: Ui) -> AnyView {
    widget(Button::new(ui.now_t("packages-consent-cancel")))
        .key("plugin-consent-cancel")
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(Msg::CancelPluginActivation))
        .into_any()
}
