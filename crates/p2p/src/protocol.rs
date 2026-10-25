use std::collections::{BTreeMap, BTreeSet};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use nanatype_core::{DisplayView, HistoryRow, Reveal};
use nanatype_model::LineId;
use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 5;
/// A room password, in characters. Empty means the invite itself is enough.
pub const PASSWORD_MAX: usize = 64;
pub const MAX_FRAME: usize = 8 * 1024 * 1024;
/// Source files larger than this are not read. The bytes on the wire are a
/// downsampled picture capped at one Steam message, not this source budget.
pub const MAX_ASSET: usize = 16 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 1000;
/// Host plus guests. The Steam lobby uses the same cap.
pub const MAX_MEMBERS: usize = 8;
/// Serialized budget of one participant's track.
/// Every member's track, plus the shared history, has to fit in one room frame.
pub const MAX_TRACK: usize = MAX_FRAME / (MAX_MEMBERS * 2);

fn json_len<T: Serialize>(value: &T) -> usize {
    serde_json::to_vec(value).map_or(usize::MAX, |bytes| bytes.len())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invite {
    pub v: u32,
    pub session: String,
    /// Authorizes the guest when the room has no password. Empty in an invite
    /// that is handed out: a password is checked in the handshake and is never
    /// written into the invite or a Steam lobby.
    #[serde(default)]
    pub secret: String,
    /// Host SteamID. The guest dials this identity. Any owner of the app can
    /// attempt that connection, so the secret or the room password still has
    /// to match.
    pub steam: u64,
    /// The guest must present the room password. `secret` is then empty.
    #[serde(default)]
    pub locked: bool,
}

/// Trim a room password. Empty is a room that does not ask for one.
pub fn room_password(text: &str) -> Result<String, String> {
    let password = text.trim();
    if password.chars().count() > PASSWORD_MAX || password.chars().any(|c| c.is_control()) {
        return Err("Invalid room password".into());
    }
    Ok(password.to_string())
}

impl Invite {
    pub fn encode(&self) -> String {
        format!(
            "nanatype1:{}",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(self).expect("invite"))
        )
    }

    pub fn decode(text: &str) -> Result<Self, String> {
        if text.len() > 16384 {
            return Err("Invitation is too long".into());
        }
        let payload = text
            .trim()
            .strip_prefix("nanatype1:")
            .ok_or("Not a NanaType invitation")?;
        let bytes = URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_| "Invalid invitation encoding")?;
        let invite: Self = serde_json::from_slice(&bytes).map_err(|_| "Invalid invitation")?;
        if invite.v != VERSION {
            return Err("Incompatible session protocol".into());
        }
        if invite.session.len() != 64 || invite.steam == 0 {
            return Err("Invalid session credentials".into());
        }
        let secret_ok = if invite.locked {
            invite.secret.is_empty()
        } else {
            invite.secret.len() == 64
        };
        if !secret_ok {
            return Err("Invalid session credentials".into());
        }
        Ok(invite)
    }

    /// The invite pasted to someone who was not selected. A room password is
    /// never in it, so they still have to type one.
    pub fn shared(&self) -> Self {
        let mut invite = self.clone();
        if invite.locked {
            invite.secret.clear();
        }
        invite
    }

    /// The invite handed to someone the host invited. It carries the session
    /// secret, so the room password is not asked again.
    pub fn admitted(&self) -> Self {
        let mut invite = self.clone();
        invite.locked = false;
        invite
    }
}

/// A row's place in the shared history. The content stays in its owner's
/// track: a line is ordered only once it is in the owner's own history, so
/// the partner sees it exactly when the owner's history window does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub participant: String,
    pub line: LineId,
    pub order: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackSnapshot {
    pub revision: u64,
    pub captured_ms: u64,
    /// Time spent waiting at intermediate peers, excluding network transit.
    pub age_ms: u64,
    pub name: String,
    pub display: DisplayView,
    pub rows: Vec<HistoryRow>,
    #[serde(default)]
    pub portraits: Vec<PortraitAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortraitAsset {
    pub variant: String,
    pub asset: nanatype_model::AssetRef,
}

impl TrackSnapshot {
    pub fn validate(&mut self) -> Result<(), String> {
        if self.name.len() > 1024 || self.rows.len() > MAX_ENTRIES || self.portraits.len() > 2 {
            return Err("Session state exceeds limits".into());
        }
        std::sync::Arc::make_mut(&mut self.display.doc).normalize();
        let units = self.display.doc.unit_count() as u32;
        self.display.reveal.clamp(units);
        if let Some(mirror) = &mut self.display.mirror_reveal_ms
            && mirror.len() > units as usize
        {
            *mirror = mirror[..units as usize].into();
        }
        for row in &mut self.rows {
            row.item.doc.normalize();
            std::sync::Arc::make_mut(&mut row.doc).normalize();
            let units = row.doc.unit_count() as u32;
            if let Some(reveal) = &mut row.reveal {
                reveal.clamp(units);
            }
        }
        // Over budget: drop the oldest rows rather than the whole track.
        let mut size = json_len(self);
        let mut drop = 0;
        while size > MAX_TRACK && drop < self.rows.len() {
            size = size.saturating_sub(json_len(&self.rows[drop]) + 1);
            drop += 1;
        }
        self.rows.drain(..drop);
        if size > MAX_TRACK {
            return Err("Session state exceeds limits".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Room {
    pub session: String,
    pub host: String,
    /// SteamIDs seated besides the host, in join order. At most `MAX_MEMBERS - 1`.
    pub guests: Vec<String>,
    pub entries: Vec<Entry>,
    pub tracks: BTreeMap<String, TrackSnapshot>,
    pub next_order: u64,
    /// Local line IDs are monotonic; pruned rows must not return in a track update.
    #[serde(default)]
    pub retired_before: BTreeMap<String, u64>,
}

impl Room {
    pub fn new(session: String, host: String) -> Self {
        Self {
            session,
            host,
            guests: Vec::new(),
            entries: Vec::new(),
            tracks: BTreeMap::new(),
            next_order: 1,
            retired_before: BTreeMap::new(),
        }
    }

    /// Reserve a guest seat. Someone already seated (a reconnect) is unchanged.
    pub fn seat(&mut self, id: &str) -> Result<(), String> {
        if id.is_empty() || id == self.host {
            return Err("Invalid participant".into());
        }
        if self.guests.iter().any(|guest| guest == id) {
            return Ok(());
        }
        if self.guests.len() + 1 >= MAX_MEMBERS {
            return Err("Room is full".into());
        }
        self.guests.push(id.to_string());
        Ok(())
    }

    /// Free a seat. Their track stays, so lines they already sent remain.
    pub fn unseat(&mut self, id: &str) {
        self.guests.retain(|guest| guest != id);
    }

    pub fn update(&mut self, participant: &str, mut track: TrackSnapshot) -> Result<bool, String> {
        if self
            .tracks
            .get(participant)
            .is_some_and(|old| old.revision >= track.revision)
        {
            return Ok(false);
        }
        track.validate()?;
        let retired = self.retired_before.get(participant).copied().unwrap_or(0);
        track
            .rows
            .retain(|row| row.item.line.is_some_and(|line| line.0 > retired));
        self.tracks.insert(participant.into(), track);
        self.compact();
        Ok(true)
    }

    /// Host only: order the owner's rows the first time they appear in its
    /// history. Returns whether the shared order grew.
    pub fn index(&mut self, participant: &str) -> bool {
        let Some(track) = self.tracks.get(participant) else {
            return false;
        };
        let retired = self.retired_before.get(participant).copied().unwrap_or(0);
        let mut known: BTreeSet<LineId> = self
            .entries
            .iter()
            .filter(|entry| entry.participant == participant)
            .map(|entry| entry.line)
            .collect();
        let before = self.entries.len();
        for line in track.rows.iter().filter_map(|row| row.item.line) {
            if line.0 > retired && known.insert(line) {
                self.entries.push(Entry {
                    participant: participant.into(),
                    line,
                    order: self.next_order,
                });
                self.next_order += 1;
            }
        }
        let grew = self.entries.len() > before;
        if grew {
            self.compact();
        }
        grew
    }

    fn compact(&mut self) {
        // Leave room for the frame wrapper and both live display documents.
        // The size is tracked incrementally: re-serializing the room per
        // dropped entry made a large room quadratic to compact.
        let budget = MAX_FRAME * 3 / 4;
        let mut size = json_len(self);
        let mut drop = 0;
        while drop < self.entries.len()
            && (self.entries.len() - drop > MAX_ENTRIES || size > budget)
        {
            let entry = &self.entries[drop];
            size = size.saturating_sub(json_len(entry) + 1);
            let retired = self
                .retired_before
                .entry(entry.participant.clone())
                .or_default();
            *retired = (*retired).max(entry.line.0);
            if let Some(track) = self.tracks.get_mut(&entry.participant) {
                track.rows.retain(|row| {
                    let keep = row.item.line.is_none_or(|line| line.0 > *retired);
                    if !keep {
                        size = size.saturating_sub(json_len(row) + 1);
                    }
                    keep
                });
            }
            drop += 1;
        }
        self.entries.drain(..drop);
    }

    /// Receiver-local IDs cannot collide with local process IDs or each other.
    pub fn render_id(&self, participant: &str, line: LineId) -> LineId {
        let entry = self
            .entries
            .iter()
            .find(|e| e.participant == participant && e.line == line);
        if let Some(entry) = entry {
            return LineId(entry.order | (1 << 63));
        }
        let digest = blake3::hash(format!("{}:{participant}:{}", self.session, line.0).as_bytes());
        LineId(
            u64::from_le_bytes(digest.as_bytes()[..8].try_into().expect("eight bytes")) | (3 << 62),
        )
    }

    pub fn asset_hashes(&self) -> BTreeSet<String> {
        let value = serde_json::to_value(self).expect("room");
        asset_references(&value)
            .into_iter()
            .filter_map(|reference| reference.strip_prefix("blob:p2p-"))
            .filter(|hash| valid_hash(hash))
            .map(str::to_owned)
            .collect()
    }
}

/// Resource fields in the serialized session schema; ordinary text is excluded.
pub fn asset_references(value: &serde_json::Value) -> Vec<&str> {
    fn visit<'a>(value: &'a serde_json::Value, out: &mut Vec<&'a str>) {
        match value {
            serde_json::Value::Object(object) => {
                for (key, value) in object {
                    if matches!(key.as_str(), "asset" | "avatar")
                        && let Some(reference) = value.as_str()
                    {
                        out.push(reference);
                    } else {
                        visit(value, out);
                    }
                }
            }
            serde_json::Value::Array(array) => {
                for value in array {
                    visit(value, out);
                }
            }
            _ => {}
        }
    }
    let mut references = Vec::new();
    visit(value, &mut references);
    references
}

pub fn valid_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// The full session picture for `hash`, when it has arrived.
///
/// Files are `p2p-{hash}.webp`, `.gif`, or a legacy `.png`. A still preview
/// uses `p2p-{hash}.preview.webp` and is not a full picture.
pub fn session_full_picture(dir: &std::path::Path, hash: &str) -> Option<std::path::PathBuf> {
    if !valid_hash(hash) {
        return None;
    }
    for ext in ["webp", "gif", "png"] {
        let path = dir.join(format!("p2p-{hash}.{ext}"));
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

/// Full picture, or the still preview that arrived with the track.
pub fn session_picture(dir: &std::path::Path, hash: &str) -> Option<std::path::PathBuf> {
    session_full_picture(dir, hash).or_else(|| {
        let preview = dir.join(format!("p2p-{hash}.preview.webp"));
        preview.is_file().then_some(preview)
    })
}

fn rebase_reveal(reveal: &mut Reveal, remote_now: u64, now: u64) {
    if let Some(seg) = &mut reveal.segment {
        // Preserve elapsed reveal even when the receiver only just launched.
        seg.reveal_ms = seg
            .reveal_ms
            .iter()
            .map(|offset| {
                (seg.origin_ms
                    .saturating_add(u64::from(*offset))
                    .saturating_sub(remote_now))
                .min(u64::from(u32::MAX)) as u32
            })
            .collect();
        seg.origin_ms = now;
    }
}

pub fn rebase_display(view: &mut DisplayView, remote_now: u64, now: u64) {
    if let Some(exit) = &mut view.text_exit {
        // A peer may have a younger clock. Freeze the visible prefix, and
        // never interpret the sender's absolute deadline on the local clock.
        let first_hidden = (view.reveal.visible_from..view.reveal.hidden_from)
            .find(|u| view.reveal.unit_at(*u).is_none_or(|at| at > exit.at_ms))
            .unwrap_or(view.reveal.hidden_from);
        view.reveal.hidden_from = first_hidden;
        view.reveal.shown = first_hidden;
        view.reveal.segment = None;
        exit.ms = exit.ms.saturating_sub(
            remote_now
                .saturating_sub(exit.at_ms)
                .min(u64::from(u16::MAX)) as u16,
        );
        exit.at_ms = now;
    }
    rebase_reveal(&mut view.reveal, remote_now, now);
    if let Some(times) = &mut view.mirror_reveal_ms {
        // Zero is the already-shown sentinel; real stamps move onto our clock.
        *times = times
            .iter()
            .map(|at| match at {
                0 => 0,
                at => at.saturating_add(now).saturating_sub(remote_now).max(1),
            })
            .collect();
    }
    if let Some((at, ms)) = &mut view.fade {
        let elapsed = remote_now.saturating_sub(*at);
        if elapsed >= u64::from(*ms) {
            view.empty = true;
            view.fade = None;
        } else {
            *at = now;
            *ms -= elapsed as u32;
        }
    }
    if let Some(shake) = &mut view.shake {
        let elapsed = remote_now.saturating_sub(shake.at_ms);
        if elapsed >= u64::from(shake.ms) {
            view.shake = None;
        } else {
            shake.at_ms = now;
            shake.ms -= elapsed as u32;
        }
    }
}

pub fn rebase_row(row: &mut HistoryRow, remote_now: u64, now: u64) {
    if let Some(reveal) = &mut row.reveal {
        rebase_reveal(reveal, remote_now, now);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) enum Frame {
    Hello {
        v: u32,
        session: String,
        secret: String,
    },
    Welcome {
        room: Room,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        previews: BTreeMap<String, String>,
    },
    Track {
        track: TrackSnapshot,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        previews: BTreeMap<String, String>,
    },
    PeerTrack {
        participant: String,
        track: TrackSnapshot,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        previews: BTreeMap<String, String>,
    },
    Room {
        room: Room,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        previews: BTreeMap<String, String>,
    },
    Ping(u64),
    Pong(u64),
    Leave,
    /// The room already has `MAX_MEMBERS` people.
    Full,
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use nanatype_model::{Document, HistoryItem, SourceId, SourceKind};

    pub(crate) fn track_with(revision: u64, lines: &[u64]) -> TrackSnapshot {
        TrackSnapshot {
            revision,
            captured_ms: 1,
            age_ms: 0,
            name: "partner".into(),
            display: DisplayView::empty(nanatype_model::Entrance {
                kind: nanatype_model::EntranceKind::None,
                ms: 0,
            }),
            rows: lines
                .iter()
                .map(|line| HistoryRow {
                    key: *line,
                    item: HistoryItem {
                        source: SourceId::local(),
                        kind: SourceKind::Typewriter,
                        line: Some(LineId(*line)),
                        author: "partner".into(),
                        avatar: None,
                        doc: Document::plain("hi"),
                        at: 0,
                        uid: None,
                        reply: None,
                    },
                    doc: Document::plain("hi").into(),
                    reveal: None,
                })
                .collect(),
            portraits: Vec::new(),
        }
    }

    fn order(room: &Room) -> Vec<(&str, u64, u64)> {
        room.entries
            .iter()
            .map(|e| (e.participant.as_str(), e.line.0, e.order))
            .collect()
    }

    #[test]
    fn resource_discovery_includes_nested_media_but_excludes_text() {
        let value = serde_json::json!({
            "text": "blob:p2p-not-a-resource",
            "avatar": "blob:face",
            "doc": [{"asset":"lib:heart"}, {"quote":{"avatar":"blob:quoted"}}],
            "portraits":[{"asset":"skin:portrait.png"}]
        });
        let references: BTreeSet<_> = asset_references(&value).into_iter().collect();
        assert_eq!(
            references,
            BTreeSet::from(["blob:face", "blob:quoted", "lib:heart", "skin:portrait.png",])
        );
    }

    #[test]
    fn rows_are_ordered_once_when_they_first_reach_the_owners_history() {
        let mut room = Room::new("s".into(), "a".into());
        // A line still queued or typing is not in the owner's history yet.
        room.update("a", track_with(1, &[])).unwrap();
        assert!(!room.index("a"));
        room.update("a", track_with(2, &[1])).unwrap();
        assert!(room.index("a"));
        room.update("b", track_with(1, &[1])).unwrap();
        assert!(room.index("b"));
        room.update("a", track_with(3, &[1, 2])).unwrap();
        assert!(room.index("a"));
        // Republishing the same history never duplicates it.
        room.update("a", track_with(4, &[1, 2])).unwrap();
        assert!(!room.index("a"));
        assert_eq!(order(&room), [("a", 1, 1), ("b", 1, 2), ("a", 2, 3)]);
    }

    #[test]
    fn pruned_rows_are_not_reordered_by_a_later_track() {
        let mut room = Room::new("s".into(), "a".into());
        let lines: Vec<u64> = (1..=MAX_ENTRIES as u64 + 2).collect();
        room.update("a", track_with(1, &lines[..MAX_ENTRIES]))
            .unwrap();
        room.index("a");
        room.update("a", track_with(2, &lines[2..])).unwrap();
        room.index("a");
        assert_eq!(room.entries.len(), MAX_ENTRIES);
        assert_eq!(room.entries[0].line, LineId(3));
        // A stale history still holding retired line 2 cannot bring it back.
        room.update("a", track_with(3, &lines[1..=MAX_ENTRIES]))
            .unwrap();
        assert!(!room.index("a"));
        assert!(
            room.tracks["a"]
                .rows
                .iter()
                .all(|r| r.item.line > Some(LineId(2)))
        );
    }

    #[test]
    fn untrusted_reveal_indices_are_bounded_by_the_document() {
        let mut display = DisplayView::empty(nanatype_model::Entrance {
            kind: nanatype_model::EntranceKind::Fade,
            ms: 180,
        });
        display.doc = Document::plain("abc").into();
        display.reveal = Reveal {
            visible_from: 0,
            shown: u32::MAX,
            hidden_from: u32::MAX,
            segment: None,
        };
        display.text_exit = Some(nanatype_core::TextExit { at_ms: 0, ms: 100 });
        let mut track = TrackSnapshot {
            revision: 1,
            captured_ms: 0,
            age_ms: 0,
            name: "guest".into(),
            display,
            rows: Vec::new(),
            portraits: Vec::new(),
        };
        track.validate().unwrap();
        assert_eq!(track.display.reveal.hidden_from, 3);
        // Previously iterated u32::MAX units on the UI thread.
        rebase_display(&mut track.display, 0, 0);
        assert_eq!(track.display.reveal.hidden_from, 3);
    }
    #[test]
    fn owners_with_the_same_local_line_id_get_distinct_render_ids() {
        let mut room = Room::new("s".into(), "a".into());
        room.update("a", track_with(1, &[1])).unwrap();
        room.update("b", track_with(1, &[1])).unwrap();
        room.index("a");
        room.index("b");
        assert_ne!(
            room.render_id("a", LineId(1)),
            room.render_id("b", LineId(1))
        );
        // A line not yet in history (still typing) has a stable, distinct id.
        assert_ne!(
            room.render_id("a", LineId(2)),
            room.render_id("b", LineId(2))
        );
    }
    #[test]
    fn mirror_stamps_move_onto_the_receivers_clock() {
        let mut view = DisplayView::empty(nanatype_model::Entrance {
            kind: nanatype_model::EntranceKind::None,
            ms: 0,
        });
        view.mirror_reveal_ms = Some(std::sync::Arc::from([0u64, 1000, 1040]));
        rebase_display(&mut view, 1100, 10);
        assert_eq!(view.mirror_reveal_ms.as_deref(), Some(&[0u64, 1, 1][..]));
        view.mirror_reveal_ms = Some(std::sync::Arc::from([1200u64]));
        rebase_display(&mut view, 1100, 5000);
        assert_eq!(view.mirror_reveal_ms.as_deref(), Some(&[5100u64][..]));
    }
    #[test]
    fn rebasing_a_retirement_keeps_a_finite_local_deadline_and_frozen_prefix() {
        let mut view = DisplayView::empty(nanatype_model::Entrance {
            kind: nanatype_model::EntranceKind::Fade,
            ms: 180,
        });
        view.empty = false;
        view.reveal = Reveal {
            visible_from: 0,
            shown: 0,
            hidden_from: 3,
            segment: Some(nanatype_core::SegmentReveal {
                origin_ms: 1000,
                start: 0,
                reveal_ms: vec![0, 100, 200].into(),
            }),
        };
        view.text_exit = Some(nanatype_core::TextExit {
            at_ms: 1150,
            ms: 320,
        });
        rebase_display(&mut view, 1250, 10);
        assert_eq!(
            view.text_exit,
            Some(nanatype_core::TextExit { at_ms: 10, ms: 220 })
        );
        assert_eq!(view.reveal.hidden_from, 2);
        assert_eq!(view.reveal.unit_at(1), Some(0));
        assert_eq!(view.reveal.unit_at(2), None);
    }
    #[test]
    fn rebase_handles_receiver_clock_younger_than_the_line() {
        let mut reveal = Reveal::full(3);
        reveal.shown = 0;
        reveal.segment = Some(nanatype_core::SegmentReveal {
            origin_ms: 1000,
            start: 0,
            reveal_ms: vec![0, 50, 150].into(),
        });
        rebase_reveal(&mut reveal, 1100, 10);
        assert_eq!(reveal.unit_at(0), Some(10));
        assert_eq!(reveal.unit_at(2), Some(60));
    }
    #[test]
    fn a_password_stays_out_of_the_shared_invite() {
        let invite = Invite {
            v: VERSION,
            session: "ab".repeat(32),
            secret: "nana-room".into(),
            steam: 1,
            locked: true,
        };
        let shared = Invite::decode(&invite.shared().encode()).unwrap();
        assert!(shared.locked && shared.secret.is_empty() && shared.steam == 1);
        let open = Invite {
            locked: false,
            secret: "cd".repeat(32),
            ..invite.clone()
        };
        assert_eq!(Invite::decode(&open.encode()).unwrap().secret.len(), 64);
        let admitted = invite.admitted();
        assert!(!admitted.locked && admitted.secret == "nana-room");
        let leaked = Invite {
            locked: true,
            secret: "cd".repeat(32),
            ..invite
        };
        assert!(Invite::decode(&leaked.encode()).is_err());
        assert_eq!(room_password("  nana  ").unwrap(), "nana");
        assert!(room_password(&"x".repeat(PASSWORD_MAX + 1)).is_err());
        assert!(room_password("a\nb").is_err());
    }

    #[test]
    fn rejects_invalid_and_newer_invitations() {
        assert!(Invite::decode("hello").is_err());
        assert!(Invite::decode(&format!("nanatype1:{}", "a".repeat(17000))).is_err());
        let value = serde_json::json!({"v":2,"session":"s","secret":"x","addr":{"id":"bad"}});
        assert!(
            Invite::decode(&format!(
                "nanatype1:{}",
                URL_SAFE_NO_PAD.encode(value.to_string())
            ))
            .is_err()
        );
    }

    #[test]
    fn track_versions_cannot_move_backwards_or_cross_owners() {
        let mut room = Room::new("session".into(), "a".into());
        let track = TrackSnapshot {
            revision: 4,
            captured_ms: 20,
            age_ms: 0,
            name: "A".into(),
            display: DisplayView::empty(nanatype_model::Entrance {
                kind: nanatype_model::EntranceKind::None,
                ms: 0,
            }),
            rows: Vec::new(),
            portraits: Vec::new(),
        };
        assert!(room.update("a", track.clone()).unwrap());
        let mut stale = track.clone();
        stale.revision = 3;
        assert!(!room.update("a", stale).unwrap());
        assert!(room.update("b", track).unwrap());
        assert_eq!(room.tracks.len(), 2);
        assert_eq!(room.tracks["a"].revision, 4);
    }

    #[test]
    fn a_room_seats_seven_guests_and_no_more() {
        let mut room = Room::new("session".into(), "1".into());
        for id in 2..=8 {
            room.seat(&id.to_string()).unwrap();
        }
        assert_eq!(room.guests.len(), MAX_MEMBERS - 1);
        assert_eq!(room.seat("9").unwrap_err(), "Room is full");
        assert!(room.seat("1").is_err());
        room.unseat("2");
        room.seat("9").unwrap();
        assert!(room.guests.iter().any(|id| id == "9"));
        assert!(!room.guests.iter().any(|id| id == "2"));
        // Reconnecting does not take a second seat.
        room.seat("9").unwrap();
        assert_eq!(
            room.guests.iter().filter(|id| id.as_str() == "9").count(),
            1
        );
    }
}
