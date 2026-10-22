//! NanaType on Steam: launching through the Steam client and following the
//! user's Workshop subscriptions. Steamworks callbacks run on one worker
//! thread; the app sees plain data ([`Event`]) and never a Steamworks type.
//!
//! Without the `steamworks` feature nothing links the SDK: [`bootstrap`]
//! reports [`Bootstrap::Disabled`] and a [`SteamHandle`] does nothing.

use std::path::PathBuf;

include!(concat!(env!("OUT_DIR"), "/ids.rs"));

/// A debug build talks to the Steam client only when this is `1`, so
/// `cargo run` never needs Steam. Release Steam builds always do.
pub const OPT_IN_ENV: &str = "NANATYPE_STEAM";

/// The Workshop's browse page, narrowed to `tag` when given.
pub fn browse_url(tag: Option<&str>) -> String {
    match tag {
        Some(tag) => format!(
            "https://steamcommunity.com/workshop/browse/?appid={APP_ID}&requiredtags%5B%5D={}",
            encode(tag)
        ),
        None => format!("https://steamcommunity.com/app/{APP_ID}/workshop/"),
    }
}

/// A Workshop item's page.
pub fn item_url(item: u64) -> String {
    format!("https://steamcommunity.com/sharedfiles/filedetails/?id={item}")
}

/// `url` opened by the Steam client instead of the default browser.
pub fn client_url(url: &str) -> String {
    format!("steam://openurl/{url}")
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// How a subscribed item stands on this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemState {
    Installed,
    /// Installed, with a newer version waiting.
    NeedsUpdate,
    Downloading,
    /// Queued for download.
    Pending,
    NotInstalled,
}

/// A subscribed Workshop item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub id: u64,
    pub state: ItemState,
    /// Bytes downloaded and in total, while downloading.
    pub progress: Option<(u64, u64)>,
    /// Where Steam installed it.
    pub folder: Option<PathBuf>,
    /// When the installed copy was last updated (Unix seconds).
    pub timestamp: u32,
}

/// What the Workshop says about an item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Details {
    pub id: u64,
    pub title: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The subscriptions, whenever any of them changes.
    Items(Vec<Item>),
    Details(Vec<Details>),
    Unsubscribed {
        item: u64,
        result: Result<(), String>,
    },
}

/// The Steam client, connected.
pub struct Steam {
    #[cfg(feature = "steamworks")]
    client: steamworks::Client,
}

impl Steam {
    /// The subscriptions as they stand now, without waiting for the worker:
    /// enough to read installed items at startup.
    pub fn items(&self) -> Vec<Item> {
        #[cfg(feature = "steamworks")]
        {
            worker::snapshot(&self.client)
        }
        #[cfg(not(feature = "steamworks"))]
        {
            Vec::new()
        }
    }
}

pub enum Bootstrap {
    /// Not a Steam build, or not asked to use Steam.
    Disabled,
    Ready(Steam),
    /// Steam is not running, or does not let this account run the app.
    Unavailable(String),
}

/// Whether this build talks to Steam at all: a Steam build, and in debug
/// only with [`OPT_IN_ENV`] set.
pub fn wanted() -> bool {
    cfg!(feature = "steamworks")
        && (!cfg!(debug_assertions)
            || std::env::var(OPT_IN_ENV).is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true")))
}

/// Relaunch through Steam when a release build was started directly. Call
/// before anything else claims the process (the single-instance lock), so
/// the copy Steam starts is the one that runs.
pub fn restart_through_steam() -> bool {
    #[cfg(feature = "steamworks")]
    {
        wanted()
            && !cfg!(debug_assertions)
            && steamworks::restart_app_if_necessary(steamworks::AppId(APP_ID))
    }
    #[cfg(not(feature = "steamworks"))]
    {
        false
    }
}

/// Connect to the Steam client. Sets `SteamAppId` in this process's
/// environment, so call it before other threads start.
pub fn bootstrap() -> Bootstrap {
    if !wanted() {
        return Bootstrap::Disabled;
    }
    #[cfg(feature = "steamworks")]
    {
        match steamworks::Client::init_app(steamworks::AppId(APP_ID)) {
            Ok(client) => Bootstrap::Ready(Steam { client }),
            Err(error) => Bootstrap::Unavailable(error.to_string()),
        }
    }
    #[cfg(not(feature = "steamworks"))]
    {
        Bootstrap::Disabled
    }
}

/// The app's side of the Steam worker.
pub struct SteamHandle {
    #[cfg(feature = "steamworks")]
    inner: Option<(steamworks::Client, crossbeam_channel::Sender<Command>)>,
}

#[cfg_attr(not(feature = "steamworks"), allow(dead_code))]
enum Command {
    Unsubscribe(u64),
    Download(u64),
}

impl SteamHandle {
    /// Start following the subscriptions; `steam` is `None` when Steam is
    /// disabled or unavailable, and the handle then does nothing.
    pub fn start(steam: Option<Steam>, post: impl Fn(Event) + Send + Sync + 'static) -> Self {
        #[cfg(feature = "steamworks")]
        {
            Self {
                inner: steam.map(|steam| {
                    let client = steam.client.clone();
                    (client, worker::spawn(steam.client, post))
                }),
            }
        }
        #[cfg(not(feature = "steamworks"))]
        {
            let _ = (steam, post);
            Self {}
        }
    }

    /// Connected to Steam.
    pub fn available(&self) -> bool {
        #[cfg(feature = "steamworks")]
        {
            self.inner.is_some()
        }
        #[cfg(not(feature = "steamworks"))]
        {
            false
        }
    }

    /// Show `url` in the Steam overlay. False when the overlay is off (or
    /// Steam is not connected); the caller then opens it another way.
    pub fn overlay(&self, url: &str) -> bool {
        #[cfg(feature = "steamworks")]
        {
            let Some((client, _)) = &self.inner else {
                return false;
            };
            if !client.utils().is_overlay_enabled() {
                return false;
            }
            client.friends().activate_game_overlay_to_web_page(url);
            true
        }
        #[cfg(not(feature = "steamworks"))]
        {
            let _ = url;
            false
        }
    }

    /// Unsubscribe; Steam removes the files when the app exits.
    pub fn unsubscribe(&self, item: u64) {
        self.send(Command::Unsubscribe(item));
    }

    /// Download a subscribed item again (after a failure, say).
    pub fn download(&self, item: u64) {
        self.send(Command::Download(item));
    }

    fn send(&self, command: Command) {
        #[cfg(feature = "steamworks")]
        if let Some((_, tx)) = &self.inner {
            let _ = tx.send(command);
        }
        #[cfg(not(feature = "steamworks"))]
        let _ = command;
    }
}

#[cfg(feature = "steamworks")]
mod worker {
    use super::{Command, Details, Event, Item, ItemState};
    use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, unbounded};
    use std::collections::HashSet;
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use steamworks::{Client, PublishedFileId};

    /// Steam answers calls through callbacks, which only run when pumped.
    const PUMP: Duration = Duration::from_millis(100);
    const POLL: Duration = Duration::from_secs(1);
    const POLL_DOWNLOADING: Duration = Duration::from_millis(250);
    /// Items per Workshop details request.
    const PAGE: usize = 50;

    pub fn spawn(client: Client, post: impl Fn(Event) + Send + Sync + 'static) -> Sender<Command> {
        let (tx, rx) = unbounded();
        let post: Arc<dyn Fn(Event) + Send + Sync> = Arc::new(post);
        std::thread::Builder::new()
            .name("nanatype-steam".into())
            .spawn(move || run(client, rx, post))
            .expect("spawn Steam worker");
        tx
    }

    fn run(client: Client, rx: Receiver<Command>, post: Arc<dyn Fn(Event) + Send + Sync>) {
        let mut items: Vec<Item> = Vec::new();
        let mut described: HashSet<u64> = HashSet::new();
        // Downloads asked for, per installed timestamp, so each is asked once.
        let mut requested: HashSet<(u64, u32)> = HashSet::new();
        let mut next_poll = Instant::now();
        loop {
            match rx.recv_timeout(PUMP) {
                Ok(Command::Unsubscribe(item)) => {
                    let post = post.clone();
                    client
                        .ugc()
                        .unsubscribe_item(PublishedFileId(item), move |result| {
                            post(Event::Unsubscribed {
                                item,
                                result: result.map_err(|e| e.to_string()),
                            });
                        });
                    next_poll = Instant::now();
                }
                Ok(Command::Download(item)) => {
                    client.ugc().download_item(PublishedFileId(item), true);
                    next_poll = Instant::now();
                }
                Err(RecvTimeoutError::Timeout) => {}
                // The app is gone.
                Err(RecvTimeoutError::Disconnected) => return,
            }
            client.run_callbacks();
            if Instant::now() < next_poll {
                continue;
            }
            let now = snapshot(&client);
            for item in &now {
                if matches!(item.state, ItemState::NotInstalled | ItemState::NeedsUpdate)
                    && requested.insert((item.id, item.timestamp))
                {
                    client.ugc().download_item(PublishedFileId(item.id), false);
                }
            }
            let fresh: Vec<u64> = now
                .iter()
                .map(|i| i.id)
                .filter(|id| !described.contains(id))
                .collect();
            if !fresh.is_empty() {
                described.extend(fresh.iter().copied());
                describe(&client, fresh, post.clone());
            }
            let downloading = now
                .iter()
                .any(|i| matches!(i.state, ItemState::Downloading | ItemState::Pending));
            if now != items {
                items = now;
                post(Event::Items(items.clone()));
            }
            next_poll = Instant::now() + if downloading { POLL_DOWNLOADING } else { POLL };
        }
    }

    pub(super) fn snapshot(client: &Client) -> Vec<Item> {
        use steamworks::ItemState as S;
        let ugc = client.ugc();
        ugc.subscribed_items(false)
            .into_iter()
            .map(|id| {
                let state = ugc.item_state(id);
                let install = ugc.item_install_info(id);
                let state = if state.contains(S::DOWNLOADING) {
                    ItemState::Downloading
                } else if state.contains(S::DOWNLOAD_PENDING) {
                    ItemState::Pending
                } else if state.contains(S::NEEDS_UPDATE) {
                    ItemState::NeedsUpdate
                } else if state.contains(S::INSTALLED) && install.is_some() {
                    ItemState::Installed
                } else {
                    ItemState::NotInstalled
                };
                Item {
                    id: id.0,
                    progress: matches!(state, ItemState::Downloading)
                        .then(|| ugc.item_download_info(id))
                        .flatten(),
                    state,
                    timestamp: install.as_ref().map_or(0, |i| i.timestamp),
                    folder: install.map(|i| i.folder.into()),
                }
            })
            .collect()
    }

    /// Ask the Workshop for titles and tags; answers arrive as callbacks.
    fn describe(client: &Client, ids: Vec<u64>, post: Arc<dyn Fn(Event) + Send + Sync>) {
        for page in ids.chunks(PAGE) {
            let ids = page.iter().map(|id| PublishedFileId(*id)).collect();
            let Ok(query) = client.ugc().query_items(ids) else {
                continue;
            };
            let post = post.clone();
            query.fetch(move |results| {
                let Ok(results) = results else {
                    return;
                };
                let details = results
                    .iter()
                    .flatten()
                    .map(|r| Details {
                        id: r.published_file_id.0,
                        title: r.title,
                        tags: r.tags,
                    })
                    .collect();
                post(Event::Details(details));
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workshop_pages_name_this_app_and_escape_tags() {
        assert_eq!(
            browse_url(None),
            format!("https://steamcommunity.com/app/{APP_ID}/workshop/")
        );
        assert_eq!(
            browse_url(Some("Sound Pack")),
            format!(
                "https://steamcommunity.com/workshop/browse/?appid={APP_ID}&requiredtags%5B%5D=Sound%20Pack"
            )
        );
        assert_eq!(
            client_url(&item_url(42)),
            "steam://openurl/https://steamcommunity.com/sharedfiles/filedetails/?id=42"
        );
    }

    #[test]
    fn a_build_without_steamworks_never_starts_steam() {
        if cfg!(feature = "steamworks") {
            return;
        }
        assert!(!wanted());
        assert!(!restart_through_steam());
        assert!(matches!(bootstrap(), Bootstrap::Disabled));
        let handle = SteamHandle::start(None, |_| {});
        assert!(!handle.available() && !handle.overlay("https://example.com"));
        handle.unsubscribe(1);
    }
}
