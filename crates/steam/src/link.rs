//! Sessions of up to eight people on `ISteamNetworkingSockets` P2P. Steam is the pipe:
//! the session secret still decides who may stay, because any owner of the
//! app can call `ConnectP2P`.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use nanatype_p2p::link::lane;
use nanatype_p2p::wire::{Assembler, Packet};
use nanatype_p2p::{Carrier, Inbound, Outbound, Pipe, RouteSnap, chunks};
use steamworks::networking_types::{
    AppNetConnectionEnd, ListenSocketEvent, NetConnectionEnd, NetworkingConfigEntry,
    NetworkingConfigValue, NetworkingConnectionState, NetworkingIdentity, SendFlags,
};
use steamworks::{Client, SteamError, SteamId};
use tokio::sync::{mpsc, watch};

/// Virtual port shared by every NanaType session.
const PORT: i32 = 0;
/// Full pictures. Separate from the session so a listen does not take a seat.
const ASSET_PORT: i32 = 1;
/// Hole-punch on every ICE candidate, and keep SDR when that fails.
/// `k_nSteamNetworkingConfig_P2P_Transport_ICE_Enable_All`.
const ICE_ALL: i32 = 0x7fff_ffff;
/// Steam's 512 KiB default refuses a second near-full chunk while the first is
/// still queued. 16 MiB holds a whole session frame (8 MiB at most) plus two
/// pictures (512 KiB each) before the pump has to hold chunks back.
const SEND_BUFFER: i32 = 16 * 1024 * 1024;
const RELAYED: i32 = steamworks::sys::k_nSteamNetworkConnectionInfoFlags_Relayed as i32;

struct Request {
    port: i32,
    remote: Option<u64>,
    cancel: Arc<AtomicBool>,
    reply: Sender<Result<Pipe, String>>,
}

struct DialJob {
    conn: steamworks::networking_sockets::NetConnection,
    remote_id: String,
    cancel: Arc<AtomicBool>,
    reply: Sender<Result<Pipe, String>>,
    started: Instant,
}

struct AcceptJob {
    port: i32,
    cancel: Arc<AtomicBool>,
    reply: Sender<Result<Pipe, String>>,
    started: Instant,
}

pub fn carrier(client: Client) -> Carrier {
    let local = client.user().steam_id().raw();
    let (tx, rx) = crossbeam_channel::unbounded();
    thread::Builder::new()
        .name("nanatype-steam-link".into())
        .spawn(move || drive(client, rx))
        .expect("spawn Steam session");
    Carrier::new(local.to_string(), move |port, remote, cancel| {
        let (reply, wait) = crossbeam_channel::bounded(1);
        tx.send(Request {
            port,
            remote,
            cancel,
            reply,
        })
        .map_err(|_| "Steam session ended".to_string())?;
        wait.recv().map_err(|_| "Steam session ended".to_string())?
    })
}

fn drive(client: Client, requests: Receiver<Request>) {
    let mut session_listen: Option<steamworks::networking_sockets::ListenSocket> = None;
    let mut asset_listen: Option<steamworks::networking_sockets::ListenSocket> = None;
    let mut dials: Vec<DialJob> = Vec::new();
    let mut accepts: Vec<AcceptJob> = Vec::new();
    loop {
        let busy = !dials.is_empty() || !accepts.is_empty();
        let incoming = if busy {
            match requests.recv_timeout(Duration::from_millis(20)) {
                Ok(request) => Some(request),
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => None,
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match requests.recv() {
                Ok(request) => Some(request),
                Err(_) => break,
            }
        };
        if let Some(request) = incoming {
            enqueue(
                &client,
                request,
                &mut session_listen,
                &mut asset_listen,
                &mut dials,
                &mut accepts,
            );
            while let Ok(request) = requests.try_recv() {
                enqueue(
                    &client,
                    request,
                    &mut session_listen,
                    &mut asset_listen,
                    &mut dials,
                    &mut accepts,
                );
            }
        }
        poll_dials(&client, &mut dials);
        poll_accepts(&client, session_listen.as_ref(), PORT, &mut accepts);
        poll_accepts(&client, asset_listen.as_ref(), ASSET_PORT, &mut accepts);
    }
    for dial in dials {
        end(dial.conn);
    }
}

fn enqueue(
    client: &Client,
    request: Request,
    session_listen: &mut Option<steamworks::networking_sockets::ListenSocket>,
    asset_listen: &mut Option<steamworks::networking_sockets::ListenSocket>,
    dials: &mut Vec<DialJob>,
    accepts: &mut Vec<AcceptJob>,
) {
    if request.cancel.load(Ordering::Relaxed) {
        let _ = request.reply.send(Err("Connection cancelled".into()));
        return;
    }
    let Some(remote) = request.remote else {
        let listen = if request.port == ASSET_PORT {
            asset_listen
        } else {
            session_listen
        };
        if listen.is_none() {
            match client
                .networking_sockets()
                .create_listen_socket_p2p(request.port, ice())
            {
                Ok(socket) => *listen = Some(socket),
                Err(_) => {
                    let message = if request.port == ASSET_PORT {
                        "Could not open a picture link"
                    } else {
                        "Could not open the Steam session"
                    };
                    let _ = request.reply.send(Err(message.into()));
                    return;
                }
            }
        }
        accepts.push(AcceptJob {
            port: request.port,
            cancel: request.cancel,
            reply: request.reply,
            started: Instant::now(),
        });
        return;
    };
    match begin_dial(client, remote, request.port) {
        Ok(conn) => dials.push(DialJob {
            conn,
            remote_id: remote.to_string(),
            cancel: request.cancel,
            reply: request.reply,
            started: Instant::now(),
        }),
        Err(error) => {
            let _ = request.reply.send(Err(error));
        }
    }
}

fn begin_dial(
    client: &Client,
    remote: u64,
    port: i32,
) -> Result<steamworks::networking_sockets::NetConnection, String> {
    client
        .networking_sockets()
        .connect_p2p(
            NetworkingIdentity::new_steam_id(SteamId::from_raw(remote)),
            port,
            ice(),
        )
        .map_err(|_| "Could not reach that Steam player".to_string())
}

fn poll_dials(client: &Client, dials: &mut Vec<DialJob>) {
    let pending = std::mem::take(dials);
    for job in pending {
        if job.cancel.load(Ordering::Relaxed) || job.started.elapsed() > Duration::from_secs(30) {
            let cancelled = job.cancel.load(Ordering::Relaxed);
            end(job.conn);
            let _ = job.reply.send(Err(if cancelled {
                "Connection cancelled".into()
            } else {
                "Connection timed out".into()
            }));
            continue;
        }
        match state(&job.conn) {
            Some(NetworkingConnectionState::Connected) => {
                let DialJob {
                    conn,
                    remote_id,
                    reply,
                    ..
                } = job;
                let _ = reply.send(Ok(attach(client.clone(), conn, remote_id)));
            }
            Some(
                NetworkingConnectionState::ClosedByPeer
                | NetworkingConnectionState::ProblemDetectedLocally
                | NetworkingConnectionState::None,
            )
            | None => {
                end(job.conn);
                let _ = job.reply.send(Err("Connection failed".into()));
            }
            Some(_) => dials.push(job),
        }
    }
}

fn poll_accepts(
    client: &Client,
    listen: Option<&steamworks::networking_sockets::ListenSocket>,
    port: i32,
    accepts: &mut Vec<AcceptJob>,
) {
    let limit = if port == ASSET_PORT {
        Duration::from_secs(24 * 60 * 60)
    } else {
        Duration::from_secs(30)
    };
    let pending = std::mem::take(accepts);
    for job in pending {
        let expired = job.port == port
            && (job.cancel.load(Ordering::Relaxed) || job.started.elapsed() > limit);
        if expired {
            let cancelled = job.cancel.load(Ordering::Relaxed);
            let _ = job.reply.send(Err(if cancelled {
                "Connection cancelled".into()
            } else {
                "Connection timed out".into()
            }));
        } else {
            accepts.push(job);
        }
    }
    let Some(listen) = listen else {
        return;
    };
    if !accepts.iter().any(|job| job.port == port) {
        return;
    }
    match listen.try_receive_event() {
        Some(ListenSocketEvent::Connecting(request)) => {
            let _ = request.accept();
        }
        Some(ListenSocketEvent::Connected(event)) => {
            let remote_id = event
                .remote()
                .steam_id()
                .map(|id| id.raw().to_string())
                .unwrap_or_default();
            let conn = event.take_connection();
            if remote_id.is_empty() {
                end(conn);
                return;
            }
            let Some(index) = accepts.iter().position(|job| job.port == port) else {
                end(conn);
                return;
            };
            let job = accepts.remove(index);
            if job.cancel.load(Ordering::Relaxed) {
                end(conn);
                let _ = job.reply.send(Err("Connection cancelled".into()));
                return;
            }
            let _ = job.reply.send(Ok(attach(client.clone(), conn, remote_id)));
        }
        Some(ListenSocketEvent::Disconnected(_)) | None => {}
    }
}

fn ice() -> Vec<NetworkingConfigEntry> {
    vec![
        NetworkingConfigEntry::new_int32(NetworkingConfigValue::P2PTransportICEEnable, ICE_ALL),
        NetworkingConfigEntry::new_int32(NetworkingConfigValue::SendBufferSize, SEND_BUFFER),
    ]
}

fn attach(
    client: Client,
    conn: steamworks::networking_sockets::NetConnection,
    remote_id: String,
) -> Pipe {
    if let Err(error) =
        client
            .networking_sockets()
            .configure_connection_lanes(&conn, 2, &[10, 0], &[1, 1])
    {
        // Lanes are an optimization. A single lane still carries the session.
        let _ = error;
    }
    let (out_tx, out_rx) = mpsc::unbounded_channel();
    let (in_tx, in_rx) = mpsc::unbounded_channel();
    let (route_tx, route_rx) = watch::channel(RouteSnap {
        name: "connecting".into(),
        rtt_ms: 0,
    });
    thread::Builder::new()
        .name("nanatype-steam-pipe".into())
        .spawn(move || pump(client, conn, out_rx, in_tx, route_tx))
        .expect("spawn Steam pipe");
    Pipe {
        remote_id,
        send: out_tx,
        recv: in_rx,
        route: route_rx,
    }
}

fn pump(
    client: Client,
    mut conn: steamworks::networking_sockets::NetConnection,
    mut outbound: mpsc::UnboundedReceiver<Outbound>,
    inbound_tx: mpsc::UnboundedSender<Inbound>,
    route: watch::Sender<RouteSnap>,
) {
    let sockets = client.networking_sockets();
    let utils = client.networking_utils();
    let mut asm = Assembler::new();
    let mut next_id = 1u32;
    let mut last_route = Instant::now() - Duration::from_secs(2);
    // Chunks Steam refused because its send buffer was full, oldest first.
    // Nothing new is chunked until they are all handed over, so each lane
    // keeps its order and a Close still follows everything queued before it.
    let mut pending: VecDeque<(i32, Vec<u8>)> = VecDeque::new();
    loop {
        if closed(&conn) {
            break;
        }
        let mut stop = false;
        loop {
            match flush(&utils, &sockets, &conn, &mut pending) {
                Ok(true) => {}
                Ok(false) => break,
                Err(_) => {
                    stop = true;
                    break;
                }
            }
            match outbound.try_recv() {
                Ok(Outbound::Close) | Err(mpsc::error::TryRecvError::Disconnected) => {
                    stop = true;
                    break;
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Ok(msg) => {
                    if let Some(packet) = as_packet(msg) {
                        let channel = lane(&packet);
                        match chunks(next_id, &packet) {
                            Ok(parts) => {
                                next_id = next_id.wrapping_add(1);
                                pending.extend(parts.into_iter().map(|part| (channel, part)));
                            }
                            Err(_) => {
                                stop = true;
                                break;
                            }
                        }
                    }
                }
            }
        }
        if stop {
            break;
        }
        let messages = match conn.receive_messages(32) {
            Ok(messages) => messages,
            Err(_) => break,
        };
        let mut failed = false;
        for message in messages {
            match asm.push(message.data()) {
                Ok(Some(packet)) => {
                    if inbound_tx.send(as_inbound(packet)).is_err() {
                        failed = true;
                        break;
                    }
                }
                Ok(None) => {}
                Err(_) => {
                    failed = true;
                    break;
                }
            }
        }
        if failed {
            break;
        }
        if last_route.elapsed() >= Duration::from_secs(1) {
            last_route = Instant::now();
            let name = conn
                .info()
                .map(|info| route_name(&info).to_string())
                .unwrap_or_else(|_| "connecting".into());
            let rtt_ms = sockets
                .get_realtime_connection_status(&conn, 0)
                .map(|(status, _)| status.ping().max(0) as u64)
                .unwrap_or(0);
            let _ = route.send(RouteSnap { name, rtt_ms });
        }
        thread::sleep(Duration::from_millis(2));
    }
    let _ = inbound_tx.send(Inbound::Closed);
    end(conn);
}

fn as_packet(msg: Outbound) -> Option<Packet> {
    match msg {
        Outbound::Frame(bytes) => Some(Packet::Frame(bytes)),
        Outbound::Want(hash) => Some(Packet::Want(hash)),
        Outbound::Asset { hash, bytes } => Some(Packet::Asset { hash, bytes }),
        Outbound::Close => None,
    }
}

fn as_inbound(packet: Packet) -> Inbound {
    match packet {
        Packet::Frame(bytes) => Inbound::Frame(bytes),
        Packet::Want(hash) => Inbound::Want(hash),
        Packet::Asset { hash, bytes } => Inbound::Asset { hash, bytes },
    }
}

/// Hands queued chunks to Steam in order. `Ok(true)` once the queue is empty,
/// `Ok(false)` when Steam's send buffer is full and the rest must wait for a
/// later pass; any other refusal ends the connection.
fn flush(
    utils: &steamworks::networking_utils::NetworkingUtils,
    sockets: &steamworks::networking_sockets::NetworkingSockets,
    conn: &steamworks::networking_sockets::NetConnection,
    pending: &mut VecDeque<(i32, Vec<u8>)>,
) -> Result<bool, String> {
    while let Some((channel, bytes)) = pending.front() {
        match send_message(utils, sockets, conn, *channel, bytes.clone()) {
            Ok(()) => {
                pending.pop_front();
            }
            Err(SteamError::LimitExceeded) => return Ok(false),
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(true)
}

/// Steam takes ownership of the bytes even when it refuses them, which is why
/// [`flush`] passes a copy and keeps the original until the send succeeds.
fn send_message(
    utils: &steamworks::networking_utils::NetworkingUtils,
    sockets: &steamworks::networking_sockets::NetworkingSockets,
    conn: &steamworks::networking_sockets::NetConnection,
    channel: i32,
    bytes: Vec<u8>,
) -> Result<(), SteamError> {
    let mut message = utils.allocate_message(0);
    message.set_connection(conn);
    message.set_send_flags(SendFlags::RELIABLE_NO_NAGLE);
    message.set_channel(channel);
    message
        .set_data(bytes)
        .map_err(|_| SteamError::InvalidParameter)?;
    sockets
        .send_messages(std::iter::once(message))
        .into_iter()
        .next()
        .ok_or(SteamError::Generic)?
        .map(|_| ())
}

fn state(
    conn: &steamworks::networking_sockets::NetConnection,
) -> Option<NetworkingConnectionState> {
    conn.info().ok()?.state().ok()
}

fn closed(conn: &steamworks::networking_sockets::NetConnection) -> bool {
    !matches!(
        state(conn),
        Some(
            NetworkingConnectionState::Connected
                | NetworkingConnectionState::Connecting
                | NetworkingConnectionState::FindingRoute
        )
    )
}

fn end(conn: steamworks::networking_sockets::NetConnection) {
    conn.close(
        NetConnectionEnd::App(AppNetConnectionEnd::generic_normal()),
        Some("session"),
        true,
    );
}

fn route_name(info: &steamworks::networking_types::NetConnectionInfo) -> &'static str {
    use std::mem::{align_of, size_of};
    type Info = steamworks::networking_types::NetConnectionInfo;
    type Raw = steamworks::sys::SteamNetConnectionInfo_t;
    if size_of::<Info>() != size_of::<Raw>() || align_of::<Info>() != align_of::<Raw>() {
        return "steam";
    }
    // `NetConnectionInfo` is a newtype around the C struct. The crate keeps
    // the relay flag private, and that flag is what distinguishes SDR.
    let raw = unsafe { &*(info as *const Info as *const Raw) };
    if (raw.m_nFlags & RELAYED) != 0 || raw.m_idPOPRelay != 0 {
        "relay"
    } else {
        "direct"
    }
}
