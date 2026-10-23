//! Application adapter: networking never enters the local editor/player.
use std::collections::HashSet;
use std::sync::Arc;

use nanatype_core::{Cmd, DisplayView, Event as CoreEvent, HistoryRow};
use nanatype_model::{AssetRef, LineId, SourceId};
use nanatype_p2p::{Config, Event, PeerHandle, Resources, Room, Status, TrackSnapshot};

use crate::{app::App, clock, msg::Msg};

#[derive(Debug, Clone, PartialEq)]
pub struct SessionUi {
    pub active: bool,
    pub host: bool,
    pub invitation: String,
    pub name: String,
    pub status: Status,
}
impl Default for SessionUi {
    fn default() -> Self {
        Self {
            active: false,
            host: true,
            invitation: String::new(),
            name: String::new(),
            status: Status::Ended,
        }
    }
}

#[derive(Default)]
pub struct Session {
    pub handle: Option<PeerHandle>,
    pub generation: u64,
    pub local: String,
    pub room: Option<Arc<Room>>,
    pub room_received: Option<std::time::Instant>,
    pub remote_revision: Option<(String, u64, Option<LineId>)>,
    /// Lines sent during this session; earlier ones stay private.
    pub lines: HashSet<LineId>,
    pub revision: u64,
}

impl App {
    pub(crate) fn peer_start(&mut self, invitation: Option<String>, relay: String) {
        if self.peer.handle.is_some() {
            return;
        }
        self.peer.generation += 1;
        let generation = self.peer.generation;
        let ctx = self.ctx.clone();
        let config = Config {
            cache: self.paths.blobs(),
            relay: (!relay.trim().is_empty()).then(|| relay.trim().to_owned()),
            local_only: false,
        };
        match PeerHandle::start(config, invitation, move |event| {
            ctx.dispatch(Msg::PeerEvent(generation, event))
        }) {
            Ok(handle) => {
                self.peer = Session {
                    handle: Some(handle),
                    generation,
                    ..Session::default()
                };
                self.ui.peer_panel.set(true);
                self.ui.peer.set(Arc::new(SessionUi {
                    active: true,
                    status: Status::Starting,
                    ..SessionUi::default()
                }));
                self.ui.history.set(Arc::new(Vec::new()));
                if self.core.display().line.is_some() {
                    self.ui.display.set(Arc::new(empty()));
                }
                self.peer_publish();
                let kind = crate::windows::WindowKind::PeerDisplay;
                if !self.windows.is_open(kind) {
                    let title = self.ui.now_t(kind.title_key());
                    let descriptor =
                        crate::windows::descriptor(kind, &title, &self.skin, &self.settings);
                    self.commands
                        .push(nana_ui_platform::host::WindowCommand::Open {
                            id: kind.id(),
                            settings: descriptor,
                        });
                }
                self.refresh_spout_status();
            }
            Err(error) => self.notice(error, true),
        }
    }

    pub(crate) fn peer_leave(&mut self) {
        self.peer = Session {
            generation: self.peer.generation + 1,
            ..Session::default()
        };
        self.ui.peer.set(Arc::new(SessionUi::default()));
        self.ui.peer_display.set(Arc::new(empty()));
        self.ui.display.set(Arc::new(self.core.display().clone()));
        self.ui.peer_portraits.set(Arc::new(Default::default()));
        self.ui.peer_local_line.set(None);
        self.ui
            .history
            .set(Arc::new(self.core.history().iter().cloned().collect()));
        self.refresh_spout_status();
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
            } => {
                self.peer.local = local_id;
                let mut state = (*self.ui.peer.get_untracked()).clone();
                state.host = host;
                state.invitation = invitation;
                self.ui.peer.set(Arc::new(state));
                self.peer_publish();
            }
            Event::Status(status) => {
                if status == Status::Ended {
                    self.peer_leave();
                    return;
                }
                let mut state = (*self.ui.peer.get_untracked()).clone();
                state.status = status;
                self.ui.peer.set(Arc::new(state));
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
            Event::AssetReady(_) => {
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
        if let CoreEvent::LineSubmitted(line) = event
            && self.peer.handle.is_some()
        {
            self.peer.lines.insert(line.id);
        }
    }

    /// The display as this participant's own window draws it, and as the
    /// partner receives it.
    pub(crate) fn shared_display(&self) -> DisplayView {
        session_display(
            self.core.display(),
            self.peer.handle.is_some().then_some(&self.peer.lines),
        )
    }

    pub(crate) fn peer_publish(&mut self) {
        if self.peer.handle.is_none() {
            return;
        }
        self.peer.revision += 1;
        let rows = session_rows(self.core.history(), &self.peer.lines);
        let display = self.shared_display();
        let portrait = &self.skin.dialogue.portrait;
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
        let track = TrackSnapshot {
            revision: self.peer.revision,
            captured_ms: clock::now_ms(),
            age_ms: 0,
            name: self.ui.speaker.get_untracked().name,
            display,
            rows,
            portraits,
        };
        let resources = self.peer_resources(&serde_json::to_value(&track).expect("track"));
        if let Some(handle) = &self.peer.handle {
            handle.publish(track, resources);
        }
    }

    fn peer_resources(&self, value: &serde_json::Value) -> Resources {
        nanatype_p2p::asset_references(value)
            .into_iter()
            .filter_map(|reference| {
                let asset = reference
                    .parse::<AssetRef>()
                    .unwrap_or_else(|_| AssetRef::File(reference.into()));
                self.assets
                    .resolve(&asset, Some(&self.loaded))
                    .map(|path| (reference.into(), path))
            })
            .collect()
    }

    pub(crate) fn peer_render(&mut self) {
        let Some(room) = &self.peer.room else { return };
        let now = clock::now_ms();
        let elapsed = self
            .peer
            .room_received
            .map_or(0, |at| at.elapsed().as_millis() as u64);
        let mut rows = Vec::new();
        let mut remote_view = empty();
        let mut remote_name = String::new();
        let mut remote_revision = None;
        for (id, track) in &room.tracks {
            if id == &self.peer.local {
                continue;
            }
            remote_name = track.name.clone();
            remote_view = track.display.clone();
            nanatype_p2p::rebase_display(
                &mut remote_view,
                track
                    .captured_ms
                    .saturating_add(track.age_ms)
                    .saturating_add(elapsed),
                now,
            );
            remote_view.line = remote_view.line.map(|line| room.render_id(id, line));
            remote_revision = Some((id.clone(), track.revision, remote_view.line));
            // Re-render resource completion or resync even at the same sender revision.
            remote_view.revision = self.ui.peer_display.get_untracked().revision + 1;
        }
        // Index every owner's rows once instead of scanning per entry.
        let mut by_line: std::collections::HashMap<(&str, LineId), &HistoryRow> = self
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
        for entry in &room.entries {
            let owner = &entry.participant;
            let track = room.tracks.get(owner);
            // Visibility and reveal follow the owner's history policy.
            let source_row = by_line.get(&(owner.as_str(), entry.line)).copied();
            if let Some(row) = source_row {
                let mut row = row.clone();
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
                rows.push(row);
            }
        }
        if self.peer.remote_revision != remote_revision {
            self.ui.peer_display.set(Arc::new(remote_view));
            self.peer.remote_revision = remote_revision;
        }
        self.ui.peer_local_line.set(
            self.core
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
        if self.ui.peer.get_untracked().name != remote_name {
            let mut state = (*self.ui.peer.get_untracked()).clone();
            state.name = remote_name;
            self.ui.peer.set(Arc::new(state));
        }
        self.refresh_peer_urls();
    }

    fn refresh_peer_urls(&mut self) {
        let value = serde_json::json!({
            "display": &*self.ui.peer_display.get_untracked(),
            "history": &*self.ui.history.get_untracked(),
        });
        self.ensure_urls(
            nanatype_p2p::asset_references(&value)
                .into_iter()
                .filter_map(|reference| reference.parse().ok()),
        );
        if let Some(room) = &self.peer.room {
            let portraits: std::collections::HashMap<_, _> = room
                .tracks
                .iter()
                .filter(|(id, _)| *id != &self.peer.local)
                .flat_map(|(_, track)| track.portraits.iter())
                .filter_map(|portrait| {
                    self.assets
                        .url(&portrait.asset, Some(&self.loaded))
                        .map(|url| (portrait.variant.clone(), url))
                })
                .collect();
            if *self.ui.peer_portraits.get_untracked() != portraits {
                self.ui.peer_portraits.set(Arc::new(portraits));
            }
        }
    }
}

/// Hide a line from before the session; mirror drafts (no line yet) and the
/// session's own lines show as they are.
pub(crate) fn session_display(
    display: &DisplayView,
    lines: Option<&HashSet<LineId>>,
) -> DisplayView {
    match (display.line, lines) {
        (Some(id), Some(lines)) if !lines.contains(&id) => DisplayView {
            revision: display.revision,
            ..DisplayView::empty(display.entrance)
        },
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
        Core::new(settings, skin)
    }

    /// Type and send, recording the line the way the session does.
    fn send(core: &mut Core, lines: &mut HashSet<LineId>, text: &str, now: u64) {
        core.handle(Cmd::DraftChanged(nanatype_markup::parse(text).doc), now);
        for event in core.handle(Cmd::Send, now).events {
            if let CoreEvent::LineSubmitted(line) = event {
                lines.insert(line.id);
            }
        }
    }

    /// What the partner receives: the same view the own windows draw.
    fn published(core: &Core, lines: &HashSet<LineId>) -> (String, Vec<String>) {
        let display = session_display(core.display(), Some(lines));
        let rows = session_rows(core.history(), lines);
        (
            display.doc.plain_text(),
            rows.iter().map(|row| row.doc.plain_text()).collect(),
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
