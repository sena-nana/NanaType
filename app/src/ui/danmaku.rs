//! The chat panel: live chat from Bilibili and Douyin for the streamer to read and
//! answer. The operator can choose compact rows or neutral history cards.
//! Both presentations share hover/focus actions and the same retained feed.
//!
//! Each message has two buttons: **@** puts a label with the viewer's avatar
//! and name at the editor's caret; **reply** only quotes the message above
//! the line that is sent next, preserving the draft and caret.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use nana_ui::ControlSize;
use nana_ui::icons_tabler as ti;
use nana_ui::runtime::view::{
    AnyView, IntoView, Signal, computed, dynamic, each_virtual, node_ref, signal, widget,
};
use nana_ui::runtime::{
    Activate, Button, FocusWithinChanged, IconButton, PointerHoverChanged, RichTextView,
    ScrollAxes, ScrollView, Stack, Text, UserScroll,
};
use nana_ui::theme::space;
use nana_ui_core::{
    AlignSpec, BackgroundImage, BackgroundImageFit, JustifySpec, LengthSpec, OverflowSpec,
    PointerEventsSpec, RadiusTier, SemanticColorRole,
};
use nanatype_settings::{ChatFilter, ChatPanelStyle, Settings};

use super::history;
use super::rich::{Env, Look, MentionLook, to_rich};
use super::{I18N, Ui};
use crate::chat::{ChatKind, ChatMessage, super_chat_color};
use crate::msg::Msg;
use crate::skin_theme::rgba8;
use crate::ui::kit::theme::palette;
use crate::ui::kit::{Tone, Type, gap, inset};

/// A message's meta line (name, badges, time): one small control high.
const META_LINE_H: f32 = 22.0;
/// A row's reply and mention buttons, by panel style.
const COMPACT_ACTION_H: f32 = 22.0;
const HISTORY_ACTION_H: f32 = 26.0;
/// A message's avatar.
const AVATAR: f32 = 24.0;
/// What the virtual list assumes a row measures before it does.
const ROW_ESTIMATE_H: f32 = 56.0;

/// Disjoint identities keep live chat and local lines from colliding in this feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(super) enum RowSource {
    Chat(u64),
    History(u64),
}
impl RowSource {
    pub(super) fn track(self, ui: Ui) {
        match self {
            Self::Chat(_) => {
                ui.chat.with(|_| ());
            }
            Self::History(_) => {
                ui.history.with(|_| ());
            }
        }
    }

    pub(super) fn get(self, ui: Ui) -> Option<nanatype_core::HistoryRow> {
        match self {
            Self::History(key) => {
                let at = ui.history_index.with(|index| index.get(&key).copied())?;
                ui.history.with_untracked(|rows| rows.get(at).cloned())
            }
            Self::Chat(seq) => ui.chat.with_untracked(|rows| {
                rows.iter().find(|m| m.seq == seq).map(|m| {
                    let item = m.history_item(ui.locale.get_untracked());
                    nanatype_core::HistoryRow {
                        key: seq,
                        doc: Arc::new(item.doc.clone()),
                        item,
                        reveal: None,
                    }
                })
            }),
        }
    }
}

/// Connection state, for the dot in the header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Link {
    #[default]
    Off,
    Connecting,
    Connected,
    Error,
}

impl Link {
    /// How the link's dot and badge are coloured.
    pub fn tone(self) -> Tone {
        match self {
            Self::Off => Tone::Off,
            Self::Connecting => Tone::Warning,
            Self::Connected => Tone::Success,
            Self::Error => Tone::Danger,
        }
    }
}

/// A History-style card's horizontal padding.
const HISTORY_PAD_X: f32 = space::LG;

/// A round avatar: the name's first letter on the accent colour, with the
/// picture (when there is one) laid over it, so a picture that is still
/// loading, or cannot be read, leaves the initial rather than an empty
/// circle. (NanaUI's `Avatar` samples a host texture and leaves an empty
/// slot a plain grey circle, without initials, so it cannot stand in.)
pub fn asset_avatar(
    ui: Ui,
    asset: Option<String>,
    name: &str,
    size: f32,
    accent: [f32; 4],
) -> AnyView {
    let mut st = Type::Brand.style();
    {
        let l = Arc::make_mut(&mut st.layout);
        // The initial scales with the avatar.
        l.font_size = Some(size * 0.45);
        l.color = Some([1.0; 4]);
    }
    let initial = name.chars().next().map(String::from).unwrap_or_default();
    widget(
        Stack::row(0.0)
            .width(LengthSpec::Px(size))
            .height(LengthSpec::Px(size))
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center)
            .radius(RadiusTier::Full)
            .with_layout(move |l| {
                l.position = nana_ui_core::PositionSpec::Relative;
                l.flex_shrink = Some(0.0);
                l.background = Some(accent);
            }),
    )
    .children((
        widget(Text::new(initial).decorative().style(st)),
        reactive_picture(ui, asset, size / 2.0),
    ))
    .into_any()
}

/// Updates only the image node; late downloads never remount a message.
pub fn reactive_picture(ui: Ui, asset: Option<String>, radius: f32) -> AnyView {
    widget(
        crate::ui::kit::corner(Stack::row(0.0), radius).with_layout(move |l| {
            l.position = nana_ui_core::PositionSpec::Absolute;
            l.offset_left = Some(LengthSpec::Px(0.0));
            l.offset_top = Some(LengthSpec::Px(0.0));
            l.offset_right = Some(LengthSpec::Px(0.0));
            l.offset_bottom = Some(LengthSpec::Px(0.0));
            l.pointer_events = Some(nana_ui_core::PointerEventsSpec::None);
        }),
    )
    .bind(move |s: &mut Stack| {
        let url = asset.as_ref().and_then(|a| {
            ui.asset_urls.with(|u| u.get(a).cloned()).or_else(|| {
                if a.starts_with("blob:") {
                    None
                } else if a.contains("://") || a.starts_with("data:") {
                    Some(a.clone())
                } else {
                    crate::services::media::url(std::path::Path::new(a), None)
                }
            })
        });
        let image = url.map(|u| BackgroundImage::url_with_fit(u, BackgroundImageFit::Cover));
        s.layout_mut().paint.background_image = image;
    })
    .into_any()
}

fn action(
    ui: Ui,
    icon: nana_ui::runtime::Icon,
    key: &str,
    msg: impl Fn() -> Msg + Send + 'static,
) -> AnyView {
    widget(
        IconButton::new(icon, ui.now_t(key))
            .size(ControlSize::Small)
            .with_tooltip(ui.now_t(key)),
    )
    .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(msg()))
    .into_any()
}

fn row_view(ui: Ui, source: RowSource, width: f32) -> AnyView {
    let hovered = signal(false);
    let focused = signal(false);
    let paid = match source {
        RowSource::Chat(seq) => ui.chat.with_untracked(|rows| {
            rows.iter()
                .find(|m| m.seq == seq)
                .and_then(|m| match m.kind {
                    ChatKind::SuperChat { price } => Some(super_chat_color(price)),
                    _ => None,
                })
        }),
        _ => None,
    };
    let content = dynamic(
        move || ui.settings.with(|s| s.integrations.bilibili.panel_style),
        move |style| {
            let actions = if let RowSource::Chat(seq) = source {
                Some(row_actions(ui, seq, *style, hovered, focused))
            } else {
                None
            };
            match style {
                ChatPanelStyle::Compact => compact_message(ui, source, width, actions),
                ChatPanelStyle::History => widget(
                    Stack::column(0.0)
                        .surface(SemanticColorRole::Surface)
                        .padding_xy(HISTORY_PAD_X, inset::BAR_Y)
                        .radius(nana_ui_core::RadiusTier::Md)
                        .width(LengthSpec::Fill),
                )
                .children(compact_message(
                    ui,
                    source,
                    width - 2.0 * HISTORY_PAD_X,
                    actions,
                ))
                .into_any(),
            }
        },
    );
    widget(
        Stack::column(0.0)
            .hittable()
            .width(LengthSpec::Fill)
            .padding_step(nana_ui_core::SpacingStep::Sm, gap::step::TIGHT)
            .radius(nana_ui_core::RadiusTier::Sm)
            .with_layout(|l| {
                l.border_width = Some(1.0);
                l.border_style = Some(nana_ui_core::BorderStyle::Solid);
            }),
    )
    .key(match source {
        RowSource::Chat(seq) => format!("chat-row:{seq}"),
        RowSource::History(id) => format!("chat-row:h{id}"),
    })
    .on(move |e: &PointerHoverChanged| hovered.set(e.hovered))
    .on(move |e: &FocusWithinChanged| focused.set(e.focused))
    .bind(move |s: &mut Stack| {
        let selected = matches!(source, RowSource::Chat(seq) if ui.chat_target.get() == Some(seq));
        let active = selected || hovered.get() || focused.get();
        let mut accent = palette(ui.chrome_dark.get()).accent.as_rgba_array();
        let border = if active { accent } else { [0.0; 4] };
        accent[3] = if selected {
            crate::ui::kit::wash::SELECTED
        } else if active {
            crate::ui::kit::wash::HOVER
        } else {
            0.0
        };
        let l = s.layout_mut();
        l.background = Some(if !active {
            paid.map_or(accent, |mut p| {
                p[3] = 0.18;
                p
            })
        } else {
            accent
        });
        l.border_color = Some(border);
    })
    .children(content)
    .into_any()
}

/// Stable action slots: opacity hides paint without removing keyboard targets.
/// Pointer hit testing is disabled until the row is hovered or focus enters it.
pub(super) fn actions_width(ui: Ui) -> f32 {
    action_widths(ui).into_iter().sum::<f32>() + 4.0
}

fn action_widths(ui: Ui) -> [f32; 2] {
    match ui.locale.get_untracked() {
        nanatype_i18n::Locale::ZhCn => [54.0, 54.0],
        nanatype_i18n::Locale::EnUs => [64.0, 80.0],
        nanatype_i18n::Locale::JaJp => [54.0, 90.0],
    }
}

fn row_actions(
    ui: Ui,
    seq: u64,
    style: ChatPanelStyle,
    hovered: Signal<bool>,
    focused: Signal<bool>,
) -> AnyView {
    let height = if style == ChatPanelStyle::Compact {
        COMPACT_ACTION_H
    } else {
        HISTORY_ACTION_H
    };
    let width = actions_width(ui);
    let button = move |reply: bool| {
        let mut b = Button::new(ui.now_t(if reply { "chat-reply" } else { "chat-at-short" }))
            .accessible_name(ui.now_t(if reply {
                "chat-reply"
            } else {
                "chat-at-action"
            }))
            .icon(if reply { ti::ARROW_BACK_UP } else { ti::AT })
            .icon_size(crate::ui::kit::icon::SM)
            .icon_gap(gap::HAIR);
        let layout = Arc::make_mut(&mut b.style.layout);
        layout.width = Some(LengthSpec::Px(action_widths(ui)[usize::from(!reply)]));
        layout.height = Some(LengthSpec::Px(height));
        layout.min_height = Some(LengthSpec::Px(height));
        layout.font_size = Some(Type::Micro.size());
        layout.line_height = Some(nana_ui_core::LineHeightSpec::Absolute(14.0));
        layout.padding_left = Some(LengthSpec::Px(nana_ui::theme::space::SM));
        layout.padding_right = Some(LengthSpec::Px(nana_ui::theme::space::SM));
        layout.flex_shrink = Some(0.0);
        let layout = b.style.layout.clone();
        b = b.layout(layout);
        let (foreground, background, hover, pressed) = if reply {
            (
                SemanticColorRole::AccentOnSoft,
                SemanticColorRole::AccentSoft,
                SemanticColorRole::AccentSoftHover,
                SemanticColorRole::AccentSoftPressed,
            )
        } else {
            (
                SemanticColorRole::HighlightText,
                SemanticColorRole::Highlight,
                SemanticColorRole::HighlightHover,
                SemanticColorRole::HighlightPressed,
            )
        };
        let mut st = std::mem::take(&mut b.style);
        // A pill at these heights.
        st.radius = Some(nana_ui_core::RadiusTier::Md);
        st.foreground = Some(foreground);
        st.background = Some(background);
        st.interaction.hovered.background = Some(hover);
        st.interaction.pressed.background = Some(pressed);
        b = b.style(st);
        widget(b)
            .bind(move |b: &mut Button| {
                let show = hovered.get() || focused.get();
                let l = Arc::make_mut(&mut b.style.layout);
                l.opacity = Some(if show { 1.0 } else { 0.0 });
                l.pointer_events = Some(if show {
                    PointerEventsSpec::Auto
                } else {
                    PointerEventsSpec::None
                });
            })
            .on_cx(move |_, _: &Activate, cx| {
                super::motion::press(cx, ui.reduced_motion.get_untracked());
                cx.dispatch_program_all(Msg::ChatMention { seq, reply });
            })
    };
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::TIGHT)
            .width(LengthSpec::Px(width))
            .height(LengthSpec::Px(height))
            .with_layout(|l| l.flex_shrink = Some(0.0)),
    )
    .children((button(true), button(false)))
    .into_any()
}

/// A small avatar and a single header above full-width, freely wrapping text.
/// Controls occupy only the header, leaving the body available for reading.
fn compact_message(ui: Ui, source: RowSource, width: f32, actions: Option<AnyView>) -> AnyView {
    let Some(row) = source.get(ui) else {
        return widget(Stack::row(0.0)).into_any();
    };
    let host = matches!(source, RowSource::History(_));
    // List and row padding, scrollbar, 24px avatar and its 8px gap.
    let col_w = (width - 74.0).max(100.0);
    let show_time = width >= 360.0;
    let name_w = (col_w
        - if actions.is_some() {
            actions_width(ui)
        } else {
            0.0
        }
        - if show_time { 42.0 } else { 0.0 }
        - if host { 32.0 } else { 0.0 }
        - 8.0)
        .max(24.0);
    let mut name_style = Type::Strong.style();
    name_style.foreground = Some(if host {
        SemanticColorRole::Accent
    } else {
        SemanticColorRole::Text
    });
    Arc::make_mut(&mut name_style.layout).max_width = Some(LengthSpec::Px(name_w));
    let header = widget(
        Stack::row(0.0)
            .gap_step(gap::step::TIGHT)
            .align(AlignSpec::Center)
            .width(LengthSpec::Fill)
            .with_layout(|l| l.min_height = Some(LengthSpec::Px(META_LINE_H))),
    )
    .children((
        widget(
            Stack::row(0.0)
                .width(LengthSpec::Px(name_w))
                .with_layout(|l| {
                    l.min_width = Some(LengthSpec::Px(0.0));
                    l.overflow_x = OverflowSpec::Hidden;
                    l.flex_shrink = Some(1.0);
                }),
        )
        .children(widget(
            Text::new(row.item.author.clone())
                .style(name_style)
                .truncating(),
        )),
        host.then(|| {
            widget(
                Text::new(ui.now_t("history-role-host"))
                    .style(Type::Micro.style())
                    .color(SemanticColorRole::Muted),
            )
        }),
        widget(Stack::spacer()),
        show_time.then(|| {
            widget(
                Text::new(history::local_hm(row.item.at))
                    .style(Type::Micro.style())
                    .color(SemanticColorRole::Faint),
            )
        }),
        actions,
    ));
    let metadata = match source {
        RowSource::Chat(seq) => ui.chat.with_untracked(|rows| {
            rows.iter().find(|m| m.seq == seq).and_then(|m| {
                let mut labels = vec![ui.now_t(match m.platform {
                    crate::chat::Platform::Bilibili => "settings-bilibili",
                    crate::chat::Platform::Douyin => "settings-douyin",
                    crate::chat::Platform::Blivechat => "settings-blivechat",
                })];
                if m.owner {
                    labels.push(ui.now_t("chat-room-owner"));
                } else if m.admin {
                    labels.push(ui.now_t("chat-moderator"));
                }
                if m.guard != nanatype_bilibili::Guard::None {
                    labels.push(ui.now_t(crate::chat::guard_key(m.guard)));
                }
                if m.medal_level > 0 && !m.medal_name.is_empty() {
                    labels.push(format!("{} {}", m.medal_name, m.medal_level));
                }
                (!labels.is_empty()).then(|| {
                    widget(
                        Text::new(labels.join(" · "))
                            .style(Type::Micro.style())
                            .color(SemanticColorRole::Muted)
                            .truncating(),
                    )
                })
            })
        }),
        _ => None,
    };
    let quote = row.item.reply.as_ref().map(|r| {
        let mut st = Type::Micro.style();
        Arc::make_mut(&mut st.layout).max_width = Some(LengthSpec::Px(col_w));
        widget(
            Text::new(format!(
                "↪ {}：{}",
                r.user.name,
                r.text.replace(['\r', '\n'], " ")
            ))
            .style(st)
            .color(SemanticColorRole::Muted)
            .truncating(),
        )
    });
    let dark = ui.chrome_dark.get_untracked();
    let colors = palette(dark);
    let look = Look {
        size: 14.0,
        color: rgba8(colors.text),
        weight: 400,
        line_height: 1.35,
        body: None,
        name: None,
        accent: None,
        latin: None,
        mention: MentionLook::for_ui(dark),
        ink: None,
    };
    let mut body_style = Type::Chat.style();
    body_style.foreground = Some(SemanticColorRole::Text);
    Arc::make_mut(&mut body_style.layout).line_height =
        Some(nana_ui_core::LineHeightSpec::Absolute(14.0 * 1.35));
    let body = widget(
        RichTextView::new("")
            .style(body_style)
            .width(LengthSpec::Fill),
    )
    .bind(move |v: &mut RichTextView| {
        // Read only the matching stream; image completion updates this node in place.
        match source {
            RowSource::Chat(_) => {
                ui.chat.get();
            }
            RowSource::History(_) => {
                ui.history.get();
            }
        }
        let Some(row) = source.get(ui) else {
            return;
        };
        let rich = to_rich(
            &row.doc,
            &Env {
                look: &look,
                urls: &ui.asset_urls.get(),
                editor: false,
                locale: ui.locale.get(),
            },
        );
        if v.value != rich {
            v.value = rich;
        }
    });
    widget(
        Stack::row(0.0)
            .gap_step(nana_ui_core::SpacingStep::Md)
            .align(AlignSpec::Start)
            .width(LengthSpec::Fill),
    )
    .children((
        asset_avatar(
            ui,
            row.item.avatar.clone(),
            &row.item.author,
            AVATAR,
            colors.accent.as_rgba_array(),
        ),
        widget(
            Stack::column(0.0)
                .gap_step(gap::step::HAIR)
                .width(LengthSpec::Px(col_w))
                .with_layout(|l| l.flex_shrink = Some(0.0)),
        )
        .children((header, metadata, quote, body)),
    ))
    .into_any()
}

/// Retains first-observed order across filtering, reveal updates and image loads.
#[derive(Default)]
struct Feed {
    order: HashMap<RowSource, u64>,
    next: u64,
}
impl Feed {
    fn update(
        &mut self,
        chat: &[Arc<ChatMessage>],
        history: &[nanatype_core::HistoryRow],
        filter: &ChatFilter,
    ) -> Vec<RowSource> {
        let mut all: Vec<_> = chat
            .iter()
            .map(|m| (RowSource::Chat(m.seq), m.at))
            .chain(
                history
                    .iter()
                    .filter(|r| r.item.kind == nanatype_model::SourceKind::Typewriter)
                    .map(|r| (RowSource::History(r.key), r.item.at)),
            )
            .collect();
        let present: HashSet<_> = all.iter().map(|(k, _)| *k).collect();
        self.order.retain(|k, _| present.contains(k));
        all.sort_by_key(|(k, at)| (*at, *k));
        for (key, _) in &all {
            self.order.entry(*key).or_insert_with(|| {
                let n = self.next;
                self.next += 1;
                n
            });
        }
        let allowed: HashSet<_> = shown(chat, filter)
            .into_iter()
            .map(RowSource::Chat)
            .collect();
        let mut out: Vec<_> = all
            .into_iter()
            .map(|(k, _)| k)
            .filter(|k| matches!(k, RowSource::History(_)) || allowed.contains(k))
            .collect();
        out.sort_by_key(|k| self.order[k]);
        out
    }
}

fn filter_toggle(
    ui: Ui,
    icon: nana_ui::runtime::Icon,
    key: &'static str,
    get: fn(&ChatFilter) -> bool,
    set: fn(&mut ChatFilter, bool),
) -> AnyView {
    widget(
        IconButton::new(icon, ui.now_t(key))
            .size(ControlSize::Small)
            .with_tooltip(ui.now_t(key)),
    )
    .bind(move |b: &mut IconButton| {
        let on = ui.settings.with(|s| get(&s.integrations.bilibili.panel));
        if b.selected != on {
            b.selected = on;
        }
    })
    .on_cx(move |_, _: &Activate, cx| {
        cx.dispatch_program_all(Msg::Edit(Box::new(move |s: &mut Settings| {
            let f = &mut s.integrations.bilibili.panel;
            let on = get(f);
            set(f, !on);
        })))
    })
    .into_any()
}

/// One dot for every source: green while any is connected, then amber while
/// any connects, red when one failed, grey when all are off.
pub fn combined_link(links: [Link; 3]) -> Link {
    [Link::Connected, Link::Connecting, Link::Error]
        .into_iter()
        .find(|want| links.contains(want))
        .unwrap_or(Link::Off)
}

fn header(ui: Ui) -> AnyView {
    dynamic(
        move || ui.chat_width.get() < 560.0,
        move |narrow| header_layout(ui, *narrow),
    )
    .into_any()
}
fn header_layout(ui: Ui, narrow: bool) -> AnyView {
    let dot = widget(crate::ui::kit::dot_frame()).bind(move |s: &mut Stack| {
        let douyin = match ui.douyin.get().connection {
            nanatype_douyin::ConnectionState::Connected { .. } => Link::Connected,
            nanatype_douyin::ConnectionState::Connecting => Link::Connecting,
            nanatype_douyin::ConnectionState::Error(_) => Link::Error,
            _ => Link::Off,
        };
        let link = combined_link([
            ui.status.with(|x| x.bilibili_link),
            douyin,
            ui.status.with(|x| x.blivechat_link),
        ]);
        *s = crate::ui::kit::dot_frame().surface(link.tone().role());
    });
    let status = widget(
        Text::new("")
            .style(Type::Small.style())
            .color(SemanticColorRole::Muted)
            .truncating(),
    )
    .bind(move |t: &mut Text| {
        let mut v = ui.status.with(|s| s.bilibili.clone());
        if ui.settings.with(|s| s.integrations.douyin.enabled) {
            v = format!(
                "{v} · {}: {}",
                ui.now_t("settings-douyin"),
                super::douyin::connection(ui)
            );
        }
        if ui.settings.with(|s| s.integrations.blivechat.enabled) {
            let label = ui.now_t("settings-blivechat");
            v = ui
                .status
                .with(|s| format!("{v} · {label}: {}", s.blivechat));
        }
        if t.value != v {
            t.value = v;
        }
    });
    let status = widget(
        Stack::row(0.0)
            .gap_step(gap::step::TIGHT)
            .align(AlignSpec::Center)
            .with_layout(|l| {
                l.flex_grow = Some(1.0);
                l.flex_shrink = Some(1.0);
                l.min_width = Some(LengthSpec::Px(0.0));
            }),
    )
    .children((
        dot,
        widget(Stack::row(0.0).with_layout(|l| {
            l.flex_grow = Some(1.0);
            l.flex_shrink = Some(1.0);
            l.min_width = Some(LengthSpec::Px(0.0));
            l.overflow_x = OverflowSpec::Hidden;
        }))
        .children(status),
    ));
    let mut controls = vec![
        filter_toggle(
            ui,
            ti::MESSAGE_CIRCLE,
            "chat-filter-text",
            |f| f.text,
            |f, v| f.text = v,
        ),
        filter_toggle(
            ui,
            ti::COIN,
            "chat-filter-super-chat",
            |f| f.super_chat,
            |f, v| f.super_chat = v,
        ),
        filter_toggle(
            ui,
            ti::GIFT,
            "chat-filter-gift",
            |f| f.gift,
            |f, v| f.gift = v,
        ),
        filter_toggle(
            ui,
            ti::CROWN,
            "chat-filter-member",
            |f| f.member,
            |f, v| f.member = v,
        ),
        action(ui, ti::TRASH, "chat-clear", || Msg::ClearChat),
        action(ui, ti::SETTINGS, "window-settings", || {
            Msg::SettingsPage(super::settings::Page::Integrations)
        }),
    ];
    let bar = if narrow {
        let utilities = controls.split_off(4);
        widget(
            Stack::column(0.0)
                .gap_step(gap::step::TIGHT)
                .width(LengthSpec::Fill),
        )
        .children((
            widget(
                Stack::row(0.0)
                    .gap_step(gap::step::TIGHT)
                    .align(AlignSpec::Center)
                    .width(LengthSpec::Fill),
            )
            .children((status.into_any(), utilities))
            .into_any(),
            widget(
                Stack::row(0.0)
                    .gap_step(gap::step::TIGHT)
                    .align(AlignSpec::Center)
                    .width(LengthSpec::Fill),
            )
            .children((
                super::settings::chat_panel_style(ui),
                widget(Stack::spacer()),
                widget(Stack::row(0.0)).children(controls),
            ))
            .into_any(),
        ))
        .into_any()
    } else {
        widget(
            Stack::row(0.0)
                .gap_step(gap::step::TIGHT)
                .align(AlignSpec::Center)
                .width(LengthSpec::Fill),
        )
        .children((
            status.into_any(),
            controls,
            super::settings::chat_panel_style(ui),
        ))
        .into_any()
    };
    widget(
        Stack::column(0.0)
            .height(LengthSpec::Px(if narrow {
                crate::ui::kit::CHAT_HEADER_NARROW_H
            } else {
                crate::ui::kit::CHAT_HEADER_H
            }))
            .padding_step(inset::step::BAR_X, gap::step::TIGHT)
            .with_layout(|l| l.flex_shrink = Some(0.0)),
    )
    .children(bar)
    .into_any()
}

/// Shown when nothing is listed.
fn empty(ui: Ui) -> AnyView {
    widget(
        Stack::column(0.0)
            .padding_step(inset::step::PAGE, inset::step::PAGE)
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center)
            .width(LengthSpec::Fill)
            .with_layout(|l| l.flex_grow = Some(1.0)),
    )
    .children(
        widget(
            Text::new("")
                .style(Type::Body.style())
                .color(SemanticColorRole::Muted),
        )
        .bind(move |t: &mut Text| {
            let configured = ui.settings.with(|s| {
                let b = &s.integrations.bilibili;
                (b.enabled && !b.room.is_empty())
                    || (s.integrations.douyin.enabled && !s.integrations.douyin.room.is_empty())
                    || s.integrations.blivechat.enabled
            });
            let key = if configured {
                "chat-empty"
            } else {
                "chat-not-configured"
            };
            let v = I18N.t_in(ui.locale.get(), key);
            if t.value != v {
                t.value = v;
            }
        }),
    )
    .into_any()
}

/// The keys of the messages the panel lists, oldest first.
pub fn shown(rows: &[Arc<ChatMessage>], filter: &ChatFilter) -> Vec<u64> {
    rows.iter()
        .filter(|m| m.passes(filter))
        .map(|m| m.seq)
        .collect()
}

pub fn chat_window(ui: Ui) -> impl IntoView {
    let feed = Arc::new(Mutex::new(Feed::default()));
    let items = computed(move || {
        let filter = ui.settings.with(|s| s.integrations.bilibili.panel.clone());
        let chat = ui.chat.get();
        let history = ui.history.get();
        feed.lock().unwrap().update(&chat, &history, &filter)
    });
    let follow = signal(true);
    let scroll = node_ref();
    let list = widget(
        ScrollView::new(ScrollAxes::Vertical)
            .follow_end(true)
            .with_layout(|l| {
                l.flex_grow = Some(1.0);
                l.flex_shrink = Some(1.0);
                l.height = Some(LengthSpec::Px(0.0));
                l.min_height = Some(LengthSpec::Px(0.0));
            }),
    )
    .node_ref(scroll)
    .bind(move |v: &mut ScrollView| v.follow_end = follow.get())
    .on(move |e: &UserScroll| follow.set(e.at_end))
    .children(dynamic(
        move || {
            (
                ui.chat_width.get().round() as i32,
                ui.chrome_dark.get(),
                ui.locale.get(),
                ui.reduced_motion.get(),
            )
        },
        move |(width, _, _, _)| {
            let width = *width as f32;
            widget(
                Stack::column(0.0)
                    .gap_step(gap::step::HAIR)
                    .padding_step(nana_ui_core::SpacingStep::Sm, gap::step::TIGHT)
                    .width(LengthSpec::Fill),
            )
            .children(
                each_virtual(
                    items,
                    |k: &RowSource| *k,
                    ROW_ESTIMATE_H,
                    move |k| row_view(ui, k, width),
                )
                .measured()
                .overscan(240.0)
                .within(scroll),
            )
            .into_any()
        },
    ));
    widget(
        Stack::fill_column(0.0)
            .surface(SemanticColorRole::Background)
            .with_layout(|l| {
                l.overflow_x = OverflowSpec::Hidden;
                l.overflow_y = OverflowSpec::Hidden;
            }),
    )
    .children((
        header(ui),
        crate::ui::kit::rule(),
        list.visible(move || !items.with(|i| i.is_empty())),
        widget(Stack::fill_column(0.0).with_layout(|l| {
            l.flex_grow = Some(1.0);
            l.min_height = Some(LengthSpec::Px(0.0));
        }))
        .visible(move || items.with(|i| i.is_empty()))
        .children(empty(ui)),
        widget(
            Button::new(ui.now_t("chat-latest"))
                .icon(ti::ARROW_DOWN)
                .size(ControlSize::Small),
        )
        .visible(move || !follow.get())
        .on(move |_: &Activate| follow.set(true)),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nanatype_bilibili::Guard;
    use nanatype_model::{ChatUser, Document};

    fn message(seq: u64, kind: ChatKind) -> Arc<ChatMessage> {
        Arc::new(ChatMessage {
            platform: crate::chat::Platform::Bilibili,
            epoch: 0,
            room: String::new(),
            seq,
            id: seq.to_string(),
            kind,
            user: ChatUser::default(),
            guard: Guard::None,
            owner: false,
            admin: false,
            medal_level: 0,
            medal_name: String::new(),
            doc: Document::plain("hi"),
            at: 1,
        })
    }

    #[test]
    fn the_panel_lists_what_its_filter_lets_through() {
        let rows = vec![
            message(1, ChatKind::Text),
            message(2, ChatKind::SuperChat { price: 30 }),
            message(
                3,
                ChatKind::Gift {
                    name: "x".into(),
                    num: 1,
                    coin: 0,
                },
            ),
        ];
        assert_eq!(shown(&rows, &ChatFilter::default()), [1, 2, 3]);
        let filter = ChatFilter {
            text: false,
            min_gift_coin: 1,
            ..ChatFilter::default()
        };
        assert_eq!(shown(&rows, &filter), [2]);
    }
    #[test]
    fn feed_keeps_disjoint_ids_and_order_across_filtering_and_late_updates() {
        use nanatype_model::{HistoryItem, SourceId, SourceKind};
        let chat = vec![
            message(1, ChatKind::Text),
            message(2, ChatKind::SuperChat { price: 30 }),
        ];
        let history = vec![nanatype_core::HistoryRow {
            key: 1,
            item: HistoryItem {
                source: SourceId::local(),
                kind: SourceKind::Typewriter,
                line: Some(nanatype_model::LineId(1)),
                author: "主播".into(),
                avatar: None,
                doc: Document::plain("hello"),
                at: 2,
                uid: None,
                reply: None,
            },
            doc: Arc::new(Document::plain("hello")),
            reveal: None,
        }];
        let mut feed = Feed::default();
        let all = feed.update(&chat, &history, &ChatFilter::default());
        assert_eq!(
            all,
            [
                RowSource::Chat(1),
                RowSource::Chat(2),
                RowSource::History(1)
            ]
        );
        let hidden = ChatFilter {
            text: false,
            ..Default::default()
        };
        assert_eq!(
            feed.update(&chat, &history, &hidden),
            [RowSource::Chat(2), RowSource::History(1)]
        );
        assert_eq!(feed.update(&chat, &history, &ChatFilter::default()), all);
        let mut changed = history.clone();
        changed[0].reveal = Some(nanatype_core::Reveal::full(5));
        assert_eq!(feed.update(&chat, &changed, &ChatFilter::default()), all);
        // Clearing the viewer buffer cannot re-add a row from the captured history.
        let mut external = changed[0].clone();
        external.key = 3;
        external.item.kind = SourceKind::Chat;
        changed.push(external);
        assert_eq!(
            feed.update(&[], &changed, &ChatFilter::default()),
            [RowSource::History(1)]
        );
    }
}
