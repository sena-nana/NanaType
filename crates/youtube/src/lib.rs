//! YouTube live chat, read the way the public web chat reads it: no API key,
//! no account and no quota. Receiving only; see docs/youtube.md for sending.
mod handle;
mod innertube;
pub mod protocol;

pub use handle::YoutubeHandle;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    pub enabled: bool,
    /// A video, a channel or a handle, as typed by the user.
    pub target: String,
}

/// What the user pointed at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Video(String),
    /// Without the leading `@`.
    Handle(String),
    Channel(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ConnectionState {
    #[default]
    Off,
    /// Looking up the channel's current stream.
    Resolving,
    /// The channel is not live, or the chat is not open yet.
    Waiting,
    Connecting,
    Connected {
        video: String,
    },
    /// The stream finished.
    Ended,
    Disconnected,
    Error(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Author {
    pub channel_id: String,
    pub name: String,
    pub avatar: String,
    pub owner: bool,
    pub moderator: bool,
}

/// Channel emoji are pictures; standard emoji arrive as text.
pub use nanatype_model::ChatRun as Run;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content {
    Text(Vec<Run>),
    /// A Super Chat; `amount` is YouTube's own text, e.g. "¥1,000".
    Paid {
        amount: String,
        /// ARGB.
        color: u32,
        runs: Vec<Run>,
    },
    /// A Super Sticker.
    Sticker {
        amount: String,
        color: u32,
        url: String,
        alt: String,
    },
    /// A new member or a membership milestone.
    Membership {
        header: String,
        runs: Vec<Run>,
    },
    GiftMembership {
        count: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatEvent {
    pub id: String,
    pub author: Author,
    pub content: Content,
    /// Unix milliseconds.
    pub at: u64,
}

#[derive(Debug, Clone)]
pub enum Event {
    Connection(ConnectionState),
    Chat {
        video: String,
        event: ChatEvent,
    },
    /// A moderator or the author removed this message.
    Deleted {
        video: String,
        id: String,
    },
}

#[derive(Debug, Clone)]
pub struct SessionEvent {
    pub epoch: u64,
    pub event: Event,
}

const HOSTS: [&str; 4] = [
    "youtube.com",
    "www.youtube.com",
    "m.youtube.com",
    "youtu.be",
];

/// Accept YouTube addresses, `@handle`s, channel ids and video ids; nothing
/// else can reach the network.
pub fn target(input: &str) -> Result<Target, String> {
    let text = input.trim();
    let invalid = || "invalid-target".to_owned();
    if !(text.starts_with("https://") || text.starts_with("http://")) {
        return bare(text).ok_or_else(invalid);
    }
    let url = url::Url::parse(text).map_err(|_| invalid())?;
    let host = url.host_str().unwrap_or_default();
    if !HOSTS.contains(&host) || !url.username().is_empty() || url.password().is_some() {
        return Err(invalid());
    }
    let segments: Vec<String> = url
        .path_segments()
        .map(|s| s.filter(|s| !s.is_empty()).map(decode).collect())
        .unwrap_or_default();
    let first = segments.first().map(String::as_str).unwrap_or_default();
    let found = if host == "youtu.be" {
        video_id(first).map(Target::Video)
    } else {
        match first {
            "watch" | "live_chat" => url
                .query_pairs()
                .find(|(k, _)| k == "v")
                .and_then(|(_, v)| video_id(&v))
                .map(Target::Video),
            "live" => segments.get(1).and_then(|s| video_id(s)).map(Target::Video),
            "channel" => segments
                .get(1)
                .and_then(|s| channel_id(s))
                .map(Target::Channel),
            _ => first.strip_prefix('@').and_then(handle).map(Target::Handle),
        }
    };
    found.ok_or_else(invalid)
}

fn bare(text: &str) -> Option<Target> {
    if let Some(name) = text.strip_prefix('@') {
        return handle(name).map(Target::Handle);
    }
    channel_id(text)
        .map(Target::Channel)
        .or_else(|| video_id(text).map(Target::Video))
}
fn id_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}
fn video_id(text: &str) -> Option<String> {
    (text.len() == 11 && text.chars().all(id_char)).then(|| text.to_owned())
}
fn channel_id(text: &str) -> Option<String> {
    (text.len() == 24 && text.starts_with("UC") && text.chars().all(id_char))
        .then(|| text.to_owned())
}
fn handle(text: &str) -> Option<String> {
    let count = text.chars().count();
    ((3..=100).contains(&count)
        && text
            .chars()
            .all(|c| c.is_alphanumeric() || "._-·".contains(c)))
    .then(|| text.to_owned())
}
/// Percent-decode one path segment; invalid input is kept as typed.
fn decode(segment: &str) -> String {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(byte) = segment
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| segment.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_cover_every_address_youtube_shares() {
        let video = Target::Video("dQw4w9WgXcQ".into());
        for input in [
            "dQw4w9WgXcQ",
            " https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=3 ",
            "https://youtu.be/dQw4w9WgXcQ?si=x",
            "https://m.youtube.com/live/dQw4w9WgXcQ?feature=share",
            "https://www.youtube.com/live_chat?is_popout=1&v=dQw4w9WgXcQ",
        ] {
            assert_eq!(target(input).unwrap(), video, "{input}");
        }
        assert_eq!(
            target("https://www.youtube.com/@nana_ch/live").unwrap(),
            Target::Handle("nana_ch".into())
        );
        assert_eq!(
            target("@ナナ_ch").unwrap(),
            Target::Handle("ナナ_ch".into())
        );
        assert_eq!(
            target("https://www.youtube.com/@%E3%83%8A%E3%83%8A%E3%83%8A").unwrap(),
            Target::Handle("ナナナ".into())
        );
        assert_eq!(
            target("https://youtube.com/channel/UC0123456789abcdefghijAB/live").unwrap(),
            Target::Channel("UC0123456789abcdefghijAB".into())
        );
    }

    #[test]
    fn targets_cannot_navigate_outside_youtube() {
        for input in [
            "",
            "https://www.youtube.com.evil.test/watch?v=dQw4w9WgXcQ",
            "https://user@www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://www.youtube.com/watch?v=../../x",
            "https://www.youtube.com/results?search_query=x",
            "@a/../b",
            "../profile",
            "short",
        ] {
            assert!(target(input).is_err(), "{input}");
        }
    }
}
