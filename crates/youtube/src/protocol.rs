//! The web client's live chat JSON, read field by field. Renderers this file
//! does not know are skipped, so a new YouTube item never breaks the chat.
use crate::{Author, ChatEvent, Content, Run};
use serde_json::Value;

/// One thing a chat response asks the viewer to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Chat(ChatEvent),
    Deleted(String),
}

/// Where to continue and how long YouTube asked us to wait first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Continuation {
    pub token: String,
    pub timeout_ms: u64,
    /// A reload token answers with the recent backlog, not new messages.
    pub reload: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Batch {
    pub actions: Vec<Action>,
    /// `None` once the chat has closed.
    pub next: Option<Continuation>,
}

/// What a `live_chat` page offers.
#[derive(Debug, Clone, PartialEq)]
pub enum Page {
    Chat {
        api_key: Option<String>,
        context: Value,
        start: Continuation,
    },
    /// Chat is turned off or not open yet.
    Unavailable,
    /// The page is a replay of a finished stream.
    Replay,
}

/// Read the `ytcfg` client context and the initial chat state out of the page.
pub fn page(html: &str) -> Result<Page, String> {
    let data = embedded(
        html,
        &[
            "window[\"ytInitialData\"] = ",
            "var ytInitialData = ",
            "ytInitialData = ",
        ],
    )
    .ok_or("protocol-changed")?;
    let Some(chat) = data.pointer("/contents/liveChatRenderer") else {
        return Ok(Page::Unavailable);
    };
    if chat["isReplay"].as_bool() == Some(true) {
        return Ok(Page::Replay);
    }
    let context = embedded(html, &["\"INNERTUBE_CONTEXT\":"]).ok_or("protocol-changed")?;
    let api_key = quoted(html, "\"INNERTUBE_API_KEY\":\"");
    // "Live chat" (every message) is the last view; "Top chat" filters.
    let all = chat
        .pointer(
            "/header/liveChatHeaderRenderer/viewSelector/sortFilterSubMenuRenderer/subMenuItems",
        )
        .and_then(Value::as_array)
        .and_then(|items| items.last())
        .and_then(|item| item.pointer("/continuation/reloadContinuationData/continuation"))
        .and_then(Value::as_str)
        .map(|token| Continuation {
            token: token.to_owned(),
            timeout_ms: 0,
            reload: true,
        });
    let start = all
        .or_else(|| continuation(&chat["continuations"]))
        .ok_or("protocol-changed")?;
    Ok(Page::Chat {
        api_key,
        context,
        start,
    })
}

/// Read one `get_live_chat` answer.
pub fn batch(response: &Value) -> Batch {
    let Some(live) = response.pointer("/continuationContents/liveChatContinuation") else {
        return Batch::default();
    };
    Batch {
        actions: live["actions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(action)
            .collect(),
        next: continuation(&live["continuations"]),
    }
}

fn continuation(list: &Value) -> Option<Continuation> {
    list.as_array()?.iter().find_map(|entry| {
        let (kind, data) = [
            "invalidationContinuationData",
            "timedContinuationData",
            "reloadContinuationData",
        ]
        .into_iter()
        .find_map(|kind| entry.get(kind).map(|data| (kind, data)))?;
        Some(Continuation {
            token: data["continuation"].as_str()?.to_owned(),
            timeout_ms: data["timeoutMs"].as_u64().unwrap_or(0),
            reload: kind == "reloadContinuationData",
        })
    })
}

fn action(action: &Value) -> Option<Action> {
    if let Some(item) = action.pointer("/addChatItemAction/item") {
        return item_event(item).map(Action::Chat);
    }
    [
        "/removeChatItemAction/targetItemId",
        "/markChatItemAsDeletedAction/targetItemId",
    ]
    .into_iter()
    .find_map(|path| action.pointer(path)?.as_str())
    .map(|id| Action::Deleted(id.to_owned()))
}

fn item_event(item: &Value) -> Option<ChatEvent> {
    let (kind, r) = item.as_object()?.iter().next()?;
    let content = match kind.as_str() {
        "liveChatTextMessageRenderer" => Content::Text(runs(&r["message"])),
        "liveChatPaidMessageRenderer" => Content::Paid {
            amount: text(&r["purchaseAmountText"]),
            color: color(&r["bodyBackgroundColor"]),
            runs: runs(&r["message"]),
        },
        "liveChatPaidStickerRenderer" => Content::Sticker {
            amount: text(&r["purchaseAmountText"]),
            color: color(&r["backgroundColor"]),
            url: thumbnail(&r["sticker"]),
            alt: r
                .pointer("/sticker/accessibility/accessibilityData/label")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        },
        "liveChatMembershipItemRenderer" => {
            let primary = text(&r["headerPrimaryText"]);
            Content::Membership {
                header: if primary.is_empty() {
                    text(&r["headerSubtext"])
                } else {
                    primary
                },
                runs: runs(&r["message"]),
            }
        }
        "liveChatSponsorshipsGiftPurchaseAnnouncementRenderer" => {
            let header = r.pointer("/header/liveChatSponsorshipsHeaderRenderer")?;
            return Some(ChatEvent {
                id: r["id"].as_str()?.to_owned(),
                author: author(header, &r["authorExternalChannelId"]),
                content: Content::GiftMembership {
                    count: first_number(&text(&header["primaryText"])).unwrap_or(1),
                },
                at: millis(r),
            });
        }
        _ => return None,
    };
    Some(ChatEvent {
        id: r["id"].as_str()?.to_owned(),
        author: author(r, &r["authorExternalChannelId"]),
        content,
        at: millis(r),
    })
}

fn author(r: &Value, channel: &Value) -> Author {
    let mut author = Author {
        channel_id: channel.as_str().unwrap_or_default().to_owned(),
        name: text(&r["authorName"]),
        avatar: thumbnail(&r["authorPhoto"]),
        ..Default::default()
    };
    for badge in r["authorBadges"].as_array().into_iter().flatten() {
        let badge = &badge["liveChatAuthorBadgeRenderer"];
        match badge.pointer("/icon/iconType").and_then(Value::as_str) {
            Some("OWNER") => author.owner = true,
            Some("MODERATOR") => author.moderator = true,
            _ => {}
        }
    }
    author
}

fn millis(r: &Value) -> u64 {
    r["timestampUsec"]
        .as_str()
        .and_then(|t| t.parse::<u64>().ok())
        .map_or(0, |us| us / 1000)
}

/// Text that YouTube sends either whole or as runs.
pub(crate) fn text(value: &Value) -> String {
    if let Some(simple) = value["simpleText"].as_str() {
        return simple.to_owned();
    }
    runs(value)
        .into_iter()
        .map(|run| match run {
            Run::Text(text) => text,
            Run::Emote { alt, .. } => alt,
        })
        .collect()
}

fn runs(value: &Value) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    for run in value["runs"].as_array().into_iter().flatten() {
        let next = if let Some(text) = run["text"].as_str() {
            Run::Text(text.to_owned())
        } else if let Some(emoji) = run.get("emoji") {
            let alt = emoji["shortcuts"][0]
                .as_str()
                .or_else(|| emoji["emojiId"].as_str())
                .unwrap_or_default()
                .to_owned();
            let url = thumbnail(&emoji["image"]);
            if emoji["isCustomEmoji"].as_bool() == Some(true) && !url.is_empty() {
                Run::Emote { alt, url }
            } else {
                // A standard emoji is its own character.
                Run::Text(emoji["emojiId"].as_str().unwrap_or(&alt).to_owned())
            }
        } else {
            continue;
        };
        match (out.last_mut(), next) {
            (Some(Run::Text(previous)), Run::Text(text)) => previous.push_str(&text),
            (_, next) => out.push(next),
        }
    }
    out
}

/// The largest picture offered.
fn thumbnail(value: &Value) -> String {
    let url = value["thumbnails"]
        .as_array()
        .and_then(|list| list.last())
        .and_then(|t| t["url"].as_str())
        .unwrap_or_default();
    match url.strip_prefix("//") {
        Some(rest) => format!("https://{rest}"),
        None => url.to_owned(),
    }
}

fn color(value: &Value) -> u32 {
    value.as_u64().map_or(0, |c| c as u32)
}

fn first_number(text: &str) -> Option<u32> {
    let digits: String = text
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit() || *c == ',')
        .filter(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// The first JSON value after any of `markers`.
fn embedded(html: &str, markers: &[&str]) -> Option<Value> {
    markers.iter().find_map(|marker| {
        let start = html.find(marker)? + marker.len();
        serde_json::Deserializer::from_str(&html[start..])
            .into_iter::<Value>()
            .next()?
            .ok()
    })
}

fn quoted(html: &str, marker: &str) -> Option<String> {
    let start = html.find(marker)? + marker.len();
    let end = html[start..].find('"')?;
    let value = &html[start..start + end];
    (!value.is_empty()).then(|| value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn badges() -> Value {
        json!([
            {"liveChatAuthorBadgeRenderer": {"icon": {"iconType": "MODERATOR"}, "tooltip": "Moderator"}},
            {"liveChatAuthorBadgeRenderer": {"customThumbnail": {"thumbnails": [{"url": "https://yt3.ggpht.com/m"}]}, "tooltip": "Member (2 months)"}}
        ])
    }

    #[test]
    fn text_messages_keep_custom_emoji_as_pictures() {
        let response = json!({"continuationContents": {"liveChatContinuation": {
            "continuations": [{"timedContinuationData": {"continuation": "next", "timeoutMs": 4321}}],
            "actions": [
                {"addChatItemAction": {"item": {"liveChatTextMessageRenderer": {
                    "id": "m1",
                    "message": {"runs": [
                        {"text": "hi "},
                        {"emoji": {"emojiId": "😀", "shortcuts": [":grinning:"], "image": {"thumbnails": [{"url": "https://e/std"}]}}},
                        {"text": " there "},
                        {"emoji": {"emojiId": "UCx/abc", "shortcuts": [":_nanaWave:"], "isCustomEmoji": true,
                            "image": {"thumbnails": [{"url": "//yt3.ggpht.com/small"}, {"url": "//yt3.ggpht.com/big"}]}}}
                    ]},
                    "authorName": {"simpleText": "Viewer"},
                    "authorPhoto": {"thumbnails": [{"url": "https://yt4.ggpht.com/a=s32"}, {"url": "https://yt4.ggpht.com/a=s64"}]},
                    "authorExternalChannelId": "UC0123456789abcdefghijAB",
                    "timestampUsec": "1700000000123456",
                    "authorBadges": badges()
                }}}},
                {"addChatItemAction": {"item": {"liveChatViewerEngagementMessageRenderer": {"id": "x"}}}},
                {"addLiveChatTickerItemAction": {}},
                {"markChatItemAsDeletedAction": {"targetItemId": "m0"}}
            ]
        }}});
        let batch = batch(&response);
        assert_eq!(
            batch.next,
            Some(Continuation {
                token: "next".into(),
                timeout_ms: 4321,
                reload: false
            })
        );
        assert_eq!(batch.actions.len(), 2);
        let Action::Chat(event) = &batch.actions[0] else {
            panic!("not a message");
        };
        assert_eq!(event.id, "m1");
        assert_eq!(event.at, 1_700_000_000_123);
        assert_eq!(
            event.content,
            Content::Text(vec![
                Run::Text("hi 😀 there ".into()),
                Run::Emote {
                    alt: ":_nanaWave:".into(),
                    url: "https://yt3.ggpht.com/big".into()
                }
            ])
        );
        assert_eq!(
            event.author,
            Author {
                channel_id: "UC0123456789abcdefghijAB".into(),
                name: "Viewer".into(),
                avatar: "https://yt4.ggpht.com/a=s64".into(),
                owner: false,
                moderator: true,
            }
        );
        assert_eq!(batch.actions[1], Action::Deleted("m0".into()));
    }

    #[test]
    fn paid_items_and_memberships_are_read() {
        let response = json!({"continuationContents": {"liveChatContinuation": {
            "continuations": [{"invalidationContinuationData": {"continuation": "inv", "timeoutMs": 10000}}],
            "actions": [
                {"addChatItemAction": {"item": {"liveChatPaidMessageRenderer": {
                    "id": "p1", "timestampUsec": "1000",
                    "purchaseAmountText": {"simpleText": "¥1,000"},
                    "bodyBackgroundColor": 4294953512u64,
                    "message": {"runs": [{"text": "thanks"}]},
                    "authorName": {"simpleText": "Fan"},
                    "authorBadges": [{"liveChatAuthorBadgeRenderer": {"icon": {"iconType": "OWNER"}}}]
                }}}},
                {"addChatItemAction": {"item": {"liveChatPaidStickerRenderer": {
                    "id": "s1",
                    "purchaseAmountText": {"simpleText": "$5.00"},
                    "backgroundColor": 4280191205u64,
                    "sticker": {"thumbnails": [{"url": "//lh3.googleusercontent.com/s"}],
                        "accessibility": {"accessibilityData": {"label": "Cheering cat"}}},
                    "authorName": {"simpleText": "Cat"}
                }}}},
                {"addChatItemAction": {"item": {"liveChatMembershipItemRenderer": {
                    "id": "j1",
                    "headerSubtext": {"runs": [{"text": "Welcome to "}, {"text": "Nana"}, {"text": "!"}]},
                    "authorName": {"simpleText": "Newbie"}
                }}}},
                {"addChatItemAction": {"item": {"liveChatMembershipItemRenderer": {
                    "id": "j2",
                    "headerPrimaryText": {"runs": [{"text": "Member for "}, {"text": "6"}, {"text": " months"}]},
                    "headerSubtext": {"simpleText": "Nana Club"},
                    "message": {"runs": [{"text": "still here"}]},
                    "authorName": {"simpleText": "Veteran"}
                }}}},
                {"addChatItemAction": {"item": {"liveChatSponsorshipsGiftPurchaseAnnouncementRenderer": {
                    "id": "g1", "authorExternalChannelId": "UCgift",
                    "header": {"liveChatSponsorshipsHeaderRenderer": {
                        "authorName": {"simpleText": "Generous"},
                        "primaryText": {"runs": [{"text": "Gifted "}, {"text": "1,020"}, {"text": " Nana memberships"}]}
                    }}
                }}}}
            ]
        }}});
        let batch = batch(&response);
        assert!(batch.next.is_some());
        let contents: Vec<_> = batch
            .actions
            .iter()
            .map(|a| match a {
                Action::Chat(e) => e.content.clone(),
                Action::Deleted(_) => panic!("unexpected deletion"),
            })
            .collect();
        assert_eq!(
            contents,
            vec![
                Content::Paid {
                    amount: "¥1,000".into(),
                    color: 4294953512,
                    runs: vec![Run::Text("thanks".into())]
                },
                Content::Sticker {
                    amount: "$5.00".into(),
                    color: 4280191205,
                    url: "https://lh3.googleusercontent.com/s".into(),
                    alt: "Cheering cat".into()
                },
                Content::Membership {
                    header: "Welcome to Nana!".into(),
                    runs: vec![]
                },
                Content::Membership {
                    header: "Member for 6 months".into(),
                    runs: vec![Run::Text("still here".into())]
                },
                Content::GiftMembership { count: 1020 },
            ]
        );
        let Action::Chat(gift) = &batch.actions[4] else {
            unreachable!()
        };
        assert_eq!(gift.author.name, "Generous");
        assert_eq!(gift.author.channel_id, "UCgift");
        let Action::Chat(paid) = &batch.actions[0] else {
            unreachable!()
        };
        assert!(paid.author.owner);
    }

    #[test]
    fn a_response_without_continuation_ends_the_chat() {
        assert_eq!(batch(&json!({})), Batch::default());
        let ended = json!({"continuationContents": {"liveChatContinuation": {"actions": []}}});
        assert_eq!(batch(&ended).next, None);
    }

    fn html(data: &Value) -> String {
        format!(
            "<script>ytcfg.set({{\"INNERTUBE_API_KEY\":\"AIzaKey\",\"INNERTUBE_CONTEXT\":{{\"client\":{{\"clientName\":\"WEB\",\"clientVersion\":\"2.2026\"}}}}}});</script>\
             <script>window[\"ytInitialData\"] = {data};</script>"
        )
    }

    #[test]
    fn the_chat_page_opens_on_every_message_view() {
        let data = json!({"contents": {"liveChatRenderer": {
            "continuations": [{"invalidationContinuationData": {"continuation": "top", "timeoutMs": 10000}}],
            "header": {"liveChatHeaderRenderer": {"viewSelector": {"sortFilterSubMenuRenderer": {"subMenuItems": [
                {"title": "Top chat", "continuation": {"reloadContinuationData": {"continuation": "top-reload"}}},
                {"title": "Live chat", "continuation": {"reloadContinuationData": {"continuation": "all-reload"}}}
            ]}}}},
            "actions": [{"addChatItemAction": {}}]
        }}});
        let Page::Chat {
            api_key,
            context,
            start,
        } = page(&html(&data)).unwrap()
        else {
            panic!("no chat");
        };
        assert_eq!(api_key.as_deref(), Some("AIzaKey"));
        assert_eq!(context["client"]["clientVersion"], "2.2026");
        assert_eq!(start.token, "all-reload");
        assert!(start.reload);
    }

    #[test]
    fn closed_and_replayed_chats_are_told_apart() {
        let closed = json!({"contents": {"messageRenderer": {"text": {"runs": [{"text": "Chat is disabled"}]}}}});
        assert_eq!(page(&html(&closed)).unwrap(), Page::Unavailable);
        let replay = json!({"contents": {"liveChatRenderer": {"isReplay": true}}});
        assert_eq!(page(&html(&replay)).unwrap(), Page::Replay);
        assert_eq!(page("<html></html>").unwrap_err(), "protocol-changed");
    }
}
