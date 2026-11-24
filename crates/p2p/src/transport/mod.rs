mod assets;
mod guest;
mod host;
mod wire;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{RwLock, mpsc, watch};

use crate::link::{Carrier, RouteSnap};
use crate::protocol::{Invite, Room, TrackSnapshot, VERSION, room_password};

use assets::{Offers, serve_assets};
use guest::{connect_attempt, exchange};
use host::host_session;
use wire::Abort;

/// Only explicitly referenced local assets are offered to a peer.
pub type Resources = BTreeMap<String, PathBuf>;
type Emit = Arc<dyn Fn(Event) + Send + Sync>;

#[derive(Clone)]
pub struct Config {
    pub cache: PathBuf,
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
        /// Copied to people who were not invited. Omits the session secret
        /// when a password is set.
        invitation: String,
        /// Given to a Steam friend the host invited. Carries the session
        /// secret, so they skip the password.
        admit: String,
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
    #[cfg(test)]
    Disconnect,
}

/// Input channels kept across guest reconnects and shared by both room roles.
struct Controls {
    commands: mpsc::UnboundedReceiver<Command>,
    changes: watch::Receiver<Option<Arc<Publication>>>,
    stopped: watch::Receiver<bool>,
}

/// Fixed session resources used by handshakes and message exchange.
#[derive(Clone, Copy)]
struct SessionContext<'a> {
    carrier: &'a Carrier,
    invite: &'a Invite,
    local: &'a str,
    cache: &'a Path,
    offers: &'a Offers,
    emit: &'a Emit,
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
        password: impl Into<String>,
        carrier: Carrier,
        emit: impl Fn(Event) + Send + Sync + 'static,
    ) -> Result<Self, String> {
        let password = room_password(&password.into())?;
        let invite = invitation.as_deref().map(Invite::decode).transpose()?;
        if invite.as_ref().is_some_and(|invite| invite.locked) && password.is_empty() {
            return Err("Room password required".into());
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
                    if let Err(e) = run(
                        config,
                        carrier,
                        invite,
                        password,
                        Controls {
                            commands: rx,
                            changes,
                            stopped,
                        },
                        emit.clone(),
                    )
                    .await
                    {
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

fn broadcast(room: &Room, anchors: &BTreeMap<String, Instant>, emit: &Emit) -> Arc<Room> {
    let mut snapshot = room.clone();
    for (owner, track) in &mut snapshot.tracks {
        track.age_ms = track.age_ms.saturating_add(
            anchors
                .get(owner)
                .map_or(0, |at| at.elapsed().as_millis() as u64),
        );
    }
    let snapshot = Arc::new(snapshot);
    emit(Event::Room {
        room: snapshot.clone(),
        received_at: Instant::now(),
    });
    snapshot
}

fn snap(route: &watch::Receiver<RouteSnap>) -> (String, u64) {
    let route = route.borrow();
    (route.name.clone(), route.rtt_ms)
}

fn member(room: &Room, id: &str) -> bool {
    id == room.host || room.guests.iter().any(|guest| guest == id)
}

fn known_participant(room: &Room, id: &str) -> bool {
    member(room, id)
        || room.tracks.contains_key(id)
        || room.entries.iter().any(|entry| entry.participant == id)
}

fn random_secret() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the OS random source is available");
    blake3::hash(&bytes).to_hex().to_string()
}

async fn run(
    config: Config,
    carrier: Carrier,
    invite: Option<Invite>,
    password: String,
    mut controls: Controls,
    emit: Emit,
) -> Result<(), String> {
    emit(Event::Status(Status::Starting));
    let local = carrier.local_id.clone();
    let steam: u64 = local
        .parse()
        .map_err(|_| "Steam identity is unavailable".to_string())?;
    let hosting = invite.is_none();
    // The session secret and the room password are different. A selected
    // friend is given the secret; everyone else has to type the password.
    let (invite, room_password) = if hosting {
        let locked = !password.is_empty();
        (
            Invite {
                v: VERSION,
                session: random_secret(),
                secret: random_secret(),
                steam,
                locked,
            },
            password,
        )
    } else {
        let mut invite = invite.unwrap();
        if invite.locked {
            invite.secret = password;
        }
        (invite, String::new())
    };
    if hosting && local != invite.steam.to_string() {
        return Err("Steam identity is unavailable".into());
    }
    nanatype_diag::event!(crate::diag::ENDPOINT_BOUND, hosting = hosting);
    let mut room = Room::new(
        invite.session.clone(),
        if hosting {
            local.clone()
        } else {
            invite.steam.to_string()
        },
    );
    let mut anchors = BTreeMap::new();
    let offers: Offers = Arc::new(RwLock::new(BTreeMap::new()));
    let mut connected_once = false;
    emit(Event::Started {
        host: hosting,
        local_id: local.clone(),
        invitation: if hosting {
            invite.shared().encode()
        } else {
            String::new()
        },
        admit: if hosting {
            invite.admitted().encode()
        } else {
            String::new()
        },
    });

    let seated = Arc::new(RwLock::new(BTreeSet::new()));
    let session = SessionContext {
        carrier: &carrier,
        invite: &invite,
        local: &local,
        cache: &config.cache,
        offers: &offers,
        emit: &emit,
    };
    if hosting {
        return host_session(
            session,
            &room_password,
            &mut room,
            &mut anchors,
            &mut controls,
        )
        .await;
    }

    let serve = tokio::spawn(serve_assets(
        carrier.clone(),
        offers.clone(),
        seated.clone(),
        controls.stopped.clone(),
    ));
    let _serve = Abort(serve);

    loop {
        if *controls.stopped.borrow() {
            return Ok(());
        }
        emit(Event::Status(if connected_once {
            Status::Reconnecting
        } else {
            Status::Starting
        }));
        let mut sent_previews = BTreeSet::new();
        let deadline = Instant::now() + Duration::from_secs(20);
        let pipe = {
            let attempt = connect_attempt(session, &mut room, &mut anchors, &seated, deadline);
            tokio::pin!(attempt);
            tokio::select! {
                result = &mut attempt => result,
                _ = controls.stopped.changed() => return Ok(()),
            }
        };
        let pipe = match pipe {
            Ok(pipe) => pipe,
            Err(error) => {
                if !connected_once {
                    return Err(error);
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
        };
        connected_once = true;
        let (route, rtt_ms) = snap(&pipe.route);
        emit(Event::Status(Status::Connected { route, rtt_ms }));
        broadcast(&room, &anchors, &emit);
        let ended = exchange(
            session,
            pipe,
            &mut room,
            &mut anchors,
            &seated,
            &mut sent_previews,
            &mut controls,
        )
        .await;
        match ended {
            Ok(true) => {
                emit(Event::Status(Status::Ended));
                break;
            }
            Ok(false) => return Ok(()),
            Err(_) => {
                emit(Event::Status(Status::Reconnecting));
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::Hub;
    use crate::protocol::tests::track_with;
    use crate::protocol::{Frame, FrameRef, MAX_FRAME, MAX_MEMBERS, PortraitAsset};
    use crate::{picture, protocol};
    use guest::install_room;
    use nanatype_core::DisplayView;
    use nanatype_model::{
        AssetRef, Atom, AtomKind, ChatUser, Document, Effects, Entrance, EntranceKind, Inline,
        Mention,
    };
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc as channel;
    use wire::{pack, unpack};

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
        let hub = Hub::new();
        let (host_tx, host_rx) = channel::channel();
        let host = PeerHandle::start(config("host"), None, "", hub.carrier(1), move |e| {
            let _ = host_tx.send(e);
        })
        .unwrap();
        let Event::Started { invitation, .. } =
            next(&host_rx, |e| matches!(e, Event::Started { .. }))
        else {
            unreachable!()
        };
        let (guest_tx, guest_rx) = channel::channel();
        let guest = PeerHandle::start(
            config("guest"),
            Some(invitation),
            "",
            hub.carrier(2),
            move |e| {
                let _ = guest_tx.send(e);
            },
        )
        .unwrap();
        next(&host_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        next(&guest_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        host.publish(track_with(1, &[1]), BTreeMap::new());
        guest.publish(track_with(1, &[1]), BTreeMap::new());
        let has_two = |e: &Event| matches!(e, Event::Room { room, .. } if room.entries.len() == 2 && room.tracks.len() == 2);
        let Event::Room { room: a, .. } = next(&host_rx, has_two) else {
            unreachable!()
        };
        let Event::Room { room: b, .. } = next(&guest_rx, has_two) else {
            unreachable!()
        };
        assert_eq!(a.entries, b.entries);
        guest.disconnect();
        next(&guest_rx, |e| {
            matches!(e, Event::Status(Status::Reconnecting))
        });
        // A dropped pipe frees the seat. The same person can sit down again.
        next(
            &host_rx,
            |e| matches!(e, Event::Room { room, .. } if room.guests.is_empty()),
        );
        next(&guest_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        // The republished track cannot duplicate ordered history; a new row is
        // ordered once on both ends.
        guest.publish(track_with(2, &[1, 2]), BTreeMap::new());
        let has_three =
            |e: &Event| matches!(e, Event::Room { room, .. } if room.entries.len() == 3);
        let Event::Room { room: a, .. } = next(&host_rx, has_three) else {
            unreachable!()
        };
        let Event::Room { room: b, .. } = next(&guest_rx, has_three) else {
            unreachable!()
        };
        assert_eq!(a.entries, b.entries);
        drop(guest);
        // One guest leaving frees the seat and leaves the room open.
        next(&host_rx, |e| matches!(e, Event::Status(Status::Waiting)));
        drop(host);
    }

    fn join(hub: &Hub, invitation: &str, id: u64) -> (PeerHandle, channel::Receiver<Event>) {
        let (tx, rx) = channel::channel();
        let handle = PeerHandle::start(
            config(&format!("member-{id}")),
            Some(invitation.to_string()),
            "",
            hub.carrier(id),
            move |event| {
                let _ = tx.send(event);
            },
        )
        .unwrap();
        (handle, rx)
    }

    #[test]
    fn three_people_share_history_and_one_can_leave() {
        let hub = Hub::new();
        let (host_tx, host_rx) = channel::channel();
        let host = PeerHandle::start(config("trio-host"), None, "", hub.carrier(1), move |e| {
            let _ = host_tx.send(e);
        })
        .unwrap();
        let Event::Started { invitation, .. } =
            next(&host_rx, |e| matches!(e, Event::Started { .. }))
        else {
            unreachable!()
        };
        let (guest, guest_rx) = join(&hub, &invitation, 2);
        let (third, third_rx) = join(&hub, &invitation, 3);
        next(&guest_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        next(&third_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        host.publish(track_with(1, &[1]), BTreeMap::new());
        guest.publish(track_with(1, &[1]), BTreeMap::new());
        third.publish(track_with(1, &[1]), BTreeMap::new());
        let has_three = |e: &Event| matches!(e, Event::Room { room, .. } if room.entries.len() == 3 && room.guests.len() == 2);
        let Event::Room {
            room: host_room, ..
        } = next(&host_rx, has_three)
        else {
            unreachable!()
        };
        let Event::Room {
            room: guest_room, ..
        } = next(&guest_rx, has_three)
        else {
            unreachable!()
        };
        let Event::Room {
            room: third_room, ..
        } = next(&third_rx, has_three)
        else {
            unreachable!()
        };
        assert_eq!(host_room.entries, guest_room.entries);
        assert_eq!(host_room.entries, third_room.entries);
        drop(third);
        next(
            &host_rx,
            |e| matches!(e, Event::Room { room, .. } if room.guests.len() == 1),
        );
        guest.publish(track_with(2, &[1, 2]), BTreeMap::new());
        let has_four = |e: &Event| matches!(e, Event::Room { room, .. } if room.entries.len() == 4);
        let Event::Room {
            room: host_room, ..
        } = next(&host_rx, has_four)
        else {
            unreachable!()
        };
        let Event::Room {
            room: guest_room, ..
        } = next(&guest_rx, has_four)
        else {
            unreachable!()
        };
        assert_eq!(host_room.entries, guest_room.entries);
        assert!(host_room.guests.iter().any(|id| id == "2"));
        assert!(!host_room.guests.iter().any(|id| id == "3"));
        drop(guest);
        drop(host);
    }

    #[test]
    fn someone_leaving_before_their_first_line_keeps_the_others_seated() {
        let hub = Hub::new();
        let (host_tx, host_rx) = channel::channel();
        let host = PeerHandle::start(config("quiet-host"), None, "", hub.carrier(1), move |e| {
            let _ = host_tx.send(e);
        })
        .unwrap();
        let Event::Started { invitation, .. } =
            next(&host_rx, |e| matches!(e, Event::Started { .. }))
        else {
            unreachable!()
        };
        let (guest, guest_rx) = join(&hub, &invitation, 2);
        let (quiet, quiet_rx) = join(&hub, &invitation, 3);
        next(&guest_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        next(&quiet_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        // The app publishes on start, so a guest has a track before any line.
        guest.publish(track_with(1, &[]), BTreeMap::new());
        quiet.publish(track_with(1, &[]), BTreeMap::new());
        next(
            &guest_rx,
            |e| matches!(e, Event::Room { room, .. } if room.tracks.contains_key("3")),
        );
        drop(quiet);
        next(
            &guest_rx,
            |e| matches!(e, Event::Room { room, .. } if room.guests.len() == 1 && !room.tracks.contains_key("3")),
        );
        // Still seated: a new line reaches both ends.
        guest.publish(track_with(2, &[1]), BTreeMap::new());
        next(
            &host_rx,
            |e| matches!(e, Event::Room { room, .. } if room.entries.len() == 1),
        );
        next(
            &guest_rx,
            |e| matches!(e, Event::Room { room, .. } if room.entries.len() == 1),
        );
        while let Ok(event) = guest_rx.try_recv() {
            assert!(!matches!(event, Event::Status(Status::Reconnecting)));
        }
        drop(guest);
        drop(host);
    }

    #[test]
    fn a_silent_connection_does_not_hold_up_the_next_guest() {
        let hub = Hub::new();
        let (host_tx, host_rx) = channel::channel();
        let host = PeerHandle::start(config("silent-host"), None, "", hub.carrier(1), move |e| {
            let _ = host_tx.send(e);
        })
        .unwrap();
        let Event::Started { invitation, .. } =
            next(&host_rx, |e| matches!(e, Event::Started { .. }))
        else {
            unreachable!()
        };
        // Any owner of the app can open a session pipe and never say hello.
        let silent = hub
            .carrier(66)
            .connect(Some(1), Arc::new(AtomicBool::new(false)))
            .unwrap();
        let started = Instant::now();
        let (guest, guest_rx) = join(&hub, &invitation, 2);
        next(&guest_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "the guest waited behind the silent pipe: {:?}",
            started.elapsed()
        );
        drop(silent);
        drop(guest);
        drop(host);
    }

    #[test]
    fn the_ninth_person_is_refused_until_a_seat_opens() {
        let hub = Hub::new();
        let (host_tx, host_rx) = channel::channel();
        let host = PeerHandle::start(config("full-host"), None, "", hub.carrier(1), move |e| {
            let _ = host_tx.send(e);
        })
        .unwrap();
        let Event::Started { invitation, .. } =
            next(&host_rx, |e| matches!(e, Event::Started { .. }))
        else {
            unreachable!()
        };
        let mut seated = Vec::new();
        for id in 2..=8 {
            let (handle, rx) = join(&hub, &invitation, id);
            next(&rx, |e| {
                matches!(e, Event::Status(Status::Connected { .. }))
            });
            seated.push(handle);
        }
        next(
            &host_rx,
            |e| matches!(e, Event::Room { room, .. } if room.guests.len() == MAX_MEMBERS - 1),
        );
        let (full_tx, full_rx) = channel::channel();
        let refused = PeerHandle::start(
            config("full-refused"),
            Some(invitation.clone()),
            "",
            hub.carrier(9),
            move |event| {
                let _ = full_tx.send(event);
            },
        )
        .unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            let event = full_rx
                .recv_timeout(until.saturating_duration_since(Instant::now()))
                .unwrap();
            if let Event::Status(Status::Error(error)) = &event {
                assert_eq!(error, "Room is full");
                break;
            }
            assert!(!matches!(event, Event::Status(Status::Connected { .. })));
        }
        drop(refused);
        drop(seated.remove(0));
        next(
            &host_rx,
            |e| matches!(e, Event::Room { room, .. } if room.guests.len() == MAX_MEMBERS - 2),
        );
        let (late, late_rx) = join(&hub, &invitation, 10);
        next(&late_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        next(
            &host_rx,
            |e| matches!(e, Event::Room { room, .. } if room.guests.len() == MAX_MEMBERS - 1 && room.guests.iter().any(|id| id == "10")),
        );
        drop(late);
        drop(seated);
        drop(host);
    }

    #[test]
    fn network_assets_cannot_reference_receiver_files() {
        let mut t = track(1);
        t.display.speaker.avatar = Some("C:/secret.png".into());
        t.display.doc = Document::from_inlines(vec![
            Inline::Atom(Atom {
                kind: AtomKind::Image,
                asset: AssetRef::File("C:/secret.png".into()),
                height_em: 1.0,
                alt: String::new(),
                effects: Effects::default(),
            }),
            Inline::Mention(Mention::new(ChatUser {
                uid: "1".into(),
                name: "viewer".into(),
                avatar: Some(AssetRef::File("C:/secret.png".into())),
            })),
        ])
        .into();
        t.portraits.push(PortraitAsset {
            variant: "default".into(),
            asset: AssetRef::File("C:/secret.png".into()),
        });
        t.sanitize_assets();
        assert!(t.display.speaker.avatar.is_none());
        match &t.display.doc.paragraphs[0].inlines[0] {
            Inline::Atom(atom) => assert_eq!(atom.asset.to_string(), "blob:p2p-missing"),
            other => panic!("{other:?}"),
        }
        match &t.display.doc.paragraphs[0].inlines[1] {
            Inline::Mention(mention) => assert!(mention.user.avatar.is_none()),
            other => panic!("{other:?}"),
        }
        assert_eq!(t.portraits[0].asset.to_string(), "blob:p2p-missing");
    }

    #[test]
    fn frame_ref_serializes_identically_to_frame() {
        let mut room = Room::new("session".into(), "host".into());
        let t = track_with(1, &[1]);
        room.update("host", t.clone()).unwrap();
        let previews = BTreeMap::from([("a".repeat(64), "AQID".into())]);
        let empty = BTreeMap::new();

        assert_eq!(
            serde_json::to_vec(&FrameRef::Room {
                room: &room,
                previews: &empty,
            })
            .unwrap(),
            serde_json::to_vec(&Frame::Room {
                room: room.clone(),
                previews: empty.clone(),
            })
            .unwrap()
        );
        assert_eq!(
            serde_json::to_vec(&FrameRef::PeerTrack {
                participant: "host",
                track: &t,
                previews: &previews,
            })
            .unwrap(),
            serde_json::to_vec(&Frame::PeerTrack {
                participant: "host".into(),
                track: t,
                previews,
            })
            .unwrap()
        );
    }

    #[test]
    fn journal_acknowledgements_preserve_newer_local_mirrors() {
        let mut room = Room::new("session".into(), "host".into());
        room.guests.push("guest".into());
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
        let hub = Hub::new();
        let (host_tx, host_rx) = channel::channel();
        let host = PeerHandle::start(
            config("auth-host"),
            None,
            "",
            hub.carrier(1),
            move |event| {
                let _ = host_tx.send(event);
            },
        )
        .unwrap();
        let Event::Started { invitation, .. } =
            next(&host_rx, |e| matches!(e, Event::Started { .. }))
        else {
            unreachable!()
        };
        let mut bad = Invite::decode(&invitation).unwrap();
        bad.secret = "0".repeat(64);
        let (bad_tx, bad_rx) = channel::channel();
        let intruder = PeerHandle::start(
            config("intruder"),
            Some(bad.encode()),
            "",
            hub.carrier(3),
            move |e| {
                let _ = bad_tx.send(e);
            },
        )
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
        let guest = PeerHandle::start(
            config("auth-guest"),
            Some(invitation),
            "",
            hub.carrier(2),
            move |e| {
                let _ = guest_tx.send(e);
            },
        )
        .unwrap();
        next(&guest_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        drop(guest);
        drop(host);
    }

    #[test]
    fn a_room_password_is_required_and_stays_out_of_the_invitation() {
        let hub = Hub::new();
        let (host_tx, host_rx) = channel::channel();
        let host = PeerHandle::start(
            config("password-host"),
            None,
            "nana-room",
            hub.carrier(1),
            move |event| {
                let _ = host_tx.send(event);
            },
        )
        .unwrap();
        let Event::Started {
            invitation, admit, ..
        } = next(&host_rx, |e| matches!(e, Event::Started { .. }))
        else {
            unreachable!()
        };
        let shared = Invite::decode(&invitation).unwrap();
        assert!(shared.locked && shared.secret.is_empty());
        assert!(!invitation.contains("nana-room") && !admit.contains("nana-room"));
        let friend = Invite::decode(&admit).unwrap();
        assert!(!friend.locked && friend.secret.len() == 64);
        assert!(
            PeerHandle::start(
                config("password-missing"),
                Some(invitation.clone()),
                "",
                hub.carrier(3),
                |_| {},
            )
            .is_err()
        );
        let (bad_tx, bad_rx) = channel::channel();
        let intruder = PeerHandle::start(
            config("password-wrong"),
            Some(invitation.clone()),
            "nope",
            hub.carrier(3),
            move |event| {
                let _ = bad_tx.send(event);
            },
        )
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
        let guest = PeerHandle::start(
            config("password-guest"),
            Some(invitation),
            "nana-room",
            hub.carrier(2),
            move |event| {
                let _ = guest_tx.send(event);
            },
        )
        .unwrap();
        next(&guest_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        next(&host_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        drop(guest);
        drop(host);
    }

    #[test]
    fn an_invited_friend_skips_the_room_password() {
        let hub = Hub::new();
        let (host_tx, host_rx) = channel::channel();
        let host = PeerHandle::start(
            config("admit-host"),
            None,
            "nana-room",
            hub.carrier(1),
            move |event| {
                let _ = host_tx.send(event);
            },
        )
        .unwrap();
        let Event::Started { admit, .. } = next(&host_rx, |e| matches!(e, Event::Started { .. }))
        else {
            unreachable!()
        };
        let (guest_tx, guest_rx) = channel::channel();
        let guest = PeerHandle::start(
            config("admit-guest"),
            Some(admit),
            "",
            hub.carrier(2),
            move |event| {
                let _ = guest_tx.send(event);
            },
        )
        .unwrap();
        next(&guest_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        next(&host_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        drop(guest);
        drop(host);
    }

    #[test]
    fn session_frames_roundtrip_through_zlib() {
        let bytes = pack(&Frame::Ping(7)).unwrap();
        assert_eq!(bytes.first().copied(), Some(0x78));
        let frame: Frame = serde_json::from_slice(&unpack(&bytes, MAX_FRAME).unwrap()).unwrap();
        assert!(matches!(frame, Frame::Ping(7)));
    }

    #[test]
    fn image_bytes_travel_on_a_separate_stream_and_are_verified() {
        let host_config = config("image-host");
        let guest_config = config("image-guest");
        std::fs::create_dir_all(&host_config.cache).unwrap();
        let image_path = host_config.cache.join("source.png");
        let source = image::RgbaImage::from_pixel(300, 40, image::Rgba([20, 40, 80, 255]));
        source.save(&image_path).unwrap();
        let guest_cache = guest_config.cache.clone();
        let hub = Hub::new();
        let (host_tx, host_rx) = channel::channel();
        let host = PeerHandle::start(host_config.clone(), None, "", hub.carrier(1), move |e| {
            let _ = host_tx.send(e);
        })
        .unwrap();
        let Event::Started { invitation, .. } =
            next(&host_rx, |e| matches!(e, Event::Started { .. }))
        else {
            unreachable!()
        };
        let (guest_tx, guest_rx) = channel::channel();
        let guest = PeerHandle::start(
            guest_config,
            Some(invitation),
            "",
            hub.carrier(2),
            move |e| {
                let _ = guest_tx.send(e);
            },
        )
        .unwrap();
        next(&host_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        let reference = format!("file:{}", image_path.display());
        let mut snapshot = track(1);
        snapshot.display.reply = Some(nanatype_model::ChatRef::new(
            nanatype_model::ChatUser {
                uid: "x".into(),
                name: "viewer".into(),
                avatar: Some(AssetRef::File(image_path.to_string_lossy().into_owned())),
            },
            "hello",
            nanatype_model::SourceKind::Chat,
        ));
        host.publish(snapshot, BTreeMap::from([(reference, image_path.clone())]));
        let hash = loop {
            let Event::AssetReady(hash) = next(&guest_rx, |e| matches!(e, Event::AssetReady(_)))
            else {
                unreachable!()
            };
            if protocol::session_full_picture(&guest_cache, &hash).is_some() {
                break hash;
            }
        };
        let full = protocol::session_full_picture(&guest_cache, &hash).unwrap();
        let bytes = std::fs::read(&full).unwrap();
        assert_eq!(blake3::hash(&bytes).to_hex().as_str(), hash);
        assert_eq!(picture::wire_kind(&bytes), Some(picture::Kind::Webp));
        let image = image::load_from_memory(&bytes).unwrap();
        assert!(image.width().max(image.height()) <= 256);
        assert!(protocol::session_picture(&guest_cache, &hash).is_some());
        assert!(
            guest_cache
                .join(format!("p2p-{hash}.preview.webp"))
                .is_file()
        );
        drop(guest);
        drop(host);
        // No recursive cleanup: only files created by this test.
        let _ = std::fs::remove_file(&image_path);
        let _ = std::fs::remove_file(host_config.cache.join(format!("p2p-{hash}.webp")));
        let _ = std::fs::remove_dir(&host_config.cache);
        let _ = std::fs::remove_file(&full);
        let _ = std::fs::remove_file(guest_cache.join(format!("p2p-{hash}.preview.webp")));
        let _ = std::fs::remove_dir(&guest_cache);
    }

    #[test]
    fn a_guests_picture_reaches_the_other_guest() {
        share_a_picture(true);
    }

    #[test]
    fn a_guest_asks_the_host_when_the_direct_picture_link_fails() {
        share_a_picture(false);
    }

    fn share_a_picture(direct: bool) {
        let source_config = config("relay-source");
        std::fs::create_dir_all(&source_config.cache).unwrap();
        let image_path = source_config.cache.join("source.png");
        image::RgbaImage::from_pixel(80, 40, image::Rgba([20, 40, 80, 255]))
            .save(&image_path)
            .unwrap();
        let other_config = config("relay-other");
        let other_cache = other_config.cache.clone();
        let hub = Hub::new();
        if !direct {
            hub.fail_asset_dials();
        }
        let (host_tx, host_rx) = channel::channel();
        let host = PeerHandle::start(config("relay-host"), None, "", hub.carrier(1), move |e| {
            let _ = host_tx.send(e);
        })
        .unwrap();
        let Event::Started { invitation, .. } =
            next(&host_rx, |e| matches!(e, Event::Started { .. }))
        else {
            unreachable!()
        };
        let (source_tx, source_rx) = channel::channel();
        let source = PeerHandle::start(
            source_config,
            Some(invitation.clone()),
            "",
            hub.carrier(2),
            move |event| {
                let _ = source_tx.send(event);
            },
        )
        .unwrap();
        let (other_tx, other_rx) = channel::channel();
        let other = PeerHandle::start(
            other_config,
            Some(invitation),
            "",
            hub.carrier(3),
            move |event| {
                let _ = other_tx.send(event);
            },
        )
        .unwrap();
        next(&source_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        next(&other_rx, |e| {
            matches!(e, Event::Status(Status::Connected { .. }))
        });
        next(
            &source_rx,
            |e| matches!(e, Event::Room { room, .. } if room.guests.len() == 2),
        );
        let reference = format!("file:{}", image_path.display());
        let mut snapshot = track(1);
        snapshot.display.reply = Some(nanatype_model::ChatRef::new(
            nanatype_model::ChatUser {
                uid: "x".into(),
                name: "viewer".into(),
                avatar: Some(AssetRef::File(image_path.to_string_lossy().into_owned())),
            },
            "hello",
            nanatype_model::SourceKind::Chat,
        ));
        source.publish(snapshot, BTreeMap::from([(reference, image_path.clone())]));
        let hash = loop {
            let Event::AssetReady(hash) = next(&other_rx, |e| matches!(e, Event::AssetReady(_)))
            else {
                unreachable!()
            };
            if protocol::session_full_picture(&other_cache, &hash).is_some() {
                break hash;
            }
        };
        let full = protocol::session_full_picture(&other_cache, &hash).unwrap();
        let bytes = std::fs::read(&full).unwrap();
        assert_eq!(blake3::hash(&bytes).to_hex().as_str(), hash);
        assert!(
            other_cache
                .join(format!("p2p-{hash}.preview.webp"))
                .is_file()
        );
        if direct {
            assert!(hub.asset_pairs() >= 1);
        } else {
            assert_eq!(hub.asset_pairs(), 0);
        }
        drop(other);
        drop(source);
        drop(host);
        let _ = std::fs::remove_file(&image_path);
        let _ = std::fs::remove_file(&full);
        let _ = std::fs::remove_file(other_cache.join(format!("p2p-{hash}.preview.webp")));
    }
}
