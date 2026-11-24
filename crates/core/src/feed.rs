use std::collections::VecDeque;

use nanatype_model::{HistoryItem, LineId};

use crate::core::Effects;
use crate::view::{HistoryRow, Reveal};

pub(crate) struct MessageFeed {
    pub history: VecDeque<HistoryRow>,
    pub danmaku: VecDeque<HistoryRow>,
    next_row: u64,
    pub separate: bool,
    pub cap: usize,
}

impl MessageFeed {
    pub fn new(cap: usize, separate: bool) -> Self {
        Self {
            history: VecDeque::new(),
            danmaku: VecDeque::new(),
            next_row: 1,
            separate,
            cap: cap.max(1),
        }
    }

    fn take_key(&mut self) -> u64 {
        let key = self.next_row;
        self.next_row += 1;
        key
    }

    pub fn push_history(&mut self, item: HistoryItem, reveal: Option<Reveal>, fx: &mut Effects) {
        let key = self.take_key();
        self.history.push_back(HistoryRow { key, item, reveal });
        cap_rows(&mut self.history, self.cap);
        fx.history_changed = true;
    }

    pub fn push_external(&mut self, item: HistoryItem, fx: &mut Effects) {
        if self.separate {
            let key = self.take_key();
            self.danmaku.push_back(HistoryRow {
                key,
                item,
                reveal: None,
            });
            cap_rows(&mut self.danmaku, self.cap);
            fx.danmaku_changed = true;
        } else {
            self.push_history(item, None, fx);
        }
    }

    pub fn follow_row(&mut self, id: LineId, reveal: Reveal, fx: &mut Effects) {
        if let Some(row) = self
            .history
            .iter_mut()
            .rev()
            .find(|r| r.item.line == Some(id))
            && row.reveal.is_some()
        {
            // Paging clears the display, but history keeps the whole line.
            row.reveal = Some(Reveal {
                visible_from: 0,
                ..reveal
            });
            fx.history_changed = true;
        }
    }

    pub fn complete_row(&mut self, id: LineId, fx: &mut Effects) {
        let Some(row) = self
            .history
            .iter_mut()
            .rev()
            .find(|r| r.item.line == Some(id))
        else {
            return;
        };
        let Some(reveal) = row.reveal.clone() else {
            return;
        };
        let units = row.doc().unit_count() as u32;
        if reveal.is_full(units) {
            return;
        }
        // Let the running animation finish; nothing stays hidden any more.
        row.reveal = reveal.segment.is_some().then_some(Reveal {
            visible_from: 0,
            hidden_from: units,
            ..reveal
        });
        fx.history_changed = true;
    }

    pub fn remove_non_local(&mut self, keys: &[u64], fx: &mut Effects) {
        if retain_non_local(&mut self.history, keys) {
            fx.history_changed = true;
        }
        if retain_non_local(&mut self.danmaku, keys) {
            fx.danmaku_changed = true;
        }
    }

    pub fn clear_history(&mut self, fx: &mut Effects) {
        if !self.history.is_empty() {
            self.history.clear();
            fx.history_changed = true;
        }
    }

    pub fn clear_danmaku(&mut self, fx: &mut Effects) {
        if !self.danmaku.is_empty() {
            self.danmaku.clear();
            fx.danmaku_changed = true;
        }
    }

    pub fn reconfigure(&mut self, cap: usize, separate: bool, fx: &mut Effects) {
        let cap = cap.max(1);
        let was_separate = self.separate;
        self.cap = cap;
        self.separate = separate;
        if !was_separate && separate {
            let mut kept = VecDeque::with_capacity(self.history.len());
            let mut moved = Vec::new();
            for row in self.history.drain(..) {
                if row.item.source.is_local() {
                    kept.push_back(row);
                } else {
                    moved.push(row);
                }
            }
            self.history = kept;
            if !moved.is_empty() {
                self.danmaku.extend(moved);
                self.danmaku.make_contiguous().sort_by_key(|r| r.key);
                fx.history_changed = true;
                fx.danmaku_changed = true;
            }
        } else if was_separate && !separate && !self.danmaku.is_empty() {
            let moved = std::mem::take(&mut self.danmaku);
            fx.danmaku_changed = true;
            self.history.extend(moved);
            self.history.make_contiguous().sort_by_key(|r| r.key);
            fx.history_changed = true;
        }
        if cap_rows(&mut self.history, cap) {
            fx.history_changed = true;
        }
        if cap_rows(&mut self.danmaku, cap) {
            fx.danmaku_changed = true;
        }
    }
}

fn cap_rows(rows: &mut VecDeque<HistoryRow>, cap: usize) -> bool {
    let mut trimmed = false;
    while rows.len() > cap {
        rows.pop_front();
        trimmed = true;
    }
    trimmed
}

fn retain_non_local(rows: &mut VecDeque<HistoryRow>, keys: &[u64]) -> bool {
    let before = rows.len();
    rows.retain(|row| row.item.source.is_local() || !keys.contains(&row.key));
    rows.len() != before
}
