use crate::{
    AccountState, Config, ConnectionState, Delivery, DeliveryState, Event, MAX_CHARS, Profile,
    Room, SessionEvent,
    api::{self, ApiError, Device, Http, Sent, TokenPoll, Tokens},
    credentials::Store,
    irc, socket,
};
use crossbeam_channel::{Receiver, Sender, unbounded};
use nanatype_model::{LineId, outgoing};
use std::{
    collections::{HashSet, VecDeque},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const SEND_INTERVAL: Duration = Duration::from_millis(1600);
const VALIDATE_EVERY: Duration = Duration::from_secs(3600);

enum Command {
    /// The epoch, the chat connection's generation and the new settings.
    Configure(u64, u64, Config),
    Login(u64),
    CancelLogin(u64),
    Refresh(u64),
    Logout(u64),
    Send {
        permit: Permit,
        epoch: u64,
        send_epoch: u64,
        line: LineId,
        text: String,
    },
    Retry(LineId),
    SkipFailed(LineId),
}
pub struct TwitchHandle {
    capacity: Arc<AtomicUsize>,
    tx: Sender<Command>,
    epoch: Arc<AtomicU64>,
    /// Advances only when the channel settings change, so signing in or out
    /// never drops the anonymous chat connection.
    link: Arc<AtomicU64>,
    send_epoch: Arc<AtomicU64>,
    sending: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
    config: Option<Config>,
}
impl TwitchHandle {
    pub fn new(directory: PathBuf, deliver: impl Fn(SessionEvent) + Send + Sync + 'static) -> Self {
        let (tx, rx) = unbounded();
        let handle = Self {
            capacity: Arc::new(AtomicUsize::new(0)),
            tx,
            epoch: Arc::new(AtomicU64::new(0)),
            link: Arc::new(AtomicU64::new(0)),
            send_epoch: Arc::new(AtomicU64::new(0)),
            sending: Arc::new(AtomicBool::new(false)),
            alive: Arc::new(AtomicBool::new(true)),
            config: None,
        };
        let worker = Worker::new(
            rx,
            &handle,
            Arc::new(deliver),
            Store::new(&directory),
            Box::new(api::Ureq::new()),
            irc::ENDPOINT.into(),
        );
        thread::Builder::new()
            .name("nanatype-twitch".into())
            .spawn(move || worker.run())
            .expect("start Twitch service");
        handle
    }
    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }
    fn advance(&self) -> u64 {
        self.set_sending(false);
        self.epoch.fetch_add(1, Ordering::AcqRel) + 1
    }
    pub fn set(&mut self, config: Config) {
        if self.config.as_ref() == Some(&config) {
            return;
        }
        self.config = Some(config.clone());
        let link = self.link.fetch_add(1, Ordering::AcqRel) + 1;
        let epoch = self.advance();
        let _ = self.tx.send(Command::Configure(epoch, link, config));
    }
    pub fn login(&self) {
        let e = self.advance();
        let _ = self.tx.send(Command::Login(e));
    }
    pub fn cancel_login(&self) {
        let e = self.advance();
        let _ = self.tx.send(Command::CancelLogin(e));
    }
    pub fn refresh(&self) {
        let e = self.advance();
        let _ = self.tx.send(Command::Refresh(e));
    }
    pub fn logout(&self) {
        let e = self.advance();
        let _ = self.tx.send(Command::Logout(e));
    }
    pub fn set_sending(&self, value: bool) {
        if !value {
            self.send_epoch.fetch_add(1, Ordering::AcqRel);
        }
        self.sending.store(value, Ordering::Release);
    }
    pub fn send(&self, line: LineId, text: String) -> bool {
        let Some(permit) = Permit::acquire(self.capacity.clone()) else {
            return false;
        };
        self.tx
            .send(Command::Send {
                permit,
                epoch: self.epoch(),
                send_epoch: self.send_epoch.load(Ordering::Acquire),
                line,
                text,
            })
            .is_ok()
    }
    pub fn retry(&self, line: LineId) {
        let _ = self.tx.send(Command::Retry(line));
    }
    pub fn skip_failed(&self, line: LineId) {
        let _ = self.tx.send(Command::SkipFailed(line));
    }
}
impl Drop for TwitchHandle {
    fn drop(&mut self) {
        self.set_sending(false);
        self.alive.store(false, Ordering::Release);
        self.epoch.fetch_add(1, Ordering::AcqRel);
    }
}

type Deliver = Arc<dyn Fn(SessionEvent) + Send + Sync>;
/// The joined room, tagged with the generation of the connection that joined it.
type RoomSlot = Arc<Mutex<Option<(u64, Room)>>>;

/// What a chat connection shares with the worker.
#[derive(Clone)]
struct Chat {
    link: Arc<AtomicU64>,
    epoch: Arc<AtomicU64>,
    alive: Arc<AtomicBool>,
    deliver: Deliver,
    room: RoomSlot,
}

struct Pending {
    _permit: Permit,
    delivery: Delivery,
    send_epoch: u64,
    epoch: u64,
}
/// A slot is reserved before posting to the worker, including while HTTP is busy.
struct Permit(Arc<AtomicUsize>);
impl Permit {
    fn acquire(capacity: Arc<AtomicUsize>) -> Option<Self> {
        let mut current = capacity.load(Ordering::Acquire);
        loop {
            if current >= 100 {
                return None;
            }
            match capacity.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(value) => current = value,
            }
        }
        Some(Self(capacity))
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

struct Login {
    epoch: u64,
    device: Device,
    started: Instant,
    interval: Duration,
}

struct Worker {
    rx: Receiver<Command>,
    epoch: Arc<AtomicU64>,
    send_epoch: Arc<AtomicU64>,
    sending: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
    deliver: Deliver,
    store: Store,
    http: Box<dyn Http>,
    endpoint: String,
    tokens: Option<Tokens>,
    profile: Option<Profile>,
    config: Config,
    link: Arc<AtomicU64>,
    room: RoomSlot,
    queue: VecDeque<Pending>,
    seen: HashSet<LineId>,
    login: Option<Login>,
    next_poll: Instant,
    next_send: Instant,
    next_validate: Instant,
    loaded: bool,
}
impl Worker {
    fn new(
        rx: Receiver<Command>,
        handle: &TwitchHandle,
        deliver: Deliver,
        store: Store,
        http: Box<dyn Http>,
        endpoint: String,
    ) -> Self {
        Self {
            rx,
            epoch: handle.epoch.clone(),
            send_epoch: handle.send_epoch.clone(),
            sending: handle.sending.clone(),
            alive: handle.alive.clone(),
            deliver,
            store,
            http,
            endpoint,
            tokens: None,
            profile: None,
            config: Config::default(),
            link: handle.link.clone(),
            room: Arc::new(Mutex::new(None)),
            queue: VecDeque::new(),
            seen: HashSet::new(),
            login: None,
            next_poll: Instant::now(),
            next_send: Instant::now(),
            next_validate: Instant::now() + VALIDATE_EVERY,
            loaded: false,
        }
    }
    fn emit(&self, epoch: u64, event: Event) {
        (self.deliver)(SessionEvent { epoch, event });
    }
    fn current(&self, e: u64) -> bool {
        self.alive.load(Ordering::Acquire) && self.epoch.load(Ordering::Acquire) == e
    }
    fn latest(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }
    fn run(mut self) {
        while self.alive.load(Ordering::Acquire) {
            match self.rx.recv_timeout(Duration::from_millis(100)) {
                Ok(c) => self.command(c),
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                Err(_) => {}
            }
            self.poll_login();
            self.revalidate();
            self.cancel_stale();
            self.send_next();
        }
    }
    fn command(&mut self, c: Command) {
        match c {
            Command::Configure(e, link, config) => {
                if !self.current(e) {
                    return;
                }
                self.config = config;
                self.login = None;
                // Chat first: the account check below may wait on the network.
                self.connect(e, link);
                if !self.loaded {
                    self.loaded = true;
                    match self.store.load() {
                        Ok(bytes) => self.tokens = bytes.and_then(|b| Tokens::from_bytes(&b)),
                        Err(error) => self.emit(e, Event::Warning(error)),
                    }
                    self.restore(e);
                }
            }
            Command::Login(e) => {
                if !self.current(e) {
                    return;
                }
                self.login = None;
                match api::device(self.http.as_ref()) {
                    Ok(device) if self.current(e) => {
                        self.emit(
                            e,
                            Event::Account(AccountState::Pending {
                                code: device.user_code.clone(),
                                uri: device.verification_uri.clone(),
                            }),
                        );
                        self.next_poll = Instant::now() + Duration::from_secs(device.interval);
                        self.login = Some(Login {
                            epoch: e,
                            interval: Duration::from_secs(device.interval),
                            device,
                            started: Instant::now(),
                        });
                    }
                    Err(error) if self.current(e) => {
                        self.emit(e, Event::Account(AccountState::Failed(error.to_string())))
                    }
                    _ => {}
                }
            }
            Command::CancelLogin(e) => {
                if !self.current(e) {
                    return;
                }
                self.login = None;
                self.announce(e);
            }
            Command::Refresh(e) => {
                if !self.current(e) {
                    return;
                }
                self.login = None;
                self.restore(e);
            }
            Command::Logout(e) => {
                if !self.current(e) {
                    return;
                }
                self.login = None;
                self.forget();
                match self.store.clear() {
                    Ok(()) => self.emit(e, Event::Account(AccountState::SignedOut)),
                    Err(error) => self.emit(e, Event::Account(AccountState::Failed(error))),
                }
            }
            Command::Send {
                permit,
                epoch,
                send_epoch,
                line,
                text,
            } => {
                if !self.current(epoch)
                    || !self.sending.load(Ordering::Acquire)
                    || send_epoch != self.send_epoch.load(Ordering::Acquire)
                    || !self.seen.insert(line)
                {
                    return;
                }
                let link = self.link.load(Ordering::Acquire);
                let room = self
                    .room
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .clone()
                    .filter(|(g, _)| *g == link)
                    .map(|(_, room)| room);
                let (Some(room), Some(profile)) = (room, &self.profile) else {
                    return;
                };
                // The permit already caps the queue at 100 lines.
                let (chunks, state, detail) = match outgoing::split(&text, MAX_CHARS) {
                    Ok(chunks) => (chunks, DeliveryState::Queued, String::new()),
                    Err(error) => (vec![text], DeliveryState::Failed, error.to_string()),
                };
                let delivery = Delivery {
                    line,
                    room: room.id,
                    uid: profile.user_id.clone(),
                    chunks,
                    completed: 0,
                    state,
                    detail,
                    blocked: false,
                    receipts: Vec::new(),
                };
                self.emit(epoch, Event::Delivery(delivery.clone()));
                if state == DeliveryState::Queued {
                    self.queue.push_back(Pending {
                        _permit: permit,
                        delivery,
                        send_epoch,
                        epoch,
                    });
                }
            }
            Command::Retry(line) => {
                if let Some(p) = self.queue.front_mut()
                    && p.delivery.line == line
                    && matches!(
                        p.delivery.state,
                        DeliveryState::Failed | DeliveryState::Uncertain
                    )
                {
                    p.delivery.state = DeliveryState::Queued;
                    p.delivery.blocked = false;
                }
            }
            Command::SkipFailed(line) => {
                if self
                    .queue
                    .front()
                    .is_some_and(|p| p.delivery.blocked && p.delivery.line == line)
                {
                    self.cancel_front();
                }
            }
        }
    }
    /// Repeat the account state for a new epoch.
    fn announce(&self, e: u64) {
        let state = match &self.profile {
            Some(profile) => AccountState::Ready(profile.clone()),
            None => AccountState::SignedOut,
        };
        self.emit(e, Event::Account(state));
    }
    fn forget(&mut self) {
        self.tokens = None;
        self.profile = None;
        self.sending.store(false, Ordering::Release);
    }
    fn save(&self, e: u64, tokens: &Tokens) {
        if let Err(error) = self.store.save(&tokens.to_bytes()) {
            self.emit(e, Event::Warning(error));
        }
    }
    /// Validate `tokens`, refreshing them once if Twitch says they expired.
    fn authorize(&self, e: u64, tokens: &Tokens) -> Result<(Tokens, Profile), ApiError> {
        let http = self.http.as_ref();
        let (tokens, user) = match api::validate(http, tokens) {
            Ok(user) => (tokens.clone(), user),
            Err(ApiError::Unauthorized) => {
                let fresh = api::refresh(http, tokens)?;
                // The old refresh token is spent; losing the new one signs the user out.
                self.save(e, &fresh);
                let user = api::validate(http, &fresh)?;
                (fresh, user)
            }
            Err(error) => return Err(error),
        };
        let profile = api::profile(http, &tokens, &user)?;
        Ok((tokens, profile))
    }
    fn restore(&mut self, e: u64) {
        let Some(tokens) = self.tokens.clone() else {
            self.profile = None;
            self.emit(e, Event::Account(AccountState::SignedOut));
            return;
        };
        self.emit(e, Event::Account(AccountState::Restoring));
        self.adopt(e, &tokens);
    }
    fn adopt(&mut self, e: u64, tokens: &Tokens) {
        match self.authorize(e, tokens) {
            Ok((tokens, profile)) => {
                self.tokens = Some(tokens);
                self.profile = Some(profile.clone());
                self.next_validate = Instant::now() + VALIDATE_EVERY;
                self.emit(e, Event::Account(AccountState::Ready(profile)));
            }
            Err(ApiError::Unauthorized) => self.expire(e),
            Err(error) => {
                self.profile = None;
                self.emit(e, Event::Account(AccountState::Failed(error.to_string())));
            }
        }
    }
    fn expire(&mut self, e: u64) {
        self.forget();
        if let Err(error) = self.store.clear() {
            nanatype_diag::report!(crate::diag::CREDENTIALS_CLEAR_FAILED; "{error}");
        }
        self.emit(e, Event::Account(AccountState::Expired));
    }
    fn revalidate(&mut self) {
        if self.profile.is_none() || Instant::now() < self.next_validate {
            return;
        }
        self.next_validate = Instant::now() + VALIDATE_EVERY;
        let Some(tokens) = self.tokens.clone() else {
            return;
        };
        let e = self.latest();
        match self.authorize(e, &tokens) {
            Ok((tokens, _)) => self.tokens = Some(tokens),
            Err(ApiError::Unauthorized) => self.expire(e),
            // Offline for a moment; try again at the next check.
            Err(error) => {
                nanatype_diag::report!(crate::diag::REVALIDATE_DEFERRED; "{error}");
                self.next_validate = Instant::now() + Duration::from_secs(300);
            }
        }
    }
    fn connect(&mut self, e: u64, link: u64) {
        *self.room.lock().unwrap_or_else(|p| p.into_inner()) = None;
        if !self.config.enabled {
            self.emit(e, Event::Connection(ConnectionState::Off));
            return;
        }
        let channel = match crate::channel(&self.config.channel) {
            Ok(channel) => channel,
            Err(error) => {
                self.emit(e, Event::Connection(ConnectionState::Error(error)));
                return;
            }
        };
        let endpoint = self.endpoint.clone();
        let chat = Chat {
            link: self.link.clone(),
            epoch: self.epoch.clone(),
            alive: self.alive.clone(),
            deliver: self.deliver.clone(),
            room: self.room.clone(),
        };
        if thread::Builder::new()
            .name("nanatype-twitch-chat".into())
            .spawn(move || receive(&endpoint, &channel, link, chat))
            .is_err()
        {
            self.emit(
                e,
                Event::Connection(ConnectionState::Error("worker-start-failed".into())),
            );
        }
    }
    fn poll_login(&mut self) {
        if Instant::now() < self.next_poll {
            return;
        }
        let Some(login) = &self.login else {
            return;
        };
        let e = login.epoch;
        if !self.current(e) {
            self.login = None;
            return;
        }
        self.next_poll = Instant::now() + login.interval;
        if login.started.elapsed() >= Duration::from_secs(login.device.expires_in) {
            self.login = None;
            self.emit(
                e,
                Event::Account(AccountState::Failed("login-expired".into())),
            );
            return;
        }
        let result = api::poll(self.http.as_ref(), &login.device.device_code);
        if !self.current(e) {
            return;
        }
        match result {
            Ok(TokenPoll::Pending) => {}
            Ok(TokenPoll::SlowDown) => {
                if let Some(login) = &mut self.login {
                    login.interval += Duration::from_secs(5);
                }
            }
            Ok(TokenPoll::Ready(tokens)) => {
                self.login = None;
                self.save(e, &tokens);
                self.adopt(e, &tokens);
            }
            Ok(TokenPoll::Failed(message)) => {
                self.login = None;
                self.emit(e, Event::Account(AccountState::Failed(message)));
            }
            // A dropped poll is retried at the next interval.
            Err(error) => nanatype_diag::report!(crate::diag::TOKEN_POLL_FAILED; "{error}"),
        }
    }
    fn paused(&self) -> bool {
        self.queue.front().is_some_and(|p| p.delivery.blocked)
    }
    fn cancel_front(&mut self) {
        if let Some(mut p) = self.queue.pop_front() {
            p.delivery.state = DeliveryState::Cancelled;
            p.delivery.blocked = false;
            self.emit(p.epoch, Event::Delivery(p.delivery));
        }
    }
    fn cancel_stale(&mut self) {
        while self.queue.front().is_some_and(|p| {
            !self.current(p.epoch)
                || p.send_epoch != self.send_epoch.load(Ordering::Acquire)
                || !self.sending.load(Ordering::Acquire)
        }) {
            self.cancel_front();
        }
    }
    fn send_next(&mut self) {
        if self.paused() || Instant::now() < self.next_send || self.queue.is_empty() {
            return;
        }
        let Some(tokens) = self.tokens.clone() else {
            return;
        };
        let p = &self.queue[0].delivery;
        let (room, uid, text) = (p.room.clone(), p.uid.clone(), p.chunks[p.completed].clone());
        let http = self.http.as_ref();
        let mut result = api::send(http, &tokens, &room, &uid, &text);
        if result == Err(ApiError::Unauthorized) {
            // Twitch refuses before posting, so one retry after refreshing is safe.
            match api::refresh(http, &tokens) {
                Ok(fresh) => {
                    self.save(self.latest(), &fresh);
                    result = api::send(http, &fresh, &room, &uid, &text);
                    self.tokens = Some(fresh);
                }
                Err(error) => result = Err(error),
            }
        }
        self.finish(result);
    }
    fn finish(&mut self, result: Result<Sent, ApiError>) {
        let Some(mut p) = self.queue.pop_front() else {
            return;
        };
        self.next_send = Instant::now() + SEND_INTERVAL;
        match result {
            Ok(Sent::Accepted { id }) => {
                p.delivery.completed += 1;
                if !id.is_empty() {
                    p.delivery.receipts.push(id);
                }
                p.delivery.state = if p.delivery.completed == p.delivery.chunks.len() {
                    DeliveryState::Submitted
                } else {
                    DeliveryState::Queued
                };
                self.emit(p.epoch, Event::Delivery(p.delivery.clone()));
                if p.delivery.state != DeliveryState::Submitted {
                    self.queue.push_front(p);
                }
            }
            Ok(Sent::Dropped(reason)) => self.block(p, DeliveryState::Failed, reason),
            Err(ApiError::Unauthorized) => {
                let e = p.epoch;
                self.block(p, DeliveryState::Failed, ApiError::Unauthorized.to_string());
                if self.current(e) {
                    self.expire(e);
                }
            }
            Err(error @ ApiError::Rejected { .. }) => {
                self.block(p, DeliveryState::Failed, error.to_string())
            }
            // The request may have reached Twitch; only the user can tell.
            Err(error) => self.block(p, DeliveryState::Uncertain, error.to_string()),
        }
    }
    fn block(&mut self, mut p: Pending, state: DeliveryState, detail: String) {
        p.delivery.state = state;
        p.delivery.detail = detail;
        p.delivery.blocked = true;
        self.emit(p.epoch, Event::Delivery(p.delivery.clone()));
        self.queue.push_front(p);
    }
}

fn delay(failures: u32) -> Duration {
    Duration::from_secs((1u64 << failures.saturating_sub(1).min(5)).min(30))
}

/// Keep connection `link` to `channel` until the channel settings change.
/// Events carry the latest epoch: account changes do not end the connection.
fn receive(endpoint: &str, channel: &str, link: u64, chat: Chat) {
    let current =
        || chat.alive.load(Ordering::Acquire) && chat.link.load(Ordering::Acquire) == link;
    let emit = |event| {
        (chat.deliver)(SessionEvent {
            epoch: chat.epoch.load(Ordering::Acquire),
            event,
        })
    };
    let mut failures = 0u32;
    while current() {
        emit(Event::Connection(ConnectionState::Connecting));
        let mut joined = false;
        let result = socket::run(
            endpoint,
            channel,
            &current,
            &mut |room| {
                // ROOMSTATE repeats whenever a chat mode changes.
                if !joined {
                    joined = true;
                    *chat.room.lock().unwrap_or_else(|p| p.into_inner()) =
                        Some((link, room.clone()));
                    emit(Event::Connection(ConnectionState::Connected(room)));
                }
            },
            &emit,
        );
        if !current() {
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
        let end = Instant::now() + wait;
        while current() && Instant::now() < end {
            thread::sleep(Duration::from_millis(50));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::fake::Fake;
    use serde_json::json;
    use std::net::TcpListener;
    use tungstenite::Message;

    struct Rig {
        worker: Worker,
        events: Receiver<SessionEvent>,
        fake: Fake,
        saved: Arc<Mutex<Option<Vec<u8>>>>,
        handle: TwitchHandle,
    }
    fn tokens(access: &str) -> Tokens {
        Tokens {
            access: access.into(),
            refresh: format!("{access}-refresh"),
        }
    }
    fn rig() -> Rig {
        let (tx, rx) = unbounded();
        let (events, out) = unbounded();
        let fake = Fake::default();
        let saved = Arc::new(Mutex::new(None));
        let handle = TwitchHandle {
            capacity: Arc::new(AtomicUsize::new(0)),
            tx,
            epoch: Arc::new(AtomicU64::new(1)),
            link: Arc::new(AtomicU64::new(1)),
            send_epoch: Arc::new(AtomicU64::new(1)),
            sending: Arc::new(AtomicBool::new(true)),
            alive: Arc::new(AtomicBool::new(true)),
            config: None,
        };
        let mut worker = Worker::new(
            rx,
            &handle,
            Arc::new(move |e| {
                let _ = events.send(e);
            }),
            Store::Memory(saved.clone()),
            Box::new(fake.clone()),
            "ws://127.0.0.1:9".into(),
        );
        worker.tokens = Some(tokens("a"));
        worker.profile = Some(Profile {
            user_id: "7".into(),
            login: "me".into(),
            name: "Me".into(),
            avatar: String::new(),
        });
        *worker.room.lock().unwrap() = Some((
            1,
            Room {
                login: "nana".into(),
                id: "1337".into(),
            },
        ));
        Rig {
            worker,
            events: out,
            fake,
            saved,
            handle,
        }
    }
    impl Rig {
        fn enqueue(&mut self, id: u64, text: &str) {
            self.worker.command(Command::Send {
                permit: Permit::acquire(Arc::new(AtomicUsize::new(0))).unwrap(),
                epoch: 1,
                send_epoch: 1,
                line: LineId(id),
                text: text.into(),
            });
        }
        fn tick(&mut self) {
            self.worker.next_send = Instant::now();
            self.worker.cancel_stale();
            self.worker.send_next();
        }
        fn deliveries(&self) -> Vec<Delivery> {
            self.events
                .try_iter()
                .filter_map(|e| match e.event {
                    Event::Delivery(d) => Some(d),
                    _ => None,
                })
                .collect()
        }
        fn accept(&self, id: &str) {
            self.fake.answer(
                "https://api.twitch.tv/helix/chat/messages",
                Ok((200, json!({"data": [{"message_id": id, "is_sent": true}]}))),
            );
        }
    }

    #[test]
    fn long_lines_go_out_in_chunks_with_one_receipt_each() {
        let mut rig = rig();
        let text = format!("{}, {}", "a".repeat(400), "b".repeat(300));
        rig.enqueue(1, &text);
        rig.enqueue(1, &text);
        assert_eq!(rig.worker.queue.len(), 1);
        rig.accept("m1");
        rig.accept("m2");
        rig.tick();
        rig.tick();
        let last = rig.deliveries().pop().unwrap();
        assert_eq!(last.state, DeliveryState::Submitted);
        assert_eq!(last.receipts, ["m1", "m2"]);
        assert_eq!(last.room, "1337");
        assert_eq!(last.uid, "7");
        assert_eq!(last.chunks.concat(), text);
        assert!(rig.worker.queue.is_empty());
        let asked = rig.fake.asked.lock().unwrap();
        assert!(asked[0].1.contains(&format!("{}, ", "a".repeat(400))));
    }

    #[test]
    fn a_dropped_message_blocks_the_queue_until_retried_or_skipped() {
        let mut rig = rig();
        rig.enqueue(1, "first");
        rig.enqueue(2, "second");
        rig.fake.answer(
            "https://api.twitch.tv/helix/chat/messages",
            Ok((200, json!({"data": [{"is_sent": false, "drop_reason": {"message": "AutoMod held it"}}]}))),
        );
        rig.tick();
        let failed = rig.deliveries().pop().unwrap();
        assert_eq!(failed.state, DeliveryState::Failed);
        assert_eq!(failed.detail, "AutoMod held it");
        assert!(failed.blocked);
        // Nothing else leaves while the head is blocked.
        rig.tick();
        assert!(rig.deliveries().is_empty());
        rig.worker.command(Command::Retry(LineId(1)));
        rig.accept("m1");
        rig.tick();
        assert_eq!(
            rig.deliveries().pop().unwrap().state,
            DeliveryState::Submitted
        );
        rig.fake.answer(
            "https://api.twitch.tv/helix/chat/messages",
            Err(ApiError::Network),
        );
        rig.tick();
        assert_eq!(
            rig.deliveries().pop().unwrap().state,
            DeliveryState::Uncertain
        );
        rig.worker.command(Command::SkipFailed(LineId(2)));
        assert_eq!(
            rig.deliveries().pop().unwrap().state,
            DeliveryState::Cancelled
        );
        assert!(rig.worker.queue.is_empty());
    }

    #[test]
    fn an_expired_token_is_refreshed_once_and_kept() {
        let mut rig = rig();
        rig.enqueue(1, "hi");
        let url = "https://api.twitch.tv/helix/chat/messages";
        rig.fake
            .answer(url, Ok((401, json!({"message": "Invalid OAuth token"}))));
        rig.fake.answer(
            "https://id.twitch.tv/oauth2/token",
            Ok((
                200,
                json!({"access_token": "b", "refresh_token": "b-refresh"}),
            )),
        );
        rig.accept("m1");
        rig.tick();
        assert_eq!(
            rig.deliveries().pop().unwrap().state,
            DeliveryState::Submitted
        );
        assert_eq!(rig.worker.tokens, Some(tokens("b")));
        assert_eq!(
            Tokens::from_bytes(rig.saved.lock().unwrap().as_ref().unwrap()),
            Some(tokens("b"))
        );
        assert!(rig.fake.asked.lock().unwrap()[2].1.starts_with("Bearer b "));
    }

    #[test]
    fn a_refused_refresh_signs_out_and_stops_sending() {
        let mut rig = rig();
        *rig.saved.lock().unwrap() = Some(tokens("a").to_bytes());
        rig.enqueue(1, "hi");
        rig.fake.answer(
            "https://api.twitch.tv/helix/chat/messages",
            Ok((401, json!({}))),
        );
        rig.fake.answer(
            "https://id.twitch.tv/oauth2/token",
            Ok((400, json!({"message": "Invalid refresh token"}))),
        );
        rig.tick();
        let events: Vec<_> = rig.events.try_iter().map(|e| e.event).collect();
        assert!(events.contains(&Event::Account(AccountState::Expired)));
        assert!(!rig.handle.sending.load(Ordering::Acquire));
        assert!(rig.worker.tokens.is_none());
        assert!(rig.saved.lock().unwrap().is_none());
        // The blocked line is cancelled now that sending is off.
        rig.tick();
        assert_eq!(
            rig.deliveries().pop().unwrap().state,
            DeliveryState::Cancelled
        );
    }

    #[test]
    fn stale_lines_never_reach_twitch() {
        let mut rig = rig();
        rig.enqueue(1, "hi");
        rig.handle.set_sending(false);
        rig.tick();
        assert_eq!(
            rig.deliveries().pop().unwrap().state,
            DeliveryState::Cancelled
        );
        // A line from an old room epoch is refused outright.
        rig.handle.set_sending(true);
        rig.handle.epoch.store(2, Ordering::Release);
        rig.worker.command(Command::Send {
            permit: Permit::acquire(Arc::new(AtomicUsize::new(0))).unwrap(),
            epoch: 1,
            send_epoch: rig.handle.send_epoch.load(Ordering::Acquire),
            line: LineId(9),
            text: "late".into(),
        });
        assert!(rig.worker.queue.is_empty());
        assert!(rig.fake.asked.lock().unwrap().is_empty());
    }

    #[test]
    fn the_device_flow_signs_in_and_stores_tokens_only_in_protected_storage() {
        let mut rig = rig();
        rig.worker.tokens = None;
        rig.worker.profile = None;
        rig.fake.answer(
            "https://id.twitch.tv/oauth2/device",
            Ok((
                200,
                json!({"device_code": "dev", "user_code": "ABCD-EFGH",
                "verification_uri": "https://www.twitch.tv/activate?device-code=ABCD-EFGH",
                "expires_in": 1800, "interval": 5}),
            )),
        );
        rig.worker.command(Command::Login(1));
        let token = "https://id.twitch.tv/oauth2/token";
        rig.fake.answer(
            token,
            Ok((400, json!({"message": "authorization_pending"}))),
        );
        rig.fake.answer(
            token,
            Ok((
                200,
                json!({"access_token": "n", "refresh_token": "n-refresh"}),
            )),
        );
        rig.fake.answer(
            "https://id.twitch.tv/oauth2/validate",
            Ok((200, json!({"user_id": "7", "scopes": ["user:write:chat"]}))),
        );
        rig.fake.answer(
            "https://api.twitch.tv/helix/users",
            Ok((
                200,
                json!({"data": [{"id": "7", "login": "me", "display_name": "Me",
                "profile_image_url": "https://static-cdn.jtvnw.net/me.png"}]}),
            )),
        );
        for _ in 0..2 {
            rig.worker.next_poll = Instant::now();
            rig.worker.poll_login();
        }
        let accounts: Vec<_> = rig
            .events
            .try_iter()
            .filter_map(|e| match e.event {
                Event::Account(a) => Some(a),
                _ => None,
            })
            .collect();
        assert_eq!(
            accounts,
            [
                AccountState::Pending {
                    code: "ABCD-EFGH".into(),
                    uri: "https://www.twitch.tv/activate?device-code=ABCD-EFGH".into()
                },
                AccountState::Ready(Profile {
                    user_id: "7".into(),
                    login: "me".into(),
                    name: "Me".into(),
                    avatar: "https://static-cdn.jtvnw.net/me.png".into()
                })
            ]
        );
        assert_eq!(
            Tokens::from_bytes(rig.saved.lock().unwrap().as_ref().unwrap()),
            Some(tokens("n"))
        );
    }

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
        let chat = Chat {
            link: Arc::new(AtomicU64::new(1)),
            epoch: Arc::new(AtomicU64::new(7)),
            alive: Arc::new(AtomicBool::new(true)),
            deliver: Arc::new(move |e| {
                let _ = tx.send(e);
            }),
            room: Arc::new(Mutex::new(None)),
        };
        let receiver = thread::spawn({
            let chat = chat.clone();
            move || receive(&format!("ws://{address}"), "nana", 1, chat)
        });
        let mut seen = Vec::new();
        while let Ok(event) = rx.recv_timeout(Duration::from_secs(5)) {
            // Tagged with the current epoch, whatever it was when the chat started.
            assert_eq!(event.epoch, 7);
            seen.push(event.event.clone());
            if matches!(event.event, Event::Chat { .. }) {
                break;
            }
        }
        let said = server.join().unwrap();
        chat.link.store(2, Ordering::Release);
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
        assert_eq!(*chat.room.lock().unwrap(), Some((1, room)));
    }

    /// `NANATYPE_TWITCH_CHANNEL=<login> cargo test -p nanatype-twitch -- --ignored`
    #[test]
    #[ignore = "joins a real Twitch channel"]
    fn live_chat_arrives_from_a_real_channel() {
        let channel = std::env::var("NANATYPE_TWITCH_CHANNEL").expect("NANATYPE_TWITCH_CHANNEL");
        let directory = std::env::temp_dir();
        let (tx, rx) = unbounded();
        let mut handle = TwitchHandle::new(directory, move |e| {
            let _ = tx.send(e);
        });
        handle.set(Config {
            enabled: true,
            channel,
        });
        let deadline = Instant::now() + Duration::from_secs(120);
        while let Ok(event) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            match event.event {
                Event::Chat { event, .. } => {
                    eprintln!("{}: {:?}", event.author.name, event.content);
                    return;
                }
                Event::Connection(ConnectionState::Error(e)) => panic!("{e}"),
                other => eprintln!("{other:?}"),
            }
        }
        panic!("no chat within two minutes");
    }
}
