use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use serde::Serialize;
use tokio::sync::mpsc;

use crate::link::{Carrier, Inbound, Outbound, Pipe};
use crate::protocol::Frame;
use crate::protocol::MAX_FRAME;

/// An unauthenticated peer may only send a hello this large.
pub(super) const MAX_HELLO: usize = 8 * 1024;
/// Sustained frames per second accepted from a peer; more are delayed.
pub(super) const MAX_FRAMES_PER_SECOND: u32 = 40;

pub(super) fn pack(value: &impl Serialize) -> Result<Vec<u8>, String> {
    let json = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    if json.len() > MAX_FRAME {
        return Err("Session frame exceeds limit".into());
    }
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    std::io::Write::write_all(&mut encoder, &json).map_err(|e| e.to_string())?;
    encoder.finish().map_err(|e| e.to_string())
}

pub(super) fn unpack(bytes: &[u8], max: usize) -> Result<Vec<u8>, String> {
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

pub(super) fn send_frame(
    send: &mpsc::UnboundedSender<Outbound>,
    value: &impl Serialize,
) -> Result<(), String> {
    send_packed(send, pack(value)?)
}

pub(super) fn send_packed(
    send: &mpsc::UnboundedSender<Outbound>,
    packed: Vec<u8>,
) -> Result<(), String> {
    send.send(Outbound::Frame(packed))
        .map_err(|_| "Connection closed".into())
}

pub(super) struct Arm {
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

pub(super) struct Abort(pub tokio::task::JoinHandle<()>);

impl Drop for Abort {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub(super) async fn open_pipe(
    carrier: &Carrier,
    remote: Option<u64>,
    deadline: Instant,
) -> Result<Pipe, String> {
    open_port(carrier, remote, deadline, false).await
}

pub(super) async fn open_asset(
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

pub(super) async fn read_frame(
    recv: &mut mpsc::UnboundedReceiver<Inbound>,
    max: usize,
) -> Result<Frame, String> {
    match recv.recv().await {
        Some(Inbound::Frame(bytes)) => decode_frame(&bytes, max),
        Some(Inbound::Closed) | None => Err("Connection closed".into()),
        Some(Inbound::Want(_) | Inbound::Asset { .. }) => Err("Unexpected session message".into()),
    }
}

pub(super) fn decode_frame(bytes: &[u8], max: usize) -> Result<Frame, String> {
    if bytes.len() > max {
        return Err("Session frame exceeds limit".into());
    }
    let json = unpack(bytes, max)?;
    let mut frame: Frame = serde_json::from_slice(&json).map_err(|e| e.to_string())?;
    frame.sanitize_assets();
    Ok(frame)
}

pub(super) enum ReaderSink {
    Host {
        id: String,
        generation: u64,
        tx: mpsc::Sender<(String, u64, Inbound)>,
    },
    Guest(mpsc::Sender<Inbound>),
}

impl ReaderSink {
    async fn send(&self, message: Inbound) -> bool {
        match self {
            Self::Host { id, generation, tx } => {
                tx.send((id.clone(), *generation, message)).await.is_ok()
            }
            Self::Guest(tx) => tx.send(message).await.is_ok(),
        }
    }
}

/// Forward inbound messages from one pipe with sustained per-second frame
/// rate-limiting.
pub(super) fn spawn_rate_limited_reader(
    mut recv: mpsc::UnboundedReceiver<Inbound>,
    sink: ReaderSink,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut window = Instant::now();
        let mut frames = 0u32;
        while let Some(message) = recv.recv().await {
            let closed = matches!(message, Inbound::Closed);
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
            if !sink.send(message).await || closed {
                break;
            }
        }
    })
}

/// Wait until `deadline`, or pend indefinitely when `None` (no idle wakeups).
pub(super) async fn sleep_until_opt(deadline: Option<Instant>) {
    match deadline {
        Some(at) => tokio::time::sleep_until(tokio::time::Instant::from_std(at)).await,
        None => std::future::pending::<()>().await,
    }
}
