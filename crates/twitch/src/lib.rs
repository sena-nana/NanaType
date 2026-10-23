//! Twitch live chat. Reading joins the channel's IRC anonymously; sending
//! signs in with the OAuth device flow and posts through Helix.
mod api;
mod credentials;
pub mod diag;
mod handle;
mod irc;
mod socket;

pub use handle::TwitchHandle;
use nanatype_model::LineId;
pub use nanatype_model::outgoing::DeliveryState;

/// Twitch's chat message limit, in characters.
pub const MAX_CHARS: usize = 500;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    pub enabled: bool,
    /// A channel login or its twitch.tv address.
    pub channel: String,
}

/// The signed-in account.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Profile {
    pub user_id: String,
    pub login: String,
    pub name: String,
    pub avatar: String,
}

/// The joined channel; `id` is the broadcaster id Helix sends to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Room {
    pub login: String,
    pub id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum AccountState {
    #[default]
    Restoring,
    SignedOut,
    /// Waiting for the user to enter `code` at `uri`.
    Pending {
        code: String,
        uri: String,
    },
    Ready(Profile),
    Expired,
    /// A stable code (`login-expired`, `protected-storage-unavailable`) or Twitch's message.
    Failed(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ConnectionState {
    #[default]
    Off,
    Connecting,
    Connected(Room),
    Error(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Author {
    pub user_id: String,
    pub name: String,
    pub broadcaster: bool,
    pub moderator: bool,
}

pub use nanatype_model::ChatRun as Run;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content {
    Text(Vec<Run>),
    Cheer {
        bits: u64,
        runs: Vec<Run>,
    },
    /// Subscriptions, gifts, raids and announcements; `text` is Twitch's own line.
    Notice {
        text: String,
        runs: Vec<Run>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    pub line: LineId,
    pub room: String,
    pub uid: String,
    pub chunks: Vec<String>,
    pub completed: usize,
    pub state: DeliveryState,
    pub detail: String,
    /// This delivery owns the paused queue head.
    pub blocked: bool,
    /// Twitch's message ids, one per sent chunk.
    pub receipts: Vec<String>,
}
impl Delivery {
    pub fn remaining(&self) -> String {
        self.chunks[self.completed..].join("")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Account(AccountState),
    Connection(ConnectionState),
    Chat {
        channel: String,
        event: ChatEvent,
    },
    /// A moderator removed this message.
    Deleted {
        channel: String,
        id: String,
    },
    Delivery(Delivery),
    Warning(String),
}

#[derive(Debug, Clone)]
pub struct SessionEvent {
    pub epoch: u64,
    pub event: Event,
}

/// Accept a channel login or its twitch.tv address.
pub fn channel(input: &str) -> Result<String, String> {
    let text = input.trim();
    let login = if text.starts_with("https://") || text.starts_with("http://") {
        let url = url::Url::parse(text).map_err(|_| "invalid-channel")?;
        if !matches!(
            url.host_str(),
            Some("twitch.tv" | "www.twitch.tv" | "m.twitch.tv")
        ) || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err("invalid-channel".into());
        }
        let mut segments = url
            .path_segments()
            .into_iter()
            .flatten()
            .filter(|s| !s.is_empty());
        let first = segments.next().unwrap_or_default();
        // https://www.twitch.tv/popout/<login>/chat
        if first == "popout" {
            segments.next().unwrap_or_default().to_owned()
        } else {
            first.to_owned()
        }
    } else {
        text.trim_start_matches('#').to_owned()
    };
    let login = login.to_ascii_lowercase();
    if !(3..=25).contains(&login.len())
        || !login
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err("invalid-channel".into());
    }
    Ok(login)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_input_cannot_navigate_outside_twitch() {
        assert_eq!(channel(" Nana_Live ").unwrap(), "nana_live");
        assert_eq!(channel("#nana").unwrap(), "nana");
        assert_eq!(channel("https://www.twitch.tv/nana?sr=a").unwrap(), "nana");
        assert_eq!(
            channel("https://www.twitch.tv/popout/nana/chat?popout=").unwrap(),
            "nana"
        );
        for bad in [
            "",
            "ab",
            "https://twitch.tv.evil.test/nana",
            "https://user@twitch.tv/nana",
            "nana\r\nPRIVMSG #x :hi",
            "../nana",
            "a_very_long_channel_name_over_25",
        ] {
            assert!(channel(bad).is_err(), "{bad:?}");
        }
    }
}
