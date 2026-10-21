//! A tab-controlled, in-flow sidebar beside the fixed-size stream preview.
use super::*;
use crate::ui::kit::{gap, inset};
use nanatype_settings::{InputDockRegion as Region, InputDockRegionKind as RegionKind};

/// The dock's tab strip, inside the toolbar-high bar.
const TAB_AREA_H: f32 = 32.0;
/// The partner card's scroll height in its footer popover.
const MANAGEMENT_H: f32 = 260.0;

const EDITOR_MIN: f32 = 320.0;
const HANDLE_W: f32 = 8.0;

fn sidebar_limit(client_width: f32) -> f32 {
    (client_width - EDITOR_MIN - HANDLE_W).clamp(
        nanatype_settings::INPUT_SIDEBAR_MIN,
        nanatype_settings::INPUT_SIDEBAR_MAX,
    )
}

pub(super) fn changed_width(ui: Ui, cx: &nana_ui::runtime::AppContext) -> Option<f32> {
    let pane = nana_ui::runtime::Entity::<nana_ui::runtime::SplitPane>::from_stable_id(
        ui.input_split.get_untracked()?,
    );
    let (width, limits) = cx.read(pane, |p| (p.model.size(), p.model.limits())).ok()?;
    let max = sidebar_limit(ui.input_width.get_untracked());
    let preferred = ui.input_sidebar_width.get_untracked();
    // Window/DPI layout changes only constrain the view; they do not replace
    // the user's preferred width. Ignore a model awaiting its layout binding.
    ((limits.1 - max).abs() < 0.1 && (width - preferred.min(max)).abs() > 0.1).then_some(width)
}

pub(super) fn split(ui: Ui, editor: impl IntoView) -> AnyView {
    use nana_ui::runtime::SplitPane;
    use nana_ui_core::{SplitAxis, SplitPaneModel, SplitPaneMutation};
    let model = |size, max| {
        SplitPaneModel::new(
            SplitAxis::Horizontal,
            size,
            nanatype_settings::INPUT_SIDEBAR_MIN,
            max,
        )
        .with_from_end(true)
    };
    widget(SplitPane::new(&model(
        ui.input_sidebar_width.get_untracked(),
        sidebar_limit(ui.input_width.get_untracked()),
    )))
    .key("input-workspace-split")
    .node_ref(ui.input_split)
    .bind(move |pane: &mut SplitPane| {
        let max = sidebar_limit(ui.input_width.get());
        let size = ui.input_sidebar_width.get().min(max);
        if pane.model.limits().1 != max {
            pane.model = model(size, max);
        } else {
            // Keep the in-progress drag's start position and pointer capture.
            pane.apply(SplitPaneMutation::SetSize(size));
        }
        let l = Arc::make_mut(&mut pane.style.layout);
        l.flex_grow = Some(1.0);
        l.flex_shrink = Some(1.0);
        l.min_height = Some(LengthSpec::Px(0.0));
    })
    .first(editor)
    .second(widget(Stack::fill_column(0.0)).children(dock(ui)))
    .into_any()
}

fn command(ui: Ui, key: &str, value: Marker, width: Signal<f32>) -> AnyView {
    widget(Stack::row(0.0))
        .bind(move |s: &mut Stack| {
            let narrow = width.get() < 260.0;
            let l = s.layout_mut();
            l.width = Some(if narrow {
                LengthSpec::Fill
            } else {
                LengthSpec::Px(0.0)
            });
            l.min_width = Some(LengthSpec::Px(0.0));
            l.flex_grow = Some(if narrow { 0.0 } else { 1.0 });
        })
        .children(
            widget(Button::new(ui.now_t(key)).size(ControlSize::Small)).on_cx(
                move |_, _: &Activate, cx| {
                    cx.dispatch_program_all(marker(value.clone()));
                },
            ),
        )
        .into_any()
}

fn commands(ui: Ui, width: Signal<f32>) -> AnyView {
    dynamic(
        move || ui.skin.with(|s| Arc::as_ptr(s) as usize),
        move |_| {
            let skin = ui.skin.get_untracked();
            widget(
                Stack::column(0.0)
                    .gap_step(gap::step::CONTROL)
                    .width(LengthSpec::Fill)
                    .padding_step(gap::step::TIGHT, gap::step::TIGHT),
            )
            .children((
                command_row(width).children((
                    command(ui, "dock-wait", Marker::Wait { ms: 500 }, width),
                    command(ui, "tb-pause", Marker::Pause { auto_ms: None }, width),
                )),
                command_row(width).children((
                    command(ui, "tb-page", Marker::Page, width),
                    command(
                        ui,
                        "tb-screenshake",
                        Marker::ScreenShake {
                            amp: skin.effects.screen_shake_px,
                            ms: 300,
                        },
                        width,
                    ),
                )),
            ))
            .into_any()
        },
    )
    .into_any()
}

fn command_row(width: Signal<f32>) -> nana_ui::runtime::view::El<Stack> {
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::CONTROL)
            .width(LengthSpec::Fill),
    )
    .bind(move |s: &mut Stack| {
        s.layout_mut().direction = Some(if width.get() < 260.0 {
            nana_ui_core::FlexDirection::Column
        } else {
            nana_ui_core::FlexDirection::Row
        });
    })
}

const REGION_KINDS: [RegionKind; 3] = [
    RegionKind::Commands,
    RegionKind::Stickers,
    RegionKind::Phrases,
];

fn region_meta(kind: RegionKind) -> (&'static str, nana_ui::runtime::Icon) {
    match kind {
        RegionKind::Commands | RegionKind::Unknown => ("dock-tab-commands", ti::TEXT_PLUS),
        RegionKind::Stickers => ("dock-stickers", ti::STICKER),
        RegionKind::Phrases => ("tb-phrases", ti::QUOTE),
    }
}

fn region_title(ui: Ui, region: &Region) -> String {
    let label = ui.now_t(region_meta(region.kind).0);
    if region.number > 1 {
        format!("{label} {}", region.number)
    } else {
        label
    }
}

fn region_value(region: &Region) -> String {
    match region.id {
        0 => "commands".into(),
        1 => "stickers".into(),
        id => format!("region-{id}"),
    }
}

fn register_sticker_tab_drop<V: nana_ui::runtime::ComponentView>(
    cx: &mut nana_ui::runtime::AppContext,
    target: nana_ui::runtime::Entity<V>,
    selected: Signal<u64>,
    region: u64,
) {
    let _ = cx.set_drop_target(target, nana_ui_core::DropAccepts::files());
    let _ = cx.on(
        target,
        move |_, event: &nana_ui::runtime::FileDropEvent, cx| match event {
            nana_ui::runtime::FileDropEvent::Hovered { .. } => selected.set(region),
            nana_ui::runtime::FileDropEvent::Dropped { paths, .. } => {
                selected.set(region);
                cx.dispatch_program_all(Msg::StickerImportFolders(paths.to_vec()));
            }
            nana_ui::runtime::FileDropEvent::Left => {}
        },
    );
}

fn phrases(ui: Ui) -> AnyView {
    dynamic(
        move || ui.settings.with(|s| s.assets.phrases.clone()),
        move |phrases| {
            let buttons: Vec<AnyView> = phrases
                .iter()
                .enumerate()
                .map(|(i, phrase)| {
                    widget(Button::new(phrase.label.clone()).size(ControlSize::Small))
                        .on_cx(move |_, _: &Activate, cx| {
                            cx.dispatch_program_all(Msg::InsertPhrase(i))
                        })
                        .into_any()
                })
                .collect();
            widget(
                Stack::column(0.0)
                    .gap_step(gap::step::CONTROL)
                    .width(LengthSpec::Fill)
                    .padding_step(gap::step::TIGHT, gap::step::TIGHT),
            )
            .children((
                buttons,
                phrases.is_empty().then(|| {
                    widget(
                        Button::new(ui.now_t("empty-phrases"))
                            .kind(ButtonKind::Text)
                            .size(ControlSize::Small),
                    )
                    .on_cx(|_, _: &Activate, cx| {
                        cx.dispatch_program_all(Msg::SettingsPage(
                            super::super::settings::Page::Assets,
                        ))
                    })
                }),
            ))
            .into_any()
        },
    )
    .into_any()
}

fn dock_panel(ui: Ui, region: Region, selected: Signal<u64>, width: Signal<f32>) -> AnyView {
    let body = match region.kind {
        RegionKind::Stickers => stickers::panel(ui, None, Some(region.id)),
        RegionKind::Commands | RegionKind::Unknown => commands(ui, width),
        RegionKind::Phrases => phrases(ui),
    };
    widget(
        Stack::fill_column(0.0)
            .padding_step(inset::step::PANEL, inset::step::PANEL)
            .with_layout(|l| {
                l.height = Some(LengthSpec::Px(0.0));
                l.flex_grow = Some(1.0);
                l.min_height = Some(LengthSpec::Px(0.0));
                l.min_width = Some(LengthSpec::Px(0.0));
            }),
    )
    .key(match region.id {
        0 => "input-dock-commands-panel".into(),
        1 => "input-dock-stickers-panel".into(),
        id => format!("input-dock-region-{id}-panel"),
    })
    .visible(move || selected.get() == region.id)
    .children(if region.kind == RegionKind::Stickers {
        body
    } else {
        widget(
            ScrollView::new(ScrollAxes::Vertical)
                .scrollbars(nana_ui_core::ScrollbarVisibility::Always)
                .with_layout(|l| {
                    l.width = Some(LengthSpec::Fill);
                    l.height = Some(LengthSpec::Px(0.0));
                    l.min_height = Some(LengthSpec::Px(0.0));
                    l.flex_grow = Some(1.0);
                    l.flex_shrink = Some(1.0);
                }),
        )
        .key(match region.id {
            0 => "input-dock-commands-scroll".into(),
            1 => "input-dock-stickers-scroll".into(),
            id => format!("input-dock-region-{id}-scroll"),
        })
        .children(body)
        .into_any()
    })
    .into_any()
}

// Reserve the add button before choosing either tab presentation. Use a
// conservative caption budget, leaving enough room for the icon and padding.
fn tab_space(width: f32) -> f32 {
    (width - 16.0 - TARGET - 4.0).max(TARGET)
}

fn compact_tabs(ui: Ui, regions: &[Region], width: f32) -> bool {
    let captions: f32 = regions
        .iter()
        .map(|r| {
            region_title(ui, r)
                .chars()
                .map(|c| if c.is_ascii() { 8.0 } else { 14.0 })
                .sum::<f32>()
                + 40.0
        })
        .sum();
    captions + regions.len().saturating_sub(1) as f32 * 4.0 > tab_space(width)
}

fn visible_icons(regions: &[Region], selected: u64, width: f32) -> Vec<Region> {
    let capacity = ((tab_space(width) + 4.0) / (TARGET + 4.0)).floor() as usize;
    // Overflow reserves a separate menu button; never hide the active region.
    let slots = if regions.len() > capacity {
        capacity.saturating_sub(1)
    } else {
        capacity
    }
    .max(1);
    let mut visible: Vec<_> = regions.iter().take(slots).copied().collect();
    if !visible.iter().any(|r| r.id == selected)
        && let Some(active) = regions.iter().find(|r| r.id == selected)
        && let Some(last) = visible.last_mut()
    {
        *last = *active;
    }
    visible
}

fn compact_tab_bar(
    ui: Ui,
    regions: Signal<Vec<Region>>,
    selected: Signal<u64>,
    width: Signal<f32>,
) -> AnyView {
    use nana_ui::runtime::view::each;
    let open = signal(false);
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::TIGHT)
            .width(LengthSpec::Fill)
            .with_layout(|l| {
                l.min_width = Some(LengthSpec::Px(0.0));
                l.flex_shrink = Some(1.0);
            }),
    )
    .key("input-dock-icon-tabs")
    .roving_focus(nana_ui::runtime::RovingFocusGroup::horizontal())
    .visible(move || compact_tabs(ui, &regions.get(), width.get()))
    .children((
        each(
            regions,
            |r| r.id,
            move |region| {
                let title = region_title(ui, &region);
                let target = entity_ref::<IconButton>();
                if region.kind == RegionKind::Stickers {
                    nana_ui::runtime::view::on_mount(move |cx| {
                        if let Some(target) = target.get() {
                            register_sticker_tab_drop(cx, target, selected, region.id);
                        }
                    });
                }
                widget(tool_button(
                    region_meta(region.kind).1,
                    title.clone(),
                    title,
                ))
                .entity_ref(target)
                .key(format!("input-dock-icon-{}", region_value(&region)))
                .bind(move |button: &mut IconButton| {
                    button.selected = selected.get() == region.id;
                    let l = Arc::make_mut(&mut button.style.layout);
                    l.width = Some(LengthSpec::Px(TARGET));
                    l.min_width = Some(LengthSpec::Px(TARGET));
                    l.flex_shrink = Some(0.0);
                })
                .visible(move || {
                    visible_icons(&regions.get(), selected.get(), width.get())
                        .iter()
                        .any(|r| r.id == region.id)
                })
                .on(move |_: &Activate| selected.set(region.id))
            },
        )
        .container(widget(Stack::row(0.0).gap_step(gap::step::TIGHT))),
        dynamic(
            move || {
                let all = regions.get();
                let visible = visible_icons(&all, selected.get(), width.get());
                all.into_iter()
                    .filter(|r| !visible.iter().any(|v| v.id == r.id))
                    .collect::<Vec<_>>()
            },
            move |hidden| {
                if hidden.is_empty() {
                    return widget(Stack::row(0.0)).into_any();
                }
                let items = hidden
                    .iter()
                    .map(|region| {
                        let id = region.id;
                        widget(
                            ActionMenuItem::new(region_title(ui, region))
                                .leading(region_meta(region.kind).1),
                        )
                        .on(move |_: &Activate| {
                            open.set(false);
                            selected.set(id);
                        })
                        .into_any()
                    })
                    .collect();
                menu(ti::DOTS, ui.now_t("dock-more-regions"), open, items)
            },
        ),
    ))
    .into_any()
}

fn dock(ui: Ui) -> AnyView {
    use nana_ui::runtime::view::each;
    use nana_ui::runtime::{TabOption, Tabs, TabsEvent};
    let selected = ui.input_dock_selected;
    let regions = ui.input_dock_regions;
    let width = signal(nanatype_settings::INPUT_SIDEBAR_DEFAULT);
    widget(
        Stack::fill_column(0.0)
            .surface(SemanticColorRole::Subtle)
            .with_layout(|l| {
                l.min_width = Some(LengthSpec::Px(0.0));
            }),
    )
    .key("input-insertion-dock")
    .on(move |e: &SizeChanged| {
        if width.get_untracked() != e.width {
            width.set(e.width);
        }
    })
    .children(dynamic(
        move || ui.locale.get(),
        move |_| {
            let open = signal(false);
            let mut add_items: Vec<AnyView> = REGION_KINDS
                .into_iter()
                .map(|kind| {
                    widget(
                        ActionMenuItem::new(ui.now_t(region_meta(kind).0))
                            .leading(region_meta(kind).1),
                    )
                    .on(move |_: &Activate| {
                        let current = regions.get_untracked();
                        let id = current
                            .iter()
                            .map(|r| r.id)
                            .max()
                            .and_then(|id| id.checked_add(1))
                            .unwrap_or_else(|| {
                                (0..)
                                    .find(|id| !current.iter().any(|r| r.id == *id))
                                    .unwrap()
                            });
                        let number = current
                            .iter()
                            .filter(|r| r.kind == kind)
                            .map(|r| r.number)
                            .max()
                            .unwrap_or(0)
                            .saturating_add(1);
                        // New regions are immediately visible, even in a narrow strip.
                        regions.update(|r| r.insert(0, Region { id, kind, number }));
                        selected.set(id);
                        open.set(false);
                    })
                    .into_any()
                })
                .collect();
            add_items.push(widget(Divider::horizontal()).into_any());
            add_items.push(
                widget(ActionMenuItem::new(ui.now_t("dock-close-region")))
                    .bind(move |item: &mut ActionMenuItem| item.disabled = regions.get().len() <= 1)
                    .on(move |_: &Activate| {
                        close_region(regions, selected, selected.get_untracked());
                        open.set(false);
                    })
                    .into_any(),
            );
            widget(Stack::fill_column(0.0).with_layout(|l| l.min_width = Some(LengthSpec::Px(0.0))))
                .children((
                    widget(
                        Stack::bar(0.0)
                            .gap_step(gap::step::TIGHT)
                            .height(LengthSpec::Px(TOOLBAR_H))
                            .padding_xy(inset::BAR_Y, 0.0)
                            .with_layout(|l| l.flex_shrink = Some(0.0)),
                    )
                    .children((
                        widget(Stack::row(0.0).with_layout(|l| {
                            l.width = Some(LengthSpec::Px(0.0));
                            l.min_width = Some(LengthSpec::Px(0.0));
                            l.height = Some(LengthSpec::Px(TAB_AREA_H));
                            l.flex_grow = Some(1.0);
                            l.flex_shrink = Some(1.0);
                            l.overflow_x = OverflowSpec::Hidden;
                        }))
                        .key("input-dock-tab-area")
                        .children((
                            // Hidden, the full tabs must leave the strip to the icon bar.
                            widget(Stack::row(0.0).with_layout(|l| l.flex_shrink = Some(0.0)))
                                .visible(move || !compact_tabs(ui, &regions.get(), width.get()))
                                .children(dynamic(
                                    move || regions.get(),
                                    move |all| {
                                        let target = entity_ref::<Tabs>();
                                        let all = all.clone();
                                        nana_ui::runtime::view::on_mount(move |cx| {
                                            if let Some(tabs) = target.get() {
                                                for region in all
                                                    .iter()
                                                    .filter(|r| r.kind == RegionKind::Stickers)
                                                {
                                                    if let Some(node) = cx
                                                        .read(tabs, |tabs| {
                                                            tabs.option_node(&region_value(region))
                                                        })
                                                        .ok()
                                                        .flatten()
                                                    {
                                                        let option = nana_ui::runtime::Entity::<
                                                            nana_ui::runtime::SegmentedOption,
                                                        >::from_stable_id(
                                                            node
                                                        );
                                                        register_sticker_tab_drop(
                                                            cx, option, selected, region.id,
                                                        );
                                                    }
                                                }
                                            }
                                        });
                                        widget(Tabs::new("commands").accepts_external_drop(false))
                                            .key("input-dock-tabs")
                                            .entity_ref(target)
                                            .bind(move |tabs: &mut Tabs| {
                                                let regions = regions.get();
                                                let selected = selected.get();
                                                tabs.options = regions
                                                    .iter()
                                                    .map(|region| {
                                                        TabOption::new(
                                                            region_value(region),
                                                            region_title(ui, region),
                                                        )
                                                        .icon(region_meta(region.kind).1)
                                                        .draggable(false)
                                                        .closable(true)
                                                    })
                                                    .collect();
                                                tabs.selected = regions
                                                    .iter()
                                                    .find(|r| r.id == selected)
                                                    .map(|r| Arc::from(region_value(r)));
                                                let l = Arc::make_mut(&mut tabs.style.layout);
                                                l.flex_shrink = Some(0.0);
                                            })
                                            .on(move |e: &TabsEvent| {
                                                let (TabsEvent::Select(value)
                                                | TabsEvent::Close(value)) = e
                                                else {
                                                    return;
                                                };
                                                if let Some(region) = regions
                                                    .get_untracked()
                                                    .iter()
                                                    .find(|r| region_value(r) == value.as_ref())
                                                {
                                                    if matches!(e, TabsEvent::Close(_)) {
                                                        close_region(regions, selected, region.id);
                                                    } else {
                                                        selected.set(region.id);
                                                    }
                                                }
                                            })
                                    },
                                )),
                            compact_tab_bar(ui, regions, selected, width),
                        )),
                        widget(Stack::row(0.0).with_layout(|l| {
                            l.width = Some(LengthSpec::Px(TARGET));
                            l.min_width = Some(LengthSpec::Px(TARGET));
                            l.flex_shrink = Some(0.0);
                        }))
                        .key("input-dock-add")
                        .children(menu(
                            ti::PLUS,
                            ui.now_t("dock-add-region"),
                            open,
                            add_items,
                        )),
                    )),
                    widget(Divider::horizontal()),
                    each(
                        regions,
                        |r| r.id,
                        move |r| dock_panel(ui, r, selected, width),
                    )
                    .container(widget(Stack::fill_column(0.0).with_layout(
                        |l| {
                            l.height = Some(LengthSpec::Px(0.0));
                            l.min_height = Some(LengthSpec::Px(0.0));
                            l.min_width = Some(LengthSpec::Px(0.0));
                            l.flex_grow = Some(1.0);
                        },
                    ))),
                ))
                .into_any()
        },
    ))
    .into_any()
}

fn close_region(regions: Signal<Vec<Region>>, selected: Signal<u64>, id: u64) {
    let mut current = regions.get_untracked();
    if current.len() <= 1 {
        return;
    }
    current.retain(|region| region.id != id);
    if selected.get_untracked() == id {
        selected.set(current[0].id);
    }
    regions.set(current);
}

fn modes(ui: Ui, mode: InputMode) -> AnyView {
    widget(SegmentedControl::new().size(ControlSize::Small))
        .children((
            segmented_option(ui.now_t("mode-compose-short"))
                .selected(mode == InputMode::Compose)
                .on_cx(|_, _: &nana_ui::runtime::SegmentedSelectionRequested, cx| {
                    cx.dispatch_program_all(Msg::Edit(Box::new(|s: &mut Settings| {
                        s.general.input_mode = InputMode::Compose
                    })))
                }),
            segmented_option(ui.now_t("mode-mirror-short"))
                .selected(mode == InputMode::Mirror)
                .on_cx(|_, _: &nana_ui::runtime::SegmentedSelectionRequested, cx| {
                    cx.dispatch_program_all(Msg::Edit(Box::new(|s: &mut Settings| {
                        s.general.input_mode = InputMode::Mirror
                    })))
                }),
        ))
        .into_any()
}

pub(super) fn footer(ui: Ui, invitation: Signal<String>, relay: Signal<String>) -> AnyView {
    dynamic(
        move || {
            (
                ui.locale.get(),
                ui.input_width.get() < 600.0,
                ui.settings.with(|s| s.general.input_mode),
            )
        },
        move |(_, compact, mode)| {
            let mode = *mode;
            let mode_control = if *compact {
                popover(
                    ti::ADJUSTMENTS,
                    ui.now_t("tray-mode"),
                    crate::ui::kit::popover::SM,
                    signal(false),
                    modes(ui, mode),
                )
            } else {
                modes(ui, mode)
            };
            widget(
                Stack::bar(0.0)
                    .gap_step(gap::step::CONTROL)
                    .width(LengthSpec::Fill)
                    .height(LengthSpec::Px(crate::windows::INPUT_ACTIONS_H))
                    .padding_step(inset::step::BAR_X, inset::step::BAR_Y)
                    .surface(SemanticColorRole::Subtle)
                    .align(AlignSpec::Center)
                    .with_layout(|l| l.flex_shrink = Some(0.0)),
            )
            .key("input-actions")
            .children((
                mode_control,
                status_content(ui),
                super::super::bilibili::input_send_status(ui),
                popover(
                    ti::USERS,
                    ui.now_t("input-management"),
                    crate::ui::kit::popover::LG,
                    signal(false),
                    widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|l| {
                        l.height = Some(LengthSpec::Px(MANAGEMENT_H));
                        l.min_height = Some(LengthSpec::Px(0.0));
                    }))
                    .children(super::super::p2p::card_with_fields(
                        ui, invitation, relay, true,
                    )),
                ),
                tool(ui, ti::SETTINGS, "window-settings", || Msg::OpenSettings),
                super::super::bilibili::account_button(ui),
                widget(crate::ui::kit::cta(
                    Button::new(ui.now_t("tb-send")).icon(ti::SEND),
                ))
                .on_cx(move |_, _: &Activate, cx| {
                    super::super::motion::press(cx, ui.reduced_motion.get_untracked());
                    cx.dispatch_program_all(Msg::Core(Cmd::Send));
                }),
            ))
            .into_any()
        },
    )
    .into_any()
}
