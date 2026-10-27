//! The input sidebar beside the fixed-size stream preview: a rail of panels
//! (controls, stickers, phrases, chat) at the window's edge, with live chat
//! optionally pinned above the insert panels.
use super::*;
use crate::ui::kit::{gap, inset};
use nanatype_settings::{INPUT_CHAT_MIN, InputDockLayout, InputDockTab as Tab};

/// The partner card's scroll height in its footer popover.
const MANAGEMENT_H: f32 = 260.0;

const EDITOR_MIN: f32 = 320.0;
const HANDLE_W: f32 = 8.0;
/// The rail of panel icons at the sidebar's outer edge; all that is left of
/// the sidebar when it is folded.
pub(crate) const RAIL_W: f32 = 40.0;
/// The insert panels keep at least this much under pinned chat.
const PANELS_MIN_H: f32 = 180.0;

fn sidebar_limit(client_width: f32) -> f32 {
    (client_width - EDITOR_MIN - HANDLE_W).clamp(
        nanatype_settings::INPUT_SIDEBAR_MIN,
        nanatype_settings::INPUT_SIDEBAR_MAX,
    )
}

/// The width the user dragged the sidebar to, if it changed.
pub(super) fn changed_width(ui: Ui, cx: &nana_ui::runtime::AppContext) -> Option<f32> {
    let pane = nana_ui::runtime::Entity::<nana_ui::runtime::SplitPane>::from_stable_id(
        ui.input_split.get_untracked()?,
    );
    let (width, limits) = cx.read(pane, |p| (p.model.size(), p.model.limits())).ok()?;
    let max = sidebar_limit(ui.input_width.get_untracked());
    let preferred = ui.input_sidebar_width.get_untracked();
    // Window/DPI layout changes only constrain the view; they do not replace
    // the user's preferred width. Ignore a model awaiting its layout binding,
    // and a folded sidebar, whose limits are the rail's.
    ((limits.1 - max).abs() < 0.1 && (width - preferred.min(max)).abs() > 0.1).then_some(width)
}

/// The height the user dragged pinned chat to, if it changed.
pub(super) fn changed_chat_height(ui: Ui, cx: &nana_ui::runtime::AppContext) -> Option<f32> {
    let pane = nana_ui::runtime::Entity::<nana_ui::runtime::SplitPane>::from_stable_id(
        ui.input_chat_split.get_untracked()?,
    );
    let (height, limits) = cx.read(pane, |p| (p.model.size(), p.model.limits())).ok()?;
    let dock = ui.input_dock.get_untracked();
    // A shorter window only clamps the pane; the preference stays.
    (dock.chat_pinned && (height - dock.chat_height.min(limits.1)).abs() > 0.1).then_some(height)
}

/// The window's two panes: the editor and the sidebar, which folds to its
/// rail.
pub(super) fn split(ui: Ui, editor: impl IntoView) -> AnyView {
    use nana_ui::runtime::SplitPane;
    use nana_ui_core::{SplitAxis, SplitPaneModel, SplitPaneMutation};
    let model = |size, min, max| {
        SplitPaneModel::new(SplitAxis::Horizontal, size, min, max).with_from_end(true)
    };
    // (size, min, max): a folded sidebar is held at the rail's width.
    let bounds = move |client: f32, preferred: f32, folded: bool| {
        if folded {
            (RAIL_W, RAIL_W, RAIL_W)
        } else {
            let max = sidebar_limit(client);
            (
                preferred.min(max),
                nanatype_settings::INPUT_SIDEBAR_MIN,
                max,
            )
        }
    };
    // Only folding resizes the pane: switching panels must not reset a drag.
    let folded = nana_ui::runtime::view::computed(move || ui.input_dock.with(|d| d.collapsed));
    let (size, min, max) = bounds(
        ui.input_width.get_untracked(),
        ui.input_sidebar_width.get_untracked(),
        ui.input_dock.with_untracked(|d| d.collapsed),
    );
    widget(SplitPane::new(&model(size, min, max)))
        .key("input-workspace-split")
        .node_ref(ui.input_split)
        .bind(move |pane: &mut SplitPane| {
            let (size, min, max) = bounds(
                ui.input_width.get(),
                ui.input_sidebar_width.get(),
                folded.get(),
            );
            if pane.model.limits() != (min, max) {
                pane.model = model(size, min, max);
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
        // A slot drops its child's key; the wrapper keeps the sidebar's.
        .second(widget(Stack::fill_column(0.0)).children(sidebar(ui)))
        .into_any()
}

/// The panels, then the rail at the window's edge.
fn sidebar(ui: Ui) -> AnyView {
    widget(
        Stack::row(0.0)
            .surface(SemanticColorRole::Subtle)
            .with_layout(|l| {
                l.width = Some(LengthSpec::Fill);
                l.height = Some(LengthSpec::Fill);
                l.min_width = Some(LengthSpec::Px(0.0));
                l.min_height = Some(LengthSpec::Px(0.0));
            }),
    )
    .key("input-insertion-dock")
    .children((
        widget(Stack::fill_column(0.0).with_layout(|l| {
            l.width = Some(LengthSpec::Px(0.0));
            l.flex_grow = Some(1.0);
            l.min_width = Some(LengthSpec::Px(0.0));
            l.min_height = Some(LengthSpec::Px(0.0));
        }))
        .key("input-dock-panels")
        .visible(move || ui.input_dock.with(|d| !d.collapsed))
        .children(dynamic(
            move || (ui.locale.get(), ui.input_dock.with(|d| d.chat_pinned)),
            move |(_, pinned)| {
                if *pinned {
                    chat_split(ui)
                } else {
                    panels(ui, true)
                }
            },
        )),
        widget(Divider::vertical()),
        widget(Stack::column(0.0).with_layout(|l| {
            l.width = Some(LengthSpec::Px(RAIL_W));
            l.min_width = Some(LengthSpec::Px(RAIL_W));
            l.height = Some(LengthSpec::Fill);
            l.flex_shrink = Some(0.0);
        }))
        .children(dynamic(move || ui.locale.get(), move |_| rail(ui))),
    ))
    .into_any()
}

/// Pinned chat above the insert panels, with a handle between them.
fn chat_split(ui: Ui) -> AnyView {
    use nana_ui::runtime::SplitPane;
    use nana_ui_core::{SplitAxis, SplitPaneModel, SplitPaneMutation};
    let area = signal(0.0_f32);
    let model = |size, max| SplitPaneModel::new(SplitAxis::Vertical, size, INPUT_CHAT_MIN, max);
    let max = move |h: f32| (h - PANELS_MIN_H).max(INPUT_CHAT_MIN);
    let preferred = nana_ui::runtime::view::computed(move || ui.input_dock.with(|d| d.chat_height));
    let start = ui.input_dock.with_untracked(|d| d.chat_height);
    widget(SplitPane::new(&model(start, f32::MAX)))
        .key("input-chat-split")
        .node_ref(ui.input_chat_split)
        .on(move |e: &SizeChanged| {
            if area.get_untracked() != e.height {
                area.set(e.height);
            }
        })
        .bind(move |pane: &mut SplitPane| {
            let h = area.get();
            // Unmeasured, the pane keeps the preference unclamped.
            let max = if h > 0.0 { max(h) } else { f32::MAX };
            let size = preferred.get().min(max);
            if pane.model.limits().1 != max {
                pane.model = model(size, max);
            } else {
                pane.apply(SplitPaneMutation::SetSize(size));
            }
            let l = Arc::make_mut(&mut pane.style.layout);
            l.width = Some(LengthSpec::Fill);
            l.flex_grow = Some(1.0);
            l.flex_shrink = Some(1.0);
            l.min_height = Some(LengthSpec::Px(0.0));
        })
        .first(widget(Stack::fill_column(0.0)).children(super::super::danmaku::chat_panel(ui)))
        .second(panels(ui, false))
        .into_any()
}

/// Every panel mounted, the open one shown. Chat is one of them unless it is
/// pinned above.
fn panels(ui: Ui, with_chat: bool) -> AnyView {
    let panel = move |tab: Tab, body: AnyView| {
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
        .key(format!("input-dock-{}-panel", tab_key(tab)))
        .visible(move || ui.input_dock.with(|d| d.tab == tab))
        .children(body)
        .into_any()
    };
    widget(Stack::fill_column(0.0).with_layout(|l| {
        l.min_height = Some(LengthSpec::Px(0.0));
        l.min_width = Some(LengthSpec::Px(0.0));
    }))
    .children((
        panel(Tab::Controls, controls::panel(ui)),
        panel(Tab::Stickers, stickers::panel(ui)),
        panel(Tab::Phrases, phrases(ui)),
        with_chat.then(|| {
            widget(Stack::fill_column(0.0).with_layout(|l| {
                l.height = Some(LengthSpec::Px(0.0));
                l.flex_grow = Some(1.0);
                l.min_height = Some(LengthSpec::Px(0.0));
                l.min_width = Some(LengthSpec::Px(0.0));
            }))
            .key("input-dock-chat-panel")
            .visible(move || ui.input_dock.with(|d| d.tab == Tab::Chat))
            .children(super::super::danmaku::chat_panel(ui))
        }),
    ))
    .into_any()
}

fn tab_key(tab: Tab) -> &'static str {
    match tab {
        Tab::Controls | Tab::Unknown => "controls",
        Tab::Stickers => "stickers",
        Tab::Phrases => "phrases",
        Tab::Chat => "chat",
    }
}

fn tab_meta(tab: Tab) -> (&'static str, nana_ui::runtime::Icon) {
    match tab {
        Tab::Controls | Tab::Unknown => ("dock-tab-controls", ti::ADJUSTMENTS_HORIZONTAL),
        Tab::Stickers => ("dock-stickers", ti::STICKER),
        Tab::Phrases => ("tb-phrases", ti::QUOTE),
        Tab::Chat => ("dock-tab-chat", ti::MESSAGES),
    }
}

/// Show `layout` at once; the message has the app keep it.
fn dock_to(ui: Ui, layout: InputDockLayout) -> Msg {
    let layout = layout.normalized();
    if ui.input_dock.get_untracked() != layout {
        ui.input_dock.set(layout);
    }
    Msg::Dock(layout)
}

/// Folder drops on the stickers icon open the panel and import the packs.
fn register_sticker_drop(
    ui: Ui,
    cx: &mut nana_ui::runtime::AppContext,
    target: nana_ui::runtime::Entity<IconButton>,
) {
    let _ = cx.set_drop_target(target, nana_ui_core::DropAccepts::files());
    let _ = cx.on(
        target,
        move |_, event: &nana_ui::runtime::FileDropEvent, cx| {
            let open = InputDockLayout {
                tab: Tab::Stickers,
                collapsed: false,
                ..ui.input_dock.get_untracked()
            };
            match event {
                nana_ui::runtime::FileDropEvent::Hovered { .. } => {
                    cx.dispatch_program_all(dock_to(ui, open));
                }
                nana_ui::runtime::FileDropEvent::Dropped { paths, .. } => {
                    cx.dispatch_program_all(dock_to(ui, open));
                    cx.dispatch_program_all(Msg::StickerImportFolders(paths.to_vec()));
                }
                nana_ui::runtime::FileDropEvent::Left => {}
            }
        },
    );
}

/// One rail icon: pressed while its panel shows; pressing the open one folds
/// the sidebar.
fn rail_button(ui: Ui, tab: Tab) -> AnyView {
    let (key, icon) = tab_meta(tab);
    let label = ui.now_t(key);
    let target = entity_ref::<IconButton>();
    if tab == Tab::Stickers {
        nana_ui::runtime::view::on_mount(move |cx| {
            if let Some(target) = target.get() {
                register_sticker_drop(ui, cx, target);
            }
        });
    }
    widget(tool_button(icon, label.clone(), label))
        .entity_ref(target)
        .key(format!("input-dock-rail-{}", tab_key(tab)))
        .bind(move |b: &mut IconButton| {
            let on = ui.input_dock.with(|d| d.showing(tab));
            if b.selected != on {
                b.selected = on;
            }
        })
        .on_cx(move |_, _: &Activate, cx| {
            let next = ui.input_dock.get_untracked().rail(tab);
            cx.dispatch_program_all(dock_to(ui, next));
        })
        .into_any()
}

/// The panel icons, chat set apart below them, and the fold toggle last.
fn rail(ui: Ui) -> AnyView {
    widget(
        Stack::column(0.0)
            .gap_step(gap::step::TIGHT)
            .align(AlignSpec::Center)
            .padding_step(gap::step::TIGHT, inset::step::BAR_Y)
            .with_layout(|l| {
                l.width = Some(LengthSpec::Px(RAIL_W));
                l.min_width = Some(LengthSpec::Px(RAIL_W));
                l.height = Some(LengthSpec::Fill);
                l.flex_shrink = Some(0.0);
            }),
    )
    .key("input-dock-rail")
    .roving_focus(nana_ui::runtime::RovingFocusGroup::vertical())
    .children((
        Tab::INSERTS
            .into_iter()
            .map(|tab| rail_button(ui, tab))
            .collect::<Vec<_>>(),
        widget(Stack::row(0.0).width(LengthSpec::Px(TARGET - 8.0)))
            .children(widget(Divider::horizontal())),
        rail_button(ui, Tab::Chat),
        widget(Stack::spacer()),
        dynamic(
            move || (ui.locale.get(), ui.input_dock.with(|d| d.collapsed)),
            move |(_, folded)| {
                let (icon, key) = if *folded {
                    (ti::LAYOUT_SIDEBAR_RIGHT_EXPAND, "dock-expand")
                } else {
                    (ti::LAYOUT_SIDEBAR_RIGHT_COLLAPSE, "dock-collapse")
                };
                widget(tool_button(icon, ui.now_t(key), ui.now_t(key)))
                    .key("input-dock-fold")
                    .on_cx(move |_, _: &Activate, cx| {
                        let d = ui.input_dock.get_untracked();
                        cx.dispatch_program_all(dock_to(
                            ui,
                            InputDockLayout {
                                collapsed: !d.collapsed,
                                ..d
                            },
                        ));
                    })
                    .into_any()
            },
        ),
    ))
    .into_any()
}

/// A phrase as a card: its name, and the start of what it puts in.
fn phrase_card(index: usize, phrase: &nanatype_settings::QuickPhrase) -> AnyView {
    let mut st = NodeStyle::default();
    {
        let l = Arc::make_mut(&mut st.layout);
        l.width = Some(LengthSpec::Fill);
        l.border_width = Some(1.0);
        l.border_style = Some(nana_ui_core::BorderStyle::Solid);
        l.padding_left = Some(LengthSpec::Px(inset::CHIP_X * 2.0));
        l.padding_right = Some(LengthSpec::Px(inset::CHIP_X * 2.0));
        l.padding_top = Some(LengthSpec::Px(gap::TIGHT));
        l.padding_bottom = Some(LengthSpec::Px(gap::TIGHT));
    }
    st.radius = Some(nana_ui_core::RadiusTier::Sm);
    st.border = Some(SemanticColorRole::Border);
    st.background = Some(SemanticColorRole::Surface);
    st.interaction.hovered.border = Some(SemanticColorRole::BorderStrong);
    let preview = phrase.markup.replace(['\r', '\n'], " ");
    widget(
        Button::new("")
            .accessible_name(phrase.label.clone())
            .kind(ButtonKind::Text)
            .style(st),
    )
    .key(format!("input-dock-phrase-{index}"))
    .children(
        widget(
            Stack::column(0.0)
                .gap_step(gap::step::HAIR)
                .width(LengthSpec::Fill)
                .with_layout(|l| l.min_width = Some(LengthSpec::Px(0.0))),
        )
        .children((
            widget(
                Text::new(phrase.label.clone())
                    .decorative()
                    .style(crate::ui::kit::Type::Strong.style())
                    .truncating(),
            ),
            widget(
                Text::new(preview)
                    .decorative()
                    .style(crate::ui::kit::Type::Micro.style())
                    .color(SemanticColorRole::Muted)
                    .truncating(),
            ),
        )),
    )
    .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(Msg::InsertPhrase(index)))
    .into_any()
}

/// Saved phrases, and the way to Settings to add or edit them.
fn phrases(ui: Ui) -> AnyView {
    let manage = move || {
        widget(
            Button::new(ui.now_t("dock-phrases-manage"))
                .kind(ButtonKind::Text)
                .size(ControlSize::Small),
        )
        .on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program_all(Msg::SettingsPage(super::super::settings::Page::Assets))
        })
    };
    let list = dynamic(
        move || {
            (
                ui.locale.get(),
                ui.settings.with(|s| s.assets.phrases.clone()),
            )
        },
        move |(_, phrases)| {
            if phrases.is_empty() {
                return widget(
                    Stack::column(0.0)
                        .gap_step(gap::step::CONTROL)
                        .width(LengthSpec::Fill),
                )
                .children(widget(
                    Text::new(ui.now_t("empty-phrases"))
                        .style(crate::ui::kit::Type::Body.style())
                        .color(SemanticColorRole::Muted),
                ))
                .into_any();
            }
            widget(
                Stack::column(0.0)
                    .gap_step(gap::step::TIGHT)
                    .width(LengthSpec::Fill),
            )
            .children(
                phrases
                    .iter()
                    .enumerate()
                    .map(|(i, p)| phrase_card(i, p))
                    .collect::<Vec<_>>(),
            )
            .into_any()
        },
    );
    widget(
        Stack::fill_column(0.0)
            .gap_step(gap::step::CONTROL)
            .with_layout(|l| {
                l.min_height = Some(LengthSpec::Px(0.0));
                l.min_width = Some(LengthSpec::Px(0.0));
            }),
    )
    .children((
        widget(
            Stack::row(0.0)
                .align(AlignSpec::Center)
                .width(LengthSpec::Fill),
        )
        .children((
            caption(ui.now_t("tb-phrases")),
            widget(Stack::spacer()),
            manage(),
        )),
        widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|l| {
            l.width = Some(LengthSpec::Fill);
            l.height = Some(LengthSpec::Px(0.0));
            l.min_height = Some(LengthSpec::Px(0.0));
            l.flex_grow = Some(1.0);
            l.flex_shrink = Some(1.0);
        }))
        .key("input-dock-phrases-scroll")
        .children(list),
    ))
    .into_any()
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

pub(super) fn footer(ui: Ui, invitation: Signal<String>, password: Signal<String>) -> AnyView {
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
                super::super::sources::input_send_status(ui),
                popover(
                    ti::USERS,
                    ui.now_t("input-management"),
                    crate::ui::kit::popover::LG,
                    ui.peer_popover,
                    widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|l| {
                        l.height = Some(LengthSpec::Px(MANAGEMENT_H));
                        l.min_height = Some(LengthSpec::Px(0.0));
                    }))
                    .children(super::super::p2p::card_with_fields(
                        ui, invitation, password, true,
                    )),
                ),
                pressed_tool(
                    ti::MICROPHONE,
                    ui.now_t("settings-speech"),
                    move || ui.speech.with(|s| s.phase.active()),
                    || Msg::SpeechToggle,
                ),
                popover(
                    ti::CHEVRON_DOWN,
                    ui.now_t("settings-speech"),
                    crate::ui::kit::popover::LG,
                    signal(false),
                    widget(Stack::column(0.0).gap_step(gap::step::CONTROL)).children((
                        crate::ui::speech::mode(ui),
                        crate::ui::speech::actions(ui),
                        crate::ui::settings::controls::button(ui, "speech-settings", || {
                            Msg::SettingsPage(crate::ui::settings::Page::Speech)
                        }),
                    )),
                ),
                tool(ui, ti::SETTINGS, "window-settings", || Msg::OpenSettings),
                super::super::sources::account_button(ui),
                widget(
                    Button::new(ui.now_t("tb-send"))
                        .icon(ti::SEND)
                        .kind(ButtonKind::Primary),
                )
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
