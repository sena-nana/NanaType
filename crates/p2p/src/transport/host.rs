use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, watch};

use crate::link::{Inbound, Outbound, Pipe, RouteSnap};
use crate::protocol::{
    self, Frame, FrameRef, MAX_FRAME, MAX_MEMBERS, Room, TrackSnapshot, VERSION,
};

use super::assets::{
    Offers, asset_owner, pending_previews, prepare, store_asset, store_previews, stored_asset,
};
use super::wire::{
    MAX_HELLO, ReaderSink, decode_frame, open_pipe, pack, read_frame, send_frame, send_packed,
    sleep_until_opt, spawn_rate_limited_reader,
};
use super::{Command, Controls, Emit, Event, SessionContext, Status, broadcast, snap};

/// Unauthenticated pipes the host reads a hello from at once. More are closed.
const MAX_HANDSHAKES: usize = 4;
/// A SteamID that fails this many handshakes in a row is refused for a while.
const FAILED_HANDSHAKES: u32 = 3;
const HANDSHAKE_BACKOFF: Duration = Duration::from_secs(30);
const HEARTBEAT_EVERY: Duration = Duration::from_secs(2);
const PEER_TIMEOUT: Duration = Duration::from_secs(15);

/// Host connection state updated when an authenticated guest takes a seat.
struct Admission<'a> {
    links: &'a mut BTreeMap<String, Link>,
    next_generation: &'a mut u64,
    connected_once: &'a mut bool,
    notes: &'a mpsc::Sender<(String, u64, Inbound)>,
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

/// What an unauthenticated pipe's hello is checked against.
struct Gate {
    session: String,
    secret: String,
    password: String,
    local: String,
}

fn retire(link: Link) {
    let _ = link.send.send(Outbound::Close);
    link.reader.abort();
}

fn attach(
    pipe: Pipe,
    generation: u64,
    sent_previews: BTreeSet<String>,
    notes: &mpsc::Sender<(String, u64, Inbound)>,
) -> (String, Link) {
    let id = pipe.remote_id.clone();
    let reader = spawn_rate_limited_reader(
        pipe.recv,
        ReaderSink::Host {
            id: id.clone(),
            generation,
            tx: notes.clone(),
        },
    );
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

enum FanTarget<'a> {
    Room(&'a Room),
    PeerTrack {
        participant: &'a str,
        track: &'a TrackSnapshot,
    },
}

impl<'a> FanTarget<'a> {
    fn frame<'b>(&'b self, previews: &'b BTreeMap<String, String>) -> FrameRef<'b>
    where
        'a: 'b,
    {
        match self {
            Self::Room(room) => FrameRef::Room { room, previews },
            Self::PeerTrack { participant, track } => FrameRef::PeerTrack {
                participant,
                track,
                previews,
            },
        }
    }
}

async fn fan_out(
    links: &mut BTreeMap<String, Link>,
    except: Option<&str>,
    hashes: &BTreeSet<String>,
    offers: &Offers,
    cache: &Path,
    target: FanTarget<'_>,
) -> Vec<String> {
    if links.is_empty() {
        return Vec::new();
    }
    let mut empty_packed: Option<Vec<u8>> = None;
    let mut dead = Vec::new();
    for (id, link) in links.iter_mut() {
        if except == Some(id.as_str()) {
            continue;
        }
        let previews = pending_previews(offers, cache, hashes, &mut link.sent_previews).await;
        let result = if previews.is_empty() {
            let bytes = match &empty_packed {
                Some(packed) => Ok(packed.clone()),
                None => pack(&target.frame(&previews)).inspect(|packed| {
                    empty_packed = Some(packed.clone());
                }),
            };
            bytes.and_then(|packed| send_packed(&link.send, packed))
        } else {
            send_frame(&link.send, &target.frame(&previews))
        };
        if result.is_err() {
            dead.push(id.clone());
        }
    }
    dead
}

async fn fan_room(
    links: &mut BTreeMap<String, Link>,
    except: Option<&str>,
    snapshot: &Room,
    offers: &Offers,
    cache: &Path,
) -> Vec<String> {
    let hashes = snapshot.asset_hashes();
    fan_out(
        links,
        except,
        &hashes,
        offers,
        cache,
        FanTarget::Room(snapshot),
    )
    .await
}

async fn fan_peer(
    links: &mut BTreeMap<String, Link>,
    except: &str,
    participant: &str,
    track: &TrackSnapshot,
    offers: &Offers,
    cache: &Path,
) -> Vec<String> {
    let hashes = track.asset_hashes();
    fan_out(
        links,
        Some(except),
        &hashes,
        offers,
        cache,
        FanTarget::PeerTrack { participant, track },
    )
    .await
}

struct HostRoom<'a> {
    links: &'a mut BTreeMap<String, Link>,
    room: &'a mut Room,
    anchors: &'a mut BTreeMap<String, Instant>,
    offers: &'a Offers,
    cache: &'a Path,
    emit: &'a Emit,
}

async fn apply_track_update(
    state: HostRoom<'_>,
    owner: &str,
    except: &str,
    track: TrackSnapshot,
    assets_dirty: &mut bool,
) -> bool {
    let Ok(true) = state.room.update(owner, track) else {
        return false;
    };
    state.anchors.insert(owner.to_owned(), Instant::now());
    *assets_dirty = true;
    let dead = if state.room.index(owner) {
        let snapshot = broadcast(state.room, state.anchors, state.emit);
        fan_room(state.links, None, &snapshot, state.offers, state.cache).await
    } else if let Some(stored) = state.room.tracks.get(owner) {
        fan_peer(
            state.links,
            except,
            owner,
            stored,
            state.offers,
            state.cache,
        )
        .await
    } else {
        Vec::new()
    };
    if !dead.is_empty() {
        release(
            state.links,
            state.room,
            state.anchors,
            state.offers,
            state.cache,
            state.emit,
            &dead,
        )
        .await;
        return true;
    }
    false
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
    let more = fan_room(links, None, &snapshot, offers, cache).await;
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

fn next_host_deadline(links: &BTreeMap<String, Link>) -> Option<Instant> {
    let mut next: Option<Instant> = None;
    for link in links.values() {
        let mut candidate = (link.last_seen + PEER_TIMEOUT).min(link.heartbeat + HEARTBEAT_EVERY);
        for at in link.inflight.values() {
            candidate = candidate.min(*at + PEER_TIMEOUT);
        }
        next = Some(next.map_or(candidate, |cur| cur.min(candidate)));
    }
    next
}

async fn request_missing_assets(
    room: &Room,
    links: &mut BTreeMap<String, Link>,
    offers: &Offers,
    cache: &Path,
    have_assets: &mut BTreeSet<String>,
) -> Vec<String> {
    if links.is_empty() {
        return Vec::new();
    }
    let hashes = room.asset_hashes();
    let offers_now = offers.read().await;
    let mut dead = Vec::new();
    for hash in &hashes {
        if have_assets.contains(hash) || offers_now.contains_key(hash) {
            continue;
        }
        if protocol::session_full_picture(cache, hash).is_some() {
            have_assets.insert(hash.clone());
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
    dead
}

/// The host keeps every guest pipe. One person leaving or dropping frees that
/// seat and keeps their track. The host leaving ends the room.
pub(super) async fn host_session(
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
    let mut have_assets: BTreeSet<String> = BTreeSet::new();
    let mut assets_dirty = false;
    let mut changes_closed = false;

    loop {
        if assets_dirty && !links.is_empty() {
            let dead =
                request_missing_assets(room, &mut links, offers, cache, &mut have_assets).await;
            assets_dirty = false;
            if !dead.is_empty() {
                release(&mut links, room, anchors, offers, cache, emit, &dead).await;
                emit(Event::Status(presence(
                    &links,
                    &room.guests,
                    connected_once,
                )));
            }
        }
        let deadline = next_host_deadline(&links);
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
                    assets_dirty = true;
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
                            if apply_track_update(
                                HostRoom {
                                    links: &mut links,
                                    room,
                                    anchors,
                                    offers,
                                    cache,
                                    emit,
                                },
                                &id,
                                &id,
                                track,
                                &mut assets_dirty,
                            )
                            .await
                            {
                                emit(Event::Status(presence(&links, &room.guests, connected_once)));
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
                            have_assets.insert(hash.clone());
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
                        assets_dirty = true;
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
            changed = changes.changed(), if !changes_closed => {
                if changed.is_err() {
                    changes_closed = true;
                    continue;
                }
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
                    if apply_track_update(
                        HostRoom {
                            links: &mut links,
                            room,
                            anchors,
                            offers,
                            cache,
                            emit,
                        },
                        local,
                        "",
                        track,
                        &mut assets_dirty,
                    )
                    .await
                    {
                        emit(Event::Status(presence(&links, &room.guests, connected_once)));
                    }
                }
            }
            _ = sleep_until_opt(deadline), if deadline.is_some() => {
                let mut dead = Vec::new();
                let mut refresh = false;
                let now = Instant::now();
                for (id, link) in &mut links {
                    if now.duration_since(link.last_seen) > PEER_TIMEOUT {
                        dead.push(id.clone());
                        continue;
                    }
                    let before_inflight = link.inflight.len();
                    link.inflight.retain(|_, at| now.duration_since(*at) < PEER_TIMEOUT);
                    if link.inflight.len() != before_inflight {
                        assets_dirty = true;
                    }
                    if now.duration_since(link.heartbeat) >= HEARTBEAT_EVERY {
                        link.heartbeat = now;
                        let (_, rtt_ms) = snap(&link.route);
                        if send_frame(&link.send, &Frame::Ping(rtt_ms)).is_err() {
                            dead.push(id.clone());
                        } else {
                            refresh = true;
                        }
                    }
                }
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
        &FrameRef::Welcome {
            room: &snapshot,
            previews: &previews,
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
    links.insert(id.clone(), link);
    *connected_once = true;
    // The newly seated guest already received `snapshot` in `Welcome`; only notify others.
    let dead = fan_room(links, Some(&id), &snapshot, offers, cache).await;
    release(links, room, anchors, offers, cache, emit, &dead).await;
    emit(Event::Status(presence(links, &room.guests, true)));
}

/// The session secret admits an invited friend. The room password admits
/// everyone else. Either match is enough; an empty password never matches.
fn credential_matches(presented: &str, secret: &str, password: &str) -> bool {
    let presented = blake3::hash(presented.as_bytes());
    presented == blake3::hash(secret.as_bytes())
        || (!password.is_empty() && presented == blake3::hash(password.as_bytes()))
}
