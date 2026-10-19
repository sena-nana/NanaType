//! The settings window: a sidebar of pages and a scrolling page body.
//! Every control reads the settings signal and writes back through
//! [`Msg::Edit`], so the program stays the only owner of state.

pub(super) mod controls;
mod pages;
pub mod preview;
mod telegram;

pub use controls::{base64, links_pulsing};
pub use pages::font_licence_panel;

use std::sync::Arc;

use nana_ui::icons_tabler as ti;
use nana_ui::runtime::view::{AnyView, IntoView, dynamic, entity_ref, on_mount, widget};
use nana_ui::runtime::{Activate, ListItem, NodeStyle, ScrollAxes, ScrollView, Stack, Text};
use nana_ui::theme::{space, type_scale};
use nana_ui_core::{LengthSpec, SemanticColorRole};

use super::Ui;
use crate::msg::Msg;

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
    Integrations,
    Bilibili,
    Douyin,
    Assets,
    About,
}

impl Page {
    pub const ALL: [Self; 13] = [
        Self::General,
        Self::Playback,
        Self::Skins,
        Self::Packages,
        Self::Windows,
        Self::History,
        Self::Hotkeys,
        Self::Output,
        Self::Integrations,
        Self::Bilibili,
        Self::Douyin,
        Self::Assets,
        Self::About,
    ];

    pub fn label_key(self) -> &'static str {
        match self {
            Self::Bilibili => "settings-bilibili",
            Self::Douyin => "settings-douyin",
            Self::General => "settings-general",
            Self::Playback => "settings-playback",
            Self::Skins => "settings-skins",
            Self::Packages => "settings-packages",
            Self::Windows => "settings-windows",
            Self::History => "settings-history",
            Self::Hotkeys => "settings-hotkeys",
            Self::Output => "settings-output",
            Self::Integrations => "settings-integrations",
            Self::Assets => "settings-assets",
            Self::About => "settings-about",
        }
    }

    /// The one-line description under the page title.
    pub fn subtitle_key(self) -> &'static str {
        match self {
            Self::Bilibili => "settings-bilibili-sub",
            Self::Douyin => "settings-douyin-sub",
            Self::General => "settings-general-sub",
            Self::Playback => "settings-playback-sub",
            Self::Skins => "settings-skins-sub",
            Self::Packages => "settings-packages-sub",
            Self::Windows => "settings-windows-sub",
            Self::History => "settings-history-sub",
            Self::Hotkeys => "settings-hotkeys-sub",
            Self::Output => "settings-output-sub",
            Self::Integrations => "settings-integrations-sub",
            Self::Assets => "settings-assets-sub",
            Self::About => "settings-about-sub",
        }
    }

    fn icon(self) -> nana_ui::runtime::Icon {
        match self {
            Self::Bilibili => ti::BRAND_BILIBILI,
            Self::Douyin => ti::MESSAGE_CIRCLE,
            Self::General => ti::ADJUSTMENTS,
            Self::Playback => ti::PLAYER_PLAY,
            Self::Skins => ti::PALETTE,
            Self::Packages => ti::PLUG_CONNECTED,
            Self::Windows => ti::APP_WINDOW,
            Self::History => ti::MESSAGES,
            Self::Hotkeys => ti::KEYBOARD,
            Self::Output => ti::BROADCAST,
            Self::Integrations => ti::PLUG_CONNECTED,
            Self::Assets => ti::STICKER,
            Self::About => ti::INFO_CIRCLE,
        }
    }
}

/// The sidebar's groups: a caption key and its pages. About sits apart, at
/// the foot.
const GROUPS: [(&str, [Page; 3]); 3] = [
    (
        "settings-group-input",
        [Page::General, Page::Playback, Page::Hotkeys],
    ),
    (
        "settings-group-look",
        [Page::Skins, Page::Windows, Page::History],
    ),
    (
        "settings-group-stream",
        [Page::Output, Page::Integrations, Page::Assets],
    ),
];

/// One page's row: a 16px icon (muted, text-coloured when selected) and
/// its name.
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
            widget(nana_ui::runtime::IconGlyph::new(page.icon()).size(16.0)).bind(
                move |g: &mut nana_ui::runtime::IconGlyph| {
                    g.style.foreground = Some(if selected() {
                        SemanticColorRole::Text
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
    let mut st = controls::text_style(type_scale::HINT, type_scale::SEMIBOLD);
    {
        let l = Arc::make_mut(&mut st.layout);
        l.margin_top = Some(LengthSpec::Px(if first { space::SM } else { space::XXL }));
        l.margin_bottom = Some(LengthSpec::Px(space::XS));
        l.padding_left = Some(LengthSpec::Px(space::LG));
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

fn sidebar(ui: Ui) -> AnyView {
    let mut items: Vec<AnyView> = Vec::new();
    for (i, (caption, pages)) in GROUPS.into_iter().enumerate() {
        items.push(nav_caption(ui, caption, i == 0));
        items.extend(pages.into_iter().map(|page| nav_row(ui, page)));
        if i == 1 {
            items.push(nav_row(ui, Page::Packages));
        }
        if i == 2 {
            items.push(nav_row(ui, Page::Bilibili));
            items.push(nav_row(ui, Page::Douyin));
        }
    }
    let mut title = NodeStyle::default();
    {
        let l = Arc::make_mut(&mut title.layout);
        l.font_size = Some(type_scale::TITLE);
        l.font_weight = Some(type_scale::BOLD);
    }
    widget(
        Stack::column(space::XXS)
            .width(LengthSpec::Px(196.0))
            .padding_xy(space::LG, space::XXL)
            .surface(SemanticColorRole::Subtle)
            .with_layout(|l| {
                l.height = Some(LengthSpec::Fill);
            }),
    )
    .children((
        widget(Stack::row(space::MD).padding_xy(space::MD, space::SM)).children((
            widget(
                Stack::row(0.0)
                    .width(LengthSpec::Px(28.0))
                    .height(LengthSpec::Px(28.0))
                    .with_layout(|l| {
                        l.paint.background_image =
                            Some(nana_ui_core::BackgroundImage::url_with_fit(
                                format!(
                                    "data:image/png;base64,{}",
                                    crate::ui::settings::controls::ICON_PNG_B64.as_str()
                                ),
                                nana_ui_core::BackgroundImageFit::Contain,
                            ));
                    }),
            ),
            widget(Text::new("NanaType").style(title)),
        )),
        widget(scroll_view()).children(widget(Stack::column(space::XXS)).children(items)),
        // Keep About reachable while the navigation above scrolls on short screens.
        widget(Stack::column(0.0).with_layout(|l| l.flex_shrink = Some(0.0)))
            .children(nav_row(ui, Page::About)),
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
    let body_scroll = entity_ref::<ScrollView>();
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
            widget(scroll_view()).entity_ref(body_scroll).children(
                widget(Stack::column(0.0).max_width(860.0).with_layout(|l| {
                    l.padding_top = Some(LengthSpec::Px(space::PAGE_TIGHT));
                    l.padding_right = Some(LengthSpec::Px(space::PAGE));
                    l.padding_bottom = Some(LengthSpec::Px(space::PAGE));
                    l.padding_left = Some(LengthSpec::Px(space::PAGE));
                }))
                .children(dynamic(
                    move || (ui.page.get(), ui.locale.get()),
                    move |(page, _)| {
                        // A newly opened page starts at its heading, regardless
                        // of how far the previous page was scrolled.
                        on_mount(move |cx| {
                            if let Some(scroll) = body_scroll.get() {
                                let _ = cx.scroll_to(scroll, Default::default());
                            }
                        });
                        pages::page(ui, *page)
                    },
                )),
            ),
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
            .children(super::bilibili::confirm_dialog(ui)),
    ))
}
