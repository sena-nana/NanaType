//! Translated subtitles for sent lines: an OpenAI-compatible chat endpoint
//! (OpenAI, DeepSeek, a local Ollama or LM Studio) or DeepL. One worker
//! thread sends the requests in turn from a small queue, so the caller never
//! waits; recent translations are kept in memory and answered at once.
mod http;
pub mod lang;
pub mod provider;

pub use http::is_loopback;
pub use provider::HttpRequest;

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

/// How long one request may take before it is given up on.
pub const TIMEOUT: Duration = Duration::from_secs(8);
/// Requests waiting behind the one being sent; the oldest is dropped past it.
pub const QUEUE: usize = 8;
/// Translations kept for replays and repeated lines.
pub const CACHE: usize = 256;
/// Longer lines are not sent.
pub const MAX_TEXT: usize = 2000;

/// Where translations come from. Keys are only ever held here, in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Provider {
    /// An OpenAI-compatible `/chat/completions` endpoint; a server on this
    /// computer needs no key.
    Chat {
        base_url: String,
        model: String,
        key: Option<String>,
    },
    /// DeepL; a key ending in `:fx` uses the free API.
    DeepL { key: String },
}

impl Provider {
    /// What the cache tells providers apart by: never the key.
    fn cache_id(&self) -> String {
        match self {
            Self::Chat {
                base_url, model, ..
            } => format!("chat\n{}\n{}", provider::chat_url(base_url), model.trim()),
            Self::DeepL { .. } => "deepl".into(),
        }
    }

    /// The configured address, for a chat endpoint.
    fn endpoint(&self) -> Option<&str> {
        match self {
            Self::Chat { base_url, .. } => Some(base_url),
            Self::DeepL { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub provider: Provider,
    /// `None` (or empty) uses the system's proxy settings.
    pub proxy: Option<String>,
    pub timeout: Duration,
}

impl Config {
    pub fn new(provider: Provider) -> Self {
        Self {
            provider,
            proxy: None,
            timeout: TIMEOUT,
        }
    }
}

/// A line to translate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The caller's own id, handed back in the [`Reply`].
    pub id: u64,
    pub text: String,
    /// Who says it, as context.
    pub speaker: String,
    /// A BCP 47 code such as `en` or `zh-Hant`; the source is detected.
    pub target: String,
    /// A newer request in the same lane drops this one while it still
    /// waits: on a live display only the newest line matters. `None`
    /// waits its turn.
    pub lane: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The provider needs an API key and has none.
    NoKey,
    Unauthorized,
    /// Rate limited or out of quota.
    Limited,
    Status(u16),
    Network,
    Timeout,
    /// The proxy address cannot be used.
    Proxy,
    /// The answer is not the service's JSON.
    Parse,
    /// The answer has no text.
    Empty,
}

impl Error {
    /// A stable code for diagnostics events.
    pub fn code(self) -> u64 {
        match self {
            Self::NoKey => 0,
            Self::Unauthorized => 1,
            Self::Limited => 2,
            Self::Status(_) => 3,
            Self::Network => 4,
            Self::Timeout => 5,
            Self::Proxy => 6,
            Self::Parse => 7,
            Self::Empty => 8,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Done(String),
    /// Dropped unsent: a newer line in its lane, a full queue, or the
    /// provider changed or was switched off.
    Superseded,
    Failed(Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub id: u64,
    pub outcome: Outcome,
    /// Answered from memory, without asking the provider.
    pub cached: bool,
}

/// Sends one built request. The worker owns one per configuration.
pub trait Transport: Send {
    fn post(&self, request: &HttpRequest) -> Result<(u16, Vec<u8>), Error>;
}

type MakeTransport = dyn Fn(&Config) -> Result<Box<dyn Transport>, Error> + Send + Sync;
type OnReply = dyn Fn(Reply) + Send + Sync;
/// The wire the worker built for a configuration, or why it could not.
type Wire = (Arc<Config>, Result<Box<dyn Transport>, Error>);

/// Recent translations, least recently used out first.
#[derive(Default)]
struct Cache {
    entries: HashMap<(String, String, String), (String, u64)>,
    tick: u64,
}

impl Cache {
    fn key(config: &Config, target: &str, text: &str) -> (String, String, String) {
        (
            config.provider.cache_id(),
            target.trim().to_ascii_lowercase(),
            text.to_owned(),
        )
    }

    fn get(&mut self, key: &(String, String, String)) -> Option<String> {
        self.tick += 1;
        let tick = self.tick;
        self.entries.get_mut(key).map(|(text, used)| {
            *used = tick;
            text.clone()
        })
    }

    fn put(&mut self, key: (String, String, String), text: String) {
        self.tick += 1;
        if self.entries.len() >= CACHE
            && !self.entries.contains_key(&key)
            && let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(k, _)| k.clone())
        {
            self.entries.remove(&oldest);
        }
        self.entries.insert(key, (text, self.tick));
    }
}

struct Job {
    request: Request,
    config: Arc<Config>,
}

#[derive(Default)]
struct Queue {
    pending: VecDeque<Job>,
    quit: bool,
}

struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
    cache: Mutex<Cache>,
    on_reply: Box<OnReply>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The queue, its worker thread and the cache. Dropping it stops the worker
/// after the request in flight.
pub struct Translator {
    shared: Arc<Shared>,
    config: Option<Arc<Config>>,
}

impl Translator {
    /// Translations over the network; `on_reply` is called from the worker
    /// thread (or the caller's, for an answer from memory).
    pub fn new(on_reply: impl Fn(Reply) + Send + Sync + 'static) -> Self {
        Self::with_transport(
            |config| Ok(Box::new(http::Ureq::new(config)?) as Box<dyn Transport>),
            on_reply,
        )
    }

    /// Over another wire, for tests.
    pub fn with_transport(
        make: impl Fn(&Config) -> Result<Box<dyn Transport>, Error> + Send + Sync + 'static,
        on_reply: impl Fn(Reply) + Send + Sync + 'static,
    ) -> Self {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue::default()),
            wake: Condvar::new(),
            cache: Mutex::new(Cache::default()),
            on_reply: Box::new(on_reply),
        });
        let worker = shared.clone();
        let make: Box<MakeTransport> = Box::new(make);
        std::thread::Builder::new()
            .name("translate".into())
            .spawn(move || work(&worker, &*make))
            .expect("spawn the translation worker");
        Self {
            shared,
            config: None,
        }
    }

    /// Use `config` from now on; `None` switches translation off. Waiting
    /// requests made for another configuration are dropped.
    pub fn configure(&mut self, config: Option<Config>) {
        if self.config.as_deref() == config.as_ref() {
            return;
        }
        self.config = config.map(Arc::new);
        let dropped: Vec<Job> = lock(&self.shared.queue).pending.drain(..).collect();
        self.supersede(dropped);
    }

    pub fn configured(&self) -> bool {
        self.config.is_some()
    }

    /// A translation already known for `text`.
    pub fn cached(&self, text: &str, target: &str) -> Option<String> {
        let config = self.config.as_ref()?;
        lock(&self.shared.cache).get(&Cache::key(config, target, text))
    }

    /// Translate `request`. A known translation is answered at once; the
    /// rest queue for the worker. Returns `false` when switched off or the
    /// text is empty or too long, and nothing will be answered.
    pub fn request(&self, request: Request) -> bool {
        let Some(config) = self.config.clone() else {
            return false;
        };
        if request.text.trim().is_empty() || request.text.len() > MAX_TEXT {
            return false;
        }
        if let Some(text) = self.cached(&request.text, &request.target) {
            (self.shared.on_reply)(Reply {
                id: request.id,
                outcome: Outcome::Done(text),
                cached: true,
            });
            return true;
        }
        let mut dropped = Vec::new();
        {
            let mut queue = lock(&self.shared.queue);
            if let Some(lane) = request.lane {
                let (gone, kept): (VecDeque<Job>, VecDeque<Job>) =
                    std::mem::take(&mut queue.pending)
                        .into_iter()
                        .partition(|job| job.request.lane == Some(lane));
                queue.pending = kept;
                dropped = gone.into();
            }
            while queue.pending.len() >= QUEUE {
                dropped.extend(queue.pending.pop_front());
            }
            queue.pending.push_back(Job { request, config });
        }
        self.shared.wake.notify_one();
        self.supersede(dropped);
        true
    }

    fn supersede(&self, jobs: Vec<Job>) {
        for job in jobs {
            (self.shared.on_reply)(Reply {
                id: job.request.id,
                outcome: Outcome::Superseded,
                cached: false,
            });
        }
    }
}

impl Drop for Translator {
    fn drop(&mut self) {
        let mut queue = lock(&self.shared.queue);
        queue.quit = true;
        queue.pending.clear();
        drop(queue);
        self.shared.wake.notify_all();
    }
}

fn work(shared: &Shared, make: &MakeTransport) {
    let mut transport: Option<Wire> = None;
    loop {
        let job = {
            let mut queue = lock(&shared.queue);
            loop {
                if queue.quit {
                    return;
                }
                if let Some(job) = queue.pending.pop_front() {
                    break job;
                }
                queue = shared.wake.wait(queue).unwrap_or_else(|e| e.into_inner());
            }
        };
        let provider = &job.config.provider;
        let result = provider::build(provider, &job.request).and_then(|http| {
            if !transport
                .as_ref()
                .is_some_and(|(config, _)| Arc::ptr_eq(config, &job.config))
            {
                transport = Some((job.config.clone(), make(&job.config)));
            }
            let wire = match &transport {
                Some((_, Ok(wire))) => wire,
                Some((_, Err(e))) => return Err(*e),
                None => return Err(Error::Network),
            };
            let (code, body) = wire.post(&http)?;
            provider::parse(provider, code, &body)
        });
        let outcome = match result {
            Ok(text) => {
                let key = Cache::key(&job.config, &job.request.target, &job.request.text);
                lock(&shared.cache).put(key, text.clone());
                Outcome::Done(text)
            }
            Err(e) => Outcome::Failed(e),
        };
        (shared.on_reply)(Reply {
            id: job.request.id,
            outcome,
            cached: false,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::sync::mpsc;

    fn chat(base_url: &str) -> Config {
        Config::new(Provider::Chat {
            base_url: base_url.into(),
            model: "test-model".into(),
            key: Some("sk-local".into()),
        })
    }

    fn line(id: u64, text: &str, lane: Option<u8>) -> Request {
        Request {
            id,
            text: text.into(),
            speaker: "Nana".into(),
            target: "en".into(),
            lane,
        }
    }

    /// A wire that holds each request until the test lets it through, and
    /// answers with the request's text upper-cased.
    struct Gate {
        sent: mpsc::Sender<String>,
        open: Arc<Mutex<mpsc::Receiver<()>>>,
    }
    impl Transport for Gate {
        fn post(&self, request: &HttpRequest) -> Result<(u16, Vec<u8>), Error> {
            let body: serde_json::Value = serde_json::from_str(&request.body).unwrap();
            let text = body["messages"][1]["content"].as_str().unwrap().to_owned();
            self.sent.send(text.clone()).unwrap();
            lock(&self.open).recv().map_err(|_| Error::Network)?;
            let answer = serde_json::json!({
                "choices": [{"message": {"content": text.to_uppercase()}}]
            });
            Ok((200, answer.to_string().into_bytes()))
        }
    }

    struct Harness {
        translator: Translator,
        sent: mpsc::Receiver<String>,
        open: mpsc::Sender<()>,
        replies: mpsc::Receiver<Reply>,
    }

    fn harness() -> Harness {
        let (sent_tx, sent) = mpsc::channel();
        let (open, open_rx) = mpsc::channel();
        let open_rx = Arc::new(Mutex::new(open_rx));
        let (reply_tx, replies) = mpsc::channel();
        let reply_tx = Mutex::new(reply_tx);
        let mut translator = Translator::with_transport(
            move |_| {
                Ok(Box::new(Gate {
                    sent: sent_tx.clone(),
                    open: open_rx.clone(),
                }) as Box<dyn Transport>)
            },
            move |r| lock(&reply_tx).send(r).unwrap(),
        );
        translator.configure(Some(chat("http://example.invalid/v1")));
        Harness {
            translator,
            sent,
            open,
            replies,
        }
    }

    const WAIT: Duration = Duration::from_secs(5);

    fn done(text: &str) -> Outcome {
        Outcome::Done(text.into())
    }

    #[test]
    fn a_newer_live_line_drops_the_waiting_ones_and_answers_come_from_memory() {
        let h = harness();
        assert!(h.translator.request(line(1, "one", Some(0))));
        // The first is in flight; the next two wait behind it.
        assert_eq!(h.sent.recv_timeout(WAIT).unwrap(), "one");
        h.translator.request(line(2, "two", Some(0)));
        h.translator.request(line(9, "chat", None));
        h.translator.request(line(3, "three", Some(0)));
        let r = h.replies.recv_timeout(WAIT).unwrap();
        assert_eq!((r.id, r.outcome), (2, Outcome::Superseded));
        for _ in 0..3 {
            h.open.send(()).unwrap();
        }
        let mut got: Vec<(u64, Outcome)> = (0..3)
            .map(|_| h.replies.recv_timeout(WAIT).unwrap())
            .map(|r| (r.id, r.outcome))
            .collect();
        got.sort_by_key(|(id, _)| *id);
        // The one in flight still lands (the caller drops it if its line
        // has gone); the chat line keeps its place.
        assert_eq!(
            got,
            [(1, done("ONE")), (3, done("THREE")), (9, done("CHAT"))]
        );
        assert_eq!(h.sent.recv_timeout(WAIT).unwrap(), "chat");
        assert_eq!(h.sent.recv_timeout(WAIT).unwrap(), "three");

        // Again, from memory and on this thread: nothing is sent.
        assert!(h.translator.request(line(4, "one", Some(0))));
        let r = h.replies.try_recv().unwrap();
        assert_eq!((r.id, r.outcome, r.cached), (4, done("ONE"), true));
        assert_eq!(h.translator.cached("three", "EN").as_deref(), Some("THREE"));
        assert_eq!(h.translator.cached("three", "ja"), None);
        assert!(h.sent.try_recv().is_err());
    }

    #[test]
    fn a_full_queue_drops_its_oldest_and_switching_off_drops_the_rest() {
        let mut h = harness();
        h.translator.request(line(100, "first", None));
        assert_eq!(h.sent.recv_timeout(WAIT).unwrap(), "first");
        for id in 0..=QUEUE as u64 {
            h.translator.request(line(id, &format!("line {id}"), None));
        }
        let r = h.replies.recv_timeout(WAIT).unwrap();
        assert_eq!((r.id, r.outcome), (0, Outcome::Superseded));
        h.translator.configure(None);
        let dropped: Vec<u64> = (0..QUEUE)
            .map(|_| h.replies.recv_timeout(WAIT).unwrap())
            .inspect(|r| assert_eq!(r.outcome, Outcome::Superseded))
            .map(|r| r.id)
            .collect();
        assert_eq!(dropped, (1..=QUEUE as u64).collect::<Vec<_>>());
        assert!(!h.translator.request(line(50, "off", None)));
        h.open.send(()).unwrap();
        assert_eq!(h.replies.recv_timeout(WAIT).unwrap().id, 100);
    }

    #[test]
    fn empty_or_huge_lines_are_not_sent() {
        let h = harness();
        assert!(!h.translator.request(line(1, "  ", None)));
        assert!(
            !h.translator
                .request(line(2, &"x".repeat(MAX_TEXT + 1), None))
        );
        assert!(h.sent.recv_timeout(Duration::from_millis(50)).is_err());
    }

    #[test]
    fn the_cache_forgets_the_least_recently_used() {
        let config = chat("http://a/v1");
        let mut cache = Cache::default();
        for i in 0..CACHE {
            cache.put(Cache::key(&config, "en", &i.to_string()), i.to_string());
        }
        // Touch the oldest, so the second oldest goes instead.
        assert!(cache.get(&Cache::key(&config, "en", "0")).is_some());
        cache.put(Cache::key(&config, "en", "new"), "new".into());
        assert_eq!(cache.entries.len(), CACHE);
        assert!(cache.get(&Cache::key(&config, "en", "0")).is_some());
        assert!(cache.get(&Cache::key(&config, "en", "1")).is_none());
        // Another model is another translation; the key is not part of it.
        let other = chat("http://a/v1/");
        assert!(cache.get(&Cache::key(&other, "en", "0")).is_some());
        let model = Config::new(Provider::Chat {
            base_url: "http://a/v1".into(),
            model: "other".into(),
            key: None,
        });
        assert!(cache.get(&Cache::key(&model, "en", "0")).is_none());
    }

    /// One HTTP exchange on a local socket: the request as received, and
    /// `status` with `body` back.
    fn serve_once(status: u16, body: &'static str) -> (String, mpsc::Receiver<String>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut seen = Vec::new();
            let mut buf = [0u8; 4096];
            // Headers, then as much body as Content-Length says.
            loop {
                let n = stream.read(&mut buf).unwrap();
                seen.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&seen).to_string();
                if let Some(end) = text.find("\r\n\r\n") {
                    let len: usize = text
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse().unwrap())
                        })
                        .unwrap_or(0);
                    if seen.len() >= end + 4 + len {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            tx.send(String::from_utf8_lossy(&seen).to_string()).unwrap();
            let reply = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(reply.as_bytes()).unwrap();
        });
        (url, rx)
    }

    #[test]
    fn a_local_chat_server_is_asked_over_http() {
        let (url, seen) = serve_once(
            200,
            r#"{"choices":[{"message":{"content":"Good evening, everyone!"}}]}"#,
        );
        let (tx, replies) = mpsc::channel();
        let tx = Mutex::new(tx);
        let mut translator = Translator::new(move |r| lock(&tx).send(r).unwrap());
        translator.configure(Some(chat(&url)));
        translator.request(line(7, "大家晚上好！", Some(0)));
        let reply = replies.recv_timeout(WAIT).unwrap();
        assert_eq!(reply.outcome, done("Good evening, everyone!"));
        let request = seen.recv_timeout(WAIT).unwrap();
        assert!(
            request.starts_with("POST /v1/chat/completions HTTP/1.1"),
            "{request}"
        );
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer sk-local"),
            "{request}"
        );
        assert!(request.contains("大家晚上好！"), "{request}");
    }

    #[test]
    fn a_refused_key_is_an_error_not_a_line() {
        let (url, _seen) = serve_once(401, r#"{"error":{"message":"bad key"}}"#);
        let (tx, replies) = mpsc::channel();
        let tx = Mutex::new(tx);
        let mut translator = Translator::new(move |r| lock(&tx).send(r).unwrap());
        translator.configure(Some(chat(&url)));
        translator.request(line(1, "hi", None));
        let reply = replies.recv_timeout(WAIT).unwrap();
        assert_eq!(reply.outcome, Outcome::Failed(Error::Unauthorized));
        assert_eq!(translator.cached("hi", "en"), None);
    }

    #[test]
    fn deepl_without_a_key_fails_without_sending() {
        let (tx, replies) = mpsc::channel();
        let tx = Mutex::new(tx);
        let mut translator = Translator::with_transport(
            |_| panic!("no wire is needed"),
            move |r| lock(&tx).send(r).unwrap(),
        );
        translator.configure(Some(Config::new(Provider::DeepL { key: String::new() })));
        translator.request(line(1, "hi", None));
        assert_eq!(
            replies.recv_timeout(WAIT).unwrap().outcome,
            Outcome::Failed(Error::NoKey)
        );
    }
}
