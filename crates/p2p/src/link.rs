//! A message pipe between the session and whatever carries the bytes.
//! The Steam build supplies a pipe from Networking Sockets. Tests supply a
//! pipe that never leaves the process.

#[cfg(test)]
use std::collections::VecDeque;
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(test)]
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, watch};

use crate::wire::Packet;
#[cfg(test)]
use crate::wire::{self, Assembler};

/// What the carrier last reported about the path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteSnap {
    /// `direct`, `relay`, or `connecting`.
    pub name: String,
    pub rtt_ms: u64,
}

/// What the session sends. One value is one complete payload.
#[derive(Debug)]
pub enum Outbound {
    Frame(Vec<u8>),
    Want(String),
    Asset { hash: String, bytes: Vec<u8> },
    Close,
}

/// What the session receives, after reassembly.
#[derive(Debug)]
pub enum Inbound {
    Frame(Vec<u8>),
    Want(String),
    Asset { hash: String, bytes: Vec<u8> },
    Closed,
}

/// One live connection.
pub struct Pipe {
    pub remote_id: String,
    pub send: mpsc::UnboundedSender<Outbound>,
    pub recv: mpsc::UnboundedReceiver<Inbound>,
    pub route: watch::Receiver<RouteSnap>,
}

/// Opens a connection. `local_id` is this side's SteamID in decimal.
///
/// Port 0 carries the session. Port 1 carries full pictures between the
/// publisher and a seated peer. `remote` is `None` to listen.
#[derive(Clone)]
pub struct Carrier {
    pub local_id: String,
    open: Arc<OpenPipe>,
}

type OpenPipe = dyn Fn(i32, Option<u64>, Arc<AtomicBool>) -> Result<Pipe, String> + Send + Sync;

impl Carrier {
    pub fn new(
        local_id: String,
        open: impl Fn(i32, Option<u64>, Arc<AtomicBool>) -> Result<Pipe, String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            local_id,
            open: Arc::new(open),
        }
    }

    /// Session pipe. `remote` is `None` when hosting. `cancel` becomes true
    /// when the attempt should be abandoned; the call returns after it notices.
    pub fn connect(&self, remote: Option<u64>, cancel: Arc<AtomicBool>) -> Result<Pipe, String> {
        (self.open)(0, remote, cancel)
    }

    /// Picture pipe. `remote` is `None` to accept one fetch, or the publisher's
    /// SteamID to ask them for a picture.
    pub fn connect_asset(
        &self,
        remote: Option<u64>,
        cancel: Arc<AtomicBool>,
    ) -> Result<Pipe, String> {
        (self.open)(1, remote, cancel)
    }
}

#[cfg(test)]
pub(crate) fn packet(msg: Outbound) -> Option<Packet> {
    match msg {
        Outbound::Frame(bytes) => Some(Packet::Frame(bytes)),
        Outbound::Want(hash) => Some(Packet::Want(hash)),
        Outbound::Asset { hash, bytes } => Some(Packet::Asset { hash, bytes }),
        Outbound::Close => None,
    }
}

#[cfg(test)]
pub(crate) fn inbound(packet: Packet) -> Inbound {
    match packet {
        Packet::Frame(bytes) => Inbound::Frame(bytes),
        Packet::Want(hash) => Inbound::Want(hash),
        Packet::Asset { hash, bytes } => Inbound::Asset { hash, bytes },
    }
}

/// Lane 0 is control (higher priority). Lane 1 is pictures.
pub fn lane(packet: &Packet) -> i32 {
    match packet {
        Packet::Asset { .. } => 1,
        Packet::Frame(_) | Packet::Want(_) => 0,
    }
}

#[cfg(test)]
struct Waiter {
    id: u64,
    local_id: u64,
    port: i32,
    remote: Option<u64>,
    reply: std::sync::mpsc::SyncSender<Pipe>,
}

#[cfg(test)]
struct HubInner {
    next: AtomicU64,
    waiters: Mutex<VecDeque<Waiter>>,
    fail_asset_dials: AtomicBool,
    asset_pairs: AtomicU64,
}

/// Pairs host and guest inside one process. Messages still pass through the
/// Steam chunk format, so the session tests cover reassembly.
#[derive(Clone)]
#[cfg(test)]
pub(crate) struct Hub {
    inner: Arc<HubInner>,
}

#[cfg(test)]
impl Hub {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(HubInner {
                next: AtomicU64::new(1),
                waiters: Mutex::new(VecDeque::new()),
                fail_asset_dials: AtomicBool::new(false),
                asset_pairs: AtomicU64::new(0),
            }),
        }
    }

    pub(crate) fn carrier(&self, steam_id: u64) -> Carrier {
        let inner = self.inner.clone();
        Carrier::new(steam_id.to_string(), move |port, remote, cancel| {
            rendezvous(&inner, steam_id, port, remote, &cancel)
        })
    }

    /// Picture dials fail immediately. Session pipes are unchanged.
    pub(crate) fn fail_asset_dials(&self) {
        self.inner.fail_asset_dials.store(true, Ordering::Relaxed);
    }

    pub(crate) fn asset_pairs(&self) -> u64 {
        self.inner.asset_pairs.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
fn rendezvous(
    hub: &HubInner,
    local_id: u64,
    port: i32,
    remote: Option<u64>,
    cancel: &AtomicBool,
) -> Result<Pipe, String> {
    if port == 1 && remote.is_some() && hub.fail_asset_dials.load(Ordering::Relaxed) {
        return Err("Connection failed".into());
    }
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let id = hub.next.fetch_add(1, Ordering::Relaxed);
    {
        let mut waiters = hub.waiters.lock().unwrap();
        if let Some(other) = take_match(&mut waiters, local_id, port, remote) {
            drop(waiters);
            let pipe = complete(local_id, other)?;
            if port == 1 {
                hub.asset_pairs.fetch_add(1, Ordering::Relaxed);
            }
            return Ok(pipe);
        }
        waiters.push_back(Waiter {
            id,
            local_id,
            port,
            remote,
            reply: tx,
        });
    }
    let started = Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            abandon(hub, id);
            return rx.try_recv().map_err(|_| "Connection cancelled".into());
        }
        match rx.try_recv() {
            Ok(pipe) => return Ok(pipe),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                return Err("Connection closed".into());
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
        if started.elapsed() > Duration::from_secs(30) {
            abandon(hub, id);
            return rx
                .try_recv()
                .map_err(|_| "Connection timed out".to_string());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(test)]
fn abandon(hub: &HubInner, id: u64) {
    hub.waiters.lock().unwrap().retain(|waiter| waiter.id != id);
}

#[cfg(test)]
fn take_match(
    waiters: &mut VecDeque<Waiter>,
    local_id: u64,
    port: i32,
    remote: Option<u64>,
) -> Option<Waiter> {
    let pos = waiters.iter().position(|waiter| {
        waiter.port == port
            && match (remote, waiter.remote) {
                (None, Some(target)) => target == local_id,
                (Some(target), None) => target == waiter.local_id,
                _ => false,
            }
    })?;
    waiters.remove(pos)
}

#[cfg(test)]
fn complete(local_id: u64, other: Waiter) -> Result<Pipe, String> {
    let (mine, theirs) = pair(local_id, other.local_id);
    other
        .reply
        .send(theirs)
        .map_err(|_| "Connection closed".to_string())?;
    Ok(mine)
}

#[cfg(test)]
fn pair(local_id: u64, remote_id: u64) -> (Pipe, Pipe) {
    let (out_a, out_a_rx) = mpsc::unbounded_channel();
    let (in_a, in_a_rx) = mpsc::unbounded_channel();
    let (out_b, out_b_rx) = mpsc::unbounded_channel();
    let (in_b, in_b_rx) = mpsc::unbounded_channel();
    let (_route_a, route_a) = watch::channel(RouteSnap {
        name: "direct".into(),
        rtt_ms: 1,
    });
    let (_route_b, route_b) = watch::channel(RouteSnap {
        name: "direct".into(),
        rtt_ms: 1,
    });
    spawn_forward(out_a_rx, in_b);
    spawn_forward(out_b_rx, in_a);
    (
        Pipe {
            remote_id: remote_id.to_string(),
            send: out_a,
            recv: in_a_rx,
            route: route_a,
        },
        Pipe {
            remote_id: local_id.to_string(),
            send: out_b,
            recv: in_b_rx,
            route: route_b,
        },
    )
}

#[cfg(test)]
fn spawn_forward(
    mut outbound: mpsc::UnboundedReceiver<Outbound>,
    inbound: mpsc::UnboundedSender<Inbound>,
) {
    std::thread::Builder::new()
        .name("nanatype-p2p-link".into())
        .spawn(move || {
            let mut asm = Assembler::new();
            let mut id = 1u32;
            while let Some(msg) = outbound.blocking_recv() {
                let close = matches!(msg, Outbound::Close);
                if let Some(packet) = packet(msg) {
                    let Ok(parts) = wire::chunks(id, &packet) else {
                        break;
                    };
                    id = id.wrapping_add(1);
                    let mut failed = false;
                    for part in parts {
                        match asm.push(&part) {
                            Ok(Some(packet)) => {
                                if inbound.send(inbound_msg(packet)).is_err() {
                                    return;
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
                }
                if close {
                    break;
                }
            }
            let _ = inbound.send(Inbound::Closed);
        })
        .expect("spawn session link");
}

#[cfg(test)]
fn inbound_msg(packet: Packet) -> Inbound {
    inbound(packet)
}
