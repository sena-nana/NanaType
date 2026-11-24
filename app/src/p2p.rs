//! Application adapter: networking never enters the local editor/player.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use nanatype_core::{Cmd, DisplayView, Event as CoreEvent, HistoryRow};
use nanatype_model::{AssetRef, LineId, SourceId};
use nanatype_p2p::{Config, Event, PeerHandle, Resources, Room, Status, TrackSnapshot};

use crate::ui::EditShared;
use crate::{app::App, clock, msg::Msg};

/// Whether a Steam friend has accepted the room invite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InvitePhase {
    /// The invite dialog has not been opened.
    #[default]
    Idle,
    /// The dialog is open, and nobody has joined the lobby yet.
    Waiting,
    /// A friend joined the lobby. This stays set after they leave it to connect.
    Accepted,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionUi {
    pub active: bool,
    pub host: bool,
    pub invitation: String,
    pub name: String,
    pub status: Status,
    pub invite: InvitePhase,
    /// Persona names of friends who accepted, when Steam has them.
    pub invite_names: Vec<String>,
    /// People in the room, including the local user. Zero before the first snapshot.
    pub members: usize,
    /// The name of whoever sits in each partner seat, in seat order; empty
    /// before they publish one.
    pub seats: Vec<String>,
}
impl Default for SessionUi {
    fn default() -> Self {
        Self {
            active: false,
            host: true,
            invitation: String::new(),
            name: String::new(),
            status: Status::Ended,
            invite: InvitePhase::Idle,
            invite_names: Vec::new(),
            members: 0,
            seats: Vec::new(),
        }
    }
}

/// Incremental sync state cached per participant in the room.
#[derive(Debug, Default, Clone)]
pub struct PeerTrackState {
    pub revision: u64,
    pub rendered_line: Option<LineId>,
    pub last_entry_order: u64,
    pub portraits: HashMap<String, String>,
}

#[derive(Default)]
pub struct Session {
    pub handle: Option<PeerHandle>,
    pub generation: u64,
    pub local: String,
    pub room: Option<Arc<Room>>,
    pub room_received: Option<std::time::Instant>,
    pub peers: HashMap<String, PeerTrackState>,
    /// Lines sent during this session; earlier ones stay private.
    pub lines: HashSet<LineId>,
    pub revision: u64,
    /// Steam invite for a friend the host selected. Empty for a guest.
    pub admit: String,
}

impl App {
    pub(crate) fn peer_start(&mut self, invitation: Option<String>, password: String) {
        if self.peer.handle.is_some() {
            return;
        }
        let password = match nanatype_p2p::room_password(&password) {
            Ok(password) => password,
            Err(_) => {
                self.notice(self.ui.now_t("peer-password-invalid"), true);
                return;
            }
        };
        if let Some(text) = invitation.as_deref()
            && nanatype_p2p::Invite::decode(text).is_ok_and(|invite| invite.locked)
            && password.is_empty()
        {
            self.notice(self.ui.now_t("peer-password-needed"), true);
            return;
        }
        let Some(carrier) = self.peer_carrier() else {
            self.notice(self.ui.now_t("peer-steam-required"), true);
            return;
        };
        self.peer.generation += 1;
        let generation = self.peer.generation;
        let ctx = self.ctx.clone();
        let config = Config {
            cache: self.shell.paths.blobs(),
        };
        match PeerHandle::start(config, invitation, password, carrier, move |event| {
            ctx.dispatch(Msg::PeerEvent(generation, event))
        }) {
            Ok(handle) => {
                self.peer = Session {
                    handle: Some(handle),
                    generation,
                    ..Session::default()
                };
                self.sync_draft_policy();
                self.ui.peer_panel.set(true);
                self.ui.peer.set(Arc::new(SessionUi {
                    active: true,
                    status: Status::Starting,
                    ..SessionUi::default()
                }));
                self.ui.history.set(Arc::new(Vec::new()));
                if self.session.core.display().line.is_some() {
                    self.ui.display.set(Arc::new(empty()));
                }
                self.peer_publish();
                self.refresh_output_status();
            }
            Err(error) => self.notice(error, true),
        }
    }

    pub(crate) fn peer_invite_friends(&mut self) {
        let state = self.ui.peer.get_untracked();
        if !state.host || state.invitation.is_empty() {
            return;
        }
        let admit = self.peer.admit.clone();
        if admit.is_empty() {
            return;
        }
        if !self.peer_open_invite(&admit) {
            self.notice(self.ui.now_t("peer-invite-unavailable"), true);
        }
    }

    pub(crate) fn peer_invite_opened(&mut self) {
        self.note_invite(InvitePhase::Waiting, String::new());
    }

    pub(crate) fn peer_invite_accepted(&mut self, name: String) {
        let name = name.trim();
        let name = if name.is_empty() || name == "[unknown]" {
            String::new()
        } else {
            name.chars().take(64).collect()
        };
        self.note_invite(InvitePhase::Accepted, name);
    }

    fn note_invite(&mut self, phase: InvitePhase, name: String) {
        if self.peer.handle.is_none() {
            return;
        }
        let mut state = (*self.ui.peer.get_untracked()).clone();
        // A second dialog must not hide a friend who already accepted.
        if phase == InvitePhase::Waiting && state.invite == InvitePhase::Accepted {
            return;
        }
        state.invite = phase;
        if phase == InvitePhase::Accepted
            && !name.is_empty()
            && !state.invite_names.iter().any(|have| have == &name)
            && state.invite_names.len() + 1 < nanatype_p2p::MAX_MEMBERS
        {
            state.invite_names.push(name);
        }
        self.ui.peer.set(Arc::new(state));
    }

    /// A Steam friend invite joins directly. The session secret is already in
    /// it, so the room password is not asked.
    pub(crate) fn peer_steam_invite(&mut self, invitation: Result<String, ()>) {
        if self.peer.handle.is_some() {
            return;
        }
        let Ok(invitation) = invitation else {
            self.notice(self.ui.now_t("peer-invite-invalid"), true);
            return;
        };
        if nanatype_p2p::Invite::decode(&invitation).is_ok_and(|invite| invite.locked) {
            self.ui.peer_panel.set(true);
            self.ui.peer_popover.set(true);
            self.ui.peer_invitation.set(invitation);
            self.notice(self.ui.now_t("peer-password-needed"), true);
            return;
        }
        self.peer_start(Some(invitation), String::new());
    }

    pub(crate) fn peer_leave(&mut self) {
        self.peer_close_lobby();
        self.peer = Session {
            generation: self.peer.generation + 1,
            ..Session::default()
        };
        self.sync_draft_policy();
        self.ui.peer.set(Arc::new(SessionUi::default()));
        self.ui.peer_seats.set(0);
        for lane in self.ui.peer_lanes {
            lane.display.set(Arc::new(empty()));
            lane.portraits.set(Arc::new(Default::default()));
        }
        self.ui
            .display
            .set(Arc::new(self.session.core.display().clone()));
        self.ui.peer_local_line.set(None);
        self.ui.history.set(Arc::new(
            self.session.core.history().iter().cloned().collect(),
        ));
        self.refresh_output_status();
    }

    pub(crate) fn peer_event(&mut self, generation: u64, event: Event) {
        if generation != self.peer.generation || self.peer.handle.is_none() {
            return;
        }
        match event {
            Event::Started {
                host,
                local_id,
                invitation,
                admit,
            } => {
                self.peer.local = local_id;
                self.peer.admit = admit;
                self.ui.peer.edit(|state| {
                    state.host = host;
                    state.invitation = invitation;
                });
                self.peer_publish();
            }
            Event::Status(status) => {
                if status == Status::Ended {
                    self.peer_leave();
                    return;
                }
                self.ui.peer.edit(|state| state.status = status);
            }
            Event::Room {
                mut room,
                received_at,
            } => {
                let elapsed = received_at.elapsed().as_millis() as u64;
                for track in Arc::make_mut(&mut room).tracks.values_mut() {
                    track.age_ms = track.age_ms.saturating_add(elapsed);
                }
                self.peer.room = Some(room);
                self.peer.room_received = Some(std::time::Instant::now());
                self.peer_render();
            }
            Event::AssetReady(hash) => {
                // The still preview may already be resolved under this key.
                // Drop and re-resolve it in one CoW pass so the full picture replaces it.
                self.media
                    .invalidate_and_resolve_asset(AssetRef::Blob(format!("p2p-{hash}")), self.ui);
                // Force changed-display re-render so document objects re-read the new URL.
                for state in self.peer.peers.values_mut() {
                    state.revision = 0;
                    state.portraits.clear();
                }
                self.peer_render();
            }
        }
    }

    pub(crate) fn peer_blocked(&mut self, cmd: &Cmd) -> bool {
        let active = self.peer.handle.is_some();
        let connected = matches!(
            self.ui.peer.get_untracked().status,
            Status::Connected { .. }
        );
        if active
            && !connected
            && matches!(
                cmd,
                Cmd::Send
                    | Cmd::SendMarkup(_)
                    | Cmd::Continue
                    | Cmd::Skip
                    | Cmd::ClearDisplay
                    | Cmd::ClearHistory
            )
        {
            self.notice(self.ui.now_t("peer-paused"), false);
            return true;
        }
        false
    }

    /// Mark a line as part of the session. Nothing is sent here: the partner
    /// sees it through the published display and history, when they show it.
    pub(crate) fn peer_submitted(&mut self, event: &CoreEvent) {
        if self.peer.handle.is_some() {
            session_line(&mut self.peer.lines, event, |id| {
                self.session.core.replay_of(id)
            });
        }
    }

    /// The display as this participant's own window draws it, and as the
    /// partner receives it.
    pub(crate) fn shared_display(&self) -> DisplayView {
        session_display(
            self.session.core.display(),
            self.peer.handle.is_some().then_some(&self.peer.lines),
        )
    }

    pub(crate) fn peer_publish(&mut self) {
        if self.peer.handle.is_none() {
            return;
        }
        self.peer.revision += 1;
        let rows = session_rows(self.session.core.history(), &self.peer.lines);
        let display = self.shared_display();
        let portrait = &self.appearance.skin.dialogue.portrait;
        let mut portraits = Vec::new();
        if let Some(image) = &portrait.image {
            portraits.push(nanatype_p2p::PortraitAsset {
                variant: "default".into(),
                asset: AssetRef::Skin(image.clone()),
            });
        }
        if let Some(variant) = &display.portrait
            && let Some(image) = portrait.variants.get(variant)
        {
            portraits.push(nanatype_p2p::PortraitAsset {
                variant: variant.clone(),
                asset: AssetRef::Skin(image.clone()),
            });
        }
        let mut assets = crate::app::media::display_assets(&display);
        for row in &rows {
            assets.extend(crate::app::media::row_assets(&row.item, row.doc()));
        }
        for portrait in &portraits {
            assets.push(portrait.asset.clone());
        }
        let track = TrackSnapshot {
            revision: self.peer.revision,
            captured_ms: clock::now_ms(),
            age_ms: 0,
            name: self.ui.speaker.get_untracked().name,
            display,
            rows,
            portraits,
        };
        let resources = self.peer_resources(assets);
        if let Some(handle) = &self.peer.handle {
            handle.publish(track, resources);
        }
    }

    fn peer_resources(&self, assets: Vec<AssetRef>) -> Resources {
        let mut resources = Resources::new();
        for asset in assets {
            let reference = asset.to_string();
            if resources.contains_key(reference.as_str()) {
                continue;
            }
            if let Some(path) = self.media.resolve(&asset) {
                resources.insert(reference, path);
            }
        }
        resources
    }

    pub(crate) fn peer_render(&mut self) {
        let Some(room) = self.peer.room.clone() else {
            return;
        };
        let now = clock::now_ms();
        let elapsed = self
            .peer
            .room_received
            .map_or(0, |at| at.elapsed().as_millis() as u64);
        let members = room.guests.len().saturating_add(1);
        // Drop participants who left the room.
        self.peer
            .peers
            .retain(|id, _| id == &self.peer.local || room.tracks.contains_key(id));
        // Seat order: the host, then guests. The local person stays on their own display.
        let mut seated = Vec::new();
        if room.host != self.peer.local {
            seated.push(room.host.clone());
        }
        seated.extend(
            room.guests
                .iter()
                .filter(|id| *id != &self.peer.local)
                .cloned(),
        );
        seated.truncate(crate::ui::REMOTE_LANES);

        let mut replays = Vec::new();
        let mut names = Vec::new();
        let mut seat_names = Vec::new();
        let mut new_assets = Vec::new();

        for (index, id) in seated.iter().enumerate() {
            let lane = self.ui.peer_lanes[index];
            let track = room.tracks.get(id);
            let revision = track.map_or(0, |t| t.revision);
            let rendered_line = track
                .and_then(|t| t.display.line)
                .map(|line| room.render_id(id, line));
            let prev = self.peer.peers.get(id);
            let display_changed =
                prev.is_none_or(|p| p.revision != revision || p.rendered_line != rendered_line);

            if let Some(track) = track {
                if !track.name.is_empty() {
                    names.push(track.name.clone());
                }
                seat_names.push(track.name.clone());
                // A replay has no history row to be read from: read it from the
                // display, unless it was already there when the room arrived.
                if track.display.replay
                    && let Some(line) = track.display.line
                    && prev.is_some_and(|p| p.revision != revision)
                {
                    replays.push((id.clone(), line, track.display.doc.clone()));
                }
            } else {
                seat_names.push(String::new());
            }

            if display_changed {
                let mut remote_view = track.map(|t| t.display.clone()).unwrap_or_else(empty);
                if let Some(track) = track {
                    nanatype_p2p::rebase_display(
                        &mut remote_view,
                        track
                            .captured_ms
                            .saturating_add(track.age_ms)
                            .saturating_add(elapsed),
                        now,
                    );
                }
                remote_view.line = rendered_line;
                remote_view.revision = lane.display.get_untracked().revision + 1;
                new_assets.extend(crate::app::media::display_assets(&remote_view));
                lane.display.set(Arc::new(remote_view));
            }

            let portraits: HashMap<String, String> = track
                .map(|track| {
                    track
                        .portraits
                        .iter()
                        .filter_map(|portrait| {
                            self.media
                                .url(&portrait.asset)
                                .map(|url| (portrait.variant.clone(), url))
                        })
                        .collect()
                })
                .unwrap_or_default();
            if prev.is_none_or(|p| p.portraits != portraits)
                && *lane.portraits.get_untracked() != portraits
            {
                lane.portraits.set(Arc::new(portraits.clone()));
            }

            let entry = self.peer.peers.entry(id.clone()).or_default();
            entry.revision = revision;
            entry.rendered_line = rendered_line;
            entry.portraits = portraits;
        }

        for lane in self.ui.peer_lanes.iter().skip(seated.len()) {
            if !lane.display.get_untracked().empty {
                lane.display.set(Arc::new(empty()));
            }
            if !lane.portraits.get_untracked().is_empty() {
                lane.portraits.set(Arc::new(Default::default()));
            }
        }
        let remote_name = names.join(&self.ui.now_t("peer-name-separator"));
        let reseated = self.ui.peer_seats.get_untracked() != seated.len();
        if reseated {
            self.ui.peer_seats.set(seated.len());
        }

        // Index every owner's rows once instead of scanning per entry.
        let mut by_line: HashMap<(&str, LineId), &HistoryRow> = self
            .session
            .core
            .history()
            .iter()
            .filter_map(|row| Some(((self.peer.local.as_str(), row.item.line?), row)))
            .collect();
        for (owner, track) in &room.tracks {
            if owner != &self.peer.local {
                by_line.extend(
                    track
                        .rows
                        .iter()
                        .filter_map(|row| Some(((owner.as_str(), row.item.line?), row))),
                );
            }
        }
        let mut rows = Vec::with_capacity(room.entries.len());
        let mut partner_lines: Vec<(String, LineId, nanatype_model::HistoryItem)> = Vec::new();
        for entry in &room.entries {
            let owner = &entry.participant;
            let track = room.tracks.get(owner);
            // Visibility and reveal follow the owner's history policy.
            let Some(source_row) = by_line.get(&(owner.as_str(), entry.line)).copied() else {
                continue;
            };
            let mut row = source_row.clone();
            row.key = entry.order;
            row.item.line = Some(room.render_id(owner, entry.line));
            row.item.source = SourceId(format!("peer:{owner}"));
            if owner != &self.peer.local
                && let Some(track) = track
            {
                nanatype_p2p::rebase_row(
                    &mut row,
                    track
                        .captured_ms
                        .saturating_add(track.age_ms)
                        .saturating_add(elapsed),
                    now,
                );
            }
            let seen_order = self.peer.peers.get(owner).map_or(0, |p| p.last_entry_order);
            if entry.order > seen_order {
                self.peer
                    .peers
                    .entry(owner.clone())
                    .or_default()
                    .last_entry_order = entry.order;
                new_assets.extend(crate::app::media::row_assets(&row.item, row.doc()));
                if owner != &self.peer.local {
                    partner_lines.push((owner.clone(), entry.line, row.item.clone()));
                }
            }
            rows.push(row);
        }

        self.ui.peer_local_line.set(
            self.session
                .core
                .display()
                .line
                .filter(|id| self.peer.lines.contains(id))
                .map(|id| room.render_id(&self.peer.local, id)),
        );
        // Room events arrive for every peer edit; unchanged rows must not
        // re-render the history window.
        if self.ui.history.with_untracked(|history| **history != rows) {
            self.ui.history.set(Arc::new(rows));
        }
        let shown = self.ui.peer.get_untracked();
        if shown.name != remote_name || shown.members != members || shown.seats != seat_names {
            let mut state = (*shown).clone();
            state.name = remote_name;
            state.members = members;
            state.seats = seat_names;
            self.ui.peer.set(Arc::new(state));
        }
        if reseated {
            // Someone sat down or left: their output status follows.
            self.refresh_output_status();
        }
        self.media.ensure_urls(new_assets, self.ui);
        for (owner, line, doc) in replays {
            self.tts_partner_replay(&owner, line, &doc);
        }
        for (owner, line, item) in partner_lines {
            self.tts_partner(&owner, line, &item);
            self.archive_partner(&owner, line, &item);
        }
    }
}

/// Hide a line from before the session; mirror drafts (no line yet) and the
/// session's own lines show as they are.
/// Add `event`'s line to the session's: every line sent, and a replay of
/// one that is (`replay_of`). A replay shows on the partner's display
/// marked as one, without a history row, as it does here.
fn session_line(
    lines: &mut HashSet<LineId>,
    event: &CoreEvent,
    replay_of: impl Fn(LineId) -> Option<LineId>,
) {
    match event {
        CoreEvent::LineSubmitted(line) => {
            lines.insert(line.id);
        }
        CoreEvent::LineStarted(line)
            if replay_of(line.id).is_some_and(|of| lines.contains(&of)) =>
        {
            lines.insert(line.id);
        }
        _ => {}
    }
}

pub(crate) fn session_display(
    display: &DisplayView,
    lines: Option<&HashSet<LineId>>,
) -> DisplayView {
    match (display.line, lines) {
        (Some(id), Some(lines)) if !lines.contains(&id) => {
            let mut empty = DisplayView::empty(display.entrance);
            empty.revision = display.revision;
            empty
        }
        _ => display.clone(),
    }
}

/// The newest rows of this session's lines, as the own history window has them.
pub(crate) fn session_rows(
    history: &std::collections::VecDeque<HistoryRow>,
    lines: &HashSet<LineId>,
) -> Vec<HistoryRow> {
    let mut rows: Vec<_> = history
        .iter()
        .rev()
        .filter(|row| row.item.line.is_some_and(|id| lines.contains(&id)))
        .take(nanatype_p2p::HISTORY_LIMIT)
        .cloned()
        .collect();
    rows.reverse();
    rows
}

fn empty() -> DisplayView {
    DisplayView::empty(nanatype_model::Entrance {
        kind: nanatype_model::EntranceKind::None,
        ms: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nanatype_core::Core;
    use nanatype_playback::QueuePolicy;
    use nanatype_settings::{InputMode, Settings};

    fn core(mode: InputMode) -> Core {
        let mut settings = Settings::default();
        settings.general.input_mode = mode;
        settings.playback.queue = QueuePolicy::Queue;
        let skin = nanatype_theme::SkinRegistry::new(None, "1.0.0")
            .load("nanatype.halo")
            .unwrap()
            .skin;
        Core::from_settings(&settings, &skin)
    }

    /// Type and send, recording the line the way the session does.
    fn send(core: &mut Core, lines: &mut HashSet<LineId>, text: &str, now: u64) {
        core.handle(Cmd::DraftChanged(nanatype_markup::parse(text).doc), now);
        run(core, lines, Cmd::Send, now);
    }

    fn run(core: &mut Core, lines: &mut HashSet<LineId>, cmd: Cmd, now: u64) {
        for event in core.handle(cmd, now).events {
            session_line(lines, &event, |id| core.replay_of(id));
        }
    }

    #[test]
    fn a_replay_reaches_the_partner_as_one_without_a_row() {
        let mut core = core(InputMode::Compose);
        let mut lines = HashSet::new();
        send(&mut core, &mut lines, "hello", 0);
        run(&mut core, &mut lines, Cmd::Skip, 10);
        run(&mut core, &mut lines, Cmd::Continue, 20_000);
        run(&mut core, &mut lines, Cmd::Replay, 30_000);
        let display = session_display(core.display(), Some(&lines));
        assert!(!display.empty && display.replay);
        assert_eq!(
            published(&core, &lines),
            ("hello".into(), vec!["hello".into()])
        );
        // On the wire it is one more field, which older peers skip; a line
        // that is no replay leaves it out.
        let wire = serde_json::to_value(nanatype_p2p::WireDisplay::from(&display)).unwrap();
        assert_eq!(wire["replay"], true);
        let mut old = wire.clone();
        old.as_object_mut().unwrap().remove("replay");
        let read: DisplayView = serde_json::from_value::<nanatype_p2p::WireDisplay>(old)
            .unwrap()
            .into();
        assert!(!read.replay);
        run(&mut core, &mut lines, Cmd::Skip, 30_010);
        send(&mut core, &mut lines, "next", 40_000);
        run(&mut core, &mut lines, Cmd::Continue, 60_000);
        assert_eq!(core.display().doc.plain_text(), "next");
        let wire = serde_json::to_value(nanatype_p2p::WireDisplay::from(core.display())).unwrap();
        assert!(wire.get("replay").is_none());
    }

    #[test]
    fn a_replay_of_a_line_from_before_the_session_stays_private() {
        let mut core = core(InputMode::Compose);
        send(&mut core, &mut HashSet::new(), "private", 0);
        let mut lines = HashSet::new();
        run(&mut core, &mut lines, Cmd::Skip, 10);
        run(&mut core, &mut lines, Cmd::Replay, 30_000);
        assert!(core.display().replay);
        assert!(session_display(core.display(), Some(&lines)).empty);
    }

    /// What the partner receives: the same view the own windows draw.
    fn published(core: &Core, lines: &HashSet<LineId>) -> (String, Vec<String>) {
        let display = session_display(core.display(), Some(lines));
        let rows = session_rows(core.history(), lines);
        (
            display.doc.plain_text(),
            rows.iter().map(|row| row.doc().plain_text()).collect(),
        )
    }

    #[test]
    fn compose_lines_reach_the_partner_only_when_they_show() {
        let mut core = core(InputMode::Compose);
        let mut lines = HashSet::new();
        send(&mut core, &mut lines, "first", 0);
        send(&mut core, &mut lines, "second", 10);
        assert_eq!(core.queued(), 1);
        // "first" finished; the queued line is not shown yet.
        core.handle(Cmd::Skip, 20);
        assert_eq!(
            published(&core, &lines),
            ("first".into(), vec!["first".into()])
        );
        core.handle(Cmd::Continue, 20_000);
        assert_eq!(
            published(&core, &lines),
            ("second".into(), vec!["first".into(), "second".into()])
        );
    }

    #[test]
    fn mirror_drafts_reach_the_partner_while_typing() {
        let mut core = core(InputMode::Mirror);
        core.handle(Cmd::DraftChanged(nanatype_markup::parse("hel").doc), 0);
        assert_eq!(
            published(&core, &HashSet::new()),
            ("hel".into(), Vec::new())
        );
    }

    #[test]
    fn lines_from_before_the_session_stay_private() {
        let mut core = core(InputMode::Compose);
        send(&mut core, &mut HashSet::new(), "private", 0);
        let lines = HashSet::new();
        let display = session_display(core.display(), Some(&lines));
        assert!(display.empty);
        assert_eq!(display.revision, core.display().revision);
        assert_eq!(published(&core, &lines), (String::new(), Vec::new()));
    }
}
