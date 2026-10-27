//! Live chat as the app keeps it: platform-tagged events with their pictures saved
//! locally, ready for the chat panel, the history list and replies.

use nanatype_chat::{Author, ChatEvent, Guard};
use nanatype_i18n::Locale;
use nanatype_model::{
    AssetRef, Atom, AtomKind, ChatRef, ChatRun, ChatUser, Document, HistoryItem, Inline, SourceId,
    SourceKind, Style, TextRun,
};
use nanatype_settings::ChatFilter;

use crate::ui::I18N;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Platform {
    /// A blivechat server or the 彗星号 relay.
    Blivechat,
    Twitch,
}
impl Platform {
    /// The sources this build connects to, in settings order.
    pub const ALL: &[Self] = &[Self::Twitch, Self::Blivechat];
    pub fn source(self) -> &'static str {
        match self {
            Self::Blivechat => "blivechat",
            Self::Twitch => "twitch",
        }
    }
    /// Downloaded pictures are cached per platform under this prefix.
    pub fn cache_prefix(self) -> &'static str {
        match self {
            Self::Blivechat => "blive",
            Self::Twitch => "twitch",
        }
    }
    /// The settings page title, which doubles as the platform's name.
    pub fn label_key(self) -> &'static str {
        match self {
            Self::Blivechat => "settings-blivechat",
            Self::Twitch => "settings-twitch",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChatKind {
    Text,
    SuperChat {
        /// Yuan.
        price: u64,
    },
    Gift {
        name: String,
        num: u32,
        /// Gold coins (1000 = ¥1); 0 for free gifts.
        coin: u64,
    },
    /// A guard membership (上舰).
    Member {
        num: u32,
        unit: String,
        coin: u64,
    },
    /// A paid message such as Twitch Bits; `amount` is the platform's own text.
    Paid {
        amount: String,
        /// ARGB, as the platform colours the card.
        color: u32,
    },
    /// A membership, subscription or raid, in the platform's own words.
    Notice {
        text: String,
    },
    /// Memberships given to other viewers.
    GiftMemberships {
        count: u32,
    },
}

/// One chat message, its pictures already on disk.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatMessage {
    pub platform: Platform,
    pub epoch: u64,
    pub room: String,
    /// Arrival order; the app's handle for the message.
    pub seq: u64,
    /// The platform's message id (Super Chats are taken down by it).
    pub id: String,
    pub kind: ChatKind,
    pub user: ChatUser,
    pub guard: Guard,
    pub owner: bool,
    pub admin: bool,
    pub medal_level: u32,
    pub medal_name: String,
    /// What the viewer wrote (empty for gifts and memberships); an emoticon
    /// is an inline picture.
    pub doc: Document,
    /// Unix milliseconds.
    pub at: u64,
}

impl ChatMessage {
    /// Build from a Twitch event; Bits read like a paid message.
    pub fn from_twitch(
        event: nanatype_twitch::ChatEvent,
        emote: &mut dyn FnMut(&str) -> AssetRef,
        seq: u64,
    ) -> Self {
        use nanatype_twitch::Content;
        let (kind, runs) = match event.content {
            Content::Text(runs) => (ChatKind::Text, runs),
            Content::Cheer { bits, runs } => (
                ChatKind::Paid {
                    amount: format!("{bits} Bits"),
                    // Twitch's own purple.
                    color: 0xff9146ff,
                },
                runs,
            ),
            Content::Notice { text, runs } => (ChatKind::Notice { text }, runs),
        };
        let a = event.author;
        let user = ChatUser {
            uid: namespaced("twitch", &a.user_id),
            name: a.name,
            avatar: None,
        };
        let doc = inline_doc(runs, emote);
        Self::live(Platform::Twitch, event.id, seq, kind, user, doc, event.at)
            .badged(a.broadcaster, a.moderator)
    }
    /// A message from a platform without platform-specific guards and medals.
    fn live(
        platform: Platform,
        id: String,
        seq: u64,
        kind: ChatKind,
        user: ChatUser,
        doc: Document,
        at: u64,
    ) -> Self {
        Self {
            platform,
            epoch: 0,
            room: String::new(),
            seq,
            id: if id.is_empty() {
                format!("local-{seq}")
            } else {
                id
            },
            kind,
            user,
            guard: Guard::None,
            owner: false,
            admin: false,
            medal_level: 0,
            medal_name: String::new(),
            doc,
            at: if at == 0 {
                nanatype_model::unix_millis()
            } else {
                at
            },
        }
    }
    fn badged(self, owner: bool, admin: bool) -> Self {
        Self {
            owner,
            admin,
            ..self
        }
    }
    /// Build from a blivechat event; `avatar` and `emoticon` are the saved
    /// pictures. `None` for events that are not messages.
    pub fn from_event(
        event: ChatEvent,
        avatar: Option<AssetRef>,
        emoticon: Option<AssetRef>,
        seq: u64,
    ) -> Option<Self> {
        let (id, author, kind, doc, at) = match event {
            ChatEvent::Text {
                id,
                author,
                content,
                emoticon: _,
                at_ms,
            } => {
                let doc = match emoticon {
                    Some(asset) => Document::from_inlines(vec![Inline::Atom(Atom {
                        kind: AtomKind::Emote,
                        asset,
                        height_em: 2.2,
                        alt: content,
                        effects: Default::default(),
                    })]),
                    None => Document::from_inlines(vec![Inline::Text(TextRun::plain(
                        content.replace(['\r', '\n'], " "),
                    ))]),
                };
                (id, author, ChatKind::Text, doc, at_ms)
            }
            ChatEvent::SuperChat {
                id,
                author,
                price,
                content,
                at_ms,
            } => (
                id,
                author,
                ChatKind::SuperChat { price },
                Document::plain(&content.replace(['\r', '\n'], " ")),
                at_ms,
            ),
            ChatEvent::Gift {
                id,
                author,
                gift_name,
                num,
                total_coin,
                at_ms,
            } => (
                id,
                author,
                ChatKind::Gift {
                    name: gift_name,
                    num,
                    coin: total_coin,
                },
                Document::new(),
                at_ms,
            ),
            ChatEvent::Member {
                id,
                author,
                num,
                unit,
                total_coin,
                at_ms,
            } => (
                id,
                author,
                ChatKind::Member {
                    num,
                    unit,
                    coin: total_coin,
                },
                Document::new(),
                at_ms,
            ),
            ChatEvent::DeleteSuperChats { .. } => return None,
        };
        let Author {
            uid,
            name,
            guard,
            owner,
            admin,
            medal_level,
            medal_name,
            ..
        } = author;
        Some(Self {
            platform: Platform::Blivechat,
            epoch: 0,
            room: String::new(),
            seq,
            id: if id.is_empty() {
                format!("local-{seq}")
            } else {
                id
            },
            kind,
            user: ChatUser { uid, name, avatar },
            guard,
            owner,
            admin,
            medal_level,
            medal_name,
            doc,
            at: if at == 0 {
                nanatype_model::unix_millis()
            } else {
                at
            },
        })
    }

    /// Whether `filter` lets this message through.
    pub fn passes(&self, filter: &ChatFilter) -> bool {
        match &self.kind {
            ChatKind::Text => filter.text,
            ChatKind::SuperChat { .. } => filter.super_chat,
            ChatKind::Gift { coin, .. } => filter.gift && *coin >= u64::from(filter.min_gift_coin),
            ChatKind::Member { .. } => filter.member,
            ChatKind::Paid { .. } => filter.super_chat,
            ChatKind::Notice { .. } | ChatKind::GiftMemberships { .. } => filter.member,
        }
    }

    pub fn source_kind(&self) -> SourceKind {
        match self.kind {
            ChatKind::Text => SourceKind::Chat,
            _ => SourceKind::Event,
        }
    }

    /// The event line of a gift or a membership, in the UI language.
    pub fn event_text(&self, locale: Locale) -> Option<String> {
        Some(match &self.kind {
            ChatKind::Text | ChatKind::SuperChat { .. } | ChatKind::Paid { .. } => return None,
            ChatKind::Notice { text } => text.clone(),
            ChatKind::GiftMemberships { count } => I18N.tr_in(
                locale,
                "chat-gift-memberships",
                &[("count", (*count).into())],
            ),
            ChatKind::Gift { name, num, .. } => I18N.tr_in(
                locale,
                "chat-gift",
                &[("gift", name.as_str().into()), ("count", (*num).into())],
            ),
            ChatKind::Member { num, unit, .. } => I18N.tr_in(
                locale,
                "chat-member",
                &[
                    ("guard", I18N.t_in(locale, guard_key(self.guard)).into()),
                    ("count", (*num).into()),
                    ("unit", unit.as_str().into()),
                ],
            ),
        })
    }

    /// What the message says, as a document: the viewer's text, a Super
    /// Chat with its price, or the event line.
    pub fn display_doc(&self, locale: Locale) -> Document {
        let lead = match &self.kind {
            ChatKind::Text => return self.doc.clone(),
            ChatKind::SuperChat { price } => format!("¥{price}"),
            ChatKind::Paid { amount, .. } => amount.clone(),
            ChatKind::Notice { text } if !self.doc.is_blank() => text.clone(),
            _ => return Document::plain(&self.event_text(locale).unwrap_or_default()),
        };
        let mut inlines = vec![Inline::Text(TextRun::styled(
            format!("{lead} "),
            Style {
                bold: Some(true),
                ..Style::default()
            },
        ))];
        inlines.extend(self.doc.paragraphs.iter().flat_map(|p| p.inlines.clone()));
        Document::from_inlines(inlines)
    }

    /// The message as plain text, for a quote.
    pub fn plain(&self, locale: Locale) -> String {
        self.display_doc(locale).plain_text()
    }

    /// A quote of this message for a line that answers it.
    pub fn quote(&self, locale: Locale) -> ChatRef {
        ChatRef::new(self.user.clone(), &self.plain(locale), self.source_kind())
    }

    /// A history row for this message.
    pub fn history_item(&self, locale: Locale) -> HistoryItem {
        HistoryItem {
            source: SourceId(self.platform.source().into()),
            kind: self.source_kind(),
            line: None,
            author: self.user.name.clone(),
            avatar: self.user.avatar.as_ref().map(ToString::to_string),
            doc: self.display_doc(locale),
            at: self.at,
            uid: (!self.user.uid.is_empty()).then(|| self.user.uid.clone()),
            reply: None,
        }
    }
}

/// Text and inline platform pictures as one paragraph.
fn inline_doc(runs: Vec<ChatRun>, emote: &mut dyn FnMut(&str) -> AssetRef) -> Document {
    let inlines: Vec<Inline> = runs
        .into_iter()
        .filter_map(|run| match run {
            ChatRun::Text(text) if text.is_empty() => None,
            ChatRun::Text(text) => Some(Inline::Text(TextRun::plain(
                text.replace(['\r', '\n'], " "),
            ))),
            ChatRun::Emote { alt, url } => Some(Inline::Atom(Atom {
                kind: AtomKind::Emote,
                asset: emote(&url),
                height_em: 1.4,
                alt,
                effects: Default::default(),
            })),
        })
        .collect();
    if inlines.is_empty() {
        Document::new()
    } else {
        Document::from_inlines(inlines)
    }
}

/// Platform ids stay namespaced when a platform supplies them.
fn namespaced(platform: &str, id: &str) -> String {
    if id.is_empty() {
        String::new()
    } else {
        format!("{platform}:{id}")
    }
}

/// The card colour of a paid message, by platform.
pub fn paid_color(kind: &ChatKind) -> Option<[f32; 4]> {
    let channel = |rgb: u32, shift: u32| ((rgb >> shift) & 0xff) as f32 / 255.0;
    let rgb = match kind {
        ChatKind::SuperChat { price } => return Some(super_chat_color(*price)),
        ChatKind::Paid { color, .. } if *color != 0 => *color,
        ChatKind::Paid { .. } => 0x1e88e5,
        _ => return None,
    };
    Some([channel(rgb, 16), channel(rgb, 8), channel(rgb, 0), 1.0])
}

pub fn guard_key(guard: Guard) -> &'static str {
    match guard {
        Guard::Governor => "chat-guard-governor",
        Guard::Admiral => "chat-guard-admiral",
        Guard::Captain | Guard::None => "chat-guard-captain",
    }
}

/// The colour band of a Super Chat by price.
pub fn super_chat_color(price: u64) -> [f32; 4] {
    let rgb = match price {
        2000.. => 0xe62117,
        1000.. => 0xe91e63,
        500.. => 0xf57c00,
        100.. => 0xffca28,
        50.. => 0x1de9b6,
        30.. => 0x00e5ff,
        _ => 0x1e88e5,
    };
    let f = |shift: u32| ((rgb >> shift) & 0xff) as f32 / 255.0;
    [f(16), f(8), f(0), 1.0]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn author() -> Author {
        Author {
            uid: "42".into(),
            name: "观众".into(),
            ..Author::default()
        }
    }

    #[test]
    fn a_danmaku_becomes_a_message_with_its_avatar() {
        let m = ChatMessage::from_event(
            ChatEvent::Text {
                id: "a".into(),
                author: author(),
                content: "晚上\n好".into(),
                emoticon: None,
                at_ms: 5,
            },
            Some(AssetRef::Blob("blive-face-42".into())),
            None,
            1,
        )
        .unwrap();
        assert_eq!(m.doc.plain_text(), "晚上 好");
        assert_eq!(m.user.avatar, Some(AssetRef::Blob("blive-face-42".into())));
        let item = m.history_item(Locale::ZhCn);
        assert_eq!(item.avatar.as_deref(), Some("blob:blive-face-42"));
        assert_eq!(item.kind, SourceKind::Chat);
        assert_eq!(item.uid.as_deref(), Some("42"));
    }

    #[test]
    fn an_emoticon_is_an_inline_picture_with_its_text_as_alt() {
        let m = ChatMessage::from_event(
            ChatEvent::Text {
                id: String::new(),
                author: author(),
                content: "[dog]".into(),
                emoticon: Some("https://x/e.png".into()),
                at_ms: 0,
            },
            None,
            Some(AssetRef::Blob("blive-emote-1".into())),
            7,
        )
        .unwrap();
        assert_eq!(m.id, "local-7");
        assert_eq!(m.doc.unit_count(), 1);
        assert_eq!(m.doc.plain_text(), "[dog]");
        assert!(m.at > 0, "a missing time is now");
    }

    #[test]
    fn filters_keep_kinds_and_cheap_gifts_out() {
        let gift = |coin| ChatMessage {
            platform: crate::chat::Platform::Blivechat,
            epoch: 0,
            room: String::new(),
            seq: 1,
            id: "g".into(),
            kind: ChatKind::Gift {
                name: "小花花".into(),
                num: 1,
                coin,
            },
            user: ChatUser::default(),
            guard: Guard::None,
            owner: false,
            admin: false,
            medal_level: 0,
            medal_name: String::new(),
            doc: Document::new(),
            at: 1,
        };
        let filter = ChatFilter {
            min_gift_coin: 100,
            ..ChatFilter::default()
        };
        assert!(!gift(0).passes(&filter), "free gifts are worth nothing");
        assert!(gift(100).passes(&filter));
        let no_gifts = ChatFilter {
            gift: false,
            ..ChatFilter::default()
        };
        assert!(!gift(1000).passes(&no_gifts));
    }

    fn emote(url: &str) -> AssetRef {
        AssetRef::Blob(format!("emote-{}", url.len()))
    }

    #[test]
    fn twitch_cheers_and_subscriptions_read_in_the_ui_language() {
        use nanatype_twitch::{Author as TwAuthor, ChatEvent as TwEvent, Content, Run};
        let event = |content| TwEvent {
            id: "t1".into(),
            author: TwAuthor {
                user_id: "42".into(),
                name: "Fan".into(),
                broadcaster: true,
                ..TwAuthor::default()
            },
            content,
            at: 0,
        };
        let cheer = ChatMessage::from_twitch(
            event(Content::Cheer {
                bits: 100,
                runs: vec![Run::Text("nice".into())],
            }),
            &mut emote,
            3,
        );
        assert_eq!(cheer.user.uid, "twitch:42");
        assert!(cheer.owner);
        assert!(cheer.at > 0, "a missing time is now");
        assert_eq!(cheer.plain(Locale::EnUs), "100 Bits nice");
        let sub = ChatMessage::from_twitch(
            event(Content::Notice {
                text: "Fan subscribed.".into(),
                runs: vec![],
            }),
            &mut emote,
            4,
        );
        assert_eq!(sub.plain(Locale::EnUs), "Fan subscribed.");
        let no_members = ChatFilter {
            member: false,
            ..ChatFilter::default()
        };
        assert!(!sub.passes(&no_members));
    }

    #[test]
    fn a_super_chat_quotes_with_its_price() {
        let m = ChatMessage::from_event(
            ChatEvent::SuperChat {
                id: "s".into(),
                author: author(),
                price: 30,
                content: "加油".into(),
                at_ms: 1,
            },
            None,
            None,
            1,
        )
        .unwrap();
        let q = m.quote(Locale::ZhCn);
        assert_eq!(q.text, "¥30 加油");
        assert_eq!(q.kind, SourceKind::Event);
    }
}
