//! Outbound Twitch Helix chat sender queue running on its own thread,
//! rate-limited to `SEND_INTERVAL` with 0 Hz idle waiting.
use crate::{
    Delivery, DeliveryState, Event, MAX_CHARS, Profile, Room, SessionEvent,
    api::{self, ApiError, Http, Sent, Tokens},
    reader::Deliver,
};
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TryRecvError};
use nanatype_model::{LineId, outgoing};
use std::{
    collections::{HashSet, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const SEND_INTERVAL: Duration = Duration::from_millis(1600);

/// A slot is reserved before posting to the sender, including while HTTP is busy.
pub(crate) struct Permit(Arc<AtomicUsize>);

impl Permit {
    pub fn acquire(capacity: Arc<AtomicUsize>) -> Option<Self> {
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

pub(crate) enum AuthSync {
    Refreshed(Tokens),
    Expired(u64),
}

pub(crate) enum SenderCmd {
    Configure,
    SetEpoch,
    SetSending(bool),
    SetRoom(Option<Room>),
    SetAuth(Option<(Tokens, Profile)>),
    Send {
        permit: Permit,
        epoch: u64,
        line: LineId,
        text: String,
    },
    Retry(LineId),
    SkipFailed(LineId),
}

pub(crate) struct SenderHandle {
    tx: Sender<SenderCmd>,
}

impl SenderHandle {
    pub fn spawn(
        tx: Sender<SenderCmd>,
        rx: Receiver<SenderCmd>,
        epoch: Arc<AtomicU64>,
        deliver: Deliver,
        http: Arc<dyn Http>,
        on_auth_sync: Box<dyn Fn(AuthSync) + Send>,
    ) -> Self {
        let worker = SenderWorker::new(rx, epoch, deliver, http, on_auth_sync);
        thread::Builder::new()
            .name("nanatype-twitch-sender".into())
            .spawn(move || worker.run())
            .expect("start Twitch sender worker");
        Self { tx }
    }

    pub fn sender(&self) -> Sender<SenderCmd> {
        self.tx.clone()
    }

    pub fn send(&self, cmd: SenderCmd) {
        let _ = self.tx.send(cmd);
    }
}

struct Pending {
    _permit: Permit,
    delivery: Delivery,
    epoch: u64,
}

pub(crate) struct SenderWorker {
    rx: Receiver<SenderCmd>,
    epoch: Arc<AtomicU64>,
    deliver: Deliver,
    http: Arc<dyn Http>,
    on_auth_sync: Box<dyn Fn(AuthSync) + Send>,
    tokens: Option<Tokens>,
    profile: Option<Profile>,
    room: Option<Room>,
    sending: bool,
    queue: VecDeque<Pending>,
    seen: HashSet<LineId>,
    next_send: Instant,
}

impl SenderWorker {
    pub fn new(
        rx: Receiver<SenderCmd>,
        epoch: Arc<AtomicU64>,
        deliver: Deliver,
        http: Arc<dyn Http>,
        on_auth_sync: Box<dyn Fn(AuthSync) + Send>,
    ) -> Self {
        Self {
            rx,
            epoch,
            deliver,
            http,
            on_auth_sync,
            tokens: None,
            profile: None,
            room: None,
            sending: false,
            queue: VecDeque::new(),
            seen: HashSet::new(),
            next_send: Instant::now(),
        }
    }

    fn current(&self, e: u64) -> bool {
        self.epoch.load(Ordering::Acquire) == e
    }

    fn emit(&self, epoch: u64, event: Event) {
        (self.deliver)(SessionEvent { epoch, event });
    }

    fn paused(&self) -> bool {
        self.queue.front().is_some_and(|p| p.delivery.blocked)
    }

    fn ready_to_send(&self) -> bool {
        !self.paused() && !self.queue.is_empty() && self.tokens.is_some() && self.sending
    }

    fn run(mut self) {
        loop {
            self.cancel_stale();
            let cmd = if self.ready_to_send() {
                let wait = self.next_send.saturating_duration_since(Instant::now());
                if wait.is_zero() {
                    match self.rx.try_recv() {
                        Ok(c) => Some(c),
                        Err(TryRecvError::Empty) => {
                            self.send_next();
                            continue;
                        }
                        Err(TryRecvError::Disconnected) => break,
                    }
                } else {
                    match self.rx.recv_timeout(wait) {
                        Ok(c) => Some(c),
                        Err(RecvTimeoutError::Timeout) => {
                            self.send_next();
                            continue;
                        }
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
            } else {
                match self.rx.recv() {
                    Ok(c) => Some(c),
                    Err(_) => break,
                }
            };
            if let Some(cmd) = cmd {
                self.handle(cmd);
            }
        }
    }

    pub fn handle(&mut self, cmd: SenderCmd) {
        match cmd {
            SenderCmd::Configure => {
                self.room = None;
                self.sending = false;
                self.cancel_stale();
            }
            SenderCmd::SetEpoch => {
                self.sending = false;
                self.cancel_stale();
            }
            SenderCmd::SetSending(value) => {
                self.sending = value;
                if !value {
                    self.cancel_stale();
                }
            }
            SenderCmd::SetRoom(room) => {
                self.room = room;
            }
            SenderCmd::SetAuth(auth) => match auth {
                Some((tokens, profile)) => {
                    self.tokens = Some(tokens);
                    self.profile = Some(profile);
                }
                None => {
                    self.tokens = None;
                    self.profile = None;
                    self.sending = false;
                    self.cancel_stale();
                }
            },
            SenderCmd::Send {
                permit,
                epoch,
                line,
                text,
            } => {
                if !self.current(epoch) || !self.sending || !self.seen.insert(line) {
                    return;
                }
                let (Some(room), Some(profile)) = (&self.room, &self.profile) else {
                    return;
                };
                // The permit already caps the queue at 100 lines.
                let (chunks, state, detail) = match outgoing::split(&text, MAX_CHARS) {
                    Ok(chunks) => (chunks, DeliveryState::Queued, String::new()),
                    Err(error) => (vec![text], DeliveryState::Failed, error.to_string()),
                };
                let delivery = Delivery {
                    line,
                    room: room.id.clone(),
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
                        epoch,
                    });
                }
            }
            SenderCmd::Retry(line) => {
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
            SenderCmd::SkipFailed(line) => {
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

    fn cancel_front(&mut self) {
        if let Some(mut p) = self.queue.pop_front() {
            p.delivery.state = DeliveryState::Cancelled;
            p.delivery.blocked = false;
            self.emit(p.epoch, Event::Delivery(p.delivery));
        }
    }

    pub fn cancel_stale(&mut self) {
        while self
            .queue
            .front()
            .is_some_and(|p| !self.current(p.epoch) || !self.sending)
        {
            self.cancel_front();
        }
    }

    pub fn send_next(&mut self) {
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
                    (self.on_auth_sync)(AuthSync::Refreshed(fresh.clone()));
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
                self.tokens = None;
                self.profile = None;
                self.sending = false;
                if self.current(e) {
                    (self.on_auth_sync)(AuthSync::Expired(e));
                }
            }
            Err(error @ ApiError::Rejected { .. }) => {
                self.block(p, DeliveryState::Failed, error.to_string());
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AccountState;
    use crate::api::fake::Fake;
    use crossbeam_channel::unbounded;
    use serde_json::json;
    use std::sync::Mutex;

    struct Rig {
        worker: SenderWorker,
        events: Receiver<SessionEvent>,
        syncs: Receiver<AuthSync>,
        epoch: Arc<AtomicU64>,
        fake: Fake,
    }

    fn tokens(access: &str) -> Tokens {
        Tokens {
            access: access.into(),
            refresh: format!("{access}-refresh"),
        }
    }

    fn rig() -> Rig {
        let (_tx, rx) = unbounded();
        let (events_tx, events_rx) = unbounded();
        let (sync_tx, sync_rx) = unbounded();
        let epoch = Arc::new(AtomicU64::new(1));
        let fake = Fake::default();
        let mut worker = SenderWorker::new(
            rx,
            epoch.clone(),
            Arc::new(move |e| {
                let _ = events_tx.send(e);
            }),
            Arc::new(fake.clone()),
            Box::new(move |s| {
                let _ = sync_tx.send(s);
            }),
        );
        worker.sending = true;
        worker.tokens = Some(tokens("a"));
        worker.profile = Some(Profile {
            user_id: "7".into(),
            login: "me".into(),
            name: "Me".into(),
            avatar: String::new(),
        });
        worker.room = Some(Room {
            login: "nana".into(),
            id: "1337".into(),
        });
        Rig {
            worker,
            events: events_rx,
            syncs: sync_rx,
            epoch,
            fake,
        }
    }

    impl Rig {
        fn enqueue(&mut self, id: u64, text: &str) {
            self.worker.handle(SenderCmd::Send {
                permit: Permit::acquire(Arc::new(AtomicUsize::new(0))).unwrap(),
                epoch: 1,
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
        rig.worker.handle(SenderCmd::Retry(LineId(1)));
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
        rig.worker.handle(SenderCmd::SkipFailed(LineId(2)));
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
        assert!(matches!(
            rig.syncs.try_recv(),
            Ok(AuthSync::Refreshed(t)) if t == tokens("b")
        ));
        assert!(rig.fake.asked.lock().unwrap()[2].1.starts_with("Bearer b "));
    }

    #[test]
    fn a_refused_refresh_signs_out_and_stops_sending() {
        let mut rig = rig();
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
        assert!(matches!(rig.syncs.try_recv(), Ok(AuthSync::Expired(1))));
        assert!(!rig.worker.sending);
        assert!(rig.worker.tokens.is_none());
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
        rig.worker.handle(SenderCmd::SetSending(false));
        rig.tick();
        assert_eq!(
            rig.deliveries().pop().unwrap().state,
            DeliveryState::Cancelled
        );
        // A line from an old room epoch is refused outright.
        rig.worker.handle(SenderCmd::SetSending(true));
        rig.epoch.store(2, Ordering::Release);
        rig.worker.handle(SenderCmd::Send {
            permit: Permit::acquire(Arc::new(AtomicUsize::new(0))).unwrap(),
            epoch: 1,
            line: LineId(9),
            text: "late".into(),
        });
        assert!(rig.worker.queue.is_empty());
        assert!(rig.fake.asked.lock().unwrap().is_empty());
    }

    #[test]
    fn auth_worker_wires_expired_sync_to_protected_storage() {
        let saved = Arc::new(Mutex::new(Some(tokens("a").to_bytes())));
        let (_tx, rx) = unbounded();
        let (events_tx, events_rx) = unbounded();
        let epoch = Arc::new(AtomicU64::new(1));
        let mut auth = crate::auth::AuthWorker::new(
            rx,
            epoch,
            Arc::new(move |e| {
                let _ = events_tx.send(e);
            }),
            crate::credentials::Store::Memory(saved.clone()),
            Arc::new(Fake::default()),
            Box::new(|_| {}),
        );
        auth.handle(crate::auth::AuthCmd::Expire(1));
        assert!(saved.lock().unwrap().is_none());
        assert!(matches!(
            events_rx.try_recv().unwrap().event,
            Event::Account(AccountState::Expired)
        ));
    }
}
