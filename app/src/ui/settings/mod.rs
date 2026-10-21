//! The settings window: a sidebar of pages and a scrolling page body.
//! Every control reads the settings signal and writes back through
//! [`Msg::Edit`], so the program stays the only owner of state.

pub(crate) mod controls;
mod pages;
pub mod preview;
mod telegram;

pub use controls::{base64, links_pulsing};
pub use pages::font_licence_panel;

use std::sync::Arc;

use nana_ui::icons_tabler as ti;
use nana_ui::runtime::view::{AnyView, IntoView, dynamic, widget};
use nana_ui::runtime::{Activate, ListItem, ScrollAxes, ScrollView, Stack, Text};
use nana_ui_core::{LengthSpec, SemanticColorRole};

use super::Ui;
use crate::msg::Msg;
use crate::ui::kit::{PAGE_HEADER_H, PAGE_MAX_W, SIDEBAR_W, Type, gap, icon, inset};

/// The panel and settings page use the same persisted style control.
pub(super) fn chat_panel_style(ui: Ui) -> AnyView {
    use controls::{segmented, select_keys};
    use nanatype_settings::ChatPanelStyle;
    segmented(
        ui,
        select_keys(
            ui,
            &[
                ("compact", "chat-style-compact"),
                ("history", "chat-style-history"),
            ],
        ),
        |s| match s.integrations.bilibili.panel_style {
            ChatPanelStyle::Compact => "compact".into(),
            ChatPanelStyle::History => "history".into(),
        },
        |s, v| {
            s.integrations.bilibili.panel_style = match v {
                "history" => ChatPanelStyle::History,
                _ => ChatPanelStyle::Compact,
            }
        },
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Page {
    General,
    Playback,
    Skins,
    Packages,
    Windows,
    History,
    Hotkeys,
    Output,
    Platforms,
    Integrations,
    Assets,
    About,
}

impl Page {
    pub const ALL: [Self; 12] = [
        Self::General,
        Self::Playback,
        Self::Skins,
        Self::Packages,
        Self::Windows,
        Self::History,
        Self::Hotkeys,
        Self::Output,
        Self::Platforms,
        Self::Integrations,
        Self::Assets,
        Self::About,
    ];

    pub fn label_key(self) -> &'static str {
        match self {
            Self::General => "settings-general",
            Self::Playback => "settings-playback",
            Self::Skins => "settings-skins",
            Self::Packages => "settings-packages",
            Self::Windows => "settings-windows",
            Self::History => "settings-history",
            Self::Hotkeys => "settings-hotkeys",
            Self::Output => "settings-output",
            Self::Platforms => "settings-platforms",
            Self::Integrations => "settings-integrations",
            Self::Assets => "settings-assets",
            Self::About => "settings-about",
        }
    }

    fn icon(self) -> nana_ui::runtime::Icon {
        match self {
            Self::General => ti::ADJUSTMENTS,
            Self::Playback => ti::PLAYER_PLAY,
            Self::Skins => ti::PALETTE,
            Self::Packages => ti::PLUG_CONNECTED,
            Self::Windows => ti::APP_WINDOW,
            Self::History => ti::MESSAGES,
            Self::Hotkeys => ti::KEYBOARD,
            Self::Output => ti::BROADCAST,
            Self::Platforms => ti::LIVE_VIEW,
            Self::Integrations => ti::LINK,
            Self::Assets => ti::STICKER,
            Self::About => ti::INFO_CIRCLE,
        }
    }
}

/// The sidebar's groups: a caption key and its pages. About sits apart, at
/// the foot.
const GROUPS: &[(&str, &[Page])] = &[
    (
        "settings-group-input",
        &[Page::General, Page::Playback, Page::Hotkeys],
    ),
    (
        "settings-group-look",
        &[Page::Skins, Page::Packages, Page::Windows, Page::History],
    ),
    (
        "settings-group-stream",
        &[
            Page::Platforms,
            Page::Output,
            Page::Integrations,
            Page::Assets,
        ],
    ),
];

/// One page's row: an icon (muted, accent when selected) and its name, on
/// the selected fill when it is the open page.
fn nav_row(ui: Ui, page: Page) -> AnyView {
    let selected = move || ui.page.get() == page;
    widget(ListItem::new("").size(nana_ui::ControlSize::Small))
        .bind(move |item: &mut ListItem| {
            let label = super::I18N.t_in(ui.locale.get(), page.label_key());
            if item.label != label {
                item.label = label;
            }
            item.selected = selected();
        })
        .leading(
            widget(nana_ui::runtime::IconGlyph::new(page.icon()).size(icon::MD)).bind(
                move |g: &mut nana_ui::runtime::IconGlyph| {
                    g.style.foreground = Some(if selected() {
                        SemanticColorRole::Accent
                    } else {
                        SemanticColorRole::Muted
                    });
                },
            ),
        )
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(Msg::SettingsPage(page)))
        .into_any()
}

/// A group's caption.
fn nav_caption(ui: Ui, key: &'static str, first: bool) -> AnyView {
    let mut st = Type::Overline.style();
    {
        let l = Arc::make_mut(&mut st.layout);
        l.margin_top = Some(LengthSpec::Px(if first { 0.0 } else { gap::SECTION }));
        l.margin_bottom = Some(LengthSpec::Px(gap::TIGHT));
        l.padding_left = Some(LengthSpec::Px(inset::CHIP_X));
    }
    widget(
        Text::new(super::I18N.t_in(ui.locale.get_untracked(), key))
            .style(st)
            .color(SemanticColorRole::Muted),
    )
    .bind(move |t: &mut Text| {
        let text = super::I18N.t_in(ui.locale.get(), key);
        if t.value != text {
            t.value = text;
        }
    })
    .into_any()
}

/// The app's logo at `size` px.
pub(crate) fn logo(size: f32) -> AnyView {
    widget(
        Stack::row(0.0)
            .width(LengthSpec::Px(size))
            .height(LengthSpec::Px(size))
            .with_layout(|l| {
                l.flex_shrink = Some(0.0);
                l.paint.background_image = Some(nana_ui_core::BackgroundImage::url_with_fit(
                    format!("data:image/png;base64,{}", controls::ICON_PNG_B64.as_str()),
                    nana_ui_core::BackgroundImageFit::Contain,
                ));
            }),
    )
    .into_any()
}

fn sidebar(ui: Ui) -> AnyView {
    let mut items: Vec<AnyView> = Vec::new();
    for (i, (caption, pages)) in GROUPS.iter().enumerate() {
        items.push(nav_caption(ui, caption, i == 0));
        items.extend(pages.iter().map(|page| nav_row(ui, *page)));
    }
    widget(
        Stack::row(0.0)
            .width(LengthSpec::Px(SIDEBAR_W))
            .surface(SemanticColorRole::Subtle)
            .align(nana_ui_core::AlignSpec::Stretch)
            .with_layout(|l| {
                l.height = Some(LengthSpec::Fill);
                l.flex_shrink = Some(0.0);
            }),
    )
    .children((
        widget(
            Stack::fill_column(0.0)
                .gap_step(gap::step::TIGHT)
                .padding_step(inset::step::BAR_X, inset::step::BAR_Y)
                .with_layout(|l| l.min_width = Some(LengthSpec::Px(0.0))),
        )
        .children((
            widget(
                Stack::row(0.0)
                    .gap_step(gap::step::CONTROL)
                    .height(LengthSpec::Px(PAGE_HEADER_H - inset::BAR_Y))
                    .padding_xy(inset::CHIP_X, 0.0)
                    .align(nana_ui_core::AlignSpec::Center)
                    .with_layout(|l| l.flex_shrink = Some(0.0)),
            )
            .children((logo(icon::LG + gap::TIGHT), Type::Brand.view("NanaType"))),
            widget(scroll_view())
                .children(widget(Stack::column(0.0).gap_step(gap::step::HAIR)).children(items)),
            // Keep About reachable while the navigation above scrolls on short screens.
            widget(Stack::column(0.0).with_layout(|l| l.flex_shrink = Some(0.0)))
                .children(nav_row(ui, Page::About)),
        )),
        // The hairline between the sidebar and the page.
        widget(
            Stack::column(0.0)
                .width(LengthSpec::Px(crate::ui::kit::HAIRLINE))
                .surface(SemanticColorRole::BorderSoft)
                .with_layout(|l| {
                    l.height = Some(LengthSpec::Fill);
                    l.flex_shrink = Some(0.0);
                }),
        ),
    ))
    .into_any()
}

/// A page's fixed header: its title, its page-wide actions on the right, a
/// hairline under it.
fn page_header(ui: Ui, page: Page) -> AnyView {
    widget(Stack::column(0.0).with_layout(|l| l.flex_shrink = Some(0.0)))
        .children((
            widget(
                Stack::row(0.0)
                    .gap_step(gap::step::GROUP)
                    .width(LengthSpec::Fill)
                    .padding_step(inset::step::PAGE, inset::step::CARD)
                    .align(nana_ui_core::AlignSpec::Center)
                    .with_layout(|l| l.min_height = Some(LengthSpec::Px(PAGE_HEADER_H))),
            )
            .children((
                widget(crate::ui::kit::grow_column(gap::HAIR))
                    .children(controls::heading(ui, page.label_key())),
                pages::actions(ui, page),
            )),
            crate::ui::kit::rule(),
        ))
        .into_any()
}

fn scroll_view() -> ScrollView {
    // Size the viewport from the window, rather than its overflowing content.
    ScrollView::new(ScrollAxes::Vertical).with_layout(|l| {
        l.flex_grow = Some(1.0);
        l.flex_shrink = Some(1.0);
        l.height = Some(LengthSpec::Px(0.0));
        l.min_height = Some(LengthSpec::Px(0.0));
    })
}

pub fn settings_window(ui: Ui) -> impl IntoView {
    let body = widget(
        Stack::row(0.0)
            .width(LengthSpec::Fill)
            .height(LengthSpec::Fill)
            .surface(SemanticColorRole::Background)
            .align(nana_ui_core::AlignSpec::Stretch),
    )
    .children((
        sidebar(ui),
        widget(Stack::fill_column(0.0)).children(
            dynamic(
                // A newly opened page starts at its top, regardless of how far
                // the previous page was scrolled.
                move || (ui.page.get(), ui.locale.get()),
                move |(page, _)| {
                    widget(Stack::fill_column(0.0))
                        .children((
                            page_header(ui, *page),
                            widget(scroll_view()).children(
                                widget(
                                    Stack::column(0.0)
                                        .width(LengthSpec::Fill)
                                        .align(nana_ui_core::AlignSpec::Center)
                                        .padding_step(inset::step::PAGE, inset::step::PAGE),
                                )
                                .children(
                                    widget(
                                        Stack::column(0.0)
                                            .max_width(PAGE_MAX_W)
                                            .width(LengthSpec::Fill),
                                    )
                                    .children(pages::page(ui, *page)),
                                ),
                            ),
                        ))
                        .into_any()
                },
            )
            // The switch's own container must fill too, or the scroll view collapses.
            .grow(),
        ),
    ));
    widget(
        Stack::column(0.0)
            .width(LengthSpec::Fill)
            .height(LengthSpec::Fill)
            .with_layout(|l| l.position = nana_ui_core::PositionSpec::Relative),
    )
    .children((
        body,
        widget(nana_ui::runtime::OverlayHost::new())
            .css(nana_ui::runtime::view::css! {position: absolute; inset: 0;})
            .children((
                super::bilibili::confirm_dialog(ui),
                super::twitch::confirm_dialog(ui),
            )),
    ))
}
