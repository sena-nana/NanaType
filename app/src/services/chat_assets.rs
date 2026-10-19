//! Publishes chat immediately; a bounded pool fills stable image references later.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use crate::chat::ChatMessage;
use crossbeam_channel::{Sender, bounded, unbounded};
use nanatype_bilibili::ChatEvent;
use nanatype_model::AssetRef;

const AVATAR_TTL: Duration = Duration::from_secs(3 * 24 * 3600);
const RETRY_AFTER: Duration = Duration::from_secs(10 * 60);
const TIMEOUT: Duration = Duration::from_secs(4);
const MAX_BYTES: u64 = 4 * 1024 * 1024;
const AVATAR_PX: u32 = 128;
const EMOTE_PX: u32 = 160;
const WORKERS: usize = 4;
/// Downloaded viewer and peer pictures unused this long are deleted at start.
/// Pasted images share the folder but are history content and never pruned.
const CACHE_MAX_AGE: Duration = Duration::from_secs(30 * 24 * 3600);
const CACHE_PREFIXES: &[&str] = &["bili-", "douyin-", "p2p-"];

/// Remove stale downloaded pictures; a later message downloads them again.
fn prune_cache(dir: &Path) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if CACHE_PREFIXES.iter().any(|p| name.starts_with(p))
            && file_age(&entry.path()).is_some_and(|age| age > CACHE_MAX_AGE)
        {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Account artwork uses the same bounded image format/cache as viewer artwork.
pub fn cache_profile_avatar(
    dir: PathBuf,
    uid: &str,
    url: &str,
    ready: impl Fn(AssetRef) + Send + 'static,
) -> Option<AssetRef> {
    if url.is_empty() {
        return None;
    }
    let name = format!("bili-profile-{uid}-{:016x}", fnv(url));
    let asset = AssetRef::Blob(name.clone());
    let result = asset.clone();
    let url = url.to_owned();
    thread::spawn(move || {
        let path = dir.join(format!("{name}.png"));
        if path.exists() || Saver::new().download(&url, &path, AVATAR_PX).is_ok() {
            ready(asset);
        }
    });
    Some(result)
}

pub struct ChatAssets {
    tx: Sender<(u64, String, Incoming)>,
}
enum Incoming {
    Bilibili(ChatEvent),
    Douyin(nanatype_douyin::ChatEvent),
}

#[derive(Clone)]
struct Job {
    name: String,
    url: String,
    px: u32,
    ttl: Option<Duration>,
}
impl Job {
    fn asset(&self) -> AssetRef {
        AssetRef::Blob(self.name.clone())
    }
    fn avatar(platform: &str, uid: &str, url: &str) -> Option<Self> {
        if url.is_empty() {
            return None;
        }
        let name = if uid.is_empty() || !uid.chars().all(|c| c.is_ascii_alphanumeric()) {
            format!("{platform}-face-u{:016x}", fnv(url))
        } else {
            format!("{platform}-face-{uid}")
        };
        Some(Self {
            name,
            url: url.into(),
            px: AVATAR_PX,
            ttl: Some(AVATAR_TTL),
        })
    }
    fn emote(platform: &str, url: &str) -> Self {
        Self {
            name: format!("{platform}-emote-{:016x}", fnv(url)),
            url: url.into(),
            px: EMOTE_PX,
            ttl: None,
        }
    }
}

#[derive(Default)]
struct Requests {
    pending: HashSet<String>,
    failed: HashMap<String, Instant>,
}
impl Requests {
    fn reserve(&mut self, job: &Job, dir: &Path) -> bool {
        if self.pending.contains(&job.name)
            || self
                .failed
                .get(&job.url)
                .is_some_and(|at| at.elapsed() < RETRY_AFTER)
            || file_age(&dir.join(format!("{}.png", job.name)))
                .is_some_and(|age| job.ttl.is_none_or(|ttl| age < ttl))
        {
            return false;
        }
        self.pending.insert(job.name.clone());
        true
    }
}

impl ChatAssets {
    pub fn start(
        blob_dir: PathBuf,
        deliver: impl Fn(ChatMessage) + Send + 'static,
        ready: impl Fn(AssetRef) + Send + Sync + 'static,
    ) -> Self {
        let (tx, rx) = unbounded::<(u64, String, Incoming)>();
        let (jobs, work) = bounded::<Job>(512);
        let requests = Arc::new(Mutex::new(Requests::default()));
        let ready = Arc::new(ready);
        {
            let dir = blob_dir.clone();
            let _ = thread::Builder::new()
                .name("nanatype-chat-prune".into())
                .spawn(move || prune_cache(&dir));
        }
        for n in 0..WORKERS {
            let work = work.clone();
            let dir = blob_dir.clone();
            let requests = requests.clone();
            let ready = ready.clone();
            thread::Builder::new()
                .name(format!("nanatype-chat-image-{n}"))
                .spawn(move || {
                    let saver = Saver::new();
                    for job in work {
                        let path = dir.join(format!("{}.png", job.name));
                        let result = saver.download(&job.url, &path, job.px);
                        {
                            let mut state = requests.lock().unwrap();
                            state.pending.remove(&job.name);
                            if result.is_err() {
                                state.failed.retain(|_, at| at.elapsed() < RETRY_AFTER);
                                state.failed.insert(job.url.clone(), Instant::now());
                            }
                        }
                        match result {
                            Ok(()) => ready(job.asset()),
                            Err(_) => crate::debug_log(&format!(
                                "chat picture {}: download failed",
                                job.name
                            )),
                        }
                    }
                })
                .expect("spawn chat image worker");
        }
        thread::Builder::new()
            .name("nanatype-chat-ingest".into())
            .spawn(move || {
                let mut next = 1u64;
                for (epoch, room, event) in rx {
                    let (avatar, emote) = match &event {
                        Incoming::Douyin(event) => (
                            Job::avatar("douyin", &event.author.uid, &event.author.avatar),
                            match &event.content {
                                nanatype_douyin::Content::Emote { url, .. } => {
                                    Some(Job::emote("douyin", url))
                                }
                                _ => None,
                            },
                        ),
                        Incoming::Bilibili(event) => match event {
                            ChatEvent::Text {
                                author, emoticon, ..
                            } => (
                                Job::avatar("bili", &author.uid, &author.avatar_url),
                                emoticon.as_deref().map(|url| Job::emote("bili", url)),
                            ),
                            ChatEvent::Gift { author, .. }
                            | ChatEvent::Member { author, .. }
                            | ChatEvent::SuperChat { author, .. } => {
                                (Job::avatar("bili", &author.uid, &author.avatar_url), None)
                            }
                            _ => continue,
                        },
                    };
                    let message = match event {
                        Incoming::Douyin(event) => Some(ChatMessage::from_douyin(
                            event,
                            avatar.as_ref().map(Job::asset),
                            emote.as_ref().map(Job::asset),
                            next,
                        )),
                        Incoming::Bilibili(event) => ChatMessage::from_event(
                            event,
                            avatar.as_ref().map(Job::asset),
                            emote.as_ref().map(Job::asset),
                            next,
                        ),
                    };
                    if let Some(mut message) = message {
                        message.epoch = epoch;
                        message.room = room;
                        next += 1;
                        // Deliver before scheduling any network work, preserving arrival order.
                        deliver(message);
                    }
                    for job in [avatar, emote].into_iter().flatten() {
                        let mut state = requests.lock().unwrap();
                        if state.reserve(&job, &blob_dir) && jobs.try_send(job.clone()).is_err() {
                            state.pending.remove(&job.name);
                        }
                    }
                }
            })
            .expect("spawn chat ingest");
        Self { tx }
    }
    pub fn bilibili(&self, epoch: u64, room: u64, event: ChatEvent) {
        let _ = self
            .tx
            .send((epoch, room.to_string(), Incoming::Bilibili(event)));
    }
    pub fn douyin(&self, epoch: u64, room: String, event: nanatype_douyin::ChatEvent) {
        let _ = self.tx.send((epoch, room, Incoming::Douyin(event)));
    }
}

struct Saver {
    agent: ureq::Agent,
}
impl Saver {
    fn new() -> Self {
        Self {
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(TIMEOUT))
                .user_agent(concat!("NanaType/", env!("CARGO_PKG_VERSION")))
                .build()
                .into(),
        }
    }
    fn download(&self, url: &str, path: &Path, px: u32) -> Result<(), String> {
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err("unsupported image URL".into());
        }
        let mut response = self.agent.get(url).call().map_err(|e| e.to_string())?;
        let bytes = response
            .body_mut()
            .with_config()
            .limit(MAX_BYTES)
            .read_to_vec()
            .map_err(|e| e.to_string())?;
        let image = crate::assets::decode_image(&bytes, 4096, 64 * 1024 * 1024)
            .map_err(|e| e.to_string())?;
        let image = if image.width() > px || image.height() > px {
            image.thumbnail(px, px)
        } else {
            image
        };
        std::fs::create_dir_all(path.parent().ok_or("image has no parent")?)
            .map_err(|e| e.to_string())?;
        // Written aside and moved in, so a reader never sees half a file.
        let partial = path.with_extension("part");
        image
            .into_rgba8()
            .save_with_format(&partial, image::ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        std::fs::rename(&partial, path).map_err(|e| e.to_string())
    }
}

fn file_age(path: &Path) -> Option<Duration> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(
        SystemTime::now()
            .duration_since(modified)
            .unwrap_or(Duration::ZERO),
    )
}

fn fnv(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use nanatype_bilibili::Author;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    #[test]
    fn platform_images_and_ingestion_ids_are_isolated() {
        assert_ne!(
            Job::avatar("bili", "42", "https://example.test/avatar")
                .unwrap()
                .name,
            Job::avatar("douyin", "42", "https://example.test/avatar")
                .unwrap()
                .name
        );
        let (tx, rx) = unbounded();
        let service = ChatAssets::start(
            scratch("platforms"),
            move |m| {
                let _ = tx.send(m);
            },
            |_| {},
        );
        service.bilibili(7, 42, event(String::new(), 1));
        service.douyin(
            7,
            "42".into(),
            nanatype_douyin::ChatEvent {
                id: "1".into(),
                author: nanatype_douyin::Author {
                    uid: "42".into(),
                    name: "viewer".into(),
                    avatar: String::new(),
                },
                content: nanatype_douyin::Content::Text("hi".into()),
                at: 1,
            },
        );
        let bili = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let dy = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_ne!(bili.seq, dy.seq);
        assert_ne!(bili.user.uid, dy.user.uid);
        assert_ne!(bili.platform, dy.platform);
    }

    fn event(url: String, n: u64) -> ChatEvent {
        ChatEvent::Text {
            id: n.to_string(),
            author: Author {
                uid: "42".into(),
                name: "观众".into(),
                avatar_url: url,
                ..Default::default()
            },
            content: format!("message {n}"),
            emoticon: None,
            at_ms: n,
        }
    }
    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nanatype-chat-{label}-{}-{}",
            std::process::id(),
            nanatype_model::unix_millis()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn slow_images_do_not_hold_messages_and_duplicate_requests_coalesce() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/face", listener.local_addr().unwrap());
        let (accepted_tx, accepted) = bounded(1);
        let (release, gate) = bounded(1);
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = stream.read(&mut [0; 2048]);
            accepted_tx.send(()).unwrap();
            gate.recv_timeout(Duration::from_secs(3)).unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
            listener.set_nonblocking(true).unwrap();
            thread::sleep(Duration::from_millis(50));
            assert!(listener.accept().is_err(), "duplicate avatar downloads");
        });
        let (messages, got) = unbounded();
        let (ready, assets) = unbounded();
        let service = ChatAssets::start(
            scratch("slow"),
            move |m| {
                let _ = messages.send(m);
            },
            move |a| {
                let _ = ready.send(a);
            },
        );
        service.bilibili(0, 0, event(url.clone(), 1));
        accepted.recv_timeout(Duration::from_secs(2)).unwrap();
        for n in 2..=20 {
            service.bilibili(0, 0, event(url.clone(), n));
        }
        for n in 1..=20 {
            let m = got
                .recv_timeout(Duration::from_millis(300))
                .expect("message must not wait for image");
            assert_eq!(m.seq, n);
            assert_eq!(m.doc.plain_text(), format!("message {n}"));
            assert_eq!(m.user.avatar, Some(AssetRef::Blob("bili-face-42".into())));
        }
        release.send(()).unwrap();
        server.join().unwrap();
        assert!(assets.try_recv().is_err());
    }

    #[test]
    fn successful_images_fill_the_same_reference_without_delivering_another_message() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/face", listener.local_addr().unwrap());
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(8, 8, image::Rgba([200, 20, 50, 255]))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = stream.read(&mut [0; 2048]);
            let bytes = png.into_inner();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                bytes.len()
            )
            .unwrap();
            stream.write_all(&bytes).unwrap();
        });
        let dir = scratch("ready");
        let (messages, got) = unbounded();
        let (ready, assets) = unbounded();
        let service = ChatAssets::start(
            dir.clone(),
            move |m| {
                let _ = messages.send(m);
            },
            move |a| {
                let _ = ready.send(a);
            },
        );
        service.bilibili(0, 0, event(url, 1));
        let message = got.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(
            Some(assets.recv_timeout(Duration::from_secs(2)).unwrap()),
            message.user.avatar
        );
        assert!(dir.join("bili-face-42.png").is_file());
        assert!(got.try_recv().is_err());
        server.join().unwrap();
    }

    #[test]
    fn cached_and_recently_failed_images_do_not_use_the_pool() {
        let dir = scratch("cache");
        let job = Job::avatar("bili", "42", "http://example.invalid/face").unwrap();
        let mut state = Requests::default();
        assert!(state.reserve(&job, &dir));
        assert!(!state.reserve(&job, &dir));
        state.pending.clear();
        state.failed.insert(job.url.clone(), Instant::now());
        assert!(!state.reserve(&job, &dir));
        state.failed.insert(
            job.url.clone(),
            Instant::now() - RETRY_AFTER - Duration::from_secs(1),
        );
        assert!(state.reserve(&job, &dir));
        state.pending.clear();
        state.failed.clear();
        std::fs::write(dir.join("bili-face-42.png"), b"cached").unwrap();
        assert!(!state.reserve(&job, &dir));
    }
}
