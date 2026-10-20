use super::*;

/// A licence or notice text's scroll height.
const LICENCE_H: f32 = 300.0;

/// The logo's size in the About header.
const ABOUT_ICON: f32 = 56.0;

pub(super) fn about(ui: Ui) -> AnyView {
    let locale = ui.locale.get_untracked();
    let build = I18N.tr_in(
        locale,
        "about-build",
        &[("build", env!("NANATYPE_BUILD_ID").into())],
    );
    let paths = crate::paths::Paths::resolve();
    let (logs, data) = (paths.logs.clone(), paths.data.clone());
    let icon = super::super::logo(ABOUT_ICON);
    let version_chip = crate::ui::kit::mono_tag(format!("v{}", env!("CARGO_PKG_VERSION")));
    let header = widget(
        Stack::row(0.0)
            .gap_step(gap::step::SECTION)
            .padding_step(inset::step::CARD, inset::step::CARD)
            .align(AlignSpec::Center),
    )
    .children((
        icon,
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
    let link = |key: &str, icon: nana_ui::runtime::Icon, msg: Box<dyn Fn() -> Msg + Send>| {
        let mut button = Button::new(ui.now_t(key))
            .kind(ButtonKind::Ghost)
            .size(ControlSize::Small)
            .icon(icon);
        button.style.control_padding_x = None;
        widget(button)
            .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(msg()))
            .into_any()
    };
    let links = widget(
        Stack::row(0.0)
            .gap_step(nana_ui_core::SpacingStep::Sm)
            .padding_step(inset::step::CARD, nana_ui_core::SpacingStep::Lg)
            .wrap(true),
    )
    .children(vec![
        link(
            "about-logs",
            ti::FILE_TEXT,
            Box::new(move || Msg::OpenFolder(logs.clone())),
        ),
        link(
            "about-data",
            ti::FOLDER_OPEN,
            Box::new(move || Msg::OpenFolder(data.clone())),
        ),
        link(
            "about-homepage",
            ti::EXTERNAL_LINK,
            Box::new(|| Msg::OpenUrl(HOMEPAGE)),
        ),
        // The first-run guide again, from its first step, on General.
        {
            let mut guide = Button::new(ui.now_t("about-setup-guide"))
                .kind(ButtonKind::Ghost)
                .size(ControlSize::Small)
                .icon(ti::SPARKLES);
            guide.style.control_padding_x = None;
            widget(guide)
        }
        .on_cx(move |_, _: &Activate, cx| {
            ui.onboard_step.set(0);
            cx.dispatch_program_all(edit(|s| s.general.onboarding_done = false));
            cx.dispatch_program_all(Msg::SettingsPage(Page::General));
        })
        .into_any(),
    ]);
    let mut rows = vec![(header.into_any(), None), (links.into_any(), None)];
    rows.extend(interface_licences(ui));
    rows.extend(design_sources(ui));
    rows.extend(font_licences(ui));
    widget(Stack::column(0.0).gap_step(gap::step::SECTION))
        .children(section_with(ui, "", rows))
        .into_any()
}

pub(super) fn design_sources(ui: Ui) -> Vec<(AnyView, Shown)> {
    let link = |key: &str, url: &'static str| {
        widget(
            Button::new(ui.now_t(key))
                .kind(ButtonKind::Ghost)
                .size(ControlSize::Small)
                .icon(ti::EXTERNAL_LINK),
        )
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(Msg::OpenUrl(url)))
    };
    vec![(
        widget(
            Stack::column(0.0)
                .gap_step(nana_ui_core::SpacingStep::Sm)
                .padding_step(inset::step::CARD, nana_ui_core::SpacingStep::Lg),
        )
        .children((
            widget(Text::new(ui.now_t("about-design")).style(Type::Strong.style())),
            widget(Text::new("Echo-Live · Sheep-realms").style(Type::Strong.style())),
            hint(ui.now_t("about-design-echo")),
            widget(
                Stack::row(0.0)
                    .gap_step(nana_ui_core::SpacingStep::Sm)
                    .wrap(true),
            )
            .children((
                link(
                    "about-source-repository",
                    "https://github.com/sheep-realms/Echo-Live",
                ),
                link(
                    "about-design-upstream-license",
                    "https://github.com/sheep-realms/Echo-Live/blob/master/copyright.md",
                ),
            )),
        ))
        .into_any(),
        None,
    )]
}

/// Sources stay visible; each licence is embedded for offline reading.
pub(super) fn interface_licences(ui: Ui) -> Vec<(AnyView, Shown)> {
    let mut rows = vec![(
        widget(
            Stack::column(0.0)
                .gap_step(nana_ui_core::SpacingStep::Sm)
                .padding_step(inset::step::CARD, nana_ui_core::SpacingStep::Lg),
        )
        .children((
            widget(Text::new(ui.now_t("about-interfaces")).style(Type::Strong.style())),
            hint(ui.now_t("about-interfaces-hint")),
        ))
        .into_any(),
        None,
    )];
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
    for (name, description, url, mirror, licence) in entries {
        let open = signal(false);
        let source_link = |key: &str, url: &'static str| {
            widget(
                Button::new(ui.now_t(key))
                    .kind(ButtonKind::Ghost)
                    .size(ControlSize::Small)
                    .icon(ti::EXTERNAL_LINK),
            )
            .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(Msg::OpenUrl(url)))
            .into_any()
        };
        let mut buttons = vec![source_link("about-source-repository", url)];
        if let Some(mirror) = mirror {
            buttons.push(source_link("about-source-mirror", mirror));
        }
        buttons.push(
            widget(Button::new(ui.now_t("about-fonts-show")).size(ControlSize::Small))
                .visible(move || !open.get())
                .on(move |_: &Activate| open.set(true))
                .into_any(),
        );
        buttons.push(
            widget(Button::new(ui.now_t("about-fonts-hide")).size(ControlSize::Small))
                .visible(move || open.get())
                .on(move |_: &Activate| open.set(false))
                .into_any(),
        );
        rows.push((
            widget(
                Stack::column(0.0)
                    .gap_step(nana_ui_core::SpacingStep::Sm)
                    .padding_step(inset::step::CARD, nana_ui_core::SpacingStep::Lg),
            )
            .children((
                widget(Text::new(name).style(Type::Strong.style())),
                hint(ui.now_t(description)),
                widget(
                    Stack::row(0.0)
                        .gap_step(nana_ui_core::SpacingStep::Sm)
                        .wrap(true),
                )
                .children(buttons),
            ))
            .into_any(),
            None,
        ));
        rows.push(shown_if(
            widget(
                Stack::column(0.0).padding_step(inset::step::CARD, nana_ui_core::SpacingStep::Lg),
            )
            .children(
                widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|l| {
                    l.height = Some(LengthSpec::Px(LICENCE_H));
                }))
                .children(widget(Stack::column(0.0)).children(licence_lines(licence))),
            )
            .into_any(),
            move || open.get(),
        ));
    }
    rows
}

/// The "Open-source fonts" row and, below it while open, a scrolling panel
/// with each font this build embeds: its name, author, licence and the
/// licence's full text (OFL 1.1 asks that it travel with the font). Nothing
/// when the build embeds no fonts.
pub(super) fn font_licences(ui: Ui) -> Vec<(AnyView, Shown)> {
    let fonts = nanatype_theme::BuiltinFonts::embedded().fonts();
    if fonts.is_empty() {
        return Vec::new();
    }
    let open = signal(false);
    let toggle = |key: &str, shown: bool| {
        widget(Button::new(ui.now_t(key)).size(ControlSize::Small))
            .visible(move || open.get() == shown)
            .on(move |_: &Activate| open.set(!shown))
            .into_any()
    };
    let row_view = row(
        ui,
        "about-fonts",
        Some("about-fonts-hint"),
        widget(Stack::row(0.0))
            .children((
                toggle("about-fonts-show", false),
                toggle("about-fonts-hide", true),
            ))
            .into_any(),
    );
    vec![
        (row_view, None),
        shown_if(font_licence_panel(ui), move || open.get()),
    ]
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
    widget(Stack::column(0.0).padding_step(inset::step::CARD, nana_ui_core::SpacingStep::Lg))
        .children(
            widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|l| {
                l.height = Some(LengthSpec::Px(LICENCE_H));
            }))
            .children(
                widget(
                    Stack::column(0.0)
                        .gap_step(nana_ui_core::SpacingStep::Xxxl)
                        .with_layout(|l| {
                            l.padding_right = Some(LengthSpec::Px(space::XL));
                        }),
                )
                .children(entries),
            ),
        )
        .into_any()
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
