use std::sync::Arc;

use nanatype_core::HistoryRow;
use nanatype_model::{HistoryItem, LineId};

use super::super::dialogue;
use super::super::{SceneFrameDemand, Stage};
use super::row::{FADE_FRAME_SLACK_MS, ROW_FADE_MS};

/// The visible suffix, excluding the line still on the dialogue box.
#[derive(Clone, Copy)]
pub(super) struct Selection {
    pub visible: usize,
    pub hidden: Option<LineId>,
}

pub(super) fn selection(ui: Stage) -> Selection {
    let (visible, hide) = ui
        .settings
        .with(|s| (s.history.visible_items as usize, s.history.hide_latest));
    let hide = hide.resolve(ui.skin.with(|s| s.history.hide_latest));
    let hidden = if hide && ui.log_canvas == crate::canvas::Canvas::History {
        ui.display.with(|d| d.line)
    } else {
        None
    };
    Selection { visible, hidden }
}

/// Newest first: stop after the visible suffix instead of scanning the log.
pub(super) fn selected_rows(
    rows: &[HistoryRow],
    selection: Selection,
) -> impl Iterator<Item = &HistoryRow> {
    rows.iter()
        .rev()
        .filter(move |row| row.item.line.is_none() || row.item.line != selection.hidden)
        .take(selection.visible)
}

/// Whether `canvas` offers its search: while it takes clicks, and not while it
/// is sent to OBS over Spout/Syphon, whose picture is that same window's. So
/// the broadcast never shows a filtered log or the field.
pub fn search_allowed_for(canvas: crate::canvas::Canvas, s: &nanatype_settings::Settings) -> bool {
    match canvas {
        crate::canvas::Canvas::Danmaku => {
            s.overlays.danmaku.lock != nanatype_settings::Lock::Locked && !s.output.danmaku.enabled
        }
        _ => {
            s.overlays.history.lock != nanatype_settings::Lock::Locked && !s.output.history.enabled
        }
    }
}

/// Whether the history window offers its search: while it takes clicks, and
/// not while it is sent to OBS over Spout/Syphon, whose picture is this same
/// window's. So the broadcast never shows a filtered log or the field.
pub fn search_allowed(s: &nanatype_settings::Settings) -> bool {
    search_allowed_for(crate::canvas::Canvas::History, s)
}

/// The search that filters the window now, lowercased; `None` shows all.
pub(super) fn query(ui: Stage) -> Option<String> {
    let q = ui
        .history_search
        .with(|q| q.as_deref().map(|q| q.trim().to_lowercase()))?;
    (!q.is_empty() && ui.settings.with(|s| search_allowed_for(ui.log_canvas, s))).then_some(q)
}

/// A row whose text or speaker contains `q` (already lowercased).
pub fn row_matches(row: &HistoryRow, q: &str) -> bool {
    row.item.author.to_lowercase().contains(q) || row.doc().plain_text().to_lowercase().contains(q)
}

/// Rows of one speaker this close together share a header.
const GROUP_MS: u64 = 120_000;

/// Whether `row` continues `prev`'s group: same speaker, soon after.
pub(super) fn continues(prev: &HistoryItem, row: &HistoryItem) -> bool {
    !row.author.is_empty()
        && prev.kind == row.kind
        && prev.author == row.author
        && prev.uid == row.uid
        && row.at >= prev.at
        && row.at - prev.at < GROUP_MS
}

/// The shown rows that continue the row above them.
pub(super) fn follow_ups(shown: &[HistoryRow]) -> Vec<u64> {
    shown
        .windows(2)
        .filter(|w| continues(&w[0].item, &w[1].item))
        .map(|w| w[1].key)
        .collect()
}

/// Visible rows in display order (oldest to newest within the visible window,
/// or every matching row across the kept log during a search).
pub(super) fn visible_rows(ui: Stage) -> Arc<Vec<HistoryRow>> {
    let selection = selection(ui);
    // A search looks through the whole kept log, not just the shown rows.
    if let Some(q) = query(ui) {
        return Arc::new(ui.history_rows.with(|rows| {
            rows.iter()
                .filter(|row| row.item.line.is_none() || row.item.line != selection.hidden)
                .filter(|row| row_matches(row, &q))
                .cloned()
                .collect()
        }));
    }
    Arc::new(ui.history_rows.with(|rows| {
        let mut shown: Vec<HistoryRow> = selected_rows(rows, selection).cloned().collect();
        shown.reverse();
        shown
    }))
}

pub(super) fn visible_keys(ui: Stage) -> Vec<u64> {
    let selection = selection(ui);
    if let Some(q) = query(ui) {
        return ui.history_rows.with(|rows| {
            rows.iter()
                .filter(|row| row.item.line.is_none() || row.item.line != selection.hidden)
                .filter(|row| row_matches(row, &q))
                .map(|row| row.key)
                .collect()
        });
    }
    ui.history_rows.with(|rows| {
        let mut keys: Vec<_> = selected_rows(rows, selection).map(|r| r.key).collect();
        keys.reverse();
        keys
    })
}

#[derive(Default, Debug, PartialEq)]
pub struct FadeSchedule {
    pub animating: bool,
    pub next_at: Option<u64>,
}

/// Only mounted history rows can need the history clock. Read untracked from
/// host callbacks, but share exactly the list's selection and fade interval.
pub fn fade_schedule(ui: Stage, now: u64) -> FadeSchedule {
    nana_ui::runtime::view::untrack(|| {
        let Some(after) = ui.settings.with(|s| s.history.fade_after_s) else {
            return FadeSchedule::default();
        };
        let selection = selection(ui);
        let unix_now = crate::clock::unix_of(now);
        let delay = u64::from(after) * 1000;
        ui.history_rows.with(|rows| {
            let mut schedule = FadeSchedule::default();
            for row in selected_rows(rows, selection) {
                let start = row.item.at.saturating_add(delay);
                schedule.animating |= unix_now >= start.saturating_sub(FADE_FRAME_SLACK_MS)
                    && unix_now <= start.saturating_add(ROW_FADE_MS + FADE_FRAME_SLACK_MS);
                if start > unix_now {
                    let at = now.saturating_add(start - unix_now);
                    schedule.next_at = Some(schedule.next_at.map_or(at, |old| old.min(at)));
                }
            }
            schedule
        })
    })
}

fn reveal_animating(reveal: &nanatype_core::Reveal, now: u64, entrance_ms: u16) -> bool {
    let Some(segment) = reveal.segment.as_ref() else {
        return false;
    };
    let first = reveal.shown.saturating_sub(segment.start) as usize;
    let count = reveal.hidden_from.saturating_sub(segment.start) as usize;
    segment
        .reveal_ms
        .get(first..count.min(segment.reveal_ms.len()))
        .unwrap_or_default()
        .iter()
        .any(|offset| {
            segment
                .origin_ms
                .saturating_add(u64::from(*offset))
                .saturating_add(u64::from(entrance_ms))
                > now
        })
}

pub(super) fn history_compositor_needs_tick(ui: Stage, now: u64) -> bool {
    let (setting, skin_entrance, reduced) = ui.settings.with_untracked(|s| {
        (
            s.playback.entrance,
            ui.skin.with_untracked(|skin| nanatype_model::Entrance {
                kind: skin.dialogue.reveal.entrance.kind,
                ms: skin.dialogue.reveal.entrance.ms,
            }),
            ui.reduced_motion.get_untracked(),
        )
    });
    nana_ui::runtime::view::untrack(|| {
        ui.history_rows.with(|rows| {
            rows.iter().any(|row| {
                let entrance_ms = if reduced {
                    0
                } else {
                    dialogue::resolve_entrance(row.doc(), setting, skin_entrance)
                        .0
                        .ms
                };
                row.reveal
                    .as_ref()
                    .is_some_and(|reveal| reveal_animating(reveal, now, entrance_ms))
            })
        })
    })
}

/// Frame scheduling demand for a history or danmaku stage at `now`.
pub fn frame_demand(ui: Stage, now: u64) -> SceneFrameDemand {
    let fade = fade_schedule(ui, now);
    let compositor = history_compositor_needs_tick(ui, now);
    SceneFrameDemand {
        tick_clock: fade.animating || compositor,
        render_only: false,
        next_at: fade.next_at,
    }
}

/// How visible the log is: the newest shown row's opacity, which `fade_after_s`
/// fades like every row; 0 when no row is shown.
pub(super) fn log_presence(ui: Stage) -> f32 {
    let selection = selection(ui);
    let fade_after = ui.settings.with(|s| s.history.fade_after_s);
    ui.history_rows
        .with(|rows| selected_rows(rows, selection).next().map(|r| r.item.at))
        .map_or(0.0, |at| row_opacity(ui, at, fade_after))
}

/// A row said at `at` (unix ms), faded out `fade_after_s` seconds later.
pub(super) fn row_opacity(ui: Stage, at: u64, fade_after_s: Option<u32>) -> f32 {
    // Found rows show however old they are.
    let Some(after) = fade_after_s.filter(|_| query(ui).is_none()) else {
        return 1.0;
    };
    let age = crate::clock::unix_of(ui.history_now.get()).saturating_sub(at);
    let start = u64::from(after) * 1000;
    1.0 - (age.saturating_sub(start) as f32 / ROW_FADE_MS as f32).clamp(0.0, 1.0)
}
