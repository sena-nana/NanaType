use super::*;

use nana_ui::runtime::Icon;

/// A licence or notice text's scroll height.
const LICENCE_H: f32 = 300.0;

/// The logo's size in the About header.
const ABOUT_ICON: f32 = 56.0;

/// Who made NanaType and where to find them; the local folders; then the
/// notices and licences, each folded until asked for.
pub(super) fn about(ui: Ui) -> AnyView {
    let sections = vec![
        section_with(ui, "", identity(ui)),
        section(ui, "", local(ui)),
        section_with(ui, "about-disclaimer", notices(ui)),
        section_with(ui, "about-licenses", licences(ui)),
    ];
    widget(Stack::column(0.0).gap_step(gap::step::SECTION))
        .children(sections)
        .into_any()
}

/// The logo, name and version, the official links and custom skins.
fn identity(ui: Ui) -> Vec<(AnyView, Shown)> {
    let locale = ui.locale.get_untracked();
    let build = I18N.tr_in(
        locale,
        "about-build",
        &[("build", env!("NANATYPE_BUILD_ID").into())],
    );
    let version_chip = crate::ui::kit::mono_tag(format!("v{}", env!("CARGO_PKG_VERSION")));
    let header = widget(
        Stack::row(0.0)
            .gap_step(gap::step::SECTION)
            .padding_step(inset::step::CARD, inset::step::CARD)
            .align(AlignSpec::Center),
    )
    .children((
        super::super::logo(ABOUT_ICON),
        widget(crate::ui::kit::grow_column(gap::TIGHT)).children((
            widget(
                Stack::row(0.0)
                    .gap_step(gap::step::CONTROL)
                    .align(AlignSpec::Center),
            )
            .children((Type::PageTitle.view("NanaType"), version_chip)),
            Type::Lead.view(ui.now_t("app-tagline")),
            hint(build),
        )),
    ));
    vec![
        (header.into_any(), None),
        (update_row(ui), None),
        (
            row_text(
                ui.now_t("about-repository"),
                Some(HOMEPAGE.trim_start_matches("https://").to_owned()),
                link_icon(ui, "about-open-link", ti::EXTERNAL_LINK, HOMEPAGE),
            ),
            None,
        ),
        (
            row_text(
                ui.now_t("about-bilibili"),
                Some("UID 82717585".to_owned()),
                link_icon(ui, "about-open-link", ti::EXTERNAL_LINK, BILIBILI),
            ),
            None,
        ),
        (
            row(
                ui,
                "about-custom-skins",
                Some("about-custom-skins-body"),
                link_icon(ui, "about-contact", ti::BRAND_BILIBILI, BILIBILI),
            ),
            None,
        ),
    ]
}

/// The update check's state and what can be done next.
fn update_row(ui: Ui) -> AnyView {
    widget(Stack::column(0.0))
        .children(dynamic(
            move || ui.update.get(),
            move |state| update_status(ui, state),
        ))
        .into_any()
}

fn update_status(ui: Ui, state: &crate::app::update::State) -> AnyView {
    use crate::app::update::State;
    use nanatype_update::Kind;
    let locale = ui.locale.get_untracked();
    let tr = |key: &str, args: &[(&str, nanatype_i18n::Arg<'_>)]| I18N.tr_in(locale, key, args);
    let check = || button(ui, "update-check", || Msg::CheckUpdate);
    let (title, detail, buttons) = match state {
        State::Idle => (
            ui.now_t("about-update"),
            Some(ui.now_t("about-update-hint")),
            vec![check()],
        ),
        State::Checking => (ui.now_t("update-checking"), None, Vec::new()),
        State::UpToDate => (
            ui.now_t("update-up-to-date"),
            Some(tr(
                "update-current",
                &[("version", nanatype_update::current().into())],
            )),
            vec![check()],
        ),
        State::Available { version, size } => (
            tr("update-available", &[("version", version.as_str().into())]),
            Some(tr(
                "update-available-hint",
                &[("size", megabytes(*size).into())],
            )),
            vec![
                button(ui, "update-notes", || Msg::OpenUpdateNotes),
                button(ui, "update-skip", || Msg::SkipUpdate),
                primary(ui, "update-download", || Msg::DownloadUpdate),
            ],
        ),
        State::Downloading {
            version,
            done,
            total,
            source,
        } => (
            tr(
                "update-downloading",
                &[("version", version.as_str().into())],
            ),
            Some(tr(
                "update-downloading-hint",
                &[
                    ("percent", (done * 100 / (*total).max(1)).to_string().into()),
                    ("source", source.as_str().into()),
                ],
            )),
            vec![button(ui, "update-cancel", || Msg::CancelUpdate)],
        ),
        State::Ready { version, kind, .. } => {
            let (hint_key, action) = match kind {
                Kind::Installer => ("update-ready-installer", "update-install"),
                Kind::Dmg => ("update-ready-dmg", "update-open"),
                _ => ("update-ready-folder", "update-open"),
            };
            (
                tr("update-ready", &[("version", version.as_str().into())]),
                Some(ui.now_t(hint_key)),
                vec![primary(ui, action, || Msg::InstallUpdate)],
            )
        }
        State::Failed { message, detail } => (
            message.clone(),
            Some(detail.clone()),
            vec![button(ui, "update-retry", || Msg::CheckUpdate)],
        ),
    };
    row_text(title, detail, controls(buttons))
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
}

/// The log and data folders, and the first-run guide again.
fn local(ui: Ui) -> Vec<AnyView> {
    let paths = crate::paths::Paths::resolve();
    let (logs, data) = (paths.logs.clone(), paths.data.clone());
    vec![
        row(
            ui,
            "about-logs",
            None,
            icon_action(ui, "about-open", ti::FOLDER_OPEN, move || {
                Msg::OpenFolder(logs.clone())
            }),
        ),
        row(
            ui,
            "about-data",
            None,
            icon_action(ui, "about-open", ti::FOLDER_OPEN, move || {
                Msg::OpenFolder(data.clone())
            }),
        ),
        // The first-run guide again, from its first step, on General.
        row(
            ui,
            "about-setup-guide",
            None,
            widget(
                IconButton::new(ti::ARROW_RIGHT, ui.now_t("about-open"))
                    .size(ControlSize::Small)
                    .with_tooltip(ui.now_t("about-open")),
            )
            .on_cx(move |_, _: &Activate, cx| {
                ui.onboard_step.set(0);
                cx.dispatch_program_all(edit(|s| s.general.onboarding_done = false));
                cx.dispatch_program_all(Msg::SettingsPage(Page::General));
            })
            .into_any(),
        ),
    ]
}

/// That NanaType is unaffiliated, and where the built-in Echo-Live skins'
/// designs come from.
fn notices(ui: Ui) -> Vec<(AnyView, Shown)> {
    let open = signal(false);
    vec![
        (
            row(
                ui,
                "about-disclaimer-unofficial",
                Some("about-disclaimer-unofficial-hint"),
                fold_toggle(ui, open),
            ),
            None,
        ),
        shown_if(block(hint(ui.now_t("about-disclaimer-body"))), move || {
            open.get()
        }),
        (
            row_text(
                "Echo-Live · Sheep-realms".to_owned(),
                Some(ui.now_t("about-design-echo")),
                controls(vec![
                    link_icon(
                        ui,
                        "about-source-repository",
                        ti::EXTERNAL_LINK,
                        "https://github.com/sheep-realms/Echo-Live",
                    ),
                    link_icon(
                        ui,
                        "about-design-upstream-license",
                        ti::LICENSE,
                        "https://github.com/sheep-realms/Echo-Live/blob/master/copyright.md",
                    ),
                ]),
            ),
            None,
        ),
    ]
}

/// Each licence's source stays a click away; its text is embedded for
/// offline reading and unfolds under its row. The fonts close the list.
fn licences(ui: Ui) -> Vec<(AnyView, Shown)> {
    let entries = [
        (
            "NanaType · MIT AND CC-BY-NC-4.0",
            "about-license-project",
            HOMEPAGE,
            None,
            concat!(
                include_str!("../../../../../LICENSE"),
                "\n",
                include_str!("../../../../../LICENSE-MIT")
            ),
        ),
        (
            "SocialSisterYi/bilibili-API-collect · CC BY-NC 4.0",
            "about-source-api-collect",
            "https://github.com/SocialSisterYi/bilibili-API-collect",
            Some("https://github.com/bilibili-plugins/bilibili-api-collect"),
            include_str!("../../../../../crates/bilibili/LICENSE-API-COLLECT"),
        ),
        (
            "jwwsjlm/douyinLive · MIT",
            "about-source-douyin",
            "https://github.com/jwwsjlm/douyinLive/tree/0e506f3cf935beede946296602af2e52d57ecea2",
            None,
            include_str!("../../../../../crates/douyin/LICENSE-DOUYINLIVE"),
        ),
    ];
    let mut rows = Vec::new();
    for (name, description, url, mirror, licence) in entries {
        let open = signal(false);
        let mut buttons = vec![link_icon(
            ui,
            "about-source-repository",
            ti::EXTERNAL_LINK,
            url,
        )];
        if let Some(mirror) = mirror {
            buttons.push(link_icon(ui, "about-source-mirror", ti::BOOK, mirror));
        }
        buttons.push(fold_toggle(ui, open));
        rows.push((
            row_text(
                name.to_owned(),
                Some(ui.now_t(description)),
                controls(buttons),
            ),
            None,
        ));
        rows.push(shown_if(licence_panel(licence_lines(licence)), move || {
            open.get()
        }));
    }
    if !nanatype_theme::BuiltinFonts::embedded().fonts().is_empty() {
        let open = signal(false);
        rows.push((
            row(
                ui,
                "about-fonts",
                Some("about-fonts-hint"),
                fold_toggle(ui, open),
            ),
            None,
        ));
        rows.push(shown_if(font_licence_panel(ui), move || open.get()));
    }
    rows
}

/// A small icon button that sends `msg`, its name as the tooltip.
fn icon_action(ui: Ui, key: &str, icon: Icon, msg: impl Fn() -> Msg + Send + 'static) -> AnyView {
    widget(
        IconButton::new(icon, ui.now_t(key))
            .size(ControlSize::Small)
            .with_tooltip(ui.now_t(key)),
    )
    .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(msg()))
    .into_any()
}

/// [`icon_action`] that opens `url` in the browser.
fn link_icon(ui: Ui, key: &str, icon: Icon, url: &'static str) -> AnyView {
    icon_action(ui, key, icon, move || Msg::OpenUrl(url))
}

/// A row's controls, side by side.
fn controls(buttons: Vec<AnyView>) -> AnyView {
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::TIGHT)
            .align(AlignSpec::Center),
    )
    .children(buttons)
    .into_any()
}

/// The chevron that unfolds (and folds back) what `open` shows.
fn fold_toggle(ui: Ui, open: Signal<bool>) -> AnyView {
    // Each chevron shows while it would change `open`.
    let toggle = |icon: Icon, key: &str, opens: bool| {
        widget(
            IconButton::new(icon, ui.now_t(key))
                .size(ControlSize::Small)
                .with_tooltip(ui.now_t(key)),
        )
        .visible(move || open.get() != opens)
        .on(move |_: &Activate| open.set(opens))
        .into_any()
    };
    controls(vec![
        toggle(ti::CHEVRON_DOWN, "about-expand", true),
        toggle(ti::CHEVRON_UP, "about-collapse", false),
    ])
}

/// Long text in a fixed-height scrolling panel, inset like the rows.
fn licence_panel(lines: Vec<AnyView>) -> AnyView {
    widget(Stack::column(0.0).padding_step(inset::step::CARD, nana_ui_core::SpacingStep::Lg))
        .children(
            widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|l| {
                l.height = Some(LengthSpec::Px(LICENCE_H));
            }))
            .children(widget(Stack::column(0.0)).children(lines)),
        )
        .into_any()
}

/// Each font this build embeds, its author, version and licence, and the
/// licence's full text, in a scrolling panel. Shown under About's
/// "Open-source fonts" row.
pub fn font_licence_panel(ui: Ui) -> AnyView {
    let fonts = nanatype_theme::BuiltinFonts::embedded().fonts();
    let locale = ui.locale.get_untracked();
    let entries: Vec<AnyView> = fonts
        .iter()
        .map(|font| {
            let name = font.family().unwrap_or_else(|| font.path.to_owned());
            let mut facts = Vec::new();
            if let Some(author) = font.author() {
                facts.push(I18N.tr_in(locale, "about-fonts-author", &[("author", author.into())]));
            }
            if let Some(version) = font.version() {
                facts.push(I18N.tr_in(
                    locale,
                    "about-fonts-version",
                    &[("version", version.into())],
                ));
            }
            facts.push(match font.license_name() {
                Some(l) => l.to_owned(),
                None => I18N.t_in(locale, "about-fonts-license-other"),
            });
            widget(Stack::column(0.0).gap_step(gap::step::TIGHT))
                .children((
                    widget(Text::new(name).style(Type::Strong.style())),
                    hint(facts.join(" · ")),
                    widget(Stack::column(0.0).with_layout(|l| {
                        l.padding_top = Some(LengthSpec::Px(space::SM));
                    }))
                    .children(licence_lines(font.license)),
                ))
                .into_any()
        })
        .collect();
    licence_panel(vec![
        widget(
            Stack::column(0.0)
                .gap_step(nana_ui_core::SpacingStep::Xxxl)
                .with_layout(|l| {
                    l.padding_right = Some(LengthSpec::Px(space::XL));
                }),
        )
        .children(entries)
        .into_any(),
    ])
}

/// A licence's text as it is laid out upstream: one text per line (a text
/// folds its line breaks), a blank line as a gap.
pub(super) fn licence_lines(text: &str) -> Vec<AnyView> {
    text.trim_end()
        .lines()
        .map(|line| {
            if line.trim().is_empty() {
                widget(Stack::column(0.0).height(LengthSpec::Px(gap::CONTROL))).into_any()
            } else {
                widget(
                    Text::new(line.to_owned())
                        .style(Type::Micro.mono())
                        .color(SemanticColorRole::Muted),
                )
                .into_any()
            }
        })
        .collect()
}

/// Where the project lives.
pub(super) const HOMEPAGE: &str = "https://github.com/sena-nana/NanaType";

/// The developer's Bilibili space.
pub(super) const BILIBILI: &str = "https://space.bilibili.com/82717585";
