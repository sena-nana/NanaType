use serde::{Deserialize, Serialize};
use std::sync::Arc;

use nanatype_model::{ChatRef, Document, Entrance, HistoryItem, LineId, Speaker};

use crate::Millis;

/// Units of the current segment that animate in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SegmentReveal {
    pub origin_ms: Millis,
    /// First unit of the segment.
    pub start: u32,
    /// Reveal offset of each unit from `origin_ms`.
    pub reveal_ms: Arc<[u32]>,
}

/// Which units of a document a surface draws, and when each appears.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reveal {
    /// Units before this are off the page (cleared by a page marker).
    pub visible_from: u32,
    /// Units `visible_from..shown` are fully visible.
    pub shown: u32,
    pub segment: Option<SegmentReveal>,
    /// Units at or after this stay hidden.
    pub hidden_from: u32,
}

impl Reveal {
    pub fn full(units: u32) -> Self {
        Self {
            visible_from: 0,
            shown: units,
            segment: None,
            hidden_from: units,
        }
    }

    pub fn is_full(&self, units: u32) -> bool {
        self.visible_from == 0 && self.shown >= units && self.segment.is_none()
    }

    /// Reveal time of `unit` on the host clock: `Some(0)` for units already
    /// shown, `None` for hidden or paged-out units.
    pub fn unit_at(&self, unit: u32) -> Option<Millis> {
        if unit < self.visible_from || unit >= self.hidden_from {
            return None;
        }
        if unit < self.shown {
            return Some(0);
        }
        let Some(seg) = self.segment.as_ref() else {
            return Some(0);
        };
        let i = unit.checked_sub(seg.start)? as usize;
        // Units past the animated segment but before `hidden_from` belong to an
        // interrupted line and are simply shown.
        Some(
            seg.reveal_ms
                .get(i)
                .map_or(0, |offset| seg.origin_ms + u64::from(*offset)),
        )
    }

    /// When the last unit still entering finishes an intro of `intro_ms` on the
    /// host clock, or `None` when no unit of the segment is still to enter.
    /// The timeline's last wake is when that unit *starts*; Core holds the
    /// segment end and the host keeps drawing until this instant.
    pub fn settles_at(&self, intro_ms: Millis) -> Option<Millis> {
        let seg = self.segment.as_ref()?;
        let from = self.shown.max(self.visible_from).max(seg.start);
        let first = (from - seg.start) as usize;
        let end = self.hidden_from.saturating_sub(seg.start) as usize;
        seg.reveal_ms
            .get(first..end.min(seg.reveal_ms.len()))?
            .iter()
            .max()
            .map(|offset| {
                seg.origin_ms
                    .saturating_add(u64::from(*offset))
                    .saturating_add(intro_ms)
            })
    }

    /// Bound every index to `units` so a reveal from an untrusted peer can
    /// never describe more units than its document has.
    pub fn clamp(&mut self, units: u32) {
        self.hidden_from = self.hidden_from.min(units);
        self.shown = self.shown.min(units);
        self.visible_from = self.visible_from.min(units);
        if let Some(seg) = &mut self.segment {
            seg.start = seg.start.min(units);
            let len = (units - seg.start) as usize;
            if seg.reveal_ms.len() > len {
                seg.reveal_ms = seg.reveal_ms[..len].into();
            }
        }
    }

    /// The same reveal for the page alone ([`Document::suffix`] from
    /// `visible_from`): unit `u` here is unit `u + visible_from` of the line.
    pub fn rebased(&self) -> Reveal {
        let from = self.visible_from;
        if from == 0 {
            return self.clone();
        }
        Reveal {
            visible_from: 0,
            shown: self.shown.saturating_sub(from),
            segment: self.segment.as_ref().map(|seg| {
                let cut = from.saturating_sub(seg.start) as usize;
                SegmentReveal {
                    origin_ms: seg.origin_ms,
                    start: seg.start.saturating_sub(from),
                    reveal_ms: seg.reveal_ms.get(cut..).unwrap_or_default().into(),
                }
            }),
            hidden_from: self.hidden_from.saturating_sub(from),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ShakeView {
    pub at_ms: Millis,
    pub amp: f32,
    pub ms: u32,
}

/// Retained old text during a finite presentation-only retirement.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TextExit {
    pub at_ms: Millis,
    pub ms: u16,
}

/// Everything the typewriter (and the input preview) needs to paint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayView {
    /// Bumps on every change; renderers compare it to skip work.
    pub revision: u64,
    pub line: Option<LineId>,
    /// The line, markers included (they never paint, but keep unit indices).
    pub doc: Arc<Document>,
    pub speaker: Speaker,
    /// The chat message the line answers, quoted above it.
    pub reply: Option<ChatRef>,
    pub reveal: Reveal,
    /// Per-unit mirror timestamps retained through edits. Older peers may omit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirror_reveal_ms: Option<Arc<[Millis]>>,
    #[serde(default)]
    pub text_exit: Option<TextExit>,
    /// Active portrait variant.
    pub portrait: Option<String>,
    pub continue_indicator: bool,
    /// Fade-out start and duration.
    pub fade: Option<(Millis, u32)>,
    pub shake: Option<ShakeView>,
    pub entrance: Entrance,
    /// Nothing is on screen (the box itself is hidden).
    pub empty: bool,
    /// The line is a replay of one already said: it adds no history row, and
    /// a co-op partner treats it the same. Older peers omit (and ignore) it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub replay: bool,
}

impl DisplayView {
    pub fn empty(entrance: Entrance) -> Self {
        Self {
            revision: 0,
            line: None,
            doc: Arc::new(Document::new()),
            speaker: Speaker::default(),
            reply: None,
            reveal: Reveal::full(0),
            mirror_reveal_ms: None,
            text_exit: None,
            portrait: None,
            continue_indicator: false,
            fade: None,
            shake: None,
            entrance,
            empty: true,
            replay: false,
        }
    }

    /// The current page: the document from `visible_from` on and its reveal,
    /// so text after a page break starts at the top of the box.
    pub fn page(&self) -> (Arc<Document>, Reveal) {
        let from = self.reveal.visible_from;
        if from == 0 {
            return (self.doc.clone(), self.reveal.clone());
        }
        (
            Arc::new(self.doc.suffix(from as usize)),
            self.reveal.rebased(),
        )
    }
}

/// One row of the history list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryRow {
    /// Stable key for list diffing and entrance animation.
    pub key: u64,
    pub item: HistoryItem,
    /// The row's document without markers (unit indices still match the line,
    /// since markers are not units).
    pub doc: Arc<Document>,
    /// `None` is fully shown; `Some` types along with the display.
    pub reveal: Option<Reveal>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settles_when_the_latest_entering_unit_finishes() {
        let reveal = Reveal {
            visible_from: 0,
            shown: 1,
            segment: Some(SegmentReveal {
                origin_ms: 10_000,
                start: 0,
                reveal_ms: Arc::from([0, 100, 40]),
            }),
            hidden_from: 3,
        };
        // Unit 0 is already shown; unit 1 enters last, even out of order.
        assert_eq!(reveal.settles_at(200), Some(10_300));
        let interrupted = Reveal {
            hidden_from: 2,
            ..reveal.clone()
        };
        assert_eq!(interrupted.settles_at(0), Some(10_100));
        let all_shown = Reveal { shown: 3, ..reveal };
        assert_eq!(all_shown.settles_at(200), None);
        assert_eq!(Reveal::full(3).settles_at(200), None);
    }
}
