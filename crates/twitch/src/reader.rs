//! Anonymous Twitch IRC reader thread with cancellation-aware backoff and
//! deadline-driven socket reads (0 Hz idle wakeups).
use crate::{ConnectionState, Event, Room, SessionEvent, socket};
use std::{
    net::{Shutdown, TcpStream},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

pub(crate) type Deliver = Arc<dyn Fn(SessionEvent) + Send + Sync>;

pub(crate) struct ReaderCancel {
    cancelled: AtomicBool,
    wake: Condvar,
    socket: Mutex<Option<TcpStream>>,
}

pub(crate) struct SocketGuard<'a>(&'a ReaderCancel);

impl Drop for SocketGuard<'_> {
    fn drop(&mut self) {
        *self.0.socket.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

impl ReaderCancel {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            cancelled: AtomicBool::new(false),
            wake: Condvar::new(),
            socket: Mutex::new(None),
        })
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        let stream = self
            .socket
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .and_then(|s| s.try_clone().ok());
        if let Some(stream) = stream {
            let _ = stream.shutdown(Shutdown::Both);
        }
        self.wake.notify_all();
    }

    pub fn attach_socket(&self, stream: Option<TcpStream>) -> SocketGuard<'_> {
        let mut slot = self.socket.lock().unwrap_or_else(|e| e.into_inner());
        if self.is_cancelled() {
            if let Some(s) = stream {
                let _ = s.shutdown(Shutdown::Both);
            }
        } else {
            *slot = stream;
        }
        SocketGuard(self)
    }

    /// Sleep up to `wait`, waking immediately on cancellation.
    fn wait_backoff(&self, wait: Duration) -> bool {
        let mut guard = self.socket.lock().unwrap_or_else(|e| e.into_inner());
        let deadline = Instant::now() + wait;
        while !self.is_cancelled() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return true;
            }
            let (next, _) = self
                .wake
                .wait_timeout(guard, remaining)
                .unwrap_or_else(|e| e.into_inner());
            guard = next;
        }
        false
    }
}

pub(crate) struct ReaderHandle {
    cancel: Arc<ReaderCancel>,
}

impl Drop for ReaderHandle {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

pub(crate) fn spawn(
    endpoint: String,
    channel: String,
    epoch: Arc<AtomicU64>,
    deliver: Deliver,
    on_room: impl Fn(Room) + Send + 'static,
) -> Result<ReaderHandle, ()> {
    let cancel = ReaderCancel::new();
    let worker_cancel = Arc::clone(&cancel);
    thread::Builder::new()
        .name("nanatype-twitch-chat".into())
        .spawn(move || {
            receive(
                &endpoint,
                &channel,
                &worker_cancel,
                &epoch,
                &deliver,
                &on_room,
            )
        })
        .map_err(|_| ())?;
    Ok(ReaderHandle { cancel })
}

fn delay(failures: u32) -> Duration {
    Duration::from_secs((1u64 << failures.saturating_sub(1).min(5)).min(30))
}

/// Keep connection to `channel` until `cancel` triggers.
/// Events carry the latest epoch: account changes do not drop chat.
pub(crate) fn receive(
    endpoint: &str,
    channel: &str,
    cancel: &ReaderCancel,
    epoch: &AtomicU64,
    deliver: &Deliver,
    on_room: &dyn Fn(Room),
) {
    let emit = |event| {
        deliver(SessionEvent {
            epoch: epoch.load(Ordering::Acquire),
            event,
        })
    };
    let mut failures = 0u32;
    while !cancel.is_cancelled() {
        emit(Event::Connection(ConnectionState::Connecting));
        let mut joined = false;
        let result = socket::run(
            endpoint,
            channel,
            cancel,
            &mut |room| {
                // ROOMSTATE repeats whenever a chat mode changes.
                if !joined {
                    joined = true;
                    on_room(room.clone());
                    emit(Event::Connection(ConnectionState::Connected(room)));
                }
            },
            &emit,
        );
        if cancel.is_cancelled() {
            return;
        }
        if joined {
            failures = 0;
        }
        let wait = match result {
            Ok(()) => Duration::from_secs(1),
            Err(code) => {
                failures = failures.saturating_add(1);
                emit(Event::Connection(ConnectionState::Error(code)));
                delay(failures)
            }
        };
        if !cancel.wait_backoff(wait) {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;
    use std::net::TcpListener;
    use tungstenite::Message;

    #[test]
    fn the_chat_joins_anonymously_and_delivers_after_roomstate() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut ws = tungstenite::accept(stream).unwrap();
            let mut said = Vec::new();
            while said.len() < 3 {
                if let Message::Text(text) = ws.read().unwrap() {
                    said.push(text.to_string());
                }
            }
            // Before the join is confirmed, chat is not ours yet.
            ws.send(Message::text(
                "@id=early;user-id=1 :a!a@a PRIVMSG #nana :early\r\n",
            ))
            .unwrap();
            ws.send(Message::text(
                ":tmi.twitch.tv CAP * ACK :twitch.tv/tags\r\n@room-id=1337 :tmi.twitch.tv ROOMSTATE #nana\r\nPING :tmi.twitch.tv\r\n",
            ))
            .unwrap();
            ws.send(Message::text(
                "@id=m1;user-id=2;display-name=B :b!b@b PRIVMSG #nana :hello\r\n",
            ))
            .unwrap();
            loop {
                match ws.read() {
                    Ok(Message::Text(text)) => said.push(text.to_string()),
                    Ok(_) => {}
                    Err(_) => break,
                }
                if said.iter().any(|s| s.starts_with("PONG")) {
                    break;
                }
            }
            said
        });
        let (tx, rx) = unbounded();
        let epoch = Arc::new(AtomicU64::new(7));
        let joined_room = Arc::new(Mutex::new(None));
        let cancel = ReaderCancel::new();
        let receiver = thread::spawn({
            let cancel = Arc::clone(&cancel);
            let epoch = Arc::clone(&epoch);
            let joined_room = Arc::clone(&joined_room);
            let deliver: Deliver = Arc::new(move |e| {
                let _ = tx.send(e);
            });
            move || {
                receive(
                    &format!("ws://{address}"),
                    "nana",
                    &cancel,
                    &epoch,
                    &deliver,
                    &|room| {
                        *joined_room.lock().unwrap() = Some(room);
                    },
                )
            }
        });
        let mut seen = Vec::new();
        while let Ok(event) = rx.recv_timeout(Duration::from_secs(5)) {
            assert_eq!(event.epoch, 7);
            seen.push(event.event.clone());
            if matches!(event.event, Event::Chat { .. }) {
                break;
            }
        }
        let said = server.join().unwrap();
        cancel.cancel();
        receiver.join().unwrap();
        assert_eq!(said[0], "CAP REQ :twitch.tv/tags twitch.tv/commands");
        assert!(said[1].starts_with("NICK justinfan"));
        assert_eq!(said[2], "JOIN #nana");
        assert!(said.contains(&"PONG :tmi.twitch.tv".to_owned()));
        let room = Room {
            login: "nana".into(),
            id: "1337".into(),
        };
        assert_eq!(seen[0], Event::Connection(ConnectionState::Connecting));
        assert_eq!(
            seen[1],
            Event::Connection(ConnectionState::Connected(room.clone()))
        );
        let Event::Chat { event, .. } = &seen[2] else {
            panic!("{seen:?}");
        };
        assert_eq!(event.id, "m1");
        assert_eq!(*joined_room.lock().unwrap(), Some(room));
    }
}
