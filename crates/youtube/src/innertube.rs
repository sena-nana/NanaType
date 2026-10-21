//! The requests the public web chat makes: find the stream, open its chat
//! page, then poll `get_live_chat` with the continuation YouTube hands back.
use crate::{
    ConnectionState, Event, Target,
    handle::Session,
    protocol::{self, Action, Continuation, Page},
};
use serde_json::{Value, json};
use std::{
    collections::{HashSet, VecDeque},
    time::Duration,
};

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/146.0.0.0 Safari/537.36";
const ORIGIN: &str = "https://www.youtube.com";
/// Declines optional cookies, so no consent interstitial answers instead of the page.
const CONSENT: &str = "SOCS=CAI";
const WAIT_OFFLINE: Duration = Duration::from_secs(60);
const RETRY: Duration = Duration::from_secs(3);

pub(crate) trait Transport {
    fn page(&mut self, url: &str) -> Result<String, String>;
    fn post(&mut self, url: &str, body: &Value) -> Result<Value, String>;
}

pub(crate) struct Client {
    agent: ureq::Agent,
}
impl Client {
    pub fn new() -> Self {
        Self {
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(10)))
                .max_redirects(3)
                .http_status_as_error(false)
                .build()
                .into(),
        }
    }
}
fn read(result: Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> Result<Vec<u8>, String> {
    let mut response = result.map_err(|_| "transport-failed")?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(if status == 404 {
            "not-found".into()
        } else {
            "transport-failed".into()
        });
    }
    // Redirects may only stay on YouTube (never the consent site or a login page).
    use ureq::ResponseExt;
    if response.get_uri().host() != Some("www.youtube.com") {
        return Err("redirected".into());
    }
    response
        .body_mut()
        .with_config()
        .limit(8 * 1024 * 1024)
        .read_to_vec()
        .map_err(|_| "transport-failed".into())
}
impl Transport for Client {
    fn page(&mut self, url: &str) -> Result<String, String> {
        let bytes = read(
            self.agent
                .get(url)
                .header("User-Agent", USER_AGENT)
                .header("Accept-Language", "en-US,en;q=0.9")
                .header("Cookie", CONSENT)
                .call(),
        )?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
    fn post(&mut self, url: &str, body: &Value) -> Result<Value, String> {
        let bytes = read(
            self.agent
                .post(url)
                .header("User-Agent", USER_AGENT)
                .header("Content-Type", "application/json")
                .header("Origin", ORIGIN)
                .header("Cookie", CONSENT)
                .send(body.to_string().as_bytes()),
        )?;
        serde_json::from_slice(&bytes).map_err(|_| "protocol-changed".into())
    }
}

/// The video a channel is streaming now; `None` while it is offline.
fn resolve(transport: &mut dyn Transport, target: &Target) -> Result<Option<String>, String> {
    let path = match target {
        Target::Video(id) => return Ok(Some(id.clone())),
        Target::Handle(handle) => {
            let mut url = url::Url::parse(ORIGIN).map_err(|_| "invalid-target")?;
            url.path_segments_mut()
                .map_err(|_| "invalid-target")?
                .push(&format!("@{handle}"))
                .push("live");
            return live_video(&transport.page(url.as_str())?);
        }
        Target::Channel(id) => format!("{ORIGIN}/channel/{id}/live"),
    };
    live_video(&transport.page(&path)?)
}
/// A channel's `/live` address renders the watch page only while it streams.
fn live_video(html: &str) -> Result<Option<String>, String> {
    const CANONICAL: &str = "<link rel=\"canonical\" href=\"";
    let Some(start) = html.find(CANONICAL).map(|i| i + CANONICAL.len()) else {
        return Err("protocol-changed".into());
    };
    let href = &html[start..start + html[start..].find('"').unwrap_or(0)];
    Ok(match crate::target(href) {
        Ok(Target::Video(id)) => Some(id),
        _ => None,
    })
}

struct Chat {
    url: String,
    context: Value,
    next: Continuation,
}
enum Opened {
    Chat(Chat),
    Unavailable,
    Replay,
}
fn open(transport: &mut dyn Transport, video: &str) -> Result<Opened, String> {
    let html = transport.page(&format!("{ORIGIN}/live_chat?is_popout=1&v={video}"))?;
    Ok(match protocol::page(&html)? {
        Page::Chat {
            api_key,
            context,
            start,
        } => {
            let mut url = format!("{ORIGIN}/youtubei/v1/live_chat/get_live_chat?prettyPrint=false");
            if let Some(key) = api_key {
                url.push_str("&key=");
                url.push_str(&key);
            }
            Opened::Chat(Chat {
                url,
                context,
                next: start,
            })
        }
        Page::Unavailable => Opened::Unavailable,
        Page::Replay => Opened::Replay,
    })
}

/// Ids already shown, so a reopened chat never repeats its backlog.
#[derive(Default)]
struct Seen {
    ids: HashSet<String>,
    order: VecDeque<String>,
}
impl Seen {
    const CAP: usize = 4096;
    fn insert(&mut self, id: &str) -> bool {
        if !self.ids.insert(id.to_owned()) {
            return false;
        }
        self.order.push_back(id.to_owned());
        if self.order.len() > Self::CAP
            && let Some(old) = self.order.pop_front()
        {
            self.ids.remove(&old);
        }
        true
    }
}

fn delay(failures: u32) -> Duration {
    Duration::from_secs((1u64 << failures.saturating_sub(1).min(5)).min(30))
}

pub(crate) fn run(session: &Session, target: &Target) -> Result<(), String> {
    run_with(session, target, &mut Client::new())
}

pub(crate) fn run_with(
    session: &Session,
    target: &Target,
    transport: &mut dyn Transport,
) -> Result<(), String> {
    let single = matches!(target, Target::Video(_));
    let mut seen = Seen::default();
    let mut failures = 0u32;
    let mut connected_once = false;
    while session.current() {
        let retry = |failures: &mut u32, error: String| -> Result<bool, String> {
            *failures = failures.saturating_add(1);
            if !connected_once && *failures >= 4 {
                return Err(error);
            }
            Ok(session.pause(delay(*failures)))
        };
        if !single {
            session.emit(Event::Connection(ConnectionState::Resolving));
        }
        let video = match resolve(transport, target) {
            Ok(Some(video)) => video,
            Ok(None) => {
                failures = 0;
                session.emit(Event::Connection(ConnectionState::Waiting));
                if !session.pause(WAIT_OFFLINE) {
                    return Ok(());
                }
                continue;
            }
            Err(error) if error == "not-found" => return Err("invalid-target".into()),
            Err(error) => {
                if !retry(&mut failures, error)? {
                    return Ok(());
                }
                continue;
            }
        };
        session.emit(Event::Connection(ConnectionState::Connecting));
        let mut chat = match open(transport, &video) {
            Ok(Opened::Chat(chat)) => chat,
            Ok(Opened::Unavailable) => {
                failures = 0;
                session.emit(Event::Connection(ConnectionState::Waiting));
                if !session.pause(WAIT_OFFLINE) {
                    return Ok(());
                }
                continue;
            }
            Ok(Opened::Replay) => {
                session.emit(Event::Connection(ConnectionState::Ended));
                if single || !session.pause(WAIT_OFFLINE) {
                    return Ok(());
                }
                continue;
            }
            Err(error) if error == "not-found" => return Err("invalid-target".into()),
            Err(error) => {
                if !retry(&mut failures, error)? {
                    return Ok(());
                }
                continue;
            }
        };
        session.emit(Event::Connection(ConnectionState::Connected {
            video: video.clone(),
        }));
        connected_once = true;
        failures = 0;
        let ended = poll(session, transport, &video, &mut chat, &mut seen);
        if !session.current() {
            return Ok(());
        }
        if ended {
            session.emit(Event::Connection(ConnectionState::Ended));
            if single || !session.pause(WAIT_OFFLINE) {
                return Ok(());
            }
        } else {
            session.emit(Event::Connection(ConnectionState::Disconnected));
            if !session.pause(RETRY) {
                return Ok(());
            }
        }
    }
    Ok(())
}

/// Poll until the chat closes (`true`) or keeps failing (`false`).
fn poll(
    session: &Session,
    transport: &mut dyn Transport,
    video: &str,
    chat: &mut Chat,
    seen: &mut Seen,
) -> bool {
    let mut errors = 0;
    while session.current() {
        let body = json!({"context": chat.context, "continuation": chat.next.token});
        let response = match transport.post(&chat.url, &body) {
            Ok(response) => response,
            Err(_) => {
                errors += 1;
                if errors >= 3 || !session.pause(RETRY) {
                    return false;
                }
                continue;
            }
        };
        errors = 0;
        let batch = protocol::batch(&response);
        // A reload answers with recent history, which the viewer has already seen.
        let backlog = chat.next.reload;
        for action in batch.actions {
            match action {
                Action::Chat(event) => {
                    if seen.insert(&event.id) && !backlog {
                        session.emit(Event::Chat {
                            video: video.to_owned(),
                            event,
                        });
                    }
                }
                Action::Deleted(id) => {
                    if !backlog {
                        session.emit(Event::Deleted {
                            video: video.to_owned(),
                            id,
                        });
                    }
                }
            }
        }
        let Some(next) = batch.next else {
            return true;
        };
        let wait = Duration::from_millis(next.timeout_ms.clamp(1000, 5000));
        chat.next = next;
        if !session.pause(wait) {
            return false;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Content, SessionEvent};
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Fake {
        pages: VecDeque<Result<String, String>>,
        posts: VecDeque<Value>,
        requested: Vec<String>,
        tokens: Vec<String>,
    }
    impl Transport for Fake {
        fn page(&mut self, url: &str) -> Result<String, String> {
            self.requested.push(url.to_owned());
            self.pages
                .pop_front()
                .unwrap_or(Err("transport-failed".into()))
        }
        fn post(&mut self, url: &str, body: &Value) -> Result<Value, String> {
            assert!(url.starts_with("https://www.youtube.com/youtubei/v1/live_chat/get_live_chat"));
            assert!(url.ends_with("&key=AIzaKey"));
            self.tokens
                .push(body["continuation"].as_str().unwrap().to_owned());
            assert_eq!(body["context"]["client"]["clientName"], "WEB");
            self.posts.pop_front().ok_or("transport-failed".into())
        }
    }
    fn chat_page() -> String {
        let data = json!({"contents": {"liveChatRenderer": {
            "continuations": [{"invalidationContinuationData": {"continuation": "top"}}],
            "header": {"liveChatHeaderRenderer": {"viewSelector": {"sortFilterSubMenuRenderer": {"subMenuItems": [
                {"continuation": {"reloadContinuationData": {"continuation": "top-reload"}}},
                {"continuation": {"reloadContinuationData": {"continuation": "all"}}}
            ]}}}}
        }}});
        format!(
            "ytcfg.set({{\"INNERTUBE_API_KEY\":\"AIzaKey\",\"INNERTUBE_CONTEXT\":{{\"client\":{{\"clientName\":\"WEB\"}}}}}});\
             var ytInitialData = {data};"
        )
    }
    fn message(id: &str, text: &str) -> Value {
        json!({"addChatItemAction": {"item": {"liveChatTextMessageRenderer": {
            "id": id, "message": {"runs": [{"text": text}]}, "authorName": {"simpleText": "v"}
        }}}})
    }
    fn response(actions: Vec<Value>, next: Option<&str>) -> Value {
        let continuations = next
            .map(|t| json!([{"timedContinuationData": {"continuation": t, "timeoutMs": 1}}]))
            .unwrap_or(json!([]));
        json!({"continuationContents": {"liveChatContinuation": {
            "actions": actions, "continuations": continuations
        }}})
    }
    fn session() -> (Session, Arc<Mutex<Vec<SessionEvent>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        (
            Session::fixture(move |e| sink.lock().unwrap().push(e)),
            events,
        )
    }

    #[test]
    fn a_video_chat_skips_its_backlog_and_ends_with_the_stream() {
        let mut fake = Fake::default();
        fake.pages.push_back(Ok(chat_page()));
        fake.posts
            .push_back(response(vec![message("old", "backlog")], Some("t1")));
        fake.posts.push_back(response(
            vec![message("old", "backlog"), message("new", "hello")],
            Some("t2"),
        ));
        fake.posts.push_back(response(vec![], None));
        let (session, events) = session();
        run_with(&session, &Target::Video("dQw4w9WgXcQ".into()), &mut fake).unwrap();

        assert_eq!(
            fake.requested,
            ["https://www.youtube.com/live_chat?is_popout=1&v=dQw4w9WgXcQ"]
        );
        assert_eq!(fake.tokens, ["all", "t1", "t2"]);
        let events = events.lock().unwrap();
        let mut states = Vec::new();
        let mut chats = Vec::new();
        for event in events.iter() {
            match &event.event {
                Event::Connection(state) => states.push(state.clone()),
                Event::Chat { video, event } => {
                    assert_eq!(video, "dQw4w9WgXcQ");
                    chats.push(event.content.clone());
                }
                other => panic!("unexpected {other:?}"),
            }
        }
        assert_eq!(
            states,
            [
                ConnectionState::Connecting,
                ConnectionState::Connected {
                    video: "dQw4w9WgXcQ".into()
                },
                ConnectionState::Ended
            ]
        );
        assert_eq!(
            chats,
            [Content::Text(vec![crate::Run::Text("hello".into())])]
        );
    }

    #[test]
    fn a_channel_waits_while_it_is_offline() {
        let mut fake = Fake::default();
        fake.pages.push_back(Ok(
            "<link rel=\"canonical\" href=\"https://www.youtube.com/@nana\">".into(),
        ));
        let (session, events) = session();
        let stop = session.clone();
        let worker = std::thread::spawn(move || {
            run_with(&session, &Target::Handle("nana".into()), &mut fake).map(|_| fake.requested)
        });
        while events.lock().unwrap().len() < 2 {
            std::thread::sleep(Duration::from_millis(5));
        }
        stop.stop();
        assert_eq!(
            worker.join().unwrap().unwrap(),
            ["https://www.youtube.com/@nana/live"]
        );
        let states: Vec<_> = events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match &e.event {
                Event::Connection(s) => Some(s.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            states,
            [ConnectionState::Resolving, ConnectionState::Waiting]
        );
    }

    #[test]
    fn a_live_channel_page_names_its_stream() {
        assert_eq!(
            live_video(
                "<link rel=\"canonical\" href=\"https://www.youtube.com/watch?v=dQw4w9WgXcQ\">"
            ),
            Ok(Some("dQw4w9WgXcQ".into()))
        );
        assert_eq!(live_video("<html>").unwrap_err(), "protocol-changed");
    }

    #[test]
    fn a_missing_video_is_reported_once() {
        let mut fake = Fake::default();
        fake.pages.push_back(Err("not-found".into()));
        let (session, _) = session();
        assert_eq!(
            run_with(&session, &Target::Video("dQw4w9WgXcQ".into()), &mut fake),
            Err("invalid-target".into())
        );
    }
}
