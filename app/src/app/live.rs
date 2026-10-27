//! History rows of live chat, so a moderator's removal reaches
//! them, and the Twitch lines NanaType sent itself, so they are not kept twice.
use crate::{
    app::App,
    chat::{ChatMessage, Platform},
};
use nanatype_core::Cmd;
use std::collections::{HashMap, HashSet, VecDeque};

const CAP: usize = 16384;

#[derive(Default)]
pub(super) struct LiveChat {
    rows: HashMap<(Platform, String), u64>,
    sent: HashSet<String>,
    order: VecDeque<String>,
}
impl LiveChat {
    /// Twitch confirmed one of our lines; a row its echo already made goes.
    pub fn sent(&mut self, id: &str) -> Option<u64> {
        if id.is_empty() {
            return None;
        }
        if self.sent.insert(id.to_owned()) {
            self.order.push_back(id.to_owned());
        }
        while self.order.len() > CAP {
            if let Some(old) = self.order.pop_front() {
                self.sent.remove(&old);
            }
        }
        self.rows.remove(&(Platform::Twitch, id.to_owned()))
    }
    /// Our own Twitch line coming back through chat.
    pub fn echo(&self, message: &ChatMessage) -> bool {
        message.platform == Platform::Twitch
            && matches!(message.kind, crate::chat::ChatKind::Text)
            && self.sent.contains(&message.id)
    }
}

impl App {
    pub(super) fn live_record_row(&mut self, message: &ChatMessage) {
        if let Some(row) = self.core.history().back() {
            self.live_chat
                .rows
                .insert((message.platform, message.id.clone()), row.key);
        }
        let oldest = self
            .core
            .history()
            .front()
            .map(|r| r.key)
            .unwrap_or(u64::MAX);
        self.live_chat.rows.retain(|_, row| *row >= oldest);
    }
    /// A moderator removed the message: take it off the panel and history.
    pub(super) fn live_deleted(&mut self, platform: Platform, room: &str, id: &str) {
        if let Some(row) = self.live_chat.rows.remove(&(platform, id.to_owned())) {
            self.run(Cmd::RemoveHistoryRows(vec![row]));
        }
        let before = self.chat.len();
        self.chat
            .retain(|m| !(m.platform == platform && m.room == room && m.id == id));
        if self.chat.len() != before {
            self.publish_chat();
        }
    }
    /// Our own Twitch lines, confirmed by Twitch's message ids.
    pub(super) fn live_sent(&mut self, ids: &[String]) {
        let rows: Vec<u64> = ids
            .iter()
            .filter_map(|id| self.live_chat.sent(id))
            .collect();
        if !rows.is_empty() {
            self.run(Cmd::RemoveHistoryRows(rows));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_confirmed_line_hands_back_the_row_its_echo_made() {
        let mut live = LiveChat::default();
        live.rows.insert((Platform::Twitch, "m1".into()), 7);
        live.rows.insert((Platform::Blivechat, "m1".into()), 8);
        assert_eq!(live.sent("m1"), Some(7));
        assert_eq!(live.sent("m1"), None);
        assert_eq!(live.sent(""), None);
        assert_eq!(
            live.rows.len(),
            1,
            "chat ids are kept in platform namespaces"
        );
    }
}
