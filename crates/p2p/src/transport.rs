use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use serde::{Serialize, de::DeserializeOwned};
use tokio::sync::{RwLock, mpsc, watch};

use crate::link::{Carrier, Inbound, Outbound, Pipe, RouteSnap};
use crate::picture::{self, Role};
use crate::protocol::{
    self, Frame, Invite, MAX_ASSET, MAX_FRAME, MAX_MEMBERS, Room, TrackSnapshot, VERSION,
    room_password,
};

/// Only explicitly referenced local assets are offered to a peer.
pub type Resources = BTreeMap<String, PathBuf>;
type Emit = Arc<dyn Fn(Event) + Send + Sync>;
#[derive(Clone)]
struct AssetOffer {
    source: PathBuf,
    len: u64,
    modified: Option<std::time::SystemTime>,
    encoded: PathBuf,
    preview: Vec<u8>,
    /// `Picture` covers a later use of the same file as an avatar.
    role: Role,
}
type Offers = Arc<RwLock<BTreeMap<String, AssetOffer>>>;

#[derive(Clone)]
pub struct Config {
    pub cache: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Starting,
    Waiting,
    Connected { route: String, rtt_ms: u64 },
    Reconnecting,
    Ended,
    Error(String),
}

#[derive(Debug, Clone)]
pub enum Event {
    Started {
        host: bool,
        local_id: String,
        /// Copied to people who were not invited. Omits the session secret
        /// when a password is set.
        invitation: String,
        /// Given to a Steam friend the host invited. Carries the session
        /// secret, so they skip the password.
        admit: String,
    },
    Status(Status),
    Room {
        room: Arc<Room>,
        received_at: Instant,
    },
    AssetReady(String),
}

struct Publication {
    track: TrackSnapshot,
    resources: Resources,
    published_at: Instant,
}
enum Command {
    Leave,
    #[cfg(test)]
    Disconnect,
}

/// Input channels kept across guest reconnects and shared by both room roles.
struct Controls {
    commands: mpsc::UnboundedReceiver<Command>,
    changes: watch::Receiver<Option<Arc<Publication>>>,
    stopped: watch::Receiver<bool>,
}

/// Fixed session resources used by handshakes and message exchange.
#[derive(Clone, Copy)]
struct SessionContext<'a> {
    carrier: &'a Carrier,
    invite: &'a Invite,
    local: &'a str,
    cache: &'a Path,
    offers: &'a Offers,
    emit: &'a Emit,
}

/// Host connection state updated when an authenticated guest takes a seat.
struct Admission<'a> {
    links: &'a mut BTreeMap<String, Link>,
    next_generation: &'a mut u64,
    connected_once: &'a mut bool,
    notes: &'a mpsc::Sender<(String, u64, Inbound)>,
}

pub struct PeerHandle {
    tx: mpsc::UnboundedSender<Command>,
    latest: watch::Sender<Option<Arc<Publication>>>,
    stop: watch::Sender<bool>,
}

impl PeerHandle {
    pub fn start(
        config: Config,
        invitation: Option<String>,
        password: impl Into<String>,
        carrier: Carrier,
        emit: impl Fn(Event) + Send + Sync + 'static,
    ) -> Result<Self, String> {
        let password = room_password(&password.into())?;
        let invite = invitation.as_deref().map(Invite::decode).transpose()?;
        if invite.as_ref().is_some_and(|invite| invite.locked) && password.is_empty() {
            return Err("Room password required".into());
        }
        let (tx, rx) = mpsc::unbounded_channel();
        let (latest, changes) = watch::channel(None);
        let (stop, stopped) = watch::channel(false);
        let emit: Emit = Arc::new(emit);
        std::thread::Builder::new()
            .name("nanatype-p2p".into())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        emit(Event::Status(Status::Error(e.to_string())));
                        return;
                    }
                };
                runtime.block_on(async {
                    if let Err(e) = run(
                        config,
                        carrier,
                        invite,
                        password,
                        Controls {
                            commands: rx,
                            changes,
                            stopped,
                        },
                        emit.clone(),
                    )
                    .await
                    {
                        emit(Event::Status(Status::Error(e)));
                    }
                });
            })
            .map_err(|e| e.to_string())?;
        Ok(Self { tx, latest, stop })
    }

    pub fn publish(&self, track: TrackSnapshot, resources: Resources) {
        self.latest.send_replace(Some(Arc::new(Publication {
            track,
            resources,
            published_at: Instant::now(),
        })));
    }

    #[cfg(test)]
    fn disconnect(&self) {
        let _ = self.tx.send(Command::Disconnect);
    }
}

impl Drop for PeerHandle {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Leave);
        let _ = self.stop.send(true);
    }
}

fn pack(value: &impl Serialize) -> Result<Vec<u8>, String> {
    let json = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    if json.len() > MAX_FRAME {
        return Err("Session frame exceeds limit".into());
    }
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    std::io::Write::write_all(&mut encoder, &json).map_err(|e| e.to_string())?;
    encoder.finish().map_err(|e| e.to_string())
}

fn unpack(bytes: &[u8], max: usize) -> Result<Vec<u8>, String> {
    let mut decoder = ZlibDecoder::new(bytes);
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = decoder
            .read(&mut buf)
            .map_err(|_| "Session frame is corrupt")?;
        if n == 0 {
            break;
        }
        if out.len().saturating_add(n) > max {
            return Err("Session frame exceeds limit".into());
        }
        out.extend_from_slice(&buf[..n]);
    }
    Ok(out)
}

fn send_frame(
    send: &mpsc::UnboundedSender<Outbound>,
    value: &impl Serialize,
) -> Result<(), String> {
    send.send(Outbound::Frame(pack(value)?))
        .map_err(|_| "Connection closed".into())
}

struct Arm {
    flag: Arc<AtomicBool>,
    armed: bool,
}

impl Drop for Arm {
    fn drop(&mut self) {
        if self.armed {
            self.flag.store(true, Ordering::Relaxed);
        }
    }
}

struct Abort(tokio::task::JoinHandle<()>);

impl Drop for Abort {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn open_pipe(
    carrier: &Carrier,
    remote: Option<u64>,
    deadline: Instant,
) -> Result<Pipe, String> {
    open_port(carrier, remote, deadline, false).await
}

async fn open_asset(
    carrier: &Carrier,
    remote: Option<u64>,
    deadline: Instant,
) -> Result<Pipe, String> {
    open_port(carrier, remote, deadline, true).await
}

async fn open_port(
    carrier: &Carrier,
    remote: Option<u64>,
    deadline: Instant,
    asset: bool,
) -> Result<Pipe, String> {
    let flag = Arc::new(AtomicBool::new(false));
    let flag_thread = flag.clone();
    let carrier = carrier.clone();
    let task = tokio::task::spawn_blocking(move || {
        if asset {
            carrier.connect_asset(remote, flag_thread)
        } else {
            carrier.connect(remote, flag_thread)
        }
    });
    tokio::pin!(task);
    let mut arm = Arm { flag, armed: true };
    let result = tokio::select! {
        result = &mut task => result.map_err(|error| error.to_string())?,
        _ = tokio::time::sleep(deadline.saturating_duration_since(Instant::now())) => {
            Err("Connection timed out".into())
        }
    };
    if result.is_ok() {
        arm.armed = false;
    }
    result
}

async fn read_frame(
    recv: &mut mpsc::UnboundedReceiver<Inbound>,
    max: usize,
) -> Result<Frame, String> {
    match recv.recv().await {
        Some(Inbound::Frame(bytes)) => decode_frame(&bytes, max),
        Some(Inbound::Closed) | None => Err("Connection closed".into()),
        Some(Inbound::Want(_) | Inbound::Asset { .. }) => Err("Unexpected session message".into()),
    }
}

fn decode_frame(bytes: &[u8], max: usize) -> Result<Frame, String> {
    if bytes.len() > max {
        return Err("Session frame exceeds limit".into());
    }
    let json = unpack(bytes, max)?;
    let mut value: serde_json::Value = serde_json::from_slice(&json).map_err(|e| e.to_string())?;
    sanitize_assets(&mut value);
    serde_json::from_value(value).map_err(|e| e.to_string())
}

/// An unauthenticated peer may only send a hello this large.
const MAX_HELLO: usize = 8 * 1024;
/// Sustained frames per second accepted from a peer; more are delayed.
const MAX_FRAMES_PER_SECOND: u32 = 40;
/// Unauthenticated pipes the host reads a hello from at once. More are closed.
const MAX_HANDSHAKES: usize = 4;
/// A SteamID that fails this many handshakes in a row is refused for a while.
const FAILED_HANDSHAKES: u32 = 3;
const HANDSHAKE_BACKOFF: Duration = Duration::from_secs(30);

/// Network references can only address content in the session image cache.
fn sanitize_assets(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            for (key, value) in object {
                if matches!(key.as_str(), "asset" | "avatar")
                    && let Some(s) = value.as_str()
                {
                    if !s
                        .strip_prefix("blob:p2p-")
                        .is_some_and(protocol::valid_hash)
                    {
                        *value = if key == "avatar" {
                            serde_json::Value::Null
                        } else {
                            serde_json::Value::String("blob:p2p-missing".into())
                        };
                    }
                } else {
                    sanitize_assets(value);
                }
            }
        }
        serde_json::Value::Array(array) => {
            for value in array {
                sanitize_assets(value);
            }
        }
        _ => {}
    }
}

async fn prepare<T: Serialize + DeserializeOwned>(
    value: &T,
    resources: &Resources,
    offers: &Offers,
    cache: &Path,
) -> Result<T, String> {
    let _ = tokio::fs::create_dir_all(cache).await;
    let mut value = serde_json::to_value(value).map_err(|e| e.to_string())?;
    let picture_roles = picture::roles(&value);
    // One file can be an avatar and a portrait. Encode it once, at the larger size.
    let mut role_for_path: BTreeMap<&PathBuf, Role> = BTreeMap::new();
    for (reference, path) in resources {
        let role = picture_roles
            .get(reference)
            .copied()
            .unwrap_or(Role::Picture);
        role_for_path
            .entry(path)
            .and_modify(|existing| {
                if role == Role::Picture {
                    *existing = Role::Picture;
                }
            })
            .or_insert(role);
    }
    let mut replacements = BTreeMap::new();
    let mut encoded_for_path: BTreeMap<PathBuf, String> = BTreeMap::new();
    for (reference, path) in resources {
        if let Some(hash) = encoded_for_path.get(path) {
            replacements.insert(reference.clone(), format!("blob:p2p-{hash}"));
            continue;
        }
        let Ok(meta) = tokio::fs::metadata(path).await else {
            continue;
        };
        if meta.len() > MAX_ASSET as u64 {
            continue;
        }
        let modified = meta.modified().ok();
        let role = role_for_path.get(path).copied().unwrap_or(Role::Picture);
        let cached = offers
            .read()
            .await
            .iter()
            .find(|(_, offer)| {
                offer.source == *path
                    && offer.len == meta.len()
                    && offer.modified == modified
                    && (offer.role == Role::Picture || role == Role::Avatar)
            })
            .map(|(hash, _)| hash.clone());
        if let Some(hash) = cached {
            encoded_for_path.insert(path.clone(), hash.clone());
            replacements.insert(reference.clone(), format!("blob:p2p-{hash}"));
            continue;
        }
        let Ok(bytes) = tokio::fs::read(path).await else {
            continue;
        };
        let encoded =
            tokio::task::spawn_blocking(move || picture::encode_picture(&bytes, role)).await;
        let Ok(Ok(encoded)) = encoded else {
            continue;
        };
        let hash = blake3::hash(&encoded.bytes).to_hex().to_string();
        let encoded_path = cache.join(format!("p2p-{hash}.{}", encoded.extension));
        if !encoded_path.is_file() {
            let temp = cache.join(format!("p2p-{hash}.partial"));
            if tokio::fs::write(&temp, &encoded.bytes).await.is_err() {
                continue;
            }
            if tokio::fs::rename(&temp, &encoded_path).await.is_err() {
                let _ = tokio::fs::remove_file(&temp).await;
                continue;
            }
        }
        offers.write().await.insert(
            hash.clone(),
            AssetOffer {
                source: path.clone(),
                len: meta.len(),
                modified,
                encoded: encoded_path,
                preview: encoded.preview,
                role,
            },
        );
        encoded_for_path.insert(path.clone(), hash.clone());
        replacements.insert(reference.clone(), format!("blob:p2p-{hash}"));
    }
    fn replace(value: &mut serde_json::Value, map: &BTreeMap<String, String>) {
        match value {
            serde_json::Value::String(s) => {
                if let Some(to) = map.get(s) {
                    *s = to.clone();
                }
            }
            serde_json::Value::Array(a) => {
                for v in a {
                    replace(v, map);
                }
            }
            serde_json::Value::Object(o) => {
                for v in o.values_mut() {
                    replace(v, map);
                }
            }
            _ => {}
        }
    }
    replace(&mut value, &replacements);
    // Never expose absolute local paths, library names or skin-local resources.
    // Unresolved references use a non-resolvable placeholder instead.
    sanitize_assets(&mut value);
    serde_json::from_value(value).map_err(|e| e.to_string())
}

async fn pending_previews(
    offers: &Offers,
    cache: &Path,
    hashes: &BTreeSet<String>,
    sent: &mut BTreeSet<String>,
) -> BTreeMap<String, String> {
    let offers = offers.read().await;
    let mut out = BTreeMap::new();
    let mut relayed = Vec::new();
    for hash in hashes {
        if sent.contains(hash) {
            continue;
        }
        if let Some(offer) = offers.get(hash) {
            sent.insert(hash.clone());
            out.insert(hash.clone(), URL_SAFE_NO_PAD.encode(&offer.preview));
        } else {
            relayed.push(hash.clone());
        }
    }
    drop(offers);
    for hash in relayed {
        let Some(bytes) = cached_preview(cache, &hash).await else {
            continue;
        };
        sent.insert(hash.clone());
        out.insert(hash, URL_SAFE_NO_PAD.encode(bytes));
    }
    out
}

/// A preview another participant already sent, small enough to ride on the next frame.
async fn cached_preview(cache: &Path, hash: &str) -> Option<Vec<u8>> {
    if !protocol::valid_hash(hash) {
        return None;
    }
    let bytes = tokio::fs::read(cache.join(format!("p2p-{hash}.preview.webp")))
        .await
        .ok()?;
    if bytes.len() > picture::PREVIEW_MAX || picture::wire_kind(&bytes) != Some(picture::Kind::Webp)
    {
        None
    } else {
        Some(bytes)
    }
}

/// Bytes the host can hand to someone else: its own encode, or a picture already stored.
async fn offered_asset(offers: &Offers, hash: &str) -> Option<Vec<u8>> {
    if !protocol::valid_hash(hash) {
        return None;
    }
    let path = offers
        .read()
        .await
        .get(hash)
        .map(|offer| offer.encoded.clone())?;
    let bytes = tokio::fs::read(&path).await.ok()?;
    if bytes.len() <= picture::WIRE_ASSET && blake3::hash(&bytes).to_hex().as_str() == hash {
        Some(bytes)
    } else {
        None
    }
}

fn asset_owner(room: &Room, hash: &str) -> Option<String> {
    room.tracks
        .iter()
        .find_map(|(id, track)| picture_hashes(track).contains(hash).then(|| id.clone()))
}

/// Another seated guest published this picture. The host is already on the
/// session pipe, and someone who left is not dialed.
fn direct_publisher(room: &Room, local: &str, hash: &str) -> Option<u64> {
    let owner = asset_owner(room, hash)?;
    if owner == local || owner == room.host || !room.guests.iter().any(|id| id == &owner) {
        return None;
    }
    owner.parse().ok()
}

async fn remember_seated(seated: &RwLock<BTreeSet<String>>, room: &Room) {
    let mut ids = BTreeSet::new();
    ids.insert(room.host.clone());
    ids.extend(room.guests.iter().cloned());
    *seated.write().await = ids;
}

/// Accept picture fetches from people the host has seated. Anyone else is closed
/// before a byte is sent. The session secret is not on this pipe.
async fn serve_assets(
    carrier: Carrier,
    offers: Offers,
    seated: Arc<RwLock<BTreeSet<String>>>,
    mut stopped: watch::Receiver<bool>,
) {
    loop {
        if *stopped.borrow() {
            break;
        }
        let accept = open_asset(
            &carrier,
            None,
            Instant::now() + Duration::from_secs(24 * 60 * 60),
        );
        tokio::pin!(accept);
        let pipe = tokio::select! {
            _ = stopped.changed() => break,
            result = &mut accept => result,
        };
        let Ok(pipe) = pipe else {
            continue;
        };
        let allowed = seated.read().await.contains(&pipe.remote_id);
        if pipe.remote_id.is_empty() || !allowed {
            let _ = pipe.send.send(Outbound::Close);
            continue;
        }
        let offers = offers.clone();
        tokio::spawn(serve_one(pipe, offers));
    }
}

async fn serve_one(mut pipe: Pipe, offers: Offers) {
    while let Some(message) = pipe.recv.recv().await {
        match message {
            Inbound::Want(hash) => {
                let Some(bytes) = offered_asset(&offers, &hash).await else {
                    continue;
                };
                if pipe.send.send(Outbound::Asset { hash, bytes }).is_err() {
                    break;
                }
            }
            Inbound::Closed | Inbound::Frame(_) | Inbound::Asset { .. } => break,
        }
    }
    let _ = pipe.send.send(Outbound::Close);
}

async fn fetch_asset(carrier: Carrier, publisher: u64, hash: String) -> Result<Vec<u8>, String> {
    let mut pipe = open_asset(
        &carrier,
        Some(publisher),
        Instant::now() + Duration::from_secs(8),
    )
    .await?;
    if pipe.send.send(Outbound::Want(hash.clone())).is_err() {
        return Err("Connection closed".into());
    }
    let hash_for_wait = hash.clone();
    let result = tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            match pipe.recv.recv().await {
                Some(Inbound::Asset { hash: got, bytes }) if got == hash_for_wait => {
                    return Ok(bytes);
                }
                Some(Inbound::Closed) | None => return Err("Connection closed".into()),
                Some(_) => {}
            }
        }
    })
    .await;
    let _ = pipe.send.send(Outbound::Close);
    match result {
        Ok(bytes) => bytes,
        Err(_) => Err("Connection timed out".into()),
    }
}

async fn stored_asset(offers: &Offers, cache: &Path, hash: &str) -> Option<Vec<u8>> {
    if !protocol::valid_hash(hash) {
        return None;
    }
    if let Some(path) = offers
        .read()
        .await
        .get(hash)
        .map(|offer| offer.encoded.clone())
        && let Ok(bytes) = tokio::fs::read(&path).await
        && bytes.len() <= picture::WIRE_ASSET
        && blake3::hash(&bytes).to_hex().as_str() == hash
    {
        return Some(bytes);
    }
    let path = protocol::session_full_picture(cache, hash)?;
    let bytes = tokio::fs::read(&path).await.ok()?;
    if bytes.len() <= picture::WIRE_ASSET && blake3::hash(&bytes).to_hex().as_str() == hash {
        Some(bytes)
    } else {
        None
    }
}

fn picture_hashes(value: &impl Serialize) -> BTreeSet<String> {
    let Ok(value) = serde_json::to_value(value) else {
        return BTreeSet::new();
    };
    protocol::asset_references(&value)
        .into_iter()
        .filter_map(|reference| reference.strip_prefix("blob:p2p-"))
        .filter(|hash| protocol::valid_hash(hash))
        .map(str::to_owned)
        .collect()
}

async fn store_previews(cache: &Path, previews: BTreeMap<String, String>, emit: &Emit) {
    if previews.is_empty() {
        return;
    }
    if tokio::fs::create_dir_all(cache).await.is_err() {
        return;
    }
    for (hash, encoded) in previews {
        if !protocol::valid_hash(&hash) || protocol::session_full_picture(cache, &hash).is_some() {
            continue;
        }
        let Ok(bytes) = URL_SAFE_NO_PAD.decode(encoded) else {
            continue;
        };
        if bytes.len() > picture::PREVIEW_MAX
            || picture::wire_kind(&bytes) != Some(picture::Kind::Webp)
        {
            continue;
        }
        let path = cache.join(format!("p2p-{hash}.preview.webp"));
        if path.is_file() {
            continue;
        }
        let temp = cache.join(format!("p2p-{hash}.preview.partial"));
        if tokio::fs::write(&temp, &bytes).await.is_err() {
            continue;
        }
        if tokio::fs::rename(&temp, &path).await.is_err() {
            let _ = tokio::fs::remove_file(&temp).await;
            continue;
        }
        emit(Event::AssetReady(hash));
    }
}

fn broadcast(room: &Room, anchors: &BTreeMap<String, Instant>, emit: &Emit) -> Room {
    let mut room = room.clone();
    for (owner, track) in &mut room.tracks {
        track.age_ms = track.age_ms.saturating_add(
            anchors
                .get(owner)
                .map_or(0, |at| at.elapsed().as_millis() as u64),
        );
    }
    emit(Event::Room {
        room: Arc::new(room.clone()),
        received_at: Instant::now(),
    });
    room
}

async fn run(
    config: Config,
    carrier: Carrier,
    invite: Option<Invite>,
    password: String,
    mut controls: Controls,
    emit: Emit,
) -> Result<(), String> {
    emit(Event::Status(Status::Starting));
    let local = carrier.local_id.clone();
    let steam: u64 = local
        .parse()
        .map_err(|_| "Steam identity is unavailable".to_string())?;
    let hosting = invite.is_none();
    // The session secret and the room password are different. A selected
    // friend is given the secret; everyone else has to type the password.
    let (invite, room_password) = if hosting {
        let locked = !password.is_empty();
        (
            Invite {
                v: VERSION,
                session: random_secret(),
                secret: random_secret(),
                steam,
                locked,
            },
            password,
        )
    } else {
        let mut invite = invite.unwrap();
        if invite.locked {
            invite.secret = password;
        }
        (invite, String::new())
    };
    if hosting && local != invite.steam.to_string() {
        return Err("Steam identity is unavailable".into());
    }
    nanatype_diag::event!(crate::diag::ENDPOINT_BOUND, hosting = hosting);
    let mut room = Room::new(
        invite.session.clone(),
        if hosting {
            local.clone()
        } else {
            invite.steam.to_string()
        },
    );
    let mut anchors = BTreeMap::new();
    let offers: Offers = Arc::new(RwLock::new(BTreeMap::new()));
    let mut connected_once = false;
    emit(Event::Started {
        host: hosting,
        local_id: local.clone(),
        invitation: if hosting {
            invite.shared().encode()
        } else {
            String::new()
        },
        admit: if hosting {
            invite.admitted().encode()
        } else {
            String::new()
        },
    });

    let seated = Arc::new(RwLock::new(BTreeSet::new()));
    let session = SessionContext {
        carrier: &carrier,
        invite: &invite,
        local: &local,
        cache: &config.cache,
        offers: &offers,
        emit: &emit,
    };
    if hosting {
        return host_session(
            session,
            &room_password,
            &mut room,
            &mut anchors,
            &mut controls,
        )
        .await;
    }

    let serve = tokio::spawn(serve_assets(
        carrier.clone(),
        offers.clone(),
        seated.clone(),
        controls.stopped.clone(),
    ));
    let _serve = Abort(serve);

    loop {
        if *controls.stopped.borrow() {
            return Ok(());
        }
        emit(Event::Status(if connected_once {
            Status::Reconnecting
        } else {
            Status::Starting
        }));
        let mut sent_previews = BTreeSet::new();
        let deadline = Instant::now() + Duration::from_secs(20);
        let pipe = {
            let attempt = connect_attempt(session, &mut room, &mut anchors, &seated, deadline);
            tokio::pin!(attempt);
            tokio::select! {
                result = &mut attempt => result,
                _ = controls.stopped.changed() => return Ok(()),
            }
        };
        let pipe = match pipe {
            Ok(pipe) => pipe,
            Err(error) => {
                if !connected_once {
                    return Err(error);
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
        };
        connected_once = true;
        let (route, rtt_ms) = snap(&pipe.route);
        emit(Event::Status(Status::Connected { route, rtt_ms }));
        broadcast(&room, &anchors, &emit);
        let ended = exchange(
            session,
            pipe,
            &mut room,
            &mut anchors,
            &seated,
            &mut sent_previews,
            &mut controls,
        )
        .await;
        match ended {
            Ok(true) => {
                emit(Event::Status(Status::Ended));
                break;
            }
            Ok(false) => return Ok(()),
            Err(_) => {
                emit(Event::Status(Status::Reconnecting));
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
    Ok(())
}

struct Link {
    generation: u64,
    send: mpsc::UnboundedSender<Outbound>,
    route: watch::Receiver<RouteSnap>,
    last_seen: Instant,
    heartbeat: Instant,
    inflight: BTreeMap<String, Instant>,
    /// Hashes this guest asked for before the bytes were here.
    pending_wants: BTreeSet<String>,
    sent_previews: BTreeSet<String>,
    reader: tokio::task::JoinHandle<()>,
}

fn retire(link: Link) {
    let _ = link.send.send(Outbound::Close);
    link.reader.abort();
}

fn spawn_reader(
    mut recv: mpsc::UnboundedReceiver<Inbound>,
    id: String,
    generation: u64,
    notes: mpsc::Sender<(String, u64, Inbound)>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut window = Instant::now();
        let mut frames = 0u32;
        while let Some(message) = recv.recv().await {
            if matches!(message, Inbound::Frame(_)) {
                if window.elapsed() >= Duration::from_secs(1) {
                    window = Instant::now();
                    frames = 0;
                } else if frames >= MAX_FRAMES_PER_SECOND {
                    tokio::time::sleep(Duration::from_secs(1).saturating_sub(window.elapsed()))
                        .await;
                    window = Instant::now();
                    frames = 0;
                }
                frames += 1;
            }
            let closed = matches!(message, Inbound::Closed);
            if notes.send((id.clone(), generation, message)).await.is_err() || closed {
                break;
            }
        }
    })
}

fn attach(
    pipe: Pipe,
    generation: u64,
    sent_previews: BTreeSet<String>,
    notes: &mpsc::Sender<(String, u64, Inbound)>,
) -> (String, Link) {
    let id = pipe.remote_id.clone();
    let reader = spawn_reader(pipe.recv, id.clone(), generation, notes.clone());
    (
        id,
        Link {
            generation,
            send: pipe.send,
            route: pipe.route,
            last_seen: Instant::now(),
            heartbeat: Instant::now(),
            inflight: BTreeMap::new(),
            pending_wants: BTreeSet::new(),
            sent_previews,
            reader,
        },
    )
}

fn presence(links: &BTreeMap<String, Link>, guests: &[String], connected_once: bool) -> Status {
    if links.is_empty() {
        return if connected_once && !guests.is_empty() {
            Status::Reconnecting
        } else {
            Status::Waiting
        };
    }
    let mut relay = false;
    let mut direct = false;
    let mut rtt_ms = 0u64;
    for link in links.values() {
        let (name, link_rtt) = snap(&link.route);
        rtt_ms = rtt_ms.max(link_rtt);
        match name.as_str() {
            "relay" => relay = true,
            "direct" => direct = true,
            _ => {}
        }
    }
    let route = if relay {
        "relay"
    } else if direct {
        "direct"
    } else {
        "connecting"
    };
    Status::Connected {
        route: route.into(),
        rtt_ms,
    }
}

async fn fan_room(
    links: &mut BTreeMap<String, Link>,
    snapshot: &Room,
    offers: &Offers,
    cache: &Path,
) -> Vec<String> {
    let hashes = snapshot.asset_hashes();
    let mut dead = Vec::new();
    for (id, link) in links.iter_mut() {
        let previews = pending_previews(offers, cache, &hashes, &mut link.sent_previews).await;
        if send_frame(
            &link.send,
            &Frame::Room {
                room: snapshot.clone(),
                previews,
            },
        )
        .is_err()
        {
            dead.push(id.clone());
        }
    }
    dead
}

async fn fan_peer(
    links: &mut BTreeMap<String, Link>,
    except: &str,
    participant: &str,
    track: &TrackSnapshot,
    offers: &Offers,
    cache: &Path,
) -> Vec<String> {
    let hashes = picture_hashes(track);
    let mut dead = Vec::new();
    for (id, link) in links.iter_mut() {
        if id == except {
            continue;
        }
        let previews = pending_previews(offers, cache, &hashes, &mut link.sent_previews).await;
        if send_frame(
            &link.send,
            &Frame::PeerTrack {
                participant: participant.to_string(),
                track: track.clone(),
                previews,
            },
        )
        .is_err()
        {
            dead.push(id.clone());
        }
    }
    dead
}

/// Drop these pipes and free their seats. Their tracks stay, so lines already
/// sent remain, and the same SteamID can sit down again while a seat is open.
async fn release(
    links: &mut BTreeMap<String, Link>,
    room: &mut Room,
    anchors: &mut BTreeMap<String, Instant>,
    offers: &Offers,
    cache: &Path,
    emit: &Emit,
    ids: &[String],
) {
    let mut freed = false;
    for id in ids {
        if let Some(link) = links.remove(id) {
            retire(link);
        }
        let before = room.guests.len();
        room.unseat(id);
        freed |= room.guests.len() != before;
    }
    if !freed {
        return;
    }
    let snapshot = broadcast(room, anchors, emit);
    let more = fan_room(links, &snapshot, offers, cache).await;
    if more.is_empty() {
        return;
    }
    for id in &more {
        if let Some(link) = links.remove(id) {
            retire(link);
        }
        room.unseat(id);
    }
    broadcast(room, anchors, emit);
}

/// The host keeps every guest pipe. One person leaving or dropping frees that
/// seat and keeps their track. The host leaving ends the room.
async fn host_session(
    session: SessionContext<'_>,
    password: &str,
    room: &mut Room,
    anchors: &mut BTreeMap<String, Instant>,
    controls: &mut Controls,
) -> Result<(), String> {
    let SessionContext {
        carrier,
        invite,
        local,
        cache,
        offers,
        emit,
    } = session;
    let Controls {
        commands,
        changes,
        stopped,
    } = controls;
    let (notes_tx, mut notes_rx) = mpsc::channel::<(String, u64, Inbound)>(64);
    let mut links: BTreeMap<String, Link> = BTreeMap::new();
    let mut next_generation = 1u64;
    let mut connected_once = false;
    // Hellos are read off the loop: anyone who owns the app can connect, and
    // a pipe that never speaks must not hold up the people already seated.
    let gate = Arc::new(Gate {
        session: invite.session.clone(),
        secret: invite.secret.clone(),
        password: password.to_owned(),
        local: local.to_owned(),
    });
    let handshakes = Arc::new(tokio::sync::Semaphore::new(MAX_HANDSHAKES));
    let (hello_tx, mut hello_rx) = mpsc::channel::<Result<Pipe, String>>(MAX_HANDSHAKES);
    let mut failures: BTreeMap<String, (u32, Instant)> = BTreeMap::new();
    emit(Event::Status(Status::Waiting));
    let accept = open_pipe(carrier, None, Instant::now() + Duration::from_secs(20));
    tokio::pin!(accept);
    let mut timer = tokio::time::interval(Duration::from_millis(50));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = stopped.changed() => {
                end_links(&mut links);
                emit(Event::Status(Status::Ended));
                return Ok(());
            }
            result = &mut accept => {
                match result {
                    Ok(pipe) => {
                        failures.retain(|_, (_, at)| at.elapsed() < HANDSHAKE_BACKOFF);
                        let refused = failures
                            .get(&pipe.remote_id)
                            .is_some_and(|(count, _)| *count >= FAILED_HANDSHAKES);
                        match handshakes.clone().try_acquire_owned() {
                            Ok(permit) if !refused => {
                                let gate = gate.clone();
                                let hello_tx = hello_tx.clone();
                                tokio::spawn(async move {
                                    let result = handshake(pipe, &gate).await;
                                    drop(permit);
                                    let _ = hello_tx.send(result).await;
                                });
                            }
                            _ => {
                                let _ = pipe.send.send(Outbound::Close);
                            }
                        }
                    }
                    Err(error) => {
                        if error != "Connection timed out" && error != "Connection cancelled" {
                            tokio::time::sleep(Duration::from_millis(200)).await;
                        }
                    }
                }
                accept.set(open_pipe(
                    carrier,
                    None,
                    Instant::now() + Duration::from_secs(20),
                ));
            }
            Some(hello) = hello_rx.recv() => match hello {
                Ok(pipe) => {
                    failures.remove(&pipe.remote_id);
                    admit(
                        pipe,
                        session,
                        room,
                        anchors,
                        Admission {
                            links: &mut links,
                            next_generation: &mut next_generation,
                            connected_once: &mut connected_once,
                            notes: &notes_tx,
                        },
                    )
                    .await;
                }
                Err(remote) => {
                    if !remote.is_empty() {
                        let failed = failures.entry(remote).or_insert((0, Instant::now()));
                        failed.0 = failed.0.saturating_add(1);
                        failed.1 = Instant::now();
                    }
                }
            },
            note = notes_rx.recv() => {
                let Some((id, generation, message)) = note else {
                    return Ok(());
                };
                if links.get(&id).is_none_or(|link| link.generation != generation) {
                    continue;
                }
                if let Some(link) = links.get_mut(&id) {
                    link.last_seen = Instant::now();
                }
                match message {
                    Inbound::Closed => {
                        release(
                            &mut links,
                            room,
                            anchors,
                            offers,
                            cache,
                            emit,
                            &[id],
                        )
                        .await;
                        emit(Event::Status(presence(&links, &room.guests, connected_once)));
                    }
                    Inbound::Frame(bytes) => match decode_frame(&bytes, MAX_FRAME) {
                        Ok(Frame::Leave) => {
                            release(
                                &mut links,
                                room,
                                anchors,
                                offers,
                                cache,
                                emit,
                                &[id],
                            )
                            .await;
                            emit(Event::Status(presence(&links, &room.guests, connected_once)));
                        }
                        Ok(Frame::Track { track, previews }) => {
                            store_previews(cache, previews, emit).await;
                            let forwarded = track.clone();
                            match room.update(&id, track) {
                                Ok(true) => {
                                    anchors.insert(id.clone(), Instant::now());
                                    let indexed = room.index(&id);
                                    if indexed {
                                        let snapshot = broadcast(room, anchors, emit);
                                        let dead = fan_room(&mut links, &snapshot, offers, cache).await;
                                        release(&mut links, room, anchors, offers, cache, emit, &dead).await;
                                    } else {
                                        let dead = fan_peer(&mut links, &id, &id, &forwarded, offers, cache).await;
                                        release(&mut links, room, anchors, offers, cache, emit, &dead).await;
                                    }
                                }
                                Ok(false) => {}
                                Err(_) => {}
                            }
                        }
                        Ok(Frame::Ping(nonce)) => {
                            let failed = links.get(&id).is_some_and(|link| {
                                send_frame(&link.send, &Frame::Pong(nonce)).is_err()
                            });
                            if failed {
                                release(&mut links, room, anchors, offers, cache, emit, &[id]).await;
                                emit(Event::Status(presence(&links, &room.guests, connected_once)));
                            }
                        }
                        Ok(Frame::Pong(_)) => {
                            emit(Event::Status(presence(&links, &room.guests, connected_once)));
                        }
                        // A peer that breaks the protocol gives its seat back.
                        Ok(_) | Err(_) => {
                            release(&mut links, room, anchors, offers, cache, emit, &[id]).await;
                            emit(Event::Status(presence(&links, &room.guests, connected_once)));
                        }
                    },
                    Inbound::Want(hash) => {
                        let failed = if let Some(bytes) = stored_asset(offers, cache, &hash).await {
                            links.get(&id).is_some_and(|link| {
                                link.send.send(Outbound::Asset { hash, bytes }).is_err()
                            })
                        } else if let Some(link) = links.get_mut(&id) {
                            link.pending_wants.insert(hash);
                            false
                        } else {
                            false
                        };
                        if failed {
                            release(&mut links, room, anchors, offers, cache, emit, &[id]).await;
                            emit(Event::Status(presence(&links, &room.guests, connected_once)));
                        }
                    }
                    Inbound::Asset { hash, bytes } => {
                        if let Some(link) = links.get_mut(&id) {
                            link.inflight.remove(&hash);
                        }
                        if store_asset(cache, &hash, &bytes).await.is_ok() {
                            emit(Event::AssetReady(hash.clone()));
                            let dead = fulfill_wants(&mut links, &hash, &bytes);
                            if !dead.is_empty() {
                                release(
                                    &mut links,
                                    room,
                                    anchors,
                                    offers,
                                    cache,
                                    emit,
                                    &dead,
                                )
                                .await;
                            }
                        }
                    }
                }
            }
            command = commands.recv() => match command.unwrap_or(Command::Leave) {
                Command::Leave => {
                    end_links(&mut links);
                    emit(Event::Status(Status::Ended));
                    return Ok(());
                }
                #[cfg(test)]
                Command::Disconnect => {
                    let ids: Vec<String> = links.keys().cloned().collect();
                    for id in ids {
                        if let Some(link) = links.remove(&id) {
                            retire(link);
                        }
                    }
                    emit(Event::Status(presence(&links, &room.guests, connected_once)));
                }
            },
            _ = timer.tick() => {
                let mut dead = Vec::new();
                let due: Vec<String> = links
                    .iter()
                    .filter(|(_, link)| {
                        link.last_seen.elapsed() > Duration::from_secs(15)
                            || link.heartbeat.elapsed() >= Duration::from_secs(2)
                    })
                    .map(|(id, _)| id.clone())
                    .collect();
                let mut refresh = false;
                for id in due {
                    let Some(link) = links.get_mut(&id) else { continue };
                    if link.last_seen.elapsed() > Duration::from_secs(15) {
                        dead.push(id);
                        continue;
                    }
                    link.heartbeat = Instant::now();
                    let (_, rtt_ms) = snap(&link.route);
                    if send_frame(&link.send, &Frame::Ping(rtt_ms)).is_err() {
                        dead.push(id);
                    } else {
                        refresh = true;
                    }
                }
                if changes.has_changed().unwrap_or(false) {
                    let latest = changes.borrow_and_update().clone();
                    if let Some(publication) = latest
                        && let Ok(mut track) = prepare(
                            &publication.track,
                            &publication.resources,
                            offers,
                            cache,
                        )
                        .await
                    {
                        track.age_ms = publication.published_at.elapsed().as_millis() as u64;
                        let forwarded = track.clone();
                        if matches!(room.update(local, track), Ok(true)) {
                            anchors.insert(local.to_owned(), Instant::now());
                            let indexed = room.index(local);
                            if indexed {
                                let snapshot = broadcast(room, anchors, emit);
                                dead.extend(fan_room(&mut links, &snapshot, offers, cache).await);
                            } else {
                                dead.extend(fan_peer(&mut links, "", local, &forwarded, offers, cache).await);
                            }
                        }
                    }
                }
                let hashes = room.asset_hashes();
                let offers_now = offers.read().await;
                for link in links.values_mut() {
                    link.inflight.retain(|_, at| at.elapsed() < Duration::from_secs(15));
                }
                for hash in &hashes {
                    let have = protocol::session_full_picture(cache, hash).is_some();
                    if have || offers_now.contains_key(hash) {
                        continue;
                    }
                    let Some(owner) = asset_owner(room, hash) else {
                        continue;
                    };
                    let Some(link) = links.get_mut(&owner) else {
                        continue;
                    };
                    if link.inflight.len() >= 2 || link.inflight.contains_key(hash) {
                        continue;
                    }
                    if link.send.send(Outbound::Want(hash.clone())).is_err() {
                        dead.push(owner);
                        continue;
                    }
                    link.inflight.insert(hash.clone(), Instant::now());
                }
                drop(offers_now);
                if !dead.is_empty() {
                    release(
                        &mut links,
                        room,
                        anchors,
                        offers,
                        cache,
                        emit,
                        &dead,
                    )
                    .await;
                    refresh = true;
                }
                if refresh {
                    emit(Event::Status(presence(&links, &room.guests, connected_once)));
                }
            }
        }
    }
}

fn fulfill_wants(links: &mut BTreeMap<String, Link>, hash: &str, bytes: &[u8]) -> Vec<String> {
    let waiters: Vec<String> = links
        .iter()
        .filter(|(_, link)| link.pending_wants.contains(hash))
        .map(|(id, _)| id.clone())
        .collect();
    let mut dead = Vec::new();
    for id in waiters {
        let Some(link) = links.get_mut(&id) else {
            continue;
        };
        link.pending_wants.remove(hash);
        if link
            .send
            .send(Outbound::Asset {
                hash: hash.to_string(),
                bytes: bytes.to_vec(),
            })
            .is_err()
        {
            dead.push(id);
        }
    }
    dead
}

fn end_links(links: &mut BTreeMap<String, Link>) {
    for (_, link) in std::mem::take(links) {
        let _ = send_frame(&link.send, &Frame::Leave);
        retire(link);
    }
}

/// What an unauthenticated pipe's hello is checked against.
struct Gate {
    session: String,
    secret: String,
    password: String,
    local: String,
}

/// Read and check the hello of a pipe nobody has vouched for yet. A refusal
/// carries the remote SteamID so repeated failures can be refused early.
async fn handshake(mut pipe: Pipe, gate: &Gate) -> Result<Pipe, String> {
    let remote = pipe.remote_id.clone();
    let hello = tokio::time::timeout(
        Duration::from_secs(5),
        read_frame(&mut pipe.recv, MAX_HELLO),
    )
    .await;
    let accepted = matches!(
        &hello,
        Ok(Ok(Frame::Hello { v, session, secret }))
            if *v == VERSION
                && *session == gate.session
                && credential_matches(secret, &gate.secret, &gate.password)
    );
    if !accepted || remote.is_empty() || remote == gate.local {
        let _ = pipe.send.send(Outbound::Close);
        return Err(remote);
    }
    Ok(pipe)
}

/// Seat a pipe whose hello was accepted.
async fn admit(
    pipe: Pipe,
    session: SessionContext<'_>,
    room: &mut Room,
    anchors: &mut BTreeMap<String, Instant>,
    admission: Admission<'_>,
) {
    let SessionContext {
        local,
        offers,
        emit,
        cache,
        ..
    } = session;
    let Admission {
        links,
        next_generation,
        connected_once,
        notes,
    } = admission;
    debug_assert!(!pipe.remote_id.is_empty() && pipe.remote_id != local);
    let known = room.guests.iter().any(|id| id == &pipe.remote_id);
    if !known && room.guests.len() + 1 >= MAX_MEMBERS {
        let _ = send_frame(&pipe.send, &Frame::Full);
        let _ = pipe.send.send(Outbound::Close);
        return;
    }
    if room.seat(&pipe.remote_id).is_err() {
        let _ = send_frame(&pipe.send, &Frame::Full);
        let _ = pipe.send.send(Outbound::Close);
        return;
    }
    let id = pipe.remote_id.clone();
    if let Some(old) = links.remove(&id) {
        retire(old);
    }
    anchors.insert(id.clone(), Instant::now());
    let mut sent_previews = BTreeSet::new();
    let snapshot = broadcast(room, anchors, emit);
    let previews =
        pending_previews(offers, cache, &snapshot.asset_hashes(), &mut sent_previews).await;
    if send_frame(
        &pipe.send,
        &Frame::Welcome {
            room: snapshot.clone(),
            previews,
        },
    )
    .is_err()
    {
        let _ = pipe.send.send(Outbound::Close);
        if !known {
            room.unseat(&id);
            broadcast(room, anchors, emit);
        }
        return;
    }
    let generation = *next_generation;
    *next_generation = next_generation.saturating_add(1);
    let (id, link) = attach(pipe, generation, sent_previews, notes);
    links.insert(id, link);
    *connected_once = true;
    let dead = fan_room(links, &snapshot, offers, cache).await;
    release(links, room, anchors, offers, cache, emit, &dead).await;
    emit(Event::Status(presence(links, &room.guests, true)));
}

async fn connect_attempt(
    session: SessionContext<'_>,
    room: &mut Room,
    anchors: &mut BTreeMap<String, Instant>,
    seated: &RwLock<BTreeSet<String>>,
    deadline: Instant,
) -> Result<Pipe, String> {
    let SessionContext {
        carrier, invite, ..
    } = session;
    let mut pipe = open_pipe(carrier, Some(invite.steam), deadline).await?;
    if let Err(error) = greet_guest(&mut pipe, session, room, anchors, seated).await {
        let _ = pipe.send.send(Outbound::Close);
        return Err(error);
    }
    Ok(pipe)
}

async fn greet_guest(
    pipe: &mut Pipe,
    session: SessionContext<'_>,
    room: &mut Room,
    anchors: &mut BTreeMap<String, Instant>,
    seated: &RwLock<BTreeSet<String>>,
) -> Result<(), String> {
    let SessionContext {
        invite,
        local,
        cache,
        emit,
        ..
    } = session;
    send_frame(
        &pipe.send,
        &Frame::Hello {
            v: VERSION,
            session: invite.session.clone(),
            secret: invite.secret.clone(),
        },
    )?;
    match tokio::time::timeout(
        Duration::from_secs(15),
        read_frame(&mut pipe.recv, MAX_FRAME),
    )
    .await
    .map_err(|_| "Connection timed out".to_string())??
    {
        Frame::Welcome {
            room: snapshot,
            previews,
        } => {
            store_previews(cache, previews, emit).await;
            validate_room(&snapshot, &invite.session, &room.host, local)?;
            install_room(room, anchors, snapshot);
            remember_seated(seated, room).await;
            Ok(())
        }
        Frame::Full => Err("Room is full".into()),
        _ => Err("Invalid handshake".into()),
    }
}

async fn exchange(
    session: SessionContext<'_>,
    pipe: Pipe,
    room: &mut Room,
    anchors: &mut BTreeMap<String, Instant>,
    seated: &RwLock<BTreeSet<String>>,
    sent_previews: &mut BTreeSet<String>,
    controls: &mut Controls,
) -> Result<bool, String> {
    let SessionContext {
        carrier,
        invite,
        local,
        cache,
        offers,
        emit,
    } = session;
    let Controls {
        commands,
        changes,
        stopped,
    } = controls;
    let Pipe {
        send,
        mut recv,
        route,
        ..
    } = pipe;
    let (in_tx, mut in_rx) = mpsc::channel(32);
    let reader = tokio::spawn(async move {
        let mut window = Instant::now();
        let mut frames = 0u32;
        while let Some(message) = recv.recv().await {
            if matches!(message, Inbound::Closed) {
                let _ = in_tx.send(Inbound::Closed).await;
                break;
            }
            if matches!(message, Inbound::Frame(_)) {
                if window.elapsed() >= Duration::from_secs(1) {
                    window = Instant::now();
                    frames = 0;
                } else if frames >= MAX_FRAMES_PER_SECOND {
                    tokio::time::sleep(Duration::from_secs(1).saturating_sub(window.elapsed()))
                        .await;
                    window = Instant::now();
                    frames = 0;
                }
                frames += 1;
            }
            if in_tx.send(message).await.is_err() {
                break;
            }
        }
    });
    let mut inflight: BTreeMap<String, Instant> = BTreeMap::new();
    let (fetch_tx, mut fetch_rx) = mpsc::unbounded_channel();
    let _fetch_hold = fetch_tx.clone();
    let mut direct_inflight: BTreeSet<String> = BTreeSet::new();
    let mut direct_attempts: BTreeMap<String, u8> = BTreeMap::new();
    let mut timer = tokio::time::interval(Duration::from_millis(50));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut heartbeat = Instant::now();
    let mut last_seen = Instant::now();
    changes.mark_changed();
    let result = async {
        loop {
            tokio::select! {
                _ = stopped.changed() => {
                    let _ = send_frame(&send, &Frame::Leave);
                    let _ = send.send(Outbound::Close);
                    return Ok(true);
                }
                message = in_rx.recv() => {
                    let message = message.ok_or("Connection reader ended")?;
                    last_seen = Instant::now();
                    match message {
                        Inbound::Closed => return Err("Connection closed".into()),
                        Inbound::Frame(bytes) => {
                            let frame = decode_frame(&bytes, MAX_FRAME)?;
                            match frame {
                                Frame::Leave => {
                                    let _ = send.send(Outbound::Close);
                                    return Ok(true);
                                }
                                Frame::PeerTrack { participant, track, previews }
                                    if participant != local
                                        && (participant == room.host
                                            || room.guests.iter().any(|id| id == &participant)) =>
                                {
                                    store_previews(cache, previews, emit).await;
                                    if room.update(&participant, track)? {
                                        anchors.insert(participant, Instant::now());
                                        broadcast(room, anchors, emit);
                                    }
                                }
                                Frame::Room { room: snapshot, previews } => {
                                    store_previews(cache, previews, emit).await;
                                    validate_room(&snapshot, &invite.session, &room.host, local)?;
                                    install_room(room, anchors, snapshot);
                                    remember_seated(seated, room).await;
                                    broadcast(room, anchors, emit);
                                }
                                Frame::Ping(id) => send_frame(&send, &Frame::Pong(id))?,
                                Frame::Pong(_) => {
                                    let (route_name, rtt_ms) = snap(&route);
                                    emit(Event::Status(Status::Connected { route: route_name, rtt_ms }));
                                }
                                _ => return Err("Unexpected session message".into()),
                            }
                        }
                        Inbound::Want(hash) => {
                            if let Some(bytes) = offered_asset(offers, &hash).await {
                                let _ = send.send(Outbound::Asset { hash, bytes });
                            }
                        }
                        Inbound::Asset { hash, bytes } => {
                            inflight.remove(&hash);
                            if store_asset(cache, &hash, &bytes).await.is_ok() {
                                emit(Event::AssetReady(hash));
                            }
                        }
                    }
                }
                command = commands.recv() => match command.unwrap_or(Command::Leave) {
                    Command::Leave => {
                        let _ = send_frame(&send, &Frame::Leave);
                        let _ = send.send(Outbound::Close);
                        return Ok(true);
                    }
                    #[cfg(test)]
                    Command::Disconnect => {
                        let _ = send.send(Outbound::Close);
                        return Err("Test disconnect".into());
                    }
                },
                _ = timer.tick() => {
                    if last_seen.elapsed() > Duration::from_secs(15) {
                        return Err("Peer heartbeat timed out".into());
                    }
                    if heartbeat.elapsed() >= Duration::from_secs(2) {
                        heartbeat = Instant::now();
                        let (route_name, rtt_ms) = snap(&route);
                        emit(Event::Status(Status::Connected { route: route_name, rtt_ms }));
                        send_frame(&send, &Frame::Ping(rtt_ms))?;
                    }
                    if changes.has_changed().unwrap_or(false) {
                        let latest = changes.borrow_and_update().clone();
                        if let Some(publication) = latest {
                            let mut track = prepare(&publication.track, &publication.resources, offers, cache).await?;
                            track.age_ms = publication.published_at.elapsed().as_millis() as u64;
                            // A track over budget is not sent: the host would refuse it too,
                            // and dropping the pipe would only reconnect into the same track.
                            let accepted = room.update(local, track.clone());
                            if matches!(accepted, Ok(true)) {
                                anchors.insert(local.to_owned(), Instant::now());
                                broadcast(room, anchors, emit);
                            }
                            if accepted.is_ok() {
                                let previews = pending_previews(offers, cache, &picture_hashes(&track), sent_previews).await;
                                send_frame(&send, &Frame::Track { track, previews })?;
                            }
                        }
                    }
                    inflight.retain(|_, at| at.elapsed() < Duration::from_secs(15));
                    let mut dials = Vec::new();
                    let mut wants = Vec::new();
                    {
                        let offered = offers.read().await;
                        for hash in room.asset_hashes() {
                            let have = protocol::session_full_picture(cache, &hash).is_some();
                            if have
                                || offered.contains_key(&hash)
                                || inflight.contains_key(&hash)
                                || direct_inflight.contains(&hash)
                            {
                                continue;
                            }
                            if direct_attempts.get(&hash).copied().unwrap_or(0) < 2
                                && let Some(publisher) = direct_publisher(room, local, &hash)
                            {
                                if direct_inflight.len() + dials.len() < 2 {
                                    dials.push((hash, publisher));
                                }
                                continue;
                            }
                            if inflight.len() + wants.len() < 2 {
                                wants.push(hash);
                            }
                        }
                    }
                    for (hash, publisher) in dials {
                        direct_inflight.insert(hash.clone());
                        let tx = fetch_tx.clone();
                        let carrier = carrier.clone();
                        tokio::spawn(async move {
                            let result = fetch_asset(carrier, publisher, hash.clone()).await;
                            let _ = tx.send((hash, result));
                        });
                    }
                    for hash in wants {
                        send.send(Outbound::Want(hash.clone())).map_err(|_| "Connection closed".to_string())?;
                        inflight.insert(hash, Instant::now());
                    }
                }
                fetched = fetch_rx.recv() => {
                    let Some((hash, result)) = fetched else { continue };
                    direct_inflight.remove(&hash);
                    match result {
                        Ok(bytes) if store_asset(cache, &hash, &bytes).await.is_ok() => {
                            emit(Event::AssetReady(hash));
                        }
                        _ => {
                            *direct_attempts.entry(hash).or_default() += 1;
                        }
                    }
                }
            }
        }
    }
    .await;
    reader.abort();
    result
}

fn snap(route: &watch::Receiver<RouteSnap>) -> (String, u64) {
    let route = route.borrow();
    (route.name.clone(), route.rtt_ms)
}

async fn store_asset(cache: &Path, hash: &str, bytes: &[u8]) -> Result<(), String> {
    if !protocol::valid_hash(hash) || bytes.len() > picture::WIRE_ASSET {
        return Err("Asset exceeds limit".into());
    }
    if blake3::hash(bytes).to_hex().as_str() != hash {
        return Err("Asset hash mismatch".into());
    }
    if protocol::session_full_picture(cache, hash).is_some() {
        return Ok(());
    }
    let extension = match picture::wire_kind(bytes) {
        Some(picture::Kind::Webp) => "webp",
        Some(picture::Kind::Gif) => "gif",
        None => return Err("Unexpected picture".into()),
    };
    tokio::fs::create_dir_all(cache)
        .await
        .map_err(|error| error.to_string())?;
    let temp = cache.join(format!("p2p-{hash}.partial"));
    let final_path = cache.join(format!("p2p-{hash}.{extension}"));
    tokio::fs::write(&temp, bytes)
        .await
        .map_err(|error| error.to_string())?;
    tokio::fs::rename(temp, final_path)
        .await
        .map_err(|error| error.to_string())
}

fn member(room: &Room, id: &str) -> bool {
    id == room.host || room.guests.iter().any(|guest| guest == id)
}

fn known_participant(room: &Room, id: &str) -> bool {
    member(room, id)
        || room.tracks.contains_key(id)
        || room.entries.iter().any(|entry| entry.participant == id)
}

fn validate_room(room: &Room, session: &str, host: &str, local: &str) -> Result<(), String> {
    let unique = room.guests.len() == room.guests.iter().collect::<BTreeSet<_>>().len();
    if room.session != session
        || room.host != host
        || !unique
        || room.guests.len() + 1 > MAX_MEMBERS
        || room.guests.iter().any(|id| id.is_empty() || id == host)
        || !room.guests.iter().any(|id| id == local)
        || room.entries.len() > protocol::MAX_ENTRIES
        || room.tracks.len() > protocol::MAX_TRACKS
    {
        return Err("Invalid room snapshot".into());
    }
    if room
        .tracks
        .keys()
        .any(|id| !member(room, id) && !room.entries.iter().any(|entry| &entry.participant == id))
    {
        return Err("Unknown participant".into());
    }
    if room.retired_before.len() > protocol::MAX_TRACKS
        || room
            .retired_before
            .keys()
            .any(|id| !known_participant(room, id))
        || room
            .entries
            .iter()
            .any(|entry| !known_participant(room, &entry.participant))
        || room
            .entries
            .windows(2)
            .any(|pair| pair[0].order >= pair[1].order)
    {
        return Err("Invalid session history".into());
    }
    for track in room.tracks.values() {
        track.clone().validate()?;
    }
    Ok(())
}

fn install_room(room: &mut Room, anchors: &mut BTreeMap<String, Instant>, mut snapshot: Room) {
    // A journal acknowledgement may arrive before the host sees the latest
    // local mirror. Keep newer tracks and their elapsed playback time. A
    // track the host dropped for someone who left stays dropped.
    for (owner, track) in &room.tracks {
        let newer = match snapshot.tracks.get(owner) {
            Some(incoming) => incoming.revision < track.revision,
            None => member(&snapshot, owner),
        };
        if newer {
            let mut track = track.clone();
            track.age_ms = track.age_ms.saturating_add(
                anchors
                    .get(owner)
                    .map_or(0, |at| at.elapsed().as_millis() as u64),
            );
            snapshot.tracks.insert(owner.clone(), track);
        }
    }
    *anchors = snapshot
        .tracks
        .keys()
        .map(|id| (id.clone(), Instant::now()))
        .collect();
    *room = snapshot;
}

/// The session secret admits an invited friend. The room password admits
/// everyone else. Either match is enough; an empty password never matches.
fn credential_matches(presented: &str, secret: &str, password: &str) -> bool {
    let presented = blake3::hash(presented.as_bytes());
    presented == blake3::hash(secret.as_bytes())
        || (!password.is_empty() && presented == blake3::hash(password.as_bytes()))
}

fn random_secret() -> String {
    blake3::hash(&rand::random::<[u8; 32]>())
        .to_hex()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::Hub;
    use crate::protocol::tests::track_with;
    use nanatype_core::DisplayView;
    use nanatype_model::{Entrance, EntranceKind};
    use std::sync::mpsc as channel;

    #[track_caller]
    fn next(rx: &channel::Receiver<Event>, predicate: impl Fn(&Event) -> bool) -> Event {
        let until = Instant::now() + Duration::from_secs(15);
        loop {
            let event = rx
                .recv_timeout(until.saturating_duration_since(Instant::now()))
                .expect("session event before timeout");
            if let Event::Status(Status::Error(error)) = &event {
                panic!("session failed: {error}");
            }
            if predicate(&event) {
                return event;
            }
        }
    }

    fn config(name: &str) -> Config {
        Config {
            cache: std::env::temp_dir().join(format!("nanatype-p2p-{}-{name}", random_secret())),
        }
    }

    fn track(revision: u64) -> TrackSnapshot {
        TrackSnapshot {
            revision,
            captured_ms: 1,
            age_ms: 0,
            name: "partner".into(),
            display: DisplayView::empty(Entrance {
                kind: EntranceKind::None,
                ms: 0,
            }),
            rows: Vec::new(),
            portraits: Vec::new(),
        }
    }

    #[test]
    fn two_endpoints_exchange_ordered_history_reconnect_and_leave() {
        let hub = Hub::new();
        let (host_tx, host_rx) = channel::channel();
        let host = PeerHandle::start(config("host"), None, "", hub.carrier(1), move |e| {
            let _ = host_tx.send(e);
        })
        .unwrap();
        let Event::Started { invitation, .. } =
            next(&host_rx, |e| matches!(e, Event::Started { .. }))
        else {
            unreachable!()
        };
        let (guest_tx, guest_rx) = channel::channel();
        let guest = PeerHandle::start(
            config("guest"),
            Some(invitation),
            "",
            hub.carrier(2),
            move |e| {
                let _ = guest_tx.send(e);
            },
        )
        .unwrap();
        next(&host_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        next(&guest_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        host.publish(track_with(1, &[1]), BTreeMap::new());
        guest.publish(track_with(1, &[1]), BTreeMap::new());
        let has_two = |e: &Event| matches!(e, Event::Room { room, .. } if room.entries.len() == 2 && room.tracks.len() == 2);
        let Event::Room { room: a, .. } = next(&host_rx, has_two) else {
            unreachable!()
        };
        let Event::Room { room: b, .. } = next(&guest_rx, has_two) else {
            unreachable!()
        };
        assert_eq!(a.entries, b.entries);
        guest.disconnect();
        next(&guest_rx, |e| {
            matches!(e, Event::Status(Status::Reconnecting))
        });
        // A dropped pipe frees the seat. The same person can sit down again.
        next(
            &host_rx,
            |e| matches!(e, Event::Room { room, .. } if room.guests.is_empty()),
        );
        next(&guest_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        // The republished track cannot duplicate ordered history; a new row is
        // ordered once on both ends.
        guest.publish(track_with(2, &[1, 2]), BTreeMap::new());
        let has_three =
            |e: &Event| matches!(e, Event::Room { room, .. } if room.entries.len() == 3);
        let Event::Room { room: a, .. } = next(&host_rx, has_three) else {
            unreachable!()
        };
        let Event::Room { room: b, .. } = next(&guest_rx, has_three) else {
            unreachable!()
        };
        assert_eq!(a.entries, b.entries);
        drop(guest);
        // One guest leaving frees the seat and leaves the room open.
        next(&host_rx, |e| matches!(e, Event::Status(Status::Waiting)));
        drop(host);
    }

    fn join(hub: &Hub, invitation: &str, id: u64) -> (PeerHandle, channel::Receiver<Event>) {
        let (tx, rx) = channel::channel();
        let handle = PeerHandle::start(
            config(&format!("member-{id}")),
            Some(invitation.to_string()),
            "",
            hub.carrier(id),
            move |event| {
                let _ = tx.send(event);
            },
        )
        .unwrap();
        (handle, rx)
    }

    #[test]
    fn three_people_share_history_and_one_can_leave() {
        let hub = Hub::new();
        let (host_tx, host_rx) = channel::channel();
        let host = PeerHandle::start(config("trio-host"), None, "", hub.carrier(1), move |e| {
            let _ = host_tx.send(e);
        })
        .unwrap();
        let Event::Started { invitation, .. } =
            next(&host_rx, |e| matches!(e, Event::Started { .. }))
        else {
            unreachable!()
        };
        let (guest, guest_rx) = join(&hub, &invitation, 2);
        let (third, third_rx) = join(&hub, &invitation, 3);
        next(&guest_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        next(&third_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        host.publish(track_with(1, &[1]), BTreeMap::new());
        guest.publish(track_with(1, &[1]), BTreeMap::new());
        third.publish(track_with(1, &[1]), BTreeMap::new());
        let has_three = |e: &Event| matches!(e, Event::Room { room, .. } if room.entries.len() == 3 && room.guests.len() == 2);
        let Event::Room {
            room: host_room, ..
        } = next(&host_rx, has_three)
        else {
            unreachable!()
        };
        let Event::Room {
            room: guest_room, ..
        } = next(&guest_rx, has_three)
        else {
            unreachable!()
        };
        let Event::Room {
            room: third_room, ..
        } = next(&third_rx, has_three)
        else {
            unreachable!()
        };
        assert_eq!(host_room.entries, guest_room.entries);
        assert_eq!(host_room.entries, third_room.entries);
        drop(third);
        next(
            &host_rx,
            |e| matches!(e, Event::Room { room, .. } if room.guests.len() == 1),
        );
        guest.publish(track_with(2, &[1, 2]), BTreeMap::new());
        let has_four = |e: &Event| matches!(e, Event::Room { room, .. } if room.entries.len() == 4);
        let Event::Room {
            room: host_room, ..
        } = next(&host_rx, has_four)
        else {
            unreachable!()
        };
        let Event::Room {
            room: guest_room, ..
        } = next(&guest_rx, has_four)
        else {
            unreachable!()
        };
        assert_eq!(host_room.entries, guest_room.entries);
        assert!(host_room.guests.iter().any(|id| id == "2"));
        assert!(!host_room.guests.iter().any(|id| id == "3"));
        drop(guest);
        drop(host);
    }

    #[test]
    fn someone_leaving_before_their_first_line_keeps_the_others_seated() {
        let hub = Hub::new();
        let (host_tx, host_rx) = channel::channel();
        let host = PeerHandle::start(config("quiet-host"), None, "", hub.carrier(1), move |e| {
            let _ = host_tx.send(e);
        })
        .unwrap();
        let Event::Started { invitation, .. } =
            next(&host_rx, |e| matches!(e, Event::Started { .. }))
        else {
            unreachable!()
        };
        let (guest, guest_rx) = join(&hub, &invitation, 2);
        let (quiet, quiet_rx) = join(&hub, &invitation, 3);
        next(&guest_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        next(&quiet_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        // The app publishes on start, so a guest has a track before any line.
        guest.publish(track_with(1, &[]), BTreeMap::new());
        quiet.publish(track_with(1, &[]), BTreeMap::new());
        next(
            &guest_rx,
            |e| matches!(e, Event::Room { room, .. } if room.tracks.contains_key("3")),
        );
        drop(quiet);
        next(
            &guest_rx,
            |e| matches!(e, Event::Room { room, .. } if room.guests.len() == 1 && !room.tracks.contains_key("3")),
        );
        // Still seated: a new line reaches both ends.
        guest.publish(track_with(2, &[1]), BTreeMap::new());
        next(
            &host_rx,
            |e| matches!(e, Event::Room { room, .. } if room.entries.len() == 1),
        );
        next(
            &guest_rx,
            |e| matches!(e, Event::Room { room, .. } if room.entries.len() == 1),
        );
        while let Ok(event) = guest_rx.try_recv() {
            assert!(!matches!(event, Event::Status(Status::Reconnecting)));
        }
        drop(guest);
        drop(host);
    }

    #[test]
    fn a_silent_connection_does_not_hold_up_the_next_guest() {
        let hub = Hub::new();
        let (host_tx, host_rx) = channel::channel();
        let host = PeerHandle::start(config("silent-host"), None, "", hub.carrier(1), move |e| {
            let _ = host_tx.send(e);
        })
        .unwrap();
        let Event::Started { invitation, .. } =
            next(&host_rx, |e| matches!(e, Event::Started { .. }))
        else {
            unreachable!()
        };
        // Any owner of the app can open a session pipe and never say hello.
        let silent = hub
            .carrier(66)
            .connect(Some(1), Arc::new(AtomicBool::new(false)))
            .unwrap();
        let started = Instant::now();
        let (guest, guest_rx) = join(&hub, &invitation, 2);
        next(&guest_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "the guest waited behind the silent pipe: {:?}",
            started.elapsed()
        );
        drop(silent);
        drop(guest);
        drop(host);
    }

    #[test]
    fn the_ninth_person_is_refused_until_a_seat_opens() {
        let hub = Hub::new();
        let (host_tx, host_rx) = channel::channel();
        let host = PeerHandle::start(config("full-host"), None, "", hub.carrier(1), move |e| {
            let _ = host_tx.send(e);
        })
        .unwrap();
        let Event::Started { invitation, .. } =
            next(&host_rx, |e| matches!(e, Event::Started { .. }))
        else {
            unreachable!()
        };
        let mut seated = Vec::new();
        for id in 2..=8 {
            let (handle, rx) = join(&hub, &invitation, id);
            next(&rx, |e| {
                matches!(e, Event::Status(Status::Connected { .. }))
            });
            seated.push(handle);
        }
        next(
            &host_rx,
            |e| matches!(e, Event::Room { room, .. } if room.guests.len() == MAX_MEMBERS - 1),
        );
        let (full_tx, full_rx) = channel::channel();
        let refused = PeerHandle::start(
            config("full-refused"),
            Some(invitation.clone()),
            "",
            hub.carrier(9),
            move |event| {
                let _ = full_tx.send(event);
            },
        )
        .unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            let event = full_rx
                .recv_timeout(until.saturating_duration_since(Instant::now()))
                .unwrap();
            if let Event::Status(Status::Error(error)) = &event {
                assert_eq!(error, "Room is full");
                break;
            }
            assert!(!matches!(event, Event::Status(Status::Connected { .. })));
        }
        drop(refused);
        drop(seated.remove(0));
        next(
            &host_rx,
            |e| matches!(e, Event::Room { room, .. } if room.guests.len() == MAX_MEMBERS - 2),
        );
        let (late, late_rx) = join(&hub, &invitation, 10);
        next(&late_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        next(
            &host_rx,
            |e| matches!(e, Event::Room { room, .. } if room.guests.len() == MAX_MEMBERS - 1 && room.guests.iter().any(|id| id == "10")),
        );
        drop(late);
        drop(seated);
        drop(host);
    }

    #[test]
    fn network_assets_cannot_reference_receiver_files() {
        let mut value = serde_json::json!({"doc":{"asset":"file:C:/secret.png"}, "user":{"avatar":"C:/secret.png"}});
        sanitize_assets(&mut value);
        assert_eq!(value["doc"]["asset"], "blob:p2p-missing");
        assert!(value["user"]["avatar"].is_null());
    }

    #[test]
    fn journal_acknowledgements_preserve_newer_local_mirrors() {
        let mut room = Room::new("session".into(), "host".into());
        room.guests.push("guest".into());
        room.update("guest", track(9)).unwrap();
        let mut snapshot = room.clone();
        snapshot.tracks.get_mut("guest").unwrap().revision = 8;
        snapshot.tracks.get_mut("guest").unwrap().name = "stale".into();
        let mut anchors =
            BTreeMap::from([("guest".into(), Instant::now() - Duration::from_millis(100))]);
        install_room(&mut room, &mut anchors, snapshot);
        assert_eq!(room.tracks["guest"].revision, 9);
        assert_eq!(room.tracks["guest"].name, "partner");
        assert!(room.tracks["guest"].age_ms >= 100);
    }

    #[test]
    fn bad_credentials_are_rejected_without_claiming_the_guest_slot() {
        let hub = Hub::new();
        let (host_tx, host_rx) = channel::channel();
        let host = PeerHandle::start(
            config("auth-host"),
            None,
            "",
            hub.carrier(1),
            move |event| {
                let _ = host_tx.send(event);
            },
        )
        .unwrap();
        let Event::Started { invitation, .. } =
            next(&host_rx, |e| matches!(e, Event::Started { .. }))
        else {
            unreachable!()
        };
        let mut bad = Invite::decode(&invitation).unwrap();
        bad.secret = "0".repeat(64);
        let (bad_tx, bad_rx) = channel::channel();
        let intruder = PeerHandle::start(
            config("intruder"),
            Some(bad.encode()),
            "",
            hub.carrier(3),
            move |e| {
                let _ = bad_tx.send(e);
            },
        )
        .unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            let event = bad_rx
                .recv_timeout(until.saturating_duration_since(Instant::now()))
                .unwrap();
            if matches!(event, Event::Status(Status::Error(_))) {
                break;
            }
            assert!(!matches!(event, Event::Status(Status::Connected { .. })));
        }
        drop(intruder);
        let (guest_tx, guest_rx) = channel::channel();
        let guest = PeerHandle::start(
            config("auth-guest"),
            Some(invitation),
            "",
            hub.carrier(2),
            move |e| {
                let _ = guest_tx.send(e);
            },
        )
        .unwrap();
        next(&guest_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        drop(guest);
        drop(host);
    }

    #[test]
    fn a_room_password_is_required_and_stays_out_of_the_invitation() {
        let hub = Hub::new();
        let (host_tx, host_rx) = channel::channel();
        let host = PeerHandle::start(
            config("password-host"),
            None,
            "nana-room",
            hub.carrier(1),
            move |event| {
                let _ = host_tx.send(event);
            },
        )
        .unwrap();
        let Event::Started {
            invitation, admit, ..
        } = next(&host_rx, |e| matches!(e, Event::Started { .. }))
        else {
            unreachable!()
        };
        let shared = Invite::decode(&invitation).unwrap();
        assert!(shared.locked && shared.secret.is_empty());
        assert!(!invitation.contains("nana-room") && !admit.contains("nana-room"));
        let friend = Invite::decode(&admit).unwrap();
        assert!(!friend.locked && friend.secret.len() == 64);
        assert!(
            PeerHandle::start(
                config("password-missing"),
                Some(invitation.clone()),
                "",
                hub.carrier(3),
                |_| {},
            )
            .is_err()
        );
        let (bad_tx, bad_rx) = channel::channel();
        let intruder = PeerHandle::start(
            config("password-wrong"),
            Some(invitation.clone()),
            "nope",
            hub.carrier(3),
            move |event| {
                let _ = bad_tx.send(event);
            },
        )
        .unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            let event = bad_rx
                .recv_timeout(until.saturating_duration_since(Instant::now()))
                .unwrap();
            if matches!(event, Event::Status(Status::Error(_))) {
                break;
            }
            assert!(!matches!(event, Event::Status(Status::Connected { .. })));
        }
        drop(intruder);
        let (guest_tx, guest_rx) = channel::channel();
        let guest = PeerHandle::start(
            config("password-guest"),
            Some(invitation),
            "nana-room",
            hub.carrier(2),
            move |event| {
                let _ = guest_tx.send(event);
            },
        )
        .unwrap();
        next(&guest_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        next(&host_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        drop(guest);
        drop(host);
    }

    #[test]
    fn an_invited_friend_skips_the_room_password() {
        let hub = Hub::new();
        let (host_tx, host_rx) = channel::channel();
        let host = PeerHandle::start(
            config("admit-host"),
            None,
            "nana-room",
            hub.carrier(1),
            move |event| {
                let _ = host_tx.send(event);
            },
        )
        .unwrap();
        let Event::Started { admit, .. } = next(&host_rx, |e| matches!(e, Event::Started { .. }))
        else {
            unreachable!()
        };
        let (guest_tx, guest_rx) = channel::channel();
        let guest = PeerHandle::start(
            config("admit-guest"),
            Some(admit),
            "",
            hub.carrier(2),
            move |event| {
                let _ = guest_tx.send(event);
            },
        )
        .unwrap();
        next(&guest_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        next(&host_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        drop(guest);
        drop(host);
    }

    #[test]
    fn session_frames_roundtrip_through_zlib() {
        let bytes = pack(&Frame::Ping(7)).unwrap();
        assert_eq!(bytes.first().copied(), Some(0x78));
        let frame: Frame = serde_json::from_slice(&unpack(&bytes, MAX_FRAME).unwrap()).unwrap();
        assert!(matches!(frame, Frame::Ping(7)));
    }

    #[test]
    fn image_bytes_travel_on_a_separate_stream_and_are_verified() {
        let host_config = config("image-host");
        let guest_config = config("image-guest");
        std::fs::create_dir_all(&host_config.cache).unwrap();
        let image_path = host_config.cache.join("source.png");
        let source = image::RgbaImage::from_pixel(300, 40, image::Rgba([20, 40, 80, 255]));
        source.save(&image_path).unwrap();
        let guest_cache = guest_config.cache.clone();
        let hub = Hub::new();
        let (host_tx, host_rx) = channel::channel();
        let host = PeerHandle::start(host_config.clone(), None, "", hub.carrier(1), move |e| {
            let _ = host_tx.send(e);
        })
        .unwrap();
        let Event::Started { invitation, .. } =
            next(&host_rx, |e| matches!(e, Event::Started { .. }))
        else {
            unreachable!()
        };
        let (guest_tx, guest_rx) = channel::channel();
        let guest = PeerHandle::start(
            guest_config,
            Some(invitation),
            "",
            hub.carrier(2),
            move |e| {
                let _ = guest_tx.send(e);
            },
        )
        .unwrap();
        next(&host_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        let reference = format!("file:{}", image_path.display());
        let mut snapshot = track(1);
        snapshot.display.reply = Some(nanatype_model::ChatRef::new(
            nanatype_model::ChatUser {
                uid: "x".into(),
                name: "viewer".into(),
                avatar: Some(nanatype_model::AssetRef::File(
                    image_path.to_string_lossy().into_owned(),
                )),
            },
            "hello",
            nanatype_model::SourceKind::Chat,
        ));
        host.publish(snapshot, BTreeMap::from([(reference, image_path.clone())]));
        let hash = loop {
            let Event::AssetReady(hash) = next(&guest_rx, |e| matches!(e, Event::AssetReady(_)))
            else {
                unreachable!()
            };
            if protocol::session_full_picture(&guest_cache, &hash).is_some() {
                break hash;
            }
        };
        let full = protocol::session_full_picture(&guest_cache, &hash).unwrap();
        let bytes = std::fs::read(&full).unwrap();
        assert_eq!(blake3::hash(&bytes).to_hex().as_str(), hash);
        assert_eq!(picture::wire_kind(&bytes), Some(picture::Kind::Webp));
        let image = image::load_from_memory(&bytes).unwrap();
        assert!(image.width().max(image.height()) <= 256);
        assert!(protocol::session_picture(&guest_cache, &hash).is_some());
        assert!(
            guest_cache
                .join(format!("p2p-{hash}.preview.webp"))
                .is_file()
        );
        drop(guest);
        drop(host);
        // No recursive cleanup: only files created by this test.
        let _ = std::fs::remove_file(&image_path);
        let _ = std::fs::remove_file(host_config.cache.join(format!("p2p-{hash}.webp")));
        let _ = std::fs::remove_dir(&host_config.cache);
        let _ = std::fs::remove_file(&full);
        let _ = std::fs::remove_file(guest_cache.join(format!("p2p-{hash}.preview.webp")));
        let _ = std::fs::remove_dir(&guest_cache);
    }

    #[test]
    fn a_guests_picture_reaches_the_other_guest() {
        share_a_picture(true);
    }

    #[test]
    fn a_guest_asks_the_host_when_the_direct_picture_link_fails() {
        share_a_picture(false);
    }

    fn share_a_picture(direct: bool) {
        let source_config = config("relay-source");
        std::fs::create_dir_all(&source_config.cache).unwrap();
        let image_path = source_config.cache.join("source.png");
        image::RgbaImage::from_pixel(80, 40, image::Rgba([20, 40, 80, 255]))
            .save(&image_path)
            .unwrap();
        let other_config = config("relay-other");
        let other_cache = other_config.cache.clone();
        let hub = Hub::new();
        if !direct {
            hub.fail_asset_dials();
        }
        let (host_tx, host_rx) = channel::channel();
        let host = PeerHandle::start(config("relay-host"), None, "", hub.carrier(1), move |e| {
            let _ = host_tx.send(e);
        })
        .unwrap();
        let Event::Started { invitation, .. } =
            next(&host_rx, |e| matches!(e, Event::Started { .. }))
        else {
            unreachable!()
        };
        let (source_tx, source_rx) = channel::channel();
        let source = PeerHandle::start(
            source_config,
            Some(invitation.clone()),
            "",
            hub.carrier(2),
            move |event| {
                let _ = source_tx.send(event);
            },
        )
        .unwrap();
        let (other_tx, other_rx) = channel::channel();
        let other = PeerHandle::start(
            other_config,
            Some(invitation),
            "",
            hub.carrier(3),
            move |event| {
                let _ = other_tx.send(event);
            },
        )
        .unwrap();
        next(&source_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        next(&other_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        next(
            &source_rx,
            |e| matches!(e, Event::Room { room, .. } if room.guests.len() == 2),
        );
        let reference = format!("file:{}", image_path.display());
        let mut snapshot = track(1);
        snapshot.display.reply = Some(nanatype_model::ChatRef::new(
            nanatype_model::ChatUser {
                uid: "x".into(),
                name: "viewer".into(),
                avatar: Some(nanatype_model::AssetRef::File(
                    image_path.to_string_lossy().into_owned(),
                )),
            },
            "hello",
            nanatype_model::SourceKind::Chat,
        ));
        source.publish(snapshot, BTreeMap::from([(reference, image_path.clone())]));
        let hash = loop {
            let Event::AssetReady(hash) = next(&other_rx, |e| matches!(e, Event::AssetReady(_)))
            else {
                unreachable!()
            };
            if protocol::session_full_picture(&other_cache, &hash).is_some() {
                break hash;
            }
        };
        let full = protocol::session_full_picture(&other_cache, &hash).unwrap();
        let bytes = std::fs::read(&full).unwrap();
        assert_eq!(blake3::hash(&bytes).to_hex().as_str(), hash);
        assert!(
            other_cache
                .join(format!("p2p-{hash}.preview.webp"))
                .is_file()
        );
        if direct {
            assert!(hub.asset_pairs() >= 1);
        } else {
            assert_eq!(hub.asset_pairs(), 0);
        }
        drop(other);
        drop(source);
        drop(host);
        let _ = std::fs::remove_file(&image_path);
        let _ = std::fs::remove_file(&full);
        let _ = std::fs::remove_file(other_cache.join(format!("p2p-{hash}.preview.webp")));
    }
}
