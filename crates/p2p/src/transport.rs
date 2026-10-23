use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use iroh::{
    Endpoint, RelayMap, RelayMode,
    endpoint::{Connection, RecvStream, SendStream, presets},
};
use nanatype_model::Line;
use serde::{Serialize, de::DeserializeOwned};
use tokio::sync::{RwLock, mpsc, watch};

use crate::protocol::{
    self, ALPN, Frame, Invite, MAX_ASSET, MAX_FRAME, Room, TrackSnapshot, VERSION,
};

/// Only explicitly referenced local assets are offered to a peer.
pub type Resources = BTreeMap<String, PathBuf>;
type Emit = Arc<dyn Fn(Event) + Send + Sync>;
#[derive(Clone)]
struct AssetOffer {
    path: PathBuf,
    len: u64,
    modified: Option<std::time::SystemTime>,
}
type Offers = Arc<RwLock<BTreeMap<String, AssetOffer>>>;

#[derive(Clone)]
pub struct Config {
    pub cache: PathBuf,
    pub relay: Option<String>,
    /// Useful for deterministic LAN tests, without public infrastructure.
    pub local_only: bool,
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
        invitation: String,
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
    Submit {
        sequence: u64,
        line: Line,
        avatar: Option<String>,
        resources: Resources,
    },
    #[cfg(test)]
    Disconnect,
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
        emit: impl Fn(Event) + Send + Sync + 'static,
    ) -> Result<Self, String> {
        let invite = invitation.as_deref().map(Invite::decode).transpose()?;
        if let Some(relay) = &config.relay {
            relay.parse::<iroh::RelayUrl>().map_err(|e| e.to_string())?;
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
                    if let Err(e) = run(config, invite, rx, changes, stopped, emit.clone()).await {
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

    pub fn submit(
        &self,
        sequence: u64,
        line: Line,
        avatar: Option<String>,
        resources: Resources,
    ) -> bool {
        self.tx
            .send(Command::Submit {
                sequence,
                line,
                avatar,
                resources,
            })
            .is_ok()
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

async fn write<T: Serialize>(send: &mut SendStream, value: &T) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    if bytes.len() > MAX_FRAME {
        return Err("Session frame exceeds limit".into());
    }
    tokio::time::timeout(Duration::from_secs(10), async {
        send.write_all(&(bytes.len() as u32).to_be_bytes())
            .await
            .map_err(|e| e.to_string())?;
        send.write_all(&bytes).await.map_err(|e| e.to_string())
    })
    .await
    .map_err(|_| "Session write timed out".to_string())?
}

/// An unauthenticated peer may only send a hello this large.
const MAX_HELLO: usize = 1024;
/// Sustained frames per second accepted from a peer; more are delayed.
const MAX_FRAMES_PER_SECOND: u32 = 40;

async fn read<T: DeserializeOwned>(recv: &mut RecvStream) -> Result<T, String> {
    read_limited(recv, MAX_FRAME).await
}

async fn read_limited<T: DeserializeOwned>(recv: &mut RecvStream, max: usize) -> Result<T, String> {
    let mut len = [0; 4];
    recv.read_exact(&mut len).await.map_err(|e| e.to_string())?;
    let len = u32::from_be_bytes(len) as usize;
    if len > max {
        return Err("Session frame exceeds limit".into());
    }
    let mut bytes = vec![0; len];
    recv.read_exact(&mut bytes)
        .await
        .map_err(|e| e.to_string())?;
    let mut value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    sanitize_assets(&mut value);
    serde_json::from_value(value).map_err(|e| e.to_string())
}

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
) -> Result<T, String> {
    let mut replacements = BTreeMap::new();
    for (reference, path) in resources {
        let Ok(meta) = tokio::fs::metadata(path).await else {
            continue;
        };
        if meta.len() > MAX_ASSET as u64 {
            continue;
        }
        let cached = offers
            .read()
            .await
            .iter()
            .find(|(_, offer)| {
                offer.path == *path
                    && offer.len == meta.len()
                    && offer.modified == meta.modified().ok()
            })
            .map(|(hash, _)| hash.clone());
        if let Some(hash) = cached {
            replacements.insert(reference.clone(), format!("blob:p2p-{hash}"));
            continue;
        }
        let Ok(bytes) = tokio::fs::read(path).await else {
            continue;
        };
        let hash = blake3::hash(&bytes).to_hex().to_string();
        offers.write().await.insert(
            hash.clone(),
            AssetOffer {
                path: path.clone(),
                len: meta.len(),
                modified: meta.modified().ok(),
            },
        );
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
    let mut value = serde_json::to_value(value).map_err(|e| e.to_string())?;
    replace(&mut value, &replacements);
    // Never expose absolute local paths, library names or skin-local resources.
    // Unresolved references use a non-resolvable placeholder instead.
    sanitize_assets(&mut value);
    serde_json::from_value(value).map_err(|e| e.to_string())
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
    invite: Option<Invite>,
    mut commands: mpsc::UnboundedReceiver<Command>,
    mut changes: watch::Receiver<Option<Arc<Publication>>>,
    mut stopped: watch::Receiver<bool>,
    emit: Emit,
) -> Result<(), String> {
    emit(Event::Status(Status::Starting));
    let mut builder = Endpoint::builder(presets::N0).alpns(vec![ALPN.to_vec()]);
    if config.local_only {
        builder = builder.relay_mode(RelayMode::Disabled);
    } else if let Some(relay) = &config.relay {
        builder = builder.relay_mode(RelayMode::Custom(RelayMap::from_iter([relay
            .parse::<iroh::RelayUrl>()
            .map_err(|e| e.to_string())?])));
    }
    let endpoint = builder.bind().await.map_err(|e| e.to_string())?;
    nanatype_diag::event!(
        crate::diag::ENDPOINT_BOUND,
        local_only = config.local_only,
        custom_relay = config.relay.is_some(),
    );
    if !config.local_only {
        let started = std::time::Instant::now();
        match tokio::time::timeout(Duration::from_secs(5), endpoint.online()).await {
            Ok(_) => nanatype_diag::event!(
                crate::diag::ONLINE,
                ms = started.elapsed().as_millis() as u64
            ),
            Err(_) => nanatype_diag::event!(crate::diag::ONLINE_TIMEOUT),
        }
    }
    let local = endpoint.id().to_string();
    let hosting = invite.is_none();
    let invite = invite.unwrap_or_else(|| Invite {
        v: VERSION,
        session: random_secret(),
        secret: random_secret(),
        addr: endpoint.addr(),
    });
    let mut room = Room::new(invite.session.clone(), invite.addr.id.to_string());
    let mut anchors = BTreeMap::new();
    let offers: Offers = Arc::new(RwLock::new(BTreeMap::new()));
    let mut pinned: Option<String> = None;
    let mut pending: Vec<Frame> = Vec::new();
    let mut connected_once = false;
    emit(Event::Started {
        host: hosting,
        local_id: local.clone(),
        invitation: if hosting {
            invite.encode()
        } else {
            String::new()
        },
    });

    loop {
        emit(Event::Status(if connected_once {
            Status::Reconnecting
        } else if hosting {
            Status::Waiting
        } else {
            Status::Starting
        }));
        let attempt = async {
            if hosting {
                loop {
                    let incoming = endpoint.accept().await.ok_or("Endpoint closed")?;
                    let result = tokio::time::timeout(Duration::from_secs(5), async {
                        let conn = incoming.await.map_err(|e| e.to_string())?;
                        let remote = conn.remote_id().to_string();
                        if pinned.as_ref().is_some_and(|id| id != &remote) {
                            conn.close(1u8.into(), b"room full");
                            return Err("Room full".into());
                        }
                        let (mut send, mut recv) =
                            conn.accept_bi().await.map_err(|e| e.to_string())?;
                        match read_limited::<Frame>(&mut recv, MAX_HELLO).await? {
                            // blake3::Hash compares in constant time.
                            Frame::Hello { v, session, secret }
                                if v == VERSION
                                    && session == invite.session
                                    && blake3::hash(secret.as_bytes())
                                        == blake3::hash(invite.secret.as_bytes()) => {}
                            _ => {
                                conn.close(1u8.into(), b"unauthorized");
                                return Err("Invalid session credentials".into());
                            }
                        }
                        pinned = Some(remote.clone());
                        room.guest = Some(remote);
                        let snapshot = broadcast(&room, &anchors, &emit);
                        write(&mut send, &Frame::Welcome(snapshot)).await?;
                        Ok::<_, String>((conn, send, recv))
                    })
                    .await;
                    if let Ok(Ok(connection)) = result {
                        break Ok(connection);
                    }
                }
            } else {
                let conn = endpoint
                    .connect(invite.addr.clone(), ALPN)
                    .await
                    .map_err(|e| e.to_string())?;
                let (mut send, mut recv) = conn.open_bi().await.map_err(|e| e.to_string())?;
                write(
                    &mut send,
                    &Frame::Hello {
                        v: VERSION,
                        session: invite.session.clone(),
                        secret: invite.secret.clone(),
                    },
                )
                .await?;
                match read::<Frame>(&mut recv).await? {
                    Frame::Welcome(snapshot) => {
                        validate_room(&snapshot, &invite.session, &room.host, &local)?;
                        install_room(&mut room, &mut anchors, snapshot);
                    }
                    _ => return Err("Invalid handshake".into()),
                }
                Ok((conn, send, recv))
            }
        };
        let (conn, mut send, mut recv) = match tokio::select! {
            result = tokio::time::timeout(Duration::from_secs(20), attempt) => result,
            _ = stopped.changed() => { endpoint.close().await; return Ok(()); }
        } {
            Ok(Ok(c)) => c,
            result => {
                if !hosting && !connected_once {
                    return Err(match result {
                        Ok(Err(e)) => e,
                        _ => "Connection timed out".into(),
                    });
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
        };
        connected_once = true;
        emit(Event::Status(Status::Connected {
            route: route(&conn),
            rtt_ms: 0,
        }));
        broadcast(&room, &anchors, &emit);
        // Resend unacknowledged submissions. Host-side sequence checks make this idempotent.
        if !hosting {
            let accepted = *room.accepted.get(&local).unwrap_or(&0);
            pending.retain(|f| matches!(f, Frame::Submit { sequence, .. } if *sequence > accepted));
            let mut resend_failed = false;
            for frame in &pending {
                if write(&mut send, frame).await.is_err() {
                    resend_failed = true;
                    break;
                }
            }
            if resend_failed {
                conn.close(0u8.into(), b"retry");
                continue;
            }
        }
        let (in_tx, mut in_rx) = mpsc::channel(32);
        let reader = tokio::spawn(async move {
            let mut window = Instant::now();
            let mut frames = 0;
            loop {
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
                let frame = read::<Frame>(&mut recv).await;
                let done = frame.is_err();
                if in_tx.send(frame).await.is_err() || done {
                    break;
                }
            }
        });
        let asset_server = tokio::spawn(serve_assets(conn.clone(), offers.clone()));
        let mut fetched = BTreeSet::new();
        let mut transfers = tokio::task::JoinSet::new();
        let mut timer = tokio::time::interval(Duration::from_millis(50));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut heartbeat = Instant::now();
        let ping_start = Instant::now();
        let mut last_seen = Instant::now();
        // A reconnect must republish even if no new edit happened.
        changes.mark_changed();
        let result: Result<bool, String> = async {
            loop {
                tokio::select! {
                    _ = stopped.changed() => {
                        let _ = write(&mut send, &Frame::Leave).await;
                        let _ = send.finish();
                        let _ = tokio::time::timeout(Duration::from_millis(250), conn.closed()).await;
                        return Ok(true);
                    },
                    frame = in_rx.recv() => {
                        let frame = frame.ok_or("Connection reader ended")??;
                        last_seen = Instant::now();
                        let remote = conn.remote_id().to_string();
                        let mut dirty = false;
                        match frame {
                            Frame::Leave => return Ok(true),
                            Frame::Submit { sequence, line, avatar } if hosting => {
                                dirty = room.submit(&remote, sequence, line, avatar);
                            },
                            Frame::Track(track) if hosting => {
                                if room.update(&remote, track)? {
                                    anchors.insert(remote, Instant::now());
                                    broadcast(&room, &anchors, &emit);
                                }
                            },
                            Frame::PeerTrack { participant, track } if !hosting && participant == room.host => {
                                if room.update(&participant, track)? {
                                    anchors.insert(participant, Instant::now());
                                    broadcast(&room, &anchors, &emit);
                                }
                            },
                            Frame::Room(snapshot) if !hosting => {
                                validate_room(&snapshot, &invite.session, &room.host, &local)?;
                                install_room(&mut room, &mut anchors, snapshot);
                                let accepted = *room.accepted.get(&local).unwrap_or(&0);
                                pending.retain(|f| matches!(f, Frame::Submit { sequence, .. } if *sequence > accepted));
                                broadcast(&room, &anchors, &emit);
                            },
                            Frame::Ping(id) => write(&mut send, &Frame::Pong(id)).await?,
                            Frame::Pong(id) => emit(Event::Status(Status::Connected { route: route(&conn), rtt_ms: (ping_start.elapsed().as_millis() as u64).saturating_sub(id) })),
                            _ => return Err("Unexpected session message".into()),
                        }
                        if dirty {
                            let snapshot = broadcast(&room, &anchors, &emit);
                            write(&mut send, &Frame::Room(snapshot)).await?;
                        }
                    },
                    command = commands.recv() => match command.unwrap_or(Command::Leave) {
                        Command::Leave => {
                            let _ = write(&mut send, &Frame::Leave).await;
                            let _ = send.finish();
                            let _ = tokio::time::timeout(Duration::from_millis(250), conn.closed()).await;
                            return Ok(true);
                        },
                        Command::Submit { sequence, line, avatar, resources } => {
                            let frame = prepare(&Frame::Submit { sequence, line, avatar }, &resources, &offers).await?;
                            if hosting {
                                if let Frame::Submit { sequence, line, avatar } = frame { room.submit(&local, sequence, line, avatar); }
                                let snapshot = broadcast(&room, &anchors, &emit);
                                write(&mut send, &Frame::Room(snapshot)).await?;
                            } else {
                                pending.push(frame.clone());
                                write(&mut send, &frame).await?;
                            }
                        },
                        #[cfg(test)]
                        Command::Disconnect => return Err("Test disconnect".into()),
                    },
                    _ = timer.tick() => {
                        if last_seen.elapsed() > Duration::from_secs(15) { return Err("Peer heartbeat timed out".into()); }
                        if heartbeat.elapsed() >= Duration::from_secs(2) {
                            heartbeat = Instant::now();
                            write(&mut send, &Frame::Ping(ping_start.elapsed().as_millis() as u64)).await?;
                        }
                        if changes.has_changed().unwrap_or(false) {
                            let latest = changes.borrow_and_update().clone();
                            if let Some(publication) = latest {
                                let mut track = prepare(&publication.track, &publication.resources, &offers).await?;
                                track.age_ms = publication.published_at.elapsed().as_millis() as u64;
                                if hosting {
                                    if room.update(&local, track.clone())? {
                                        anchors.insert(local.clone(), Instant::now());
                                        broadcast(&room, &anchors, &emit);
                                        write(&mut send, &Frame::PeerTrack { participant: local.clone(), track }).await?;
                                    }
                                } else {
                                    if room.update(&local, track.clone())? {
                                        anchors.insert(local.clone(), Instant::now());
                                        broadcast(&room, &anchors, &emit);
                                    }
                                    write(&mut send, &Frame::Track(track)).await?;
                                }
                            }
                        }
                        for hash in room.asset_hashes() {
                            let path = config.cache.join(format!("p2p-{hash}.png"));
                            if transfers.len() < 2 && !path.exists() && !offers.read().await.contains_key(&hash) && fetched.insert(hash.clone()) {
                                let conn = conn.clone(); let emit = emit.clone(); let cache = config.cache.clone();
                                transfers.spawn(async move {
                                    if fetch_asset(&conn, &hash, &cache).await.is_ok() { emit(Event::AssetReady(hash.clone())); }
                                    hash
                                });
                            }
                        }
                    },
                    Some(result) = transfers.join_next(), if !transfers.is_empty() => { if let Ok(hash) = result { fetched.remove(&hash); } },
                }
            }
        }.await;
        reader.abort();
        asset_server.abort();
        transfers.abort_all();
        conn.close(0u8.into(), b"session disconnected");
        if matches!(result, Ok(true)) {
            emit(Event::Status(Status::Ended));
            break;
        }
        emit(Event::Status(Status::Reconnecting));
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    endpoint.close().await;
    Ok(())
}

fn validate_room(room: &Room, session: &str, host: &str, local: &str) -> Result<(), String> {
    if room.session != session
        || room.host != host
        || room.guest.as_deref() != Some(local)
        || room.entries.len() > protocol::MAX_ENTRIES
        || room.tracks.len() > 2
    {
        return Err("Invalid room snapshot".into());
    }
    if room.tracks.keys().any(|id| id != host && id != local) {
        return Err("Unknown participant".into());
    }
    if room.accepted.len() > 2
        || room.retired_before.len() > 2
        || room
            .accepted
            .keys()
            .chain(room.retired_before.keys())
            .any(|id| id != host && id != local)
        || room.entries.iter().any(|entry| {
            entry.id.session != session
                || (entry.id.participant != host && entry.id.participant != local)
        })
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
    // local mirror. Keep newer tracks and their elapsed playback time.
    for (owner, track) in &room.tracks {
        if snapshot
            .tracks
            .get(owner)
            .is_none_or(|incoming| incoming.revision < track.revision)
        {
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

fn random_secret() -> String {
    blake3::hash(&rand::random::<[u8; 32]>())
        .to_hex()
        .to_string()
}

fn route(conn: &Connection) -> String {
    // Selected paths are exposed by iroh; do not infer directness from latency.
    conn.paths()
        .iter()
        .find(|path| path.is_selected())
        .map_or("connecting", |path| {
            if path.is_relay() { "relay" } else { "direct" }
        })
        .into()
}

async fn serve_assets(conn: Connection, offers: Offers) {
    loop {
        let Ok((mut send, mut recv)) = conn.accept_bi().await else {
            break;
        };
        // Bounded single transfer per peer; independent of the control stream.
        let transfer = tokio::time::timeout(Duration::from_secs(15), async {
            let hash: String = read(&mut recv).await?;
            if !protocol::valid_hash(&hash) {
                return Err("Invalid asset hash".to_string());
            }
            let path = offers
                .read()
                .await
                .get(&hash)
                .map(|offer| offer.path.clone())
                .ok_or("Asset not offered")?;
            let meta = tokio::fs::metadata(&path)
                .await
                .map_err(|e| e.to_string())?;
            if meta.len() > MAX_ASSET as u64 {
                return Err("Asset exceeds limit".into());
            }
            let bytes = tokio::fs::read(path).await.map_err(|e| e.to_string())?;
            if blake3::hash(&bytes).to_hex().as_str() != hash {
                return Err("Asset changed".into());
            }
            send.write_all(&(bytes.len() as u32).to_be_bytes())
                .await
                .map_err(|e| e.to_string())?;
            send.write_all(&bytes).await.map_err(|e| e.to_string())?;
            send.finish().map_err(|e| e.to_string())?;
            Ok::<_, String>(())
        })
        .await;
        match transfer {
            Ok(Ok(())) => {}
            Ok(Err(error)) => nanatype_diag::report!(
                crate::diag::ASSET_TRANSFER_FAILED,
                timed_out = false;
                "{error}"
            ),
            Err(_) => nanatype_diag::event!(crate::diag::ASSET_TRANSFER_FAILED, timed_out = true),
        }
    }
}

async fn fetch_asset(conn: &Connection, hash: &str, cache: &std::path::Path) -> Result<(), String> {
    tokio::time::timeout(Duration::from_secs(15), async {
        let (mut send, mut recv) = conn.open_bi().await.map_err(|e| e.to_string())?;
        write(&mut send, &hash).await?;
        send.finish().map_err(|e| e.to_string())?;
        let mut len = [0; 4];
        recv.read_exact(&mut len).await.map_err(|e| e.to_string())?;
        let len = u32::from_be_bytes(len) as usize;
        if len > MAX_ASSET {
            return Err("Asset exceeds limit".into());
        }
        let mut bytes = vec![0; len];
        recv.read_exact(&mut bytes)
            .await
            .map_err(|e| e.to_string())?;
        if blake3::hash(&bytes).to_hex().as_str() != hash {
            return Err("Asset hash mismatch".into());
        }
        tokio::fs::create_dir_all(cache)
            .await
            .map_err(|e| e.to_string())?;
        let temp = cache.join(format!("p2p-{hash}.partial"));
        let final_path = cache.join(format!("p2p-{hash}.png"));
        tokio::fs::write(&temp, bytes)
            .await
            .map_err(|e| e.to_string())?;
        tokio::fs::rename(temp, final_path)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|_| "Asset transfer timed out".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use nanatype_core::DisplayView;
    use nanatype_model::{Document, Entrance, EntranceKind, Speaker};
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
            relay: None,
            local_only: true,
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
        let (host_tx, host_rx) = channel::channel();
        let host = PeerHandle::start(config("host"), None, move |e| {
            let _ = host_tx.send(e);
        })
        .unwrap();
        let Event::Started { invitation, .. } =
            next(&host_rx, |e| matches!(e, Event::Started { .. }))
        else {
            unreachable!()
        };
        let (guest_tx, guest_rx) = channel::channel();
        let guest = PeerHandle::start(config("guest"), Some(invitation), move |e| {
            let _ = guest_tx.send(e);
        })
        .unwrap();
        next(&host_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        next(&guest_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        let line = Line::new(
            Document::new(),
            Speaker {
                avatar: None,
                name: "Host".into(),
                portrait: None,
            },
        );
        host.publish(track(1), BTreeMap::new());
        guest.publish(track(1), BTreeMap::new());
        assert!(host.submit(1, line.clone(), None, BTreeMap::new()));
        assert!(guest.submit(1, line.clone(), None, BTreeMap::new()));
        let has_two = |e: &Event| matches!(e, Event::Room { room, .. } if room.entries.len() == 2 && room.tracks.len() == 2);
        let Event::Room { room: a, .. } = next(&host_rx, has_two) else {
            unreachable!()
        };
        let Event::Room { room: b, .. } = next(&guest_rx, has_two) else {
            unreachable!()
        };
        assert_eq!(
            a.entries.iter().map(|e| &e.id).collect::<Vec<_>>(),
            b.entries.iter().map(|e| &e.id).collect::<Vec<_>>()
        );
        guest.disconnect();
        next(&guest_rx, |e| {
            matches!(e, Event::Status(Status::Reconnecting))
        });
        next(&guest_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        // Replayed submissions cannot duplicate committed history.
        guest.submit(1, line.clone(), None, BTreeMap::new());
        guest.submit(2, line, None, BTreeMap::new());
        next(
            &host_rx,
            |e| matches!(e, Event::Room { room, .. } if room.entries.len() == 3),
        );
        drop(guest);
        next(&host_rx, |e| matches!(e, Event::Status(Status::Ended)));
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
        room.guest = Some("guest".into());
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
        let (host_tx, host_rx) = channel::channel();
        let host = PeerHandle::start(config("auth-host"), None, move |event| {
            let _ = host_tx.send(event);
        })
        .unwrap();
        let Event::Started { invitation, .. } =
            next(&host_rx, |e| matches!(e, Event::Started { .. }))
        else {
            unreachable!()
        };
        let mut bad = Invite::decode(&invitation).unwrap();
        bad.secret = "0".repeat(64);
        let (bad_tx, bad_rx) = channel::channel();
        let intruder = PeerHandle::start(config("intruder"), Some(bad.encode()), move |e| {
            let _ = bad_tx.send(e);
        })
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
        let guest = PeerHandle::start(config("auth-guest"), Some(invitation), move |e| {
            let _ = guest_tx.send(e);
        })
        .unwrap();
        next(&guest_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        drop(guest);
        drop(host);
    }

    #[test]
    fn image_bytes_travel_on_a_separate_stream_and_are_verified() {
        let host_config = config("image-host");
        let guest_config = config("image-guest");
        std::fs::create_dir_all(&host_config.cache).unwrap();
        let image_path = host_config.cache.join("source.png");
        let bytes = b"test image bytes";
        std::fs::write(&image_path, bytes).unwrap();
        let expected = blake3::hash(bytes).to_hex().to_string();
        let guest_cache = guest_config.cache.clone();
        let (host_tx, host_rx) = channel::channel();
        let host = PeerHandle::start(host_config.clone(), None, move |e| {
            let _ = host_tx.send(e);
        })
        .unwrap();
        let Event::Started { invitation, .. } =
            next(&host_rx, |e| matches!(e, Event::Started { .. }))
        else {
            unreachable!()
        };
        let (guest_tx, guest_rx) = channel::channel();
        let guest = PeerHandle::start(guest_config, Some(invitation), move |e| {
            let _ = guest_tx.send(e);
        })
        .unwrap();
        next(&host_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
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
        host.publish(
            snapshot,
            BTreeMap::from([(format!("file:{}", image_path.display()), image_path)]),
        );
        next(
            &guest_rx,
            |e| matches!(e, Event::AssetReady(hash) if hash == &expected),
        );
        assert_eq!(
            std::fs::read(guest_cache.join(format!("p2p-{expected}.png"))).unwrap(),
            bytes
        );
        drop(guest);
        drop(host);
        // No recursive cleanup: only files created by this test.
        let _ = std::fs::remove_file(host_config.cache.join("source.png"));
        let _ = std::fs::remove_dir(host_config.cache);
        let _ = std::fs::remove_file(guest_cache.join(format!("p2p-{expected}.png")));
        let _ = std::fs::remove_dir(guest_cache);
    }
}
