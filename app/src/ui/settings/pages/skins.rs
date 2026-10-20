use super::*;
use nana_ui::runtime::view::El;

/// A skin's colour swatch in the list.
const SWATCH: f32 = 14.0;

pub(super) fn skins(ui: Ui) -> AnyView {
    let filter: Signal<Option<String>> = signal(None);
    let previewing: Signal<Option<String>> = signal(None);
    // Cards with thumbnails by default; the list stays for skins whose card
    // cannot say enough (an author's broken skin, a long description).
    let grid = ui.skins_grid;
    let list = dynamic(
        move || {
            (
                ui.skins.with(|s| Arc::as_ptr(s) as usize),
                ui.settings.with(|s| s.skin.id.clone()),
                filter.get(),
                grid.get(),
                ui.chrome_dark.get(),
            )
        },
        move |(_, active, shown, grid, _)| {
            let locale = ui.locale.get_untracked();
            let skins = ui.skins.get_untracked();
            let rows = skins
                .iter()
                .filter(|skin| matches_filter(skin, shown.as_deref()));
            if *grid {
                let cards: Vec<AnyView> = rows
                    .map(|skin| skin_card(ui, skin, skin.id == *active, previewing, locale))
                    .collect();
                widget(
                    Stack::row(GRID_GAP)
                        .wrap(true)
                        .padding_xy(inset::CARD, 0.0)
                        .with_layout(|l| {
                            l.row_gap = Some(LengthSpec::Px(GRID_GAP));
                            l.padding_bottom = Some(LengthSpec::Px(inset::CARD));
                        }),
                )
                .children(cards)
                .into_any()
            } else {
                let rows: Vec<AnyView> = rows
                    .map(|skin| skin_list_row(ui, skin, skin.id == *active, previewing, locale))
                    .collect();
                widget(Stack::column(0.0)).children(rows).into_any()
            }
        },
    );
    let fallback = ui.skin.get_untracked();
    widget(Stack::column(gap::SECTION))
        .children((
            section(
                ui,
                "skins-preview",
                vec![block(skin_preview(ui, previewing))],
            ),
            section(
                ui,
                "skins-library",
                vec![
                    widget(Stack::column(0.0).padding_xy(inset::CARD, space::LG))
                        .children(skin_filters(ui, filter, grid))
                        .into_any(),
                    list.into_any(),
                ],
            ),
            section(
                ui,
                "skins-overrides",
                vec![
                    row(
                        ui,
                        "skins-override-name",
                        None,
                        color(
                            ui,
                            fallback.dialogue.nameplate.color,
                            |s| s.skin.overrides.name_color,
                            |s, v| s.skin.overrides.name_color = v,
                        ),
                    ),
                    row(
                        ui,
                        "skins-override-box",
                        None,
                        color(
                            ui,
                            fallback.dialogue.box_.color,
                            |s| s.skin.overrides.box_color,
                            |s, v| s.skin.overrides.box_color = v,
                        ),
                    ),
                    row(
                        ui,
                        "skins-override-accent",
                        None,
                        color(
                            ui,
                            fallback.chrome.accent,
                            |s| s.skin.overrides.accent,
                            |s, v| s.skin.overrides.accent = v,
                        ),
                    ),
                    row(
                        ui,
                        "skins-override-overflow",
                        Some("skins-override-overflow-hint"),
                        select(
                            ui,
                            select_keys(
                                ui,
                                &[
                                    ("skin", "skins-override-overflow-skin"),
                                    ("clip", "skins-override-overflow-clip"),
                                    ("scroll", "skins-override-overflow-scroll"),
                                    ("page", "skins-override-overflow-page"),
                                ],
                            ),
                            |s| s.skin.overrides.overflow.map_or("skin", |o| o.key()).into(),
                            |s, v| s.skin.overrides.overflow = Overflow::from_key(v),
                        ),
                    ),
                    row(
                        ui,
                        "skins-override-page-auto",
                        Some("skins-override-page-auto-hint"),
                        slider_with(
                            ui,
                            (0.0, 10.0, 0.5),
                            "s",
                            skin_at_zero,
                            |s| f64::from(s.skin.overrides.overflow_auto_ms.unwrap_or(0)) / 1000.0,
                            |s, v| {
                                s.skin.overrides.overflow_auto_ms =
                                    (v > 0.0).then_some((v * 1000.0) as u32)
                            },
                        ),
                    ),
                    row(
                        ui,
                        "skins-override-reset",
                        None,
                        button(ui, "common-reset", || {
                            edit(|s| s.skin.overrides = Default::default())
                        }),
                    ),
                ],
            ),
            section(
                ui,
                "skins-interface",
                vec![
                    toggle_row!(ui, "skins-advanced-effects", skin.advanced_effects),
                    toggle_row!(ui, "skins-hot-reload", skin.hot_reload),
                ],
            ),
        ))
        .into_any()
}

/// A labelled menu entry that closes its menu and sends `msg()`.
pub(super) fn menu_item(
    ui: Ui,
    open: Signal<bool>,
    key: &str,
    icon: nana_ui::runtime::Icon,
    msg: impl Fn() -> Msg + Send + 'static,
) -> AnyView {
    widget(ActionMenuItem::new(ui.now_t(key)).leading(icon))
        .on_cx(move |_, _: &Activate, cx| {
            open.set(false);
            cx.dispatch_program_all(msg());
        })
        .into_any()
}

/// Space between skin cards.
pub(super) const GRID_GAP: f32 = space::XL;
/// Height of a card's thumbnail well.
pub(super) const THUMB_H: f32 = 76.0;

/// Whether skin `id` is the one previewed (the active one until another is
/// picked), tracked.
pub(super) fn previewed(
    ui: Ui,
    previewing: Signal<Option<String>>,
    id: String,
) -> impl Fn() -> bool + Clone + Send + 'static {
    move || {
        previewing.with(|p| match p {
            Some(p) => *p == id,
            None => ui.settings.with(|s| s.skin.id == id),
        })
    }
}

/// The solid Apply button that makes skin `id` the active one, shown while
/// `shown` holds.
pub(super) fn apply_button(
    ui: Ui,
    id: String,
    shown: impl Fn() -> bool + Send + 'static,
) -> AnyView {
    widget(crate::ui::kit::cta(
        Button::new(ui.now_t("common-apply")).size(ControlSize::Small),
    ))
    .visible(shown)
    .on_cx(move |_, _: &Activate, cx| {
        let id = id.clone();
        cx.dispatch_program_all(edit(move |s| s.skin.id = id));
    })
    .into_any()
}

/// A skin's "⋯" menu: duplicate, export, open folder and, for a skin the
/// user added, delete (asked before it acts).
pub(super) fn skin_actions(ui: Ui, skin: &SkinRow, locale: Locale) -> AnyView {
    let (eid, did, rid) = (skin.id.clone(), skin.id.clone(), skin.id.clone());
    let folder = skin.folder.clone();
    let menu_open = signal(false);
    let menu = move |armed: Option<Signal<bool>>| {
        let mut items = vec![
            menu_item(ui, menu_open, "skins-duplicate", ti::COPY, {
                let id = did.clone();
                move || Msg::DuplicateSkin(id.clone())
            }),
            menu_item(ui, menu_open, "skins-export", ti::FILE_EXPORT, {
                let id = eid.clone();
                move || Msg::ExportSkin(id.clone())
            }),
        ];
        if let Some(f) = folder.clone() {
            items.push(menu_item(
                ui,
                menu_open,
                "skins-open-folder",
                ti::FOLDER_OPEN,
                move || Msg::OpenFolder(f.clone()),
            ));
        }
        if let Some(armed) = armed {
            items.push(
                widget(
                    ActionMenuItem::new(ui.now_t("skins-delete"))
                        .leading(ti::TRASH)
                        .danger(true),
                )
                .on(move |_: &Activate| {
                    menu_open.set(false);
                    armed.set(true);
                })
                .into_any(),
            );
        }
        widget(
            ActionMenu::new()
                .trigger_icon(ti::DOTS, ui.now_t("skins-more"))
                .bare_trigger(true),
        )
        .bind(move |m: &mut ActionMenu| m.popover.open = menu_open.get())
        .on(move |e: &PopoverToggled| menu_open.set(e.open))
        .on(move |_: &PopoverClosed| menu_open.set(false))
        .children(items)
        .into_any()
    };
    if skin.builtin {
        menu(None)
    } else {
        confirm(
            ui,
            I18N.tr_in(
                locale,
                "skins-delete-confirm",
                &[("name", skin.name.as_str().into())],
            ),
            ui.now_t("skins-delete"),
            move |armed| menu(Some(armed)),
            move || Msg::DeleteSkin(rid.clone()),
        )
    }
}

/// One skin in the list view: swatches, name, badges and its detail line.
pub(super) fn skin_list_row(
    ui: Ui,
    skin: &SkinRow,
    selected: bool,
    previewing: Signal<Option<String>>,
    locale: Locale,
) -> AnyView {
    let swatch = |color: [f32; 4]| {
        widget(
            Stack::row(0.0)
                .width(LengthSpec::Px(SWATCH))
                .height(LengthSpec::Px(SWATCH))
                .radius(RadiusTier::Full)
                .outline(SemanticColorRole::BorderStrong, 1.0)
                .with_layout(move |l| l.background = Some(color)),
        )
        .into_any()
    };
    // The "Fallback font" badge sits with the tags; the detail line only
    // names the missing family, so it stays one line in every language.
    let fallback = uses_fallback_font(skin).then(|| {
        unshrinking(widget(
            StatusBadge::new(
                I18N.t_in(locale, "skins-fallback-font"),
                StatusTone::Warning,
            )
            .compact(true),
        ))
    });
    let detail = detail_line(ui, skin, locale);
    let usable = skin.error.is_none();
    let previewed = previewed(ui, previewing, skin.id.clone());
    let trailing = widget(Stack::row(gap::CONTROL).align(AlignSpec::Center)).children((
        selected.then(|| current_badge(ui)),
        (!selected && usable).then(|| apply_button(ui, skin.id.clone(), previewed.clone())),
        skin_actions(ui, skin, locale),
    ));
    // The current skin keeps a 3px accent bar; the row being previewed
    // takes the selected fill.
    widget(
        Stack::row(0.0)
            .width(LengthSpec::Fill)
            .with_layout(|l| l.position = PositionSpec::Relative),
    )
    .children((
        previewable(
            widget(
                ListItem::new(skin.name.clone())
                    .style({
                        let mut st = ListItem::new("").style;
                        let l = Arc::make_mut(&mut st.layout);
                        l.flex_grow = Some(1.0);
                        l.flex_shrink = Some(1.0);
                        l.min_width = Some(LengthSpec::Px(0.0));
                        l.padding_top = Some(LengthSpec::Px(space::MD));
                        l.padding_bottom = Some(LengthSpec::Px(space::MD));
                        st
                    })
                    .auto_height(true),
            ),
            ui,
            previewing,
            skin,
            previewed,
        )
        .leading(widget(Stack::row(gap::TIGHT)).children((
            swatch(skin.box_color),
            swatch(skin.text_color),
            swatch(skin.accent),
        )))
        .content(
            widget(
                Stack::column(gap::HAIR)
                    .grow(1.0)
                    .shrink(1.0)
                    .with_layout(|l| l.min_width = Some(LengthSpec::Px(0.0))),
            )
            .children((
                // A long name is cut before the tags and the badge are.
                widget(
                    Stack::row(gap::TIGHT)
                        .align(AlignSpec::Center)
                        .width(LengthSpec::Fill)
                        .with_layout(|l| l.min_width = Some(LengthSpec::Px(0.0))),
                )
                .children((
                    widget(Text::new(skin.name.clone()).style({
                        let mut st = Type::Label.one_line();
                        Arc::make_mut(&mut st.layout).flex_grow = Some(0.0);
                        st
                    })),
                    badges(locale, skin)
                        .into_iter()
                        .map(unshrinking)
                        .collect::<Vec<_>>(),
                    fallback,
                )),
                hint(detail),
            )),
        )
        .trailing(trailing),
        widget({
            let bar = Stack::column(0.0)
                .width(LengthSpec::Px(3.0))
                .with_layout(accent_bar_layout);
            if selected {
                bar.surface(SemanticColorRole::Accent)
            } else {
                bar
            }
        }),
    ))
    .into_any()
}

/// The body family a skin asks for first, which this system lacks.
pub(super) fn missing_family(row: &SkinRow) -> Option<&str> {
    row.skin
        .as_ref()?
        .fonts
        .body
        .families
        .first()
        .map(String::as_str)
}

/// "Light · Classic": the skin's tone and tags on one line.
pub(super) fn tag_line(locale: Locale, row: &SkinRow) -> String {
    let tone = if row.dark { "dark" } else { "light" };
    std::iter::once(tone)
        .chain(row.tags.iter().map(String::as_str))
        .map(|t| tag_label(locale, t))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// Whether `skin` loads but lacks its body font, so it is drawn in a
/// fallback font and says so.
pub(super) fn uses_fallback_font(skin: &SkinRow) -> bool {
    skin.error.is_none() && skin.fallback_font
}

/// The list row's detail line: why a skin is broken, else its description
/// or author and version; then the family a fallback-font skin lacks.
pub(super) fn detail_line(ui: Ui, skin: &SkinRow, locale: Locale) -> String {
    let detail = match &skin.error {
        Some(e) => format!("{} · {e}", ui.now_t("skins-broken")),
        None if !skin.description.is_empty() && skin.builtin => skin.description.clone(),
        None if skin.builtin => ui.now_t("skins-builtin"),
        None => format!(
            "{} · {}",
            I18N.tr_in(
                locale,
                "skins-by",
                &[("author", skin.author.as_str().into())]
            ),
            I18N.tr_in(
                locale,
                "skins-version",
                &[("version", skin.version.as_str().into())]
            )
        ),
    };
    match missing_family(skin).filter(|_| uses_fallback_font(skin)) {
        Some(family) => format!(
            "{detail} · {}",
            I18N.tr_in(
                locale,
                "skins-fallback-font-missing",
                &[("family", family.into())]
            )
        ),
        None => detail,
    }
}

/// The "Current" badge of the active skin.
pub(super) fn current_badge(ui: Ui) -> AnyView {
    widget(StatusBadge::new(ui.now_t("skins-active"), StatusTone::Success).compact(true)).into_any()
}

/// `view` at its own width: a row cuts its neighbours before it.
pub(super) fn unshrinking(view: impl IntoView) -> AnyView {
    widget(Stack::row(0.0).with_layout(|l| l.flex_shrink = Some(0.0)))
        .children(view)
        .into_any()
}

/// A skin's row or card `item`: selected while `previewed` holds, and
/// previewing the skin when activated.
pub(super) fn previewable(
    item: El<ListItem>,
    ui: Ui,
    previewing: Signal<Option<String>>,
    skin: &SkinRow,
    previewed: impl Fn() -> bool + Send + 'static,
) -> El<ListItem> {
    let pick = skin.clone();
    item.bind(move |i: &mut ListItem| i.selected = previewed())
        .on_cx(move |_, _: &Activate, cx| {
            if let Some(message) = crate::ui::settings::preview::select(ui, previewing, &pick) {
                cx.dispatch_program_all(message);
            }
        })
}

/// One skin in the grid: a thumbnail of its box over its name and tags.
/// The previewed card takes the selected fill; the current skin carries a
/// "Current" badge in its name row and a 3px accent bar.
pub(super) fn skin_card(
    ui: Ui,
    skin: &SkinRow,
    current: bool,
    previewing: Signal<Option<String>>,
    locale: Locale,
) -> AnyView {
    let usable = skin.error.is_none();
    let previewed = previewed(ui, previewing, skin.id.clone());
    let dark = ui.chrome_dark.get_untracked();
    let backdrop = BackgroundImage::url(crate::ui::kit::checker_url(dark));
    let well = widget(
        Stack::row(0.0)
            .width(LengthSpec::Fill)
            .height(LengthSpec::Px(THUMB_H))
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center)
            .radius(nana_ui_core::RadiusTier::Sm)
            .with_layout(move |l| {
                l.position = PositionSpec::Relative;
                l.overflow_x = OverflowSpec::Hidden;
                l.overflow_y = OverflowSpec::Hidden;
                l.paint.background_image = Some(backdrop);
            }),
    )
    .children(super::skin_grid::miniature(&super::skin_grid::thumb(skin)));
    let (tags, tone) = match &skin.error {
        Some(_) => (I18N.t_in(locale, "skins-broken"), SemanticColorRole::Danger),
        None => (tag_line(locale, skin), SemanticColorRole::Muted),
    };
    // "· Fallback font" never shrinks: when the line is too long for the
    // card (English, Japanese), the tags are cut, not the hint.
    let font_note = uses_fallback_font(skin).then(|| {
        let mut st = Type::Caption.style();
        let l = Arc::make_mut(&mut st.layout);
        l.white_space_nowrap = true;
        l.flex_shrink = Some(0.0);
        widget(
            Text::new(format!(" · {}", I18N.t_in(locale, "skins-fallback-font")))
                .style(st)
                .color(SemanticColorRole::Muted),
        )
    });
    let tags_row = widget(
        Stack::row(0.0)
            .width(LengthSpec::Fill)
            .with_layout(|l| l.min_width = Some(LengthSpec::Px(0.0))),
    )
    .children((
        widget(
            Text::new(tags)
                .style({
                    // Shrinks with an ellipsis but does not grow, so the
                    // hint follows the tags rather than the card's edge.
                    let mut st = Type::Caption.one_line();
                    Arc::make_mut(&mut st.layout).flex_grow = Some(0.0);
                    st
                })
                .color(tone),
        ),
        font_note,
    ));
    let name_row = widget(
        Stack::row(gap::TIGHT)
            .align(AlignSpec::Center)
            .width(LengthSpec::Fill),
    )
    .children((
        widget(Text::new(skin.name.clone()).style(Type::Label.one_line())),
        // The current skin's badge takes the Apply button's place, off the
        // thumbnail, so no box is covered.
        current.then(|| unshrinking(current_badge(ui))),
        (!current && usable).then(|| apply_button(ui, skin.id.clone(), previewed.clone())),
        skin_actions(ui, skin, locale),
    ));
    let card = previewable(
        widget(
            ListItem::new(skin.name.clone())
                .style({
                    let mut st = ListItem::new("")
                        .style
                        .outline(SemanticColorRole::BorderSoft, 1.0)
                        .radius(nana_ui_core::RadiusTier::Md);
                    st.control_padding_x = None;
                    let l = Arc::make_mut(&mut st.layout);
                    l.width = Some(LengthSpec::Fill);
                    l.padding_left = Some(LengthSpec::Px(space::MD));
                    l.padding_right = Some(LengthSpec::Px(space::MD));
                    l.padding_top = Some(LengthSpec::Px(space::MD));
                    l.padding_bottom = Some(LengthSpec::Px(space::MD));
                    st
                })
                .auto_height(true),
        ),
        ui,
        previewing,
        skin,
        previewed,
    )
    .content(
        widget(
            Stack::column(gap::CONTROL)
                .grow(1.0)
                .shrink(1.0)
                .with_layout(|l| l.min_width = Some(LengthSpec::Px(0.0))),
        )
        .children((
            well,
            widget(Stack::column(space::XXS).padding_xy(space::XXS, 0.0))
                .children((name_row, tags_row)),
        )),
    );
    widget(Stack::column(0.0).with_layout(|l| {
        l.position = PositionSpec::Relative;
        // Three to a row; the half pixel keeps rounding from wrapping one.
        l.width = Some(crate::ui::surface::calc(
            100.0 / 3.0,
            -GRID_GAP * 2.0 / 3.0 - 0.5,
        ));
    }))
    .children((
        card,
        current.then(|| {
            widget(
                Stack::column(0.0)
                    .width(LengthSpec::Px(3.0))
                    .surface(SemanticColorRole::Accent)
                    .radius(nana_ui_core::RadiusTier::Xs)
                    .with_layout(|l| {
                        l.position = PositionSpec::Absolute;
                        l.offset_left = Some(LengthSpec::Px(0.0));
                        l.offset_top = Some(LengthSpec::Px(14.0));
                        l.offset_bottom = Some(LengthSpec::Px(14.0));
                    }),
            )
        }),
    ))
    .into_any()
}

/// Grid or list: a two-state toggle of two icons in one outlined pill, the
/// current view selected. Fixed width, so it never wraps.
pub(super) fn view_switch(ui: Ui, grid: Signal<bool>) -> AnyView {
    let item = |icon: nana_ui::runtime::Icon, key: &str, as_grid: bool| {
        widget(
            IconButton::new(icon, ui.now_t(key))
                .size(ControlSize::Small)
                .with_tooltip(ui.now_t(key)),
        )
        .bind(move |b: &mut IconButton| b.selected = grid.get() == as_grid)
        .on(move |_: &Activate| grid.set(as_grid))
        .into_any()
    };
    widget(
        Stack::row(space::XXS)
            .align(AlignSpec::Center)
            .padding_xy(space::XXS, space::XXS)
            .outline(SemanticColorRole::BorderSoft, 1.0)
            .radius(nana_ui_core::RadiusTier::Md)
            .with_layout(|l| {
                l.flex_shrink = Some(0.0);
            }),
    )
    .children((
        item(ti::LAYOUT_GRID, "skins-view-grid", true),
        item(ti::LIST, "skins-view-list", false),
    ))
    .into_any()
}

/// `light`, `dark` or a tag from [`nanatype_theme::SKIN_TAGS`], localized.
pub(super) fn tag_label(locale: Locale, tag: &str) -> String {
    I18N.t_in(locale, &format!("skins-tag-{tag}"))
}

/// The light/dark badge and the tag badges of a skin that loads.
pub(super) fn badges(locale: Locale, row: &SkinRow) -> Vec<AnyView> {
    if row.error.is_some() {
        return Vec::new();
    }
    let tone = if row.dark { "dark" } else { "light" };
    std::iter::once(tone)
        .chain(row.tags.iter().map(String::as_str))
        .map(|t| {
            widget(StatusBadge::new(tag_label(locale, t), StatusTone::Neutral).compact(true))
                .into_any()
        })
        .collect()
}

/// Whether `row` passes the picker's filter (`light`, `dark` or a tag).
pub(super) fn matches_filter(row: &SkinRow, filter: Option<&str>) -> bool {
    match filter {
        None => true,
        Some("light") => !row.dark,
        Some("dark") => row.dark,
        Some(tag) => row.tags.iter().any(|t| t == tag),
    }
}

/// Chips that narrow the list: all, light, dark, then every tag in use;
/// then the grid/list switch.
pub(super) fn skin_filters(ui: Ui, filter: Signal<Option<String>>, grid: Signal<bool>) -> AnyView {
    dynamic(
        move || {
            (
                filter.get(),
                ui.skins.with(|s| Arc::as_ptr(s) as usize),
                ui.locale.get(),
            )
        },
        move |(current, _, locale)| {
            let rows = ui.skins.get_untracked();
            let mut keys: Vec<Option<String>> =
                vec![None, Some("light".into()), Some("dark".into())];
            for tag in nanatype_theme::SKIN_TAGS {
                if rows.iter().any(|r| r.tags.iter().any(|t| t == tag)) {
                    keys.push(Some((*tag).to_owned()));
                }
            }
            let chips: Vec<AnyView> = keys
                .into_iter()
                .map(|key| {
                    let text = match &key {
                        None => I18N.t_in(*locale, "skins-filter-all"),
                        Some(tag) => tag_label(*locale, tag),
                    };
                    let selected = *current == key;
                    widget(Chip::new(text).selected(selected).size(ControlSize::Small))
                        .on_cx(move |_, _: &Activate, _| filter.set(key.clone()))
                        .into_any()
                })
                .collect();
            // The chips wrap among themselves; the grid/list toggle keeps
            // the top right of the row, beside the first line, in every
            // language.
            widget(
                Stack::row(gap::GROUP)
                    .align(AlignSpec::Start)
                    .width(LengthSpec::Fill),
            )
            .children((
                widget(
                    Stack::row(gap::TIGHT)
                        .wrap(true)
                        .align(AlignSpec::Center)
                        .grow(1.0)
                        .shrink(1.0)
                        .with_layout(|l| {
                            l.min_width = Some(LengthSpec::Px(0.0));
                            l.row_gap = Some(LengthSpec::Px(6.0));
                        }),
                )
                .children(chips),
                view_switch(ui, grid),
            ))
            .into_any()
        },
    )
    .into_any()
}

/// A live sample of the previewed skin (the active one by default) with the
/// user's personalisation, scaled to fit the page.
pub(super) fn skin_preview(ui: Ui, previewing: Signal<Option<String>>) -> AnyView {
    crate::ui::settings::preview::view(ui, previewing)
}
