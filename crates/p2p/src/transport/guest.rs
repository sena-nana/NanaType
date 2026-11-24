use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::{Duration, Instant};

use tokio::sync::{RwLock, mpsc};

use crate::link::{Carrier, Inbound, Outbound, Pipe};
use crate::protocol::{self, Frame, FrameRef, MAX_FRAME, MAX_MEMBERS, Room, VERSION};

use super::assets::{
    Offers, direct_publisher, fetch_asset, offered_asset, pending_previews, prepare,
    remember_seated, store_asset, store_previews,
};
use super::wire::{
    ReaderSink, decode_frame, open_pipe, read_frame, send_frame, sleep_until_opt,
    spawn_rate_limited_reader,
};
use super::{
    Command, Controls, Event, SessionContext, Status, broadcast, known_participant, member, snap,
};

const HEARTBEAT_EVERY: Duration = Duration::from_secs(2);
const PEER_TIMEOUT: Duration = Duration::from_secs(15);

pub(super) async fn connect_attempt(
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
            room: mut snapshot,
            previews,
        } => {
            store_previews(cache, previews, emit).await;
            validate_room(&mut snapshot, &invite.session, &room.host, local)?;
            install_room(room, anchors, snapshot);
            remember_seated(seated, room).await;
            Ok(())
        }
        Frame::Full => Err("Room is full".into()),
        _ => Err("Invalid handshake".into()),
    }
}

fn next_guest_deadline(
    last_seen: Instant,
    heartbeat: Instant,
    inflight: &BTreeMap<String, Instant>,
) -> Instant {
    let mut next = (last_seen + PEER_TIMEOUT).min(heartbeat + HEARTBEAT_EVERY);
    for at in inflight.values() {
        next = next.min(*at + PEER_TIMEOUT);
    }
    next
}

struct MissingRequestContext<'a> {
    room: &'a Room,
    local: &'a str,
    carrier: &'a Carrier,
    offers: &'a Offers,
    cache: &'a Path,
    send: &'a mpsc::UnboundedSender<Outbound>,
    fetch_tx: &'a mpsc::UnboundedSender<(String, Result<Vec<u8>, String>)>,
}

struct MissingRequestState<'a> {
    have_assets: &'a mut BTreeSet<String>,
    inflight: &'a mut BTreeMap<String, Instant>,
    direct_inflight: &'a mut BTreeSet<String>,
    direct_attempts: &'a mut BTreeMap<String, u8>,
}

async fn request_missing_assets(
    ctx: MissingRequestContext<'_>,
    state: MissingRequestState<'_>,
) -> Result<(), String> {
    let mut dials = Vec::new();
    let mut wants = Vec::new();
    {
        let offered = ctx.offers.read().await;
        for hash in ctx.room.asset_hashes() {
            if state.have_assets.contains(&hash)
                || offered.contains_key(&hash)
                || state.inflight.contains_key(&hash)
                || state.direct_inflight.contains(&hash)
            {
                continue;
            }
            if protocol::session_full_picture(ctx.cache, &hash).is_some() {
                state.have_assets.insert(hash);
                continue;
            }
            if state.direct_attempts.get(&hash).copied().unwrap_or(0) < 2
                && let Some(publisher) = direct_publisher(ctx.room, ctx.local, &hash)
            {
                if state.direct_inflight.len() + dials.len() < 2 {
                    dials.push((hash, publisher));
                }
                continue;
            }
            if state.inflight.len() + wants.len() < 2 {
                wants.push(hash);
            }
        }
    }
    for (hash, publisher) in dials {
        state.direct_inflight.insert(hash.clone());
        let tx = ctx.fetch_tx.clone();
        let carrier = ctx.carrier.clone();
        tokio::spawn(async move {
            let result = fetch_asset(carrier, publisher, hash.clone()).await;
            let _ = tx.send((hash, result));
        });
    }
    for hash in wants {
        ctx.send
            .send(Outbound::Want(hash.clone()))
            .map_err(|_| "Connection closed".to_string())?;
        state.inflight.insert(hash, Instant::now());
    }
    Ok(())
}

pub(super) async fn exchange(
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
        send, recv, route, ..
    } = pipe;
    let (in_tx, mut in_rx) = mpsc::channel(32);
    let reader = spawn_rate_limited_reader(recv, ReaderSink::Guest(in_tx));
    let mut inflight: BTreeMap<String, Instant> = BTreeMap::new();
    let (fetch_tx, mut fetch_rx) = mpsc::unbounded_channel();
    let _fetch_hold = fetch_tx.clone();
    let mut direct_inflight: BTreeSet<String> = BTreeSet::new();
    let mut direct_attempts: BTreeMap<String, u8> = BTreeMap::new();
    let mut have_assets: BTreeSet<String> = BTreeSet::new();
    let mut assets_dirty = true;
    let mut changes_closed = false;
    let mut heartbeat = Instant::now();
    let mut last_seen = Instant::now();
    changes.mark_changed();

    let result = async {
        loop {
            if assets_dirty {
                request_missing_assets(
                    MissingRequestContext {
                        room,
                        local,
                        carrier,
                        offers,
                        cache,
                        send: &send,
                        fetch_tx: &fetch_tx,
                    },
                    MissingRequestState {
                        have_assets: &mut have_assets,
                        inflight: &mut inflight,
                        direct_inflight: &mut direct_inflight,
                        direct_attempts: &mut direct_attempts,
                    },
                )
                .await?;
                assets_dirty = false;
            }
            let deadline = next_guest_deadline(last_seen, heartbeat, &inflight);
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
                                        assets_dirty = true;
                                    }
                                }
                                Frame::Room { room: mut snapshot, previews } => {
                                    store_previews(cache, previews, emit).await;
                                    validate_room(&mut snapshot, &invite.session, &room.host, local)?;
                                    install_room(room, anchors, snapshot);
                                    remember_seated(seated, room).await;
                                    broadcast(room, anchors, emit);
                                    assets_dirty = true;
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
                                have_assets.insert(hash.clone());
                                emit(Event::AssetReady(hash));
                            }
                            assets_dirty = true;
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
                changed = changes.changed(), if !changes_closed => {
                    if changed.is_err() {
                        changes_closed = true;
                        continue;
                    }
                    let latest = changes.borrow_and_update().clone();
                    if let Some(publication) = latest {
                        let mut track = prepare(&publication.track, &publication.resources, offers, cache).await?;
                        track.age_ms = publication.published_at.elapsed().as_millis() as u64;
                        // A track over budget is not sent: the host would refuse it too,
                        // and dropping the pipe would only reconnect into the same track.
                        let accepted = room.update(local, track);
                        if matches!(accepted, Ok(true)) {
                            anchors.insert(local.to_owned(), Instant::now());
                            broadcast(room, anchors, emit);
                            assets_dirty = true;
                        }
                        if accepted.is_ok() && let Some(stored) = room.tracks.get(local) {
                            let previews = pending_previews(offers, cache, &stored.asset_hashes(), sent_previews).await;
                            send_frame(&send, &FrameRef::Track { track: stored, previews: &previews })?;
                        }
                    }
                }
                _ = sleep_until_opt(Some(deadline)) => {
                    let now = Instant::now();
                    if now.duration_since(last_seen) > PEER_TIMEOUT {
                        return Err("Peer heartbeat timed out".into());
                    }
                    let before_inflight = inflight.len();
                    inflight.retain(|_, at| now.duration_since(*at) < PEER_TIMEOUT);
                    if inflight.len() != before_inflight {
                        assets_dirty = true;
                    }
                    if now.duration_since(heartbeat) >= HEARTBEAT_EVERY {
                        heartbeat = now;
                        let (route_name, rtt_ms) = snap(&route);
                        emit(Event::Status(Status::Connected { route: route_name, rtt_ms }));
                        send_frame(&send, &Frame::Ping(rtt_ms))?;
                    }
                }
                fetched = fetch_rx.recv() => {
                    let Some((hash, result)) = fetched else { continue };
                    direct_inflight.remove(&hash);
                    match result {
                        Ok(bytes) if store_asset(cache, &hash, &bytes).await.is_ok() => {
                            have_assets.insert(hash.clone());
                            emit(Event::AssetReady(hash));
                        }
                        _ => {
                            *direct_attempts.entry(hash).or_default() += 1;
                        }
                    }
                    assets_dirty = true;
                }
            }
        }
    }
    .await;
    reader.abort();
    result
}

pub(super) fn validate_room(
    room: &mut Room,
    session: &str,
    host: &str,
    local: &str,
) -> Result<(), String> {
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
    for track in room.tracks.values_mut() {
        track.validate()?;
    }
    Ok(())
}

pub(super) fn install_room(
    room: &mut Room,
    anchors: &mut BTreeMap<String, Instant>,
    mut snapshot: Room,
) {
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
