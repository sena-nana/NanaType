//! Twitch OAuth device flow, credential loading, and hourly validation
//! running on its own thread so HTTP never blocks commands or sending.
use crate::{
    AccountState, Event, Profile, SessionEvent,
    api::{self, ApiError, Device, Http, TokenPoll, Tokens},
    credentials::Store,
    reader::Deliver,
};
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const VALIDATE_EVERY: Duration = Duration::from_secs(3600);

pub(crate) enum AuthCmd {
    EnsureLoaded(u64),
    Login(u64),
    CancelLogin(u64),
    Refresh(u64),
    Logout(u64),
    SaveRefreshed(Tokens),
    Expire(u64),
}

pub(crate) type AuthNotify = Box<dyn Fn(Option<(Tokens, Profile)>) + Send>;

pub(crate) struct AuthHandle {
    tx: Sender<AuthCmd>,
}

impl AuthHandle {
    pub fn spawn(
        tx: Sender<AuthCmd>,
        rx: Receiver<AuthCmd>,
        epoch: Arc<AtomicU64>,
        deliver: Deliver,
        store: Store,
        http: Arc<dyn Http>,
        on_auth: AuthNotify,
    ) -> Self {
        let worker = AuthWorker::new(rx, epoch, deliver, store, http, on_auth);
        thread::Builder::new()
            .name("nanatype-twitch-auth".into())
            .spawn(move || worker.run())
            .expect("start Twitch auth worker");
        Self { tx }
    }

    pub fn send(&self, cmd: AuthCmd) {
        let _ = self.tx.send(cmd);
    }
}

struct Login {
    epoch: u64,
    device: Device,
    started: Instant,
    interval: Duration,
}

pub(crate) struct AuthWorker {
    rx: Receiver<AuthCmd>,
    epoch: Arc<AtomicU64>,
    deliver: Deliver,
    store: Store,
    http: Arc<dyn Http>,
    on_auth: AuthNotify,
    tokens: Option<Tokens>,
    profile: Option<Profile>,
    login: Option<Login>,
    next_poll: Instant,
    next_validate: Instant,
    loaded: bool,
}

impl AuthWorker {
    pub fn new(
        rx: Receiver<AuthCmd>,
        epoch: Arc<AtomicU64>,
        deliver: Deliver,
        store: Store,
        http: Arc<dyn Http>,
        on_auth: AuthNotify,
    ) -> Self {
        Self {
            rx,
            epoch,
            deliver,
            store,
            http,
            on_auth,
            tokens: None,
            profile: None,
            login: None,
            next_poll: Instant::now(),
            next_validate: Instant::now() + VALIDATE_EVERY,
            loaded: false,
        }
    }

    fn current(&self, e: u64) -> bool {
        self.epoch.load(Ordering::Acquire) == e
    }

    fn latest(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }

    fn emit(&self, epoch: u64, event: Event) {
        (self.deliver)(SessionEvent { epoch, event });
    }

    fn run(mut self) {
        loop {
            let next_deadline = match (&self.login, &self.profile) {
                (Some(_), Some(_)) => Some(self.next_poll.min(self.next_validate)),
                (Some(_), None) => Some(self.next_poll),
                (None, Some(_)) => Some(self.next_validate),
                (None, None) => None,
            };
            let cmd = match next_deadline {
                Some(deadline) => {
                    let wait = deadline.saturating_duration_since(Instant::now());
                    match self.rx.recv_timeout(wait) {
                        Ok(cmd) => Some(cmd),
                        Err(RecvTimeoutError::Timeout) => None,
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
                None => match self.rx.recv() {
                    Ok(cmd) => Some(cmd),
                    Err(_) => break,
                },
            };
            if let Some(cmd) = cmd {
                self.handle(cmd);
            }
            self.poll_login();
            self.revalidate();
        }
    }

    pub fn handle(&mut self, cmd: AuthCmd) {
        match cmd {
            AuthCmd::EnsureLoaded(e) => {
                self.login = None;
                if !self.loaded {
                    self.loaded = true;
                    match self.store.load() {
                        Ok(bytes) => self.tokens = bytes.and_then(|b| Tokens::from_bytes(&b)),
                        Err(error) => self.emit(e, Event::Warning(error)),
                    }
                    self.restore(e);
                }
            }
            AuthCmd::Login(e) => {
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
                        self.emit(e, Event::Account(AccountState::Failed(error.to_string())));
                    }
                    _ => {}
                }
            }
            AuthCmd::CancelLogin(e) => {
                if !self.current(e) {
                    return;
                }
                self.login = None;
                self.announce(e);
            }
            AuthCmd::Refresh(e) => {
                if !self.current(e) {
                    return;
                }
                self.login = None;
                self.restore(e);
            }
            AuthCmd::Logout(e) => {
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
            AuthCmd::SaveRefreshed(tokens) => {
                self.save(self.latest(), &tokens);
                self.tokens = Some(tokens);
            }
            AuthCmd::Expire(e) => {
                if self.current(e) {
                    self.expire(e);
                }
            }
        }
    }

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
        (self.on_auth)(None);
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
            self.forget();
            self.emit(e, Event::Account(AccountState::SignedOut));
            return;
        };
        self.emit(e, Event::Account(AccountState::Restoring));
        self.adopt(e, &tokens);
    }

    fn adopt(&mut self, e: u64, tokens: &Tokens) {
        match self.authorize(e, tokens) {
            Ok((tokens, profile)) => {
                self.tokens = Some(tokens.clone());
                self.profile = Some(profile.clone());
                self.next_validate = Instant::now() + VALIDATE_EVERY;
                (self.on_auth)(Some((tokens, profile.clone())));
                self.emit(e, Event::Account(AccountState::Ready(profile)));
            }
            Err(ApiError::Unauthorized) => self.expire(e),
            Err(error) => {
                self.profile = None;
                (self.on_auth)(None);
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
            Ok((tokens, profile)) => {
                self.tokens = Some(tokens.clone());
                self.profile = Some(profile.clone());
                (self.on_auth)(Some((tokens, profile)));
            }
            Err(ApiError::Unauthorized) => self.expire(e),
            // Offline for a moment; try again at the next check.
            Err(error) => {
                nanatype_diag::report!(crate::diag::REVALIDATE_DEFERRED; "{error}");
                self.next_validate = Instant::now() + Duration::from_secs(300);
            }
        }
    }

    pub fn poll_login(&mut self) {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::fake::Fake;
    use crossbeam_channel::unbounded;
    use serde_json::json;
    use std::sync::Mutex;

    fn tokens(access: &str) -> Tokens {
        Tokens {
            access: access.into(),
            refresh: format!("{access}-refresh"),
        }
    }

    #[test]
    fn the_device_flow_signs_in_and_stores_tokens_only_in_protected_storage() {
        let (_tx, rx) = unbounded();
        let (events_tx, events_rx) = unbounded();
        let fake = Fake::default();
        let saved = Arc::new(Mutex::new(None));
        let epoch = Arc::new(AtomicU64::new(1));
        let mut worker = AuthWorker::new(
            rx,
            epoch,
            Arc::new(move |e| {
                let _ = events_tx.send(e);
            }),
            Store::Memory(saved.clone()),
            Arc::new(fake.clone()),
            Box::new(|_| {}),
        );
        fake.answer(
            "https://id.twitch.tv/oauth2/device",
            Ok((
                200,
                json!({"device_code": "dev", "user_code": "ABCD-EFGH",
                "verification_uri": "https://www.twitch.tv/activate?device-code=ABCD-EFGH",
                "expires_in": 1800, "interval": 5}),
            )),
        );
        worker.handle(AuthCmd::Login(1));
        let token = "https://id.twitch.tv/oauth2/token";
        fake.answer(
            token,
            Ok((400, json!({"message": "authorization_pending"}))),
        );
        fake.answer(
            token,
            Ok((
                200,
                json!({"access_token": "n", "refresh_token": "n-refresh"}),
            )),
        );
        fake.answer(
            "https://id.twitch.tv/oauth2/validate",
            Ok((200, json!({"user_id": "7", "scopes": ["user:write:chat"]}))),
        );
        fake.answer(
            "https://api.twitch.tv/helix/users",
            Ok((
                200,
                json!({"data": [{"id": "7", "login": "me", "display_name": "Me",
                "profile_image_url": "https://static-cdn.jtvnw.net/me.png"}]}),
            )),
        );
        for _ in 0..2 {
            worker.next_poll = Instant::now();
            worker.poll_login();
        }
        let accounts: Vec<_> = events_rx
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
            Tokens::from_bytes(saved.lock().unwrap().as_ref().unwrap()),
            Some(tokens("n"))
        );
    }
}
