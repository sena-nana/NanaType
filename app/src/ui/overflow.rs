//! Text taller than the dialogue box.
//!
//! The typewriter lays a whole page out at once and reveals it glyph by glyph
//! on the GPU, so nothing in the view knows which line is being typed. The
//! page's text publishes each grapheme's reveal time here when it is built;
//! the host pairs those with the text layout's line boxes
//! ([`crate::host`]) to scroll along with the typing, or to work out where a
//! full box should turn the page.

use std::ops::Range;
use std::sync::{Arc, Mutex};

use nanatype_core::DisplayView;
use nanatype_model::LineId;

use super::rich::unit_graphemes;

/// One page on the typewriter. A new key starts the scroll over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageKey {
    pub line: Option<LineId>,
    pub visible_from: u32,
}

impl PageKey {
    pub fn of(view: &DisplayView) -> Self {
        Self {
            line: view.line,
            visible_from: view.reveal.visible_from,
        }
    }
}

/// A grapheme of the page's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyph {
    /// Byte offset in the rich text.
    pub byte: usize,
    /// Page-relative unit; `None` for the paragraph breaks between units.
    pub unit: Option<u32>,
    /// Reveal time on the session clock: `Some(0)` shown, `None` hidden.
    pub at: Option<u64>,
}

/// The typewriter's current page as built.
#[derive(Debug, Clone, PartialEq)]
pub struct PageText {
    pub key: PageKey,
    /// Length of the rich text, with and without trailing white space; a
    /// layout of the same text ends in between.
    pub text_len: usize,
    pub text_trimmed: usize,
    pub glyphs: Vec<Glyph>,
}

static PUBLISHED: [Mutex<Option<Arc<PageText>>>; 2] = [Mutex::new(None), Mutex::new(None)];

/// Record the page the typewriter just built.
pub fn publish_on(track: usize, page: PageText) {
    if let Ok(mut slot) = PUBLISHED[track].lock() {
        *slot = Some(Arc::new(page));
    }
}

pub fn published_on(track: usize) -> Option<Arc<PageText>> {
    PUBLISHED[track].lock().ok().and_then(|slot| slot.clone())
}

/// The original single-track API addresses the local track.
pub fn publish(page: PageText) {
    publish_on(0, page);
}

pub fn published() -> Option<Arc<PageText>> {
    published_on(0)
}

/// Pair the graphemes of `text` with a reveal plan's times (one per
/// grapheme, paragraph breaks included).
pub fn glyphs(text: &str, at_ms: &[Option<u64>]) -> Vec<Glyph> {
    unit_graphemes(text)
        .zip(at_ms)
        .map(|((byte, unit), at)| Glyph {
            byte,
            unit,
            at: *at,
        })
        .collect()
}

/// A laid out line/column in the engine's line-relative coordinates.
/// `top`/`bottom` are block-axis distances (down in horizontal, left in vertical).
#[derive(Debug, Clone, PartialEq)]
pub struct LineSpan {
    pub bytes: Range<usize>,
    pub top: f32,
    pub bottom: f32,
}

/// The lines of a layout as the typewriter scrolls them. A line reaches over
/// the ruby on it, and starts below any inline object of the lines before:
/// an `@` label can stand a pixel or two out of its line box, and a scroll to
/// the next line's top would leave that edge showing above it.
pub fn line_spans(layout: &nana_text::TextLayout) -> Vec<LineSpan> {
    let mut objects_end = f32::NEG_INFINITY;
    layout
        .lines
        .iter()
        .map(|l| {
            let on_line = |line: u32| line == l.index;
            let ruby_top = layout
                .rubies
                .iter()
                .filter(|r| on_line(r.line))
                .map(|r| r.rect.y)
                .fold(l.bounds.y, f32::min);
            let top = ruby_top.max(objects_end);
            let mut bottom = l.bounds.y + l.bounds.height;
            for o in layout.objects.iter().filter(|o| on_line(o.line)) {
                objects_end = objects_end.max(o.rect.y + o.rect.height);
                bottom = bottom.max(o.rect.y + o.rect.height);
            }
            LineSpan {
                bytes: l.source.clone(),
                top,
                bottom,
            }
        })
        .collect()
}

/// `top..bottom` fits a box `viewport` tall (with rounding slack).
fn fits(top: f32, bottom: f32, viewport: f32) -> bool {
    bottom - top <= viewport + 0.5
}

fn line_of(lines: &[LineSpan], byte: usize) -> usize {
    lines
        .partition_point(|l| l.bytes.end <= byte)
        .min(lines.len().saturating_sub(1))
}

/// The first glyph that is a unit at or after `byte`.
fn unit_from(glyphs: &[Glyph], byte: usize) -> Option<&Glyph> {
    glyphs[glyphs.partition_point(|g| g.byte < byte)..]
        .iter()
        .find(|g| g.unit.is_some())
}

/// The line holding the last glyph revealed by `now`.
pub fn frontier_line(lines: &[LineSpan], glyphs: &[Glyph], now: u64) -> usize {
    glyphs
        .iter()
        .rev()
        .find(|g| g.unit.is_some() && g.at.is_some_and(|t| t <= now))
        .map_or(0, |g| line_of(lines, g.byte))
}

/// The scroll that shows line `frontier` at the bottom of a `viewport` tall
/// box, moved up to the nearest line top so no line is cut in half at the top.
pub fn scroll_target(lines: &[LineSpan], frontier: usize, viewport: f32) -> f32 {
    let Some(last) = lines.get(frontier) else {
        return 0.0;
    };
    if fits(0.0, last.bottom, viewport) {
        return 0.0;
    }
    lines[..=frontier]
        .iter()
        .find(|l| fits(l.top, last.bottom, viewport))
        .map_or(last.top, |l| l.top)
        .max(0.0)
}

/// Units (page-relative) that start a new page when the page is cut into
/// boxes `viewport` tall, a whole line at a time. Empty when it all fits.
pub fn page_breaks(lines: &[LineSpan], glyphs: &[Glyph], viewport: f32) -> Vec<u32> {
    let mut breaks = Vec::new();
    let Some(first) = lines.first() else {
        return breaks;
    };
    let mut top = first.top;
    for line in &lines[1..] {
        if fits(top, line.bottom, viewport) {
            continue;
        }
        top = line.top;
        if let Some(unit) = unit_from(glyphs, line.bytes.start).and_then(|g| g.unit)
            && breaks.last().is_none_or(|b| *b < unit)
            && unit > 0
        {
            breaks.push(unit);
        }
    }
    breaks
}

/// What the host reads back from the last layout of a page.
pub struct Measure<'a> {
    pub vertical: bool,
    pub lines: Vec<LineSpan>,
    pub viewport: f32,
    pub glyphs: &'a [Glyph],
}

const EASE_MS: u64 = 180;

#[derive(Debug, Clone, Copy)]
struct Ease {
    from: f32,
    start: u64,
}

/// The typewriter's scroll: eases a line up as typing reaches the bottom,
/// and starts every page at its own top.
#[derive(Debug, Default)]
pub struct Follow {
    vertical: bool,
    key: Option<PageKey>,
    /// A layout of this page has been read.
    measured: bool,
    /// When typing first reaches the line below the frontier, while the
    /// page is taller than the box.
    next_line_at: Option<u64>,
    target: f32,
    current: f32,
    ease: Option<Ease>,
}

impl Follow {
    /// Advance one frame; the scroll to draw. `measure` is `None` while no
    /// layout of `key`'s page is available yet.
    pub fn frame(
        &mut self,
        key: Option<PageKey>,
        measure: Option<&Measure<'_>>,
        now: u64,
        reduced: bool,
    ) -> f32 {
        if key != self.key {
            *self = Self {
                key,
                ..Self::default()
            };
        }
        if key.is_none() {
            return 0.0;
        }
        if let Some(m) = measure {
            let axis_changed = self.vertical != m.vertical;
            self.vertical = m.vertical;
            let overflowing = m
                .lines
                .last()
                .is_some_and(|l| !fits(m.lines[0].top, l.bottom, m.viewport));
            let (target, next) = if overflowing {
                let frontier = frontier_line(&m.lines, m.glyphs, now);
                let next = m
                    .lines
                    .get(frontier + 1)
                    .and_then(|l| unit_from(m.glyphs, l.bytes.start)?.at)
                    .filter(|t| *t > now);
                (scroll_target(&m.lines, frontier, m.viewport), next)
            } else {
                (0.0, None)
            };
            self.next_line_at = next;
            if !self.measured || reduced || axis_changed {
                self.measured = true;
                self.current = target;
                self.ease = None;
            } else if (target - self.target).abs() > 0.01 {
                self.ease = Some(Ease {
                    from: self.current,
                    start: now,
                });
            }
            self.target = target;
        }
        if let Some(e) = self.ease {
            let t = (now.saturating_sub(e.start) as f32 / EASE_MS as f32).clamp(0.0, 1.0);
            let k = 1.0 - (1.0 - t).powi(3);
            self.current = e.from + (self.target - e.from) * k;
            if t >= 1.0 {
                self.ease = None;
            }
        }
        self.current
    }

    /// Every frame is needed to measure a new page and while easing.
    pub fn wants_frame(&self) -> bool {
        self.key.is_some() && (!self.measured || self.ease.is_some())
    }

    /// Otherwise the next look is due when typing reaches the next line.
    pub fn next_line_at(&self) -> Option<u64> {
        self.next_line_at
    }
}

/// Page overflow: measures each new page once and reports where the box is
/// full, as units of the line.
#[derive(Debug, Default)]
pub struct Pager {
    /// Include line/column wrapping, not just the viewport's block extent.
    sent: Option<(PageKey, u64)>,
    /// A page not measured yet.
    waiting: bool,
}

impl Pager {
    /// Breaks to send for `key`'s page, the first time it is measured.
    pub fn frame(
        &mut self,
        key: Option<PageKey>,
        measure: Option<&Measure<'_>>,
    ) -> Option<Vec<u32>> {
        let Some(key) = key else {
            self.waiting = false;
            return None;
        };
        self.waiting = self.sent.is_none_or(|(k, _)| k != key);
        let m = measure?;
        use std::hash::{Hash, Hasher};
        let mut geometry = std::collections::hash_map::DefaultHasher::new();
        m.viewport.to_bits().hash(&mut geometry);
        m.vertical.hash(&mut geometry);
        for line in &m.lines {
            line.bytes.hash(&mut geometry);
            line.top.to_bits().hash(&mut geometry);
            line.bottom.to_bits().hash(&mut geometry);
        }
        let geometry = geometry.finish();
        if self.sent == Some((key, geometry)) {
            return None;
        }
        self.sent = Some((key, geometry));
        self.waiting = false;
        let breaks: Vec<u32> = page_breaks(&m.lines, m.glyphs, m.viewport)
            .into_iter()
            .map(|b| b + key.visible_from)
            .collect();
        (!breaks.is_empty()).then_some(breaks)
    }

    pub fn wants_frame(&self) -> bool {
        self.waiting
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `n` lines of `per` bytes each, 40 px tall.
    fn lines(n: usize, per: usize) -> Vec<LineSpan> {
        (0..n)
            .map(|i| LineSpan {
                bytes: i * per..(i + 1) * per,
                top: i as f32 * 40.0,
                bottom: (i + 1) as f32 * 40.0,
            })
            .collect()
    }

    /// One glyph per byte, revealed 100 ms apart from t=1000.
    fn typed(len: usize) -> Vec<Glyph> {
        (0..len)
            .map(|i| Glyph {
                byte: i,
                unit: Some(i as u32),
                at: Some(1000 + i as u64 * 100),
            })
            .collect()
    }

    #[test]
    fn glyphs_skip_paragraph_breaks_when_counting_units() {
        let g = glyphs("ab\nc", &[Some(0), Some(5), Some(5), Some(9)]);
        assert_eq!(
            g.iter().map(|g| g.unit).collect::<Vec<_>>(),
            vec![Some(0), Some(1), None, Some(2)]
        );
        assert_eq!(g[3].byte, 3);
    }

    #[test]
    fn the_frontier_is_the_line_of_the_last_revealed_glyph() {
        let l = lines(4, 10);
        let g = typed(40);
        assert_eq!(frontier_line(&l, &g, 0), 0);
        assert_eq!(frontier_line(&l, &g, 1000 + 15 * 100), 1);
        assert_eq!(frontier_line(&l, &g, u64::MAX), 3);
    }

    #[test]
    fn scrolling_keeps_whole_lines_and_stays_put_while_it_fits() {
        let l = lines(5, 10);
        assert_eq!(scroll_target(&l, 2, 120.0), 0.0);
        assert_eq!(scroll_target(&l, 3, 120.0), 40.0);
        assert_eq!(scroll_target(&l, 4, 120.0), 80.0);
        // A box that holds 2.5 lines shows the last two whole.
        assert_eq!(scroll_target(&l, 4, 100.0), 120.0);
    }

    #[test]
    fn pages_break_at_the_first_line_that_does_not_fit() {
        let l = lines(7, 10);
        let g = typed(70);
        assert_eq!(page_breaks(&l, &g, 120.0), vec![30, 60]);
        assert!(page_breaks(&l[..3], &g[..30], 120.0).is_empty());
        // A box shorter than a line still moves on a line at a time.
        assert_eq!(page_breaks(&l[..3], &g[..30], 10.0), vec![10, 20]);
    }

    #[test]
    fn follow_snaps_on_a_new_page_and_eases_within_one() {
        let key = Some(PageKey {
            line: None,
            visible_from: 0,
        });
        let l = lines(5, 10);
        let g = typed(50);
        let m = Measure {
            vertical: false,
            lines: l,
            viewport: 120.0,
            glyphs: &g,
        };
        let m = Some(&m);
        let mut f = Follow::default();
        assert_eq!(f.frame(key, None, 0, false), 0.0);
        assert!(f.wants_frame(), "measure the new page");
        assert_eq!(f.frame(key, m, 1000, false), 0.0);
        assert!(!f.wants_frame());
        // The next look is due when typing reaches the second line.
        assert_eq!(f.next_line_at(), Some(1000 + 10 * 100));
        // Typing reaches line 3: ease up one line.
        let t = 1000 + 31 * 100;
        f.frame(key, m, t, false);
        assert!(f.wants_frame());
        let mid = f.frame(key, m, t + EASE_MS / 2, false);
        assert!(mid > 0.0 && mid < 40.0, "{mid}");
        assert_eq!(f.frame(key, m, t + EASE_MS, false), 40.0);
        // Everything typed: nothing more to look for once settled.
        let end = 1000 + 50 * 100 + EASE_MS * 2;
        f.frame(key, m, end, false);
        f.frame(key, m, end + EASE_MS, false);
        assert!(!f.wants_frame());
        assert_eq!(f.next_line_at(), None);
        // A new page starts at the top.
        let next = Some(PageKey {
            line: None,
            visible_from: 9,
        });
        assert_eq!(f.frame(next, None, end, false), 0.0);
    }

    #[test]
    fn pages_are_measured_once_each() {
        let l = lines(7, 10);
        let g = typed(70);
        let m = Measure {
            vertical: false,
            lines: l,
            viewport: 120.0,
            glyphs: &g,
        };
        let key = Some(PageKey {
            line: None,
            visible_from: 5,
        });
        let mut p = Pager::default();
        assert_eq!(p.frame(key, None), None);
        assert!(p.wants_frame());
        assert_eq!(p.frame(key, Some(&m)), Some(vec![35, 65]));
        assert!(!p.wants_frame());
        assert_eq!(p.frame(key, Some(&m)), None, "sent already");
        assert_eq!(p.frame(None, None), None);
        assert!(!p.wants_frame());
    }

    #[test]
    fn reflow_remeasures_columns_even_when_the_block_viewport_stays_the_same() {
        let g = typed(70);
        let key = Some(PageKey {
            line: None,
            visible_from: 0,
        });
        let mut pager = Pager::default();
        let mut m = Measure {
            vertical: false,
            lines: lines(7, 10),
            viewport: 120.,
            glyphs: &g,
        };
        assert_eq!(pager.frame(key, Some(&m)), Some(vec![30, 60]));
        m.lines = lines(14, 5);
        assert_eq!(pager.frame(key, Some(&m)), Some(vec![15, 30, 45, 60]));
        assert_eq!(pager.frame(key, Some(&m)), None);
    }

    #[test]
    fn changing_writing_axis_snaps_to_the_revealed_frontier_without_restarting_the_page() {
        let key = Some(PageKey {
            line: None,
            visible_from: 0,
        });
        let g = typed(50);
        let mut follow = Follow::default();
        let mut m = Measure {
            vertical: false,
            lines: lines(5, 10),
            viewport: 120.,
            glyphs: &g,
        };
        assert_eq!(follow.frame(key, Some(&m), 10000, false), 80.);
        m.vertical = true;
        m.viewport = 80.;
        assert_eq!(follow.frame(key, Some(&m), 10001, false), 120.);
        assert!(
            !follow.wants_frame(),
            "the old axis must not ease into the new one"
        );
        assert_eq!(follow.key, key);
    }

    #[test]
    fn reduced_motion_jumps() {
        let key = Some(PageKey {
            line: None,
            visible_from: 0,
        });
        let l = lines(5, 10);
        let g = typed(50);
        let mut f = Follow::default();
        let m = Measure {
            vertical: false,
            lines: l,
            viewport: 120.0,
            glyphs: &g,
        };
        f.frame(key, Some(&m), 1000, true);
        assert_eq!(f.frame(key, Some(&m), 1000 + 45 * 100, true), 80.0);
    }
}
