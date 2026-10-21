//! Twitch chat over IRC-over-WebSocket, joined anonymously: reading needs no
//! account. Lines are parsed by pure functions; the socket loop is below them.
use crate::{Author, ChatEvent, Content, Event, Room, Run};
use std::collections::HashMap;

pub(crate) const ENDPOINT: &str = "wss://irc-ws.chat.twitch.tv:443";

/// One IRC line with its IRCv3 tags.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Line {
    pub tags: HashMap<String, String>,
    /// The nick part of the prefix.
    pub nick: String,
    pub command: String,
    pub params: Vec<String>,
}
impl Line {
    fn tag(&self, key: &str) -> &str {
        self.tags.get(key).map_or("", String::as_str)
    }
}

pub fn parse(raw: &str) -> Option<Line> {
    let mut rest = raw.trim_end_matches(['\r', '\n']);
    let mut line = Line::default();
    if let Some(tagged) = rest.strip_prefix('@') {
        let (tags, after) = tagged.split_once(' ')?;
        for pair in tags.split(';') {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            line.tags.insert(key.to_owned(), unescape(value));
        }
        rest = after;
    }
    if let Some(prefixed) = rest.strip_prefix(':') {
        let (prefix, after) = prefixed.split_once(' ')?;
        line.nick = prefix.split('!').next().unwrap_or_default().to_owned();
        rest = after;
    }
    let (command, mut rest) = rest.split_once(' ').unwrap_or((rest, ""));
    if command.is_empty() {
        return None;
    }
    line.command = command.to_owned();
    while !rest.is_empty() {
        if let Some(trailing) = rest.strip_prefix(':') {
            line.params.push(trailing.to_owned());
            break;
        }
        let (param, after) = rest.split_once(' ').unwrap_or((rest, ""));
        if !param.is_empty() {
            line.params.push(param.to_owned());
        }
        rest = after;
    }
    Some(line)
}

/// IRCv3 tag value escaping.
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some(':') => out.push(';'),
            Some('s') => out.push(' '),
            Some('r') => out.push('\r'),
            Some('n') => out.push('\n'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// What a line means for the chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Incoming {
    Ping(String),
    Reconnect,
    Joined(Room),
    Event(Event),
    /// The channel cannot be joined (suspended, banned, nonexistent).
    Refused(String),
    Other,
}

pub fn interpret(line: &Line, channel: &str) -> Incoming {
    let here = || line.params.first().map(String::as_str) == Some(&format!("#{channel}"));
    match line.command.as_str() {
        "PING" => Incoming::Ping(line.params.first().cloned().unwrap_or_default()),
        "RECONNECT" => Incoming::Reconnect,
        "ROOMSTATE" if here() && !line.tag("room-id").is_empty() => Incoming::Joined(Room {
            login: channel.to_owned(),
            id: line.tag("room-id").to_owned(),
        }),
        "PRIVMSG" if here() => {
            let text = line.params.get(1).map_or("", String::as_str);
            let text = text
                .strip_prefix("\u{1}ACTION ")
                .and_then(|t| t.strip_suffix('\u{1}'))
                .unwrap_or(text);
            let runs = runs(text, line.tag("emotes"));
            let content = match line.tag("bits").parse::<u64>() {
                Ok(bits) if bits > 0 => Content::Cheer { bits, runs },
                _ => Content::Text(runs),
            };
            chat(line, channel, content)
        }
        "USERNOTICE" if here() => {
            let text = line.params.get(1).map_or("", String::as_str);
            chat(
                line,
                channel,
                Content::Notice {
                    text: line.tag("system-msg").trim().to_owned(),
                    runs: runs(text, line.tag("emotes")),
                },
            )
        }
        "CLEARMSG" if here() && !line.tag("target-msg-id").is_empty() => {
            Incoming::Event(Event::Deleted {
                channel: channel.to_owned(),
                id: line.tag("target-msg-id").to_owned(),
            })
        }
        "NOTICE" if here() => match line.tag("msg-id") {
            id @ ("msg_channel_suspended" | "msg_banned" | "tos_ban" | "msg_room_not_found") => {
                Incoming::Refused(id.to_owned())
            }
            _ => Incoming::Other,
        },
        _ => Incoming::Other,
    }
}

fn chat(line: &Line, channel: &str, content: Content) -> Incoming {
    let badges = line.tag("badges");
    let has = |name: &str| badges.split(',').any(|b| b.split('/').next() == Some(name));
    let name = [line.tag("display-name"), line.tag("login"), &line.nick]
        .into_iter()
        .find(|n| !n.is_empty())
        .unwrap_or_default()
        .to_owned();
    Incoming::Event(Event::Chat {
        channel: channel.to_owned(),
        event: ChatEvent {
            id: line.tag("id").to_owned(),
            author: Author {
                user_id: line.tag("user-id").to_owned(),
                name,
                broadcaster: has("broadcaster"),
                moderator: has("moderator"),
            },
            content,
            at: line.tag("tmi-sent-ts").parse().unwrap_or(0),
        },
    })
}

/// Split `text` at the emote ranges, which count Unicode scalars.
fn runs(text: &str, emotes: &str) -> Vec<Run> {
    let mut ranges: Vec<(usize, usize, &str)> = emotes
        .split('/')
        .filter_map(|entry| entry.split_once(':'))
        .flat_map(|(id, spans)| {
            spans.split(',').filter_map(move |span| {
                let (start, end) = span.split_once('-')?;
                Some((start.parse().ok()?, end.parse().ok()?, id))
            })
        })
        .collect();
    ranges.sort_unstable();
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut at = 0;
    for (start, end, id) in ranges {
        if start < at || end < start || end >= chars.len() {
            continue;
        }
        if start > at {
            out.push(Run::Text(chars[at..start].iter().collect()));
        }
        out.push(Run::Emote {
            alt: chars[start..=end].iter().collect(),
            url: format!("https://static-cdn.jtvnw.net/emoticons/v2/{id}/default/dark/2.0"),
        });
        at = end + 1;
    }
    if at < chars.len() {
        out.push(Run::Text(chars[at..].iter().collect()));
    }
    out
}

/// A random anonymous nick, as Twitch's own embeds use.
pub(crate) fn anonymous_nick() -> String {
    let mut bytes = [0u8; 4];
    let _ = getrandom::fill(&mut bytes);
    format!("justinfan{}", 10_000 + u32::from_le_bytes(bytes) % 89_999)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRIVMSG: &str = "@badge-info=subscriber/8;badges=moderator/1,subscriber/6;color=#1E90FF;display-name=Nana\\sFan;emotes=25:0-4,12-16/1902:6-10;id=abc-1;room-id=1337;tmi-sent-ts=1700000000123;user-id=42 :nanafan!nanafan@nanafan.tmi.twitch.tv PRIVMSG #nana :Kappa Keepo Kappa\r\n";

    #[test]
    fn lines_split_into_tags_prefix_command_and_params() {
        let line = parse(PRIVMSG).unwrap();
        assert_eq!(line.command, "PRIVMSG");
        assert_eq!(line.nick, "nanafan");
        assert_eq!(line.params, ["#nana", "Kappa Keepo Kappa"]);
        assert_eq!(line.tag("display-name"), "Nana Fan");
        assert_eq!(
            parse("PING :tmi.twitch.tv").unwrap().params,
            ["tmi.twitch.tv"]
        );
        assert_eq!(unescape(r"a\:b\sc\\d\ne\"), "a;b c\\d\ne");
        assert!(parse("").is_none());
    }

    #[test]
    fn messages_carry_badges_and_emotes() {
        let Incoming::Event(Event::Chat { channel, event }) =
            interpret(&parse(PRIVMSG).unwrap(), "nana")
        else {
            panic!("not chat");
        };
        assert_eq!(channel, "nana");
        assert_eq!(event.id, "abc-1");
        assert_eq!(event.at, 1_700_000_000_123);
        assert_eq!(
            event.author,
            Author {
                user_id: "42".into(),
                name: "Nana Fan".into(),
                broadcaster: false,
                moderator: true,
            }
        );
        let emote = |alt: &str, id: &str| Run::Emote {
            alt: alt.into(),
            url: format!("https://static-cdn.jtvnw.net/emoticons/v2/{id}/default/dark/2.0"),
        };
        assert_eq!(
            event.content,
            Content::Text(vec![
                emote("Kappa", "25"),
                Run::Text(" ".into()),
                emote("Keepo", "1902"),
                Run::Text(" ".into()),
                emote("Kappa", "25"),
            ])
        );
        // Another channel's line on a shared connection is not ours.
        assert_eq!(
            interpret(&parse(PRIVMSG).unwrap(), "other"),
            Incoming::Other
        );
    }

    #[test]
    fn emote_ranges_count_characters_not_bytes() {
        assert_eq!(
            runs("😀 Kappa!", "25:2-6"),
            vec![
                Run::Text("😀 ".into()),
                Run::Emote {
                    alt: "Kappa".into(),
                    url: "https://static-cdn.jtvnw.net/emoticons/v2/25/default/dark/2.0".into()
                },
                Run::Text("!".into()),
            ]
        );
        // Ranges past the text are ignored rather than panicking.
        assert_eq!(runs("hi", "25:0-9"), vec![Run::Text("hi".into())]);
        assert_eq!(runs("", ""), vec![]);
    }

    #[test]
    fn cheers_actions_notices_and_deletions_are_read() {
        let cheer = parse("@bits=100;id=c1;user-id=1;display-name=A :a!a@a PRIVMSG #nana :\u{1}ACTION cheer100 wow\u{1}").unwrap();
        let Incoming::Event(Event::Chat { event, .. }) = interpret(&cheer, "nana") else {
            panic!()
        };
        assert_eq!(
            event.content,
            Content::Cheer {
                bits: 100,
                runs: vec![Run::Text("cheer100 wow".into())]
            }
        );
        let sub = parse("@msg-id=resub;login=fan;display-name=;system-msg=fan\\ssubscribed\\sfor\\s3\\smonths.;id=s1;user-id=9 :tmi.twitch.tv USERNOTICE #nana :still here").unwrap();
        let Incoming::Event(Event::Chat { event, .. }) = interpret(&sub, "nana") else {
            panic!()
        };
        assert_eq!(event.author.name, "fan");
        assert_eq!(
            event.content,
            Content::Notice {
                text: "fan subscribed for 3 months.".into(),
                runs: vec![Run::Text("still here".into())]
            }
        );
        let clear =
            parse("@login=x;target-msg-id=abc-1 :tmi.twitch.tv CLEARMSG #nana :bad").unwrap();
        assert_eq!(
            interpret(&clear, "nana"),
            Incoming::Event(Event::Deleted {
                channel: "nana".into(),
                id: "abc-1".into()
            })
        );
        let room = parse("@emote-only=0;room-id=1337 :tmi.twitch.tv ROOMSTATE #nana").unwrap();
        assert_eq!(
            interpret(&room, "nana"),
            Incoming::Joined(Room {
                login: "nana".into(),
                id: "1337".into()
            })
        );
        let refused = parse("@msg-id=msg_channel_suspended :tmi.twitch.tv NOTICE #nana :This channel does not exist or has been suspended.").unwrap();
        assert_eq!(
            interpret(&refused, "nana"),
            Incoming::Refused("msg_channel_suspended".into())
        );
        assert!(anonymous_nick().starts_with("justinfan"));
    }
}
