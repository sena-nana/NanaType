//! Host-owned widgets and paths generated from a validated, retained value tree.
use crate::msg::{PackageMsg, SkinMsg};
use crate::ui::kit::{self, Tone, Type, gap, inset};
use crate::{
    msg::{FileTarget, Msg},
    ui::Ui,
};
use nana_ui::ButtonKind;
use nana_ui::icons_tabler as ti;
use nana_ui::runtime::view::{
    AnyView, IntoView, Signal, dynamic, segmented_option, signal, widget,
};
use nana_ui::runtime::{
    Activate, Button, Checkbox, IconButton, SegmentedControl, SegmentedOptionChosen, Stack, Switch,
    Text, ToggleChanged,
};
use nana_ui_core::ControlSize;
use nana_ui_core::{AlignSpec, LengthSpec, SemanticColorRole};
use nanatype_plugins::Mount;
use std::sync::Arc;

/// The script source shown for review before it is trusted.
const SOURCE_PREVIEW_H: f32 = 160.0;
/// A package's lead mark on its row.
const PACKAGE_TILE: f32 = 28.0;
/// The room a row's name and summary ask for before its actions take a
/// line of their own.
const IDENTITY_MIN: f32 = 200.0;

pub use nanatype_frame::ui::plugin_view::{
    Provenance, fingerprint, integrity_label, provenance_label,
};

/// The live plugins' views at `slot`; their events go back to the plugin.
pub fn mounted(ui: Ui, slot: Mount) -> AnyView {
    nanatype_frame::ui::plugin_view::mounted(*ui, slot, |id| {
        Arc::new(move |event| {
            Msg::Package(PackageMsg::PluginInput {
                id: id.clone(),
                event,
            })
        })
    })
}

/// Which installed packages the Packages page lists.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PackageFilter {
    All,
    Enabled,
    Disabled,
}

impl PackageFilter {
    fn admits(self, on: bool) -> bool {
        match self {
            Self::All => true,
            Self::Enabled => on,
            Self::Disabled => !on,
        }
    }

    /// A collab sticker pack has no off switch: it is in use once
    /// downloaded, and otherwise only in the full list.
    fn admits_stickers(self, ready: bool) -> bool {
        match self {
            Self::All => true,
            Self::Enabled => ready,
            Self::Disabled => false,
        }
    }
}

/// The installed / enabled / disabled tabs over the package list.
fn filter_tabs(ui: Ui) -> AnyView {
    let tab = |filter: PackageFilter, key: &str| {
        segmented_option(ui.now_t(key))
            .selected(move || ui.packages_filter.get() == filter)
            .on(move |_: &SegmentedOptionChosen| ui.packages_filter.set(filter))
            .into_any()
    };
    widget(
        SegmentedControl::new()
            .size(ControlSize::Small)
            .label(ui.now_t("settings-packages")),
    )
    .children((
        tab(PackageFilter::All, "packages-installed"),
        tab(PackageFilter::Enabled, "packages-enabled"),
        tab(PackageFilter::Disabled, "packages-disabled"),
    ))
    .into_any()
}

/// The Packages page's header: the list's tabs, then import a package and
/// reload them all.
pub fn actions(ui: Ui) -> AnyView {
    kit::hstack((
        filter_tabs(ui),
        kit::tool(
            nana_ui::icons_tabler::REFRESH,
            ui.now_t("packages-refresh"),
            || Msg::Package(PackageMsg::Refresh),
        ),
        kit::tool(
            nana_ui::icons_tabler::DOWNLOAD,
            ui.now_t("packages-import"),
            || Msg::PickFile(FileTarget::ImportSkin),
        ),
    ))
}

/// A row's icon action: `glyph`, named and tipped by `key`.
fn icon_action(
    ui: Ui,
    key: &str,
    glyph: nana_ui::runtime::Icon,
    msg: impl Fn() -> Msg + Send + 'static,
) -> AnyView {
    widget(
        IconButton::new(glyph, ui.now_t(key))
            .size(ControlSize::Small)
            .with_tooltip(ui.now_t(key)),
    )
    .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(msg()))
    .into_any()
}

/// One installed package as a row of the list: what it is and where it comes
/// from on the left, what can be done with it at the top right (export,
/// remove, the on/off switch), and its credits folded away until `open` names it.
fn package_row(
    ui: Ui,
    row: &crate::plugins::PackageRow,
    on: bool,
    error: Option<&String>,
    open: Signal<Vec<String>>,
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
    let provenance = Provenance::of(&row.origin, &row.integrity);
    let source_tone = match provenance {
        Provenance::Builtin => Tone::Success,
        Provenance::ThirdParty => Tone::Warning,
        Provenance::Tampered => Tone::Danger,
    };
    // The badge names the source in a word or two; why a seal failed is a
    // sentence, and goes under the row.
    let source = match provenance {
        Provenance::Tampered => ui.now_t("package-integrity-failed"),
        _ => provenance_label(*ui, provenance, &row.integrity),
    };
    let tampered = (provenance == Provenance::Tampered)
        .then(|| kit::callout(Tone::Danger, integrity_label(*ui, &row.integrity)));
    // What the package brings, one label per capability: a package may be
    // a skin, a sound pack and a script at once.
    let capabilities: Vec<AnyView> = package
        .capabilities
        .iter()
        .map(|cap| {
            widget(Stack::row(0.0).with_layout(|l| l.flex_shrink = Some(0.0)))
                .children(kit::tag(ui.now_t(match cap {
                    Cap::Skin => "packages-kind-skin",
                    Cap::Resources => "packages-kind-resources",
                    Cap::Lua => "packages-kind-function",
                    Cap::Sounds => "packages-kind-sounds",
                })))
                .into_any()
        })
        .collect();
    let version = format!("{id} · {}", package.meta.version);
    let identity = widget(
        Stack::row(0.0)
            .gap_step(gap::step::CONTROL)
            .align(AlignSpec::Center)
            .width(LengthSpec::Px(IDENTITY_MIN))
            .grow(1.0)
            .shrink(1.0)
            .with_layout(|l| l.min_width = Some(LengthSpec::Px(0.0))),
    )
    .children((
        kit::icon_tile(
            glyph,
            PACKAGE_TILE,
            if on { Tone::Accent } else { Tone::Neutral },
        ),
        widget(kit::grow_column(gap::HAIR)).children((
            // A long name is cut before the source badge and labels are.
            widget(
                Stack::row(0.0)
                    .gap_step(gap::step::TIGHT)
                    .align(AlignSpec::Center)
                    .width(LengthSpec::Fill)
                    .with_layout(|l| l.min_width = Some(LengthSpec::Px(0.0))),
            )
            .children((
                widget(Text::new(name.clone()).style({
                    let mut st = Type::Strong.one_line();
                    Arc::make_mut(&mut st.layout).flex_grow = Some(0.0);
                    st
                })),
                kit::badge(source, source_tone),
                capabilities,
            )),
            widget(
                Text::new(version)
                    .style(Type::Caption.one_line())
                    .color(SemanticColorRole::Muted),
            ),
        )),
    ));

    let export = {
        let id = id.clone();
        icon_action(ui, "skins-export", ti::FILE_EXPORT, move || {
            Msg::Skin(SkinMsg::Export(id.clone()))
        })
    };
    // Removing is asked first: the icon is one slip away from the others.
    // The built-in package cannot go, and its button says so by being off.
    let remove = if builtin {
        widget(
            IconButton::new(ti::TRASH, ui.now_t("packages-remove"))
                .size(ControlSize::Small)
                .disabled(true),
        )
        .into_any()
    } else {
        let remove_id = id.clone();
        nanatype_frame::ui::controls::confirm(
            *ui,
            ui.tr("packages-remove-confirm", &[("name", name.as_str().into())]),
            ui.now_t("packages-remove"),
            move |armed| {
                widget(
                    IconButton::new(ti::TRASH, ui.now_t("packages-remove"))
                        .size(ControlSize::Small)
                        .with_tooltip(ui.now_t("packages-remove")),
                )
                .on(move |_: &Activate| armed.set(true))
                .into_any()
            },
            move || Msg::Package(PackageMsg::Remove(remove_id.clone())),
        )
    };
    // Every package has the switch. Turning a functional one on asks for
    // the review, which grants the consent; until then the switch falls
    // back to off. The built-in package is always on.
    let toggle = {
        let id = id.clone();
        let label = ui.now_t(if on {
            "packages-disable"
        } else {
            "packages-enable"
        });
        widget(Switch::new("", on).accessible_name(label))
            .key(format!("package-review:{id}"))
            .bind(move |switch: &mut Switch| {
                ui.plugin_activation.with(|_| ());
                ui.plugin_activation_error.with(|_| ());
                switch.checked = on;
                switch.disabled = builtin || (!on && ui.plugin_activation_busy.get());
            })
            .on_cx(move |_, e: &ToggleChanged, cx| {
                if e.checked != on {
                    cx.dispatch_program_all(Msg::Package(PackageMsg::SetEnabled(
                        id.clone(),
                        e.checked,
                    )))
                }
            })
            .into_any()
    };
    let is_open = {
        let id = id.clone();
        move || open.with(|o| o.contains(&id))
    };
    let fold = fold_button(ui, &id, open, is_open.clone());
    let actions = widget(
        Stack::row(0.0)
            .gap_step(gap::step::TIGHT)
            .align(AlignSpec::Center)
            .wrap(true)
            .shrink(1.0)
            .with_layout(|l| l.min_width = Some(LengthSpec::Px(0.0))),
    )
    .children((export, remove, toggle, fold));

    let third_party = (provenance == Provenance::ThirdParty)
        .then(|| kit::callout(Tone::Warning, ui.now_t("package-third-party-warning")));
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
    let details = widget(Stack::column(0.0).gap_step(gap::step::TIGHT))
        .visible(is_open)
        .children((
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
            third_party,
        ));
    widget(
        Stack::column(0.0)
            .gap_step(gap::step::CONTROL)
            .padding_step(inset::step::CARD, inset::step::BAR_Y),
    )
    .key(id)
    .children((
        widget(
            Stack::row(0.0)
                .gap_step(gap::step::CONTROL)
                .align(AlignSpec::Center)
                .wrap(true)
                .width(LengthSpec::Fill)
                .with_layout(|l| l.row_gap = Some(LengthSpec::Px(gap::TIGHT))),
        )
        .children((identity, actions)),
        tampered,
        error.map(|e| kit::callout(Tone::Danger, e.clone())),
        details,
    ))
    .into_any()
}

/// What a sticker pack is doing, in a few words.
pub(crate) fn sticker_status(ui: Ui, pack: &crate::stickers::PackRow) -> String {
    use crate::stickers::Phase;
    let progress = |key: &str, done: usize, total: usize| {
        ui.tr(
            key,
            &[
                ("done", (done as u64).into()),
                ("total", (total as u64).into()),
            ],
        )
    };
    match &pack.phase {
        Phase::NotDownloaded => ui.now_t("stickers-not-downloaded"),
        Phase::Downloading(done, total) => progress("stickers-downloading", *done, *total),
        Phase::Processing(done, total) => progress("stickers-processing", *done, *total),
        Phase::Ready => ui.tr("stickers-ready", &[("count", (pack.count as u64).into())]),
        Phase::Failed(error) => ui.tr("stickers-failed", &[("message", error.clone().into())]),
    }
}

/// A collab sticker pack (one NanaType downloads from its partner project on
/// request) as a row of the package list, laid out like an installed
/// package: download it, open its repository, remove it, and its credits
/// folded away until `open` names it.
fn sticker_row(ui: Ui, pack: &crate::stickers::PackRow, open: Signal<Vec<String>>) -> AnyView {
    use crate::msg::StickerMsg;
    use crate::stickers::Phase;
    let Some(preset) = pack.preset else {
        return widget(Stack::column(0.0)).into_any();
    };
    let id = format!("stickers:{}", preset.id());
    let name = ui.now_t(preset.title_key());
    let ready = pack.phase == Phase::Ready;
    let failed = match &pack.phase {
        Phase::Failed(error) => Some(error.clone()),
        _ => None,
    };
    // The failure is a sentence, and goes under the row.
    let summary = if failed.is_some() {
        preset.id().to_owned()
    } else {
        format!("{} · {}", preset.id(), sticker_status(ui, pack))
    };
    let identity = widget(
        Stack::row(0.0)
            .gap_step(gap::step::CONTROL)
            .align(AlignSpec::Center)
            .width(LengthSpec::Px(IDENTITY_MIN))
            .grow(1.0)
            .shrink(1.0)
            .with_layout(|l| l.min_width = Some(LengthSpec::Px(0.0))),
    )
    .children((
        kit::icon_tile(
            ti::MOOD_SMILE,
            PACKAGE_TILE,
            if ready { Tone::Accent } else { Tone::Neutral },
        ),
        widget(kit::grow_column(gap::HAIR)).children((
            widget(
                Stack::row(0.0)
                    .gap_step(gap::step::TIGHT)
                    .align(AlignSpec::Center)
                    .width(LengthSpec::Fill)
                    .with_layout(|l| l.min_width = Some(LengthSpec::Px(0.0))),
            )
            .children((
                widget(Text::new(name.clone()).style({
                    let mut st = Type::Strong.one_line();
                    Arc::make_mut(&mut st.layout).flex_grow = Some(0.0);
                    st
                })),
                kit::badge(ui.now_t("package-source-collab"), Tone::Accent),
                widget(Stack::row(0.0).with_layout(|l| l.flex_shrink = Some(0.0)))
                    .children(kit::tag(ui.now_t("packages-kind-stickers"))),
            )),
            widget(
                Text::new(summary)
                    .style(Type::Caption.one_line())
                    .color(SemanticColorRole::Muted),
            ),
        )),
    ));

    let mut actions = Vec::new();
    if pack
        .repository
        .as_deref()
        .is_some_and(|url| url.starts_with("https://"))
    {
        let root = pack.root.clone();
        actions.push(icon_action(
            ui,
            "stickers-repository",
            ti::EXTERNAL_LINK,
            move || Msg::Sticker(StickerMsg::Repository(root.clone())),
        ));
    }
    if !pack.phase.busy() && !ready {
        let (key, glyph) = if pack.phase == Phase::NotDownloaded {
            ("stickers-download", ti::CLOUD_DOWNLOAD)
        } else {
            ("stickers-retry", ti::REFRESH)
        };
        actions.push(icon_action(ui, key, glyph, move || {
            Msg::Sticker(StickerMsg::Download(preset))
        }));
    }
    if pack.phase.busy() {
        let root = pack.root.clone();
        actions.push(icon_action(ui, "common-cancel", ti::X, move || {
            Msg::Sticker(StickerMsg::Remove(root.clone()))
        }));
    } else if pack.phase != Phase::NotDownloaded {
        let root = pack.root.clone();
        actions.push(nanatype_frame::ui::controls::confirm(
            *ui,
            ui.tr("packages-remove-confirm", &[("name", name.as_str().into())]),
            ui.now_t("packages-remove"),
            move |armed| {
                widget(
                    IconButton::new(ti::TRASH, ui.now_t("packages-remove"))
                        .size(ControlSize::Small)
                        .with_tooltip(ui.now_t("packages-remove")),
                )
                .on(move |_: &Activate| armed.set(true))
                .into_any()
            },
            move || Msg::Sticker(StickerMsg::Remove(root.clone())),
        ));
    }
    let is_open = {
        let id = id.clone();
        move || open.with(|o| o.contains(&id))
    };
    actions.push(fold_button(ui, &id, open, is_open.clone()));
    let actions = widget(
        Stack::row(0.0)
            .gap_step(gap::step::TIGHT)
            .align(AlignSpec::Center)
            .wrap(true)
            .shrink(1.0)
            .with_layout(|l| l.min_width = Some(LengthSpec::Px(0.0))),
    )
    .children(actions);

    let location = (pack.phase != Phase::NotDownloaded).then(|| {
        widget(caption(format!(
            "{}: {}",
            ui.now_t("packages-location"),
            pack.root.display()
        )))
    });
    let details = widget(Stack::column(0.0).gap_step(gap::step::TIGHT))
        .visible(is_open)
        .children((
            widget(caption(format!(
                "{}: {} · {}: {}",
                ui.now_t("package-author"),
                provided(ui, &pack.author),
                ui.now_t("package-license"),
                provided(ui, &pack.license)
            ))),
            location,
        ));
    widget(
        Stack::column(0.0)
            .gap_step(gap::step::CONTROL)
            .padding_step(inset::step::CARD, inset::step::BAR_Y),
    )
    .key(id)
    .children((
        widget(
            Stack::row(0.0)
                .gap_step(gap::step::CONTROL)
                .align(AlignSpec::Center)
                .wrap(true)
                .width(LengthSpec::Fill)
                .with_layout(|l| l.row_gap = Some(LengthSpec::Px(gap::TIGHT))),
        )
        .children((identity, actions)),
        failed.map(|e| {
            kit::callout(
                Tone::Danger,
                ui.tr("stickers-failed", &[("message", e.into())]),
            )
        }),
        details,
    ))
    .into_any()
}

/// The chevron that shows or folds a row's credits; `open` keeps which
/// rows are showing them.
fn fold_button(
    ui: Ui,
    id: &str,
    open: Signal<Vec<String>>,
    shown: impl Fn() -> bool + Clone + Send + 'static,
) -> AnyView {
    let id = id.to_owned();
    widget(
        IconButton::new(ti::CHEVRON_DOWN, ui.now_t("packages-details"))
            .size(ControlSize::Small)
            .with_tooltip(ui.now_t("packages-details")),
    )
    .key(format!("package-details:{id}"))
    .bind(move |b: &mut IconButton| {
        let open = shown();
        b.selected = open;
        b.icon = if open {
            ti::CHEVRON_UP
        } else {
            ti::CHEVRON_DOWN
        };
    })
    .on(move |_: &Activate| {
        open.update(|o| match o.iter().position(|p| *p == id) {
            Some(at) => {
                o.remove(at);
            }
            None => o.push(id.clone()),
        })
    })
    .into_any()
}

pub fn settings(ui: Ui) -> AnyView {
    // Which rows show their credits; kept here so a reload of the list (a
    // setting changed, a download moved on) leaves them as they were.
    let open: Signal<Vec<String>> = signal(Vec::new());
    let packages = dynamic(
        move || {
            (
                ui.packages.get(),
                ui.settings
                    .with(|s| (s.disabled_packages.clone(), s.enabled_plugins.clone())),
                ui.plugin_errors.get(),
                ui.packages_filter.get(),
                ui.sticker_packs.get(),
            )
        },
        move |(rows, (disabled_packages, enabled_plugins), errors, filter, stickers)| {
            let collab: Vec<_> = stickers.iter().filter(|p| p.preset.is_some()).collect();
            if rows.is_empty() && collab.is_empty() {
                return kit::empty_state::<Msg>(ui.now_t("packages-none"), None);
            }
            let rows = rows
                .iter()
                .filter_map(|row| {
                    let id = &row.manifest.meta.id;
                    // On unless switched off; a script also needs its consent.
                    let on = !disabled_packages.contains(id)
                        && (!row
                            .manifest
                            .capabilities
                            .contains(&nanatype_theme::PackageCapability::Lua)
                            || enabled_plugins.contains(id));
                    filter
                        .admits(on)
                        .then(|| (package_row(ui, row, on, errors.get(id), open), None))
                })
                .chain(
                    collab
                        .into_iter()
                        .filter(|pack| {
                            filter.admits_stickers(pack.phase == crate::stickers::Phase::Ready)
                        })
                        .map(|pack| (sticker_row(ui, pack, open), None)),
                )
                .collect::<Vec<_>>();
            if rows.is_empty() {
                return kit::empty_state::<Msg>(ui.now_t("packages-none-filtered"), None);
            }
            kit::section_titled(None, rows)
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
                                cx.dispatch_program_all(Msg::Package(PackageMsg::PluginCommand {
                                    id: id.clone(),
                                    command: command_id.clone(),
                                }))
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
        packages,
        widget(Stack::column(0.0))
            .visible(has_commands)
            .children(commands),
        widget(Stack::column(0.0))
            .visible(has_panels)
            .children(mounted(ui, Mount::Panel)),
    ))
}

/// [`readable`] in the caption role.
fn caption(value: impl Into<String>) -> Text {
    readable(value)
        .style({
            let mut st = Type::Caption.style();
            kit::wrap_anywhere(&mut st);
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
    kit::wrap_anywhere(&mut text.style);
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
    let show_source = signal(false);
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
                    // Where it comes from, its seal, and the third-party warning.
                    (
                        widget(readable(provenance_label(
                            *ui,
                            Provenance::of(&review.origin, &Ok(None)),
                            &Ok(None),
                        ))),
                        review.integrity.is_some().then(|| {
                            widget(readable(integrity_label(
                                *ui,
                                &Ok(review.integrity.clone()),
                            )))
                        }),
                        matches!(
                            review.origin,
                            nanatype_theme::SkinOrigin::Directory(_)
                                | nanatype_theme::SkinOrigin::Archive(_)
                        )
                        .then(|| {
                            kit::callout(Tone::Warning, ui.now_t("package-third-party-warning"))
                        }),
                    ),
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
                        kit::text_area(source)
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
                        widget(
                            Button::new(ui.now_t("packages-consent-confirm"))
                                .kind(ButtonKind::Primary),
                        )
                        .key("plugin-consent-confirm")
                        .bind(move |button: &mut Button| {
                            button.disabled =
                                !ui.plugin_source_trusted.get() || ui.plugin_activation_busy.get()
                        })
                        .on_cx(|_, _: &Activate, cx| {
                            cx.dispatch_program_all(Msg::Package(PackageMsg::ConfirmActivation))
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
        .on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program_all(Msg::Package(PackageMsg::CancelActivation))
        })
        .into_any()
}
