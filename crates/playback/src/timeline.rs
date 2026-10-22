use std::ops::Range;
use std::sync::Arc;

use nanatype_model::{CharClass, Document, Item, Marker, VtsAction};
use serde::{Deserialize, Serialize};

use crate::{PacingParams, TypingSoundRule};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cue", rename_all = "snake_case")]
pub enum CueKind {
    TypeSound {
        unit: u32,
        variant: u8,
        seed: u32,
    },
    Sound {
        sound: String,
        gain: f32,
    },
    Expression {
        name: String,
        intensity: Option<f32>,
    },
    Portrait {
        variant: String,
    },
    Shake {
        amp: f32,
        ms: u32,
    },
    /// The speaker starts or stops "talking" (mouth flap for avatars).
    SpeakingStart,
    SpeakingStop,
    Vts(VtsAction),
}

impl CueKind {
    /// Cues that still matter when the reveal is skipped.
    pub fn survives_skip(&self) -> bool {
        matches!(
            self,
            Self::Sound { .. } | Self::Expression { .. } | Self::Portrait { .. } | Self::Vts(_)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cue {
    /// Offset from the segment origin.
    pub at_ms: u32,
    pub kind: CueKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentEnd {
    /// Wait for continue; continue on its own after `auto_ms`.
    Pause {
        auto_ms: Option<u32>,
    },
    /// Wait for continue, then clear the box; continue on its own after
    /// `auto_ms` (pages turned because the box was full).
    Page {
        auto_ms: Option<u32>,
    },
    LineEnd,
}

impl SegmentEnd {
    /// How long a gate waits before it continues on its own.
    pub fn auto_ms(self) -> Option<u32> {
        match self {
            Self::Pause { auto_ms } | Self::Page { auto_ms } => auto_ms,
            Self::LineEnd => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    /// Units this segment reveals.
    pub units: Range<u32>,
    /// First unit that is visible while this segment plays (page start).
    pub visible_from: u32,
    /// Reveal offset per unit in `units`, from the segment origin.
    pub reveal_ms: Arc<[u32]>,
    pub cues: Arc<[Cue]>,
    /// When the segment's reveal (and trailing waits) are over.
    pub duration_ms: u32,
    pub end: SegmentEnd,
}

impl Segment {
    /// Every unit shown at once.
    pub fn instant(&self) -> Arc<[u32]> {
        vec![0; self.reveal_ms.len()].into()
    }

    /// Cues not reached `elapsed` ms into the segment.
    pub fn cues_after(&self, elapsed: u64) -> impl Iterator<Item = &Cue> {
        self.cues
            .iter()
            .filter(move |c| u64::from(c.at_ms) > elapsed)
    }
}

/// A compiled line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Timeline {
    pub unit_count: u32,
    pub segments: Vec<Segment>,
}

impl Timeline {
    pub fn last_segment(&self) -> usize {
        self.segments.len().saturating_sub(1)
    }

    /// Start a new page at `unit` (the box is full there): the segment that
    /// holds it ends at `unit` with a page break, and the rest of the page it
    /// was on moves to the new one. A pause right before `unit` becomes the
    /// page break. Returns whether anything changed; a page that already
    /// starts at `unit` is left alone.
    pub fn split_page(&mut self, unit: u32, auto_ms: Option<u32>) -> bool {
        let Some(si) = self
            .segments
            .iter()
            .position(|s| s.units.start <= unit && unit < s.units.end)
        else {
            return false;
        };
        let page = self.segments[si].visible_from;
        if page == unit {
            return false;
        }
        if self.segments[si].units.start == unit {
            let prev = &mut self.segments[si - 1];
            prev.end = SegmentEnd::Page {
                auto_ms: prev.end.auto_ms().or(auto_ms),
            };
            self.move_page(si, page, unit);
            return true;
        }
        let s = &self.segments[si];
        let k = (unit - s.units.start) as usize;
        let cut = s.reveal_ms[k];
        let mut first = Vec::new();
        let mut rest = Vec::new();
        for c in s.cues.iter() {
            let in_first = match c.kind {
                CueKind::TypeSound { unit: u, .. } => u < unit,
                _ => c.at_ms < cut,
            };
            if in_first {
                first.push(c.clone());
            } else {
                rest.push(Cue {
                    at_ms: c.at_ms.saturating_sub(cut),
                    kind: c.kind.clone(),
                });
            }
        }
        // Keep each segment's talking bracketed: stop at the cut, start
        // again where the new page picks up.
        let talking = first.iter().fold(false, |on, c| match c.kind {
            CueKind::SpeakingStart => true,
            CueKind::SpeakingStop => false,
            _ => on,
        });
        if talking {
            first.push(Cue {
                at_ms: cut,
                kind: CueKind::SpeakingStop,
            });
            rest.insert(
                0,
                Cue {
                    at_ms: 0,
                    kind: CueKind::SpeakingStart,
                },
            );
        }
        first.sort_by_key(|c| c.at_ms);
        rest.sort_by_key(|c| c.at_ms);
        let a = Segment {
            units: s.units.start..unit,
            visible_from: s.visible_from,
            reveal_ms: s.reveal_ms[..k].into(),
            cues: first.into(),
            duration_ms: cut,
            end: SegmentEnd::Page { auto_ms },
        };
        let b = Segment {
            units: unit..s.units.end,
            visible_from: unit,
            reveal_ms: s.reveal_ms[k..]
                .iter()
                .map(|t| t.saturating_sub(cut))
                .collect(),
            cues: rest.into(),
            duration_ms: s.duration_ms.saturating_sub(cut),
            end: s.end,
        };
        self.segments[si] = a;
        self.segments.insert(si + 1, b);
        self.move_page(si + 2, page, unit);
        true
    }

    /// Segments from `from` on that were still on page `old` start at `new`.
    fn move_page(&mut self, from: usize, old: u32, new: u32) {
        for s in self.segments[from..].iter_mut() {
            if s.visible_from != old {
                break;
            }
            s.visible_from = new;
        }
    }
}

struct Draft {
    units: Vec<u32>,
    cues: Vec<Cue>,
    start: u32,
    visible_from: u32,
    clock: u32,
    speaking: bool,
}

/// A pause at least this long closes the speaker's mouth until the next
/// character.
const SPEAKING_GAP_MS: u32 = 250;

/// How long the mouth stays open into such a pause.
const SPEAKING_TAIL_MS: u32 = 80;

/// Compile `doc` into segments. Pure; call again when pacing settings change.
pub fn compile(doc: &Document, pacing: &PacingParams, sound: &TypingSoundRule) -> Timeline {
    let mut segments = Vec::new();
    let mut ordinal: u32 = 0;
    let mut eligible: u32 = 0;
    let mut last_click: Option<u32> = None;
    let mut variant: u8 = 0;
    let mut d = Draft {
        units: Vec::new(),
        cues: Vec::new(),
        start: 0,
        visible_from: 0,
        clock: 0,
        speaking: false,
    };

    fn close(d: &mut Draft, end: SegmentEnd, ordinal: u32, segments: &mut Vec<Segment>) {
        if d.speaking {
            d.cues.push(Cue {
                at_ms: d.clock,
                kind: CueKind::SpeakingStop,
            });
            d.speaking = false;
        }
        d.cues.sort_by_key(|c| c.at_ms);
        segments.push(Segment {
            units: d.start..ordinal,
            visible_from: d.visible_from,
            reveal_ms: std::mem::take(&mut d.units).into(),
            cues: std::mem::take(&mut d.cues).into(),
            duration_ms: d.clock,
            end,
        });
        d.start = ordinal;
        d.clock = 0;
    }

    let mut pending_delay = 0u32;
    doc.walk(|item| match item {
        Item::Unit(unit) => {
            let class = unit.class();
            // Emphasis waits before and after each of its characters.
            let emphasis = match unit.emphasis {
                Some(e) if pacing.typing && class != CharClass::Space => u32::from(e.ms),
                _ => 0,
            };
            pending_delay = pending_delay.saturating_add(emphasis);
            if d.speaking && pending_delay >= SPEAKING_GAP_MS {
                d.cues.push(Cue {
                    at_ms: d.clock.saturating_add(SPEAKING_TAIL_MS),
                    kind: CueKind::SpeakingStop,
                });
                d.speaking = false;
            }
            d.clock = d.clock.saturating_add(pending_delay);
            if !d.speaking && !matches!(class, CharClass::Space) {
                d.cues.push(Cue {
                    at_ms: d.clock,
                    kind: CueKind::SpeakingStart,
                });
                d.speaking = true;
            }
            d.units.push(d.clock);
            let clicks = sound.enabled
                && !(sound.skip_space && class == CharClass::Space)
                && !(sound.skip_punct && matches!(class, CharClass::Punct(_)));
            if clicks {
                let due = eligible.is_multiple_of(u32::from(sound.every_n.max(1)));
                eligible += 1;
                let spaced = last_click
                    .is_none_or(|t| d.clock.saturating_sub(t) >= u32::from(sound.min_interval_ms));
                if due && spaced {
                    d.cues.push(Cue {
                        at_ms: d.clock,
                        kind: CueKind::TypeSound {
                            unit: ordinal,
                            variant,
                            seed: ordinal.wrapping_mul(0x9E37_79B9),
                        },
                    });
                    variant = (variant + 1) % sound.variants.max(1);
                    last_click = Some(d.clock);
                }
            }
            pending_delay = pacing.delay_ms(class, unit.pace).saturating_add(emphasis);
            ordinal += 1;
        }
        Item::Marker(marker) => {
            let at = d.clock.saturating_add(pending_delay);
            match marker {
                Marker::Wait { ms } => {
                    pending_delay =
                        pending_delay.saturating_add(if pacing.typing { *ms } else { 0 });
                }
                Marker::Pause { auto_ms } => {
                    d.clock = at;
                    pending_delay = 0;
                    close(
                        &mut d,
                        SegmentEnd::Pause { auto_ms: *auto_ms },
                        ordinal,
                        &mut segments,
                    );
                    last_click = None;
                }
                Marker::Page => {
                    d.clock = at;
                    pending_delay = 0;
                    close(
                        &mut d,
                        SegmentEnd::Page { auto_ms: None },
                        ordinal,
                        &mut segments,
                    );
                    d.visible_from = ordinal;
                    last_click = None;
                }
                Marker::Sound { sound, gain } => d.cues.push(Cue {
                    at_ms: at,
                    kind: CueKind::Sound {
                        sound: sound.clone(),
                        gain: *gain,
                    },
                }),
                Marker::Expression { name, intensity } => d.cues.push(Cue {
                    at_ms: at,
                    kind: CueKind::Expression {
                        name: name.clone(),
                        intensity: *intensity,
                    },
                }),
                Marker::Portrait { variant } => d.cues.push(Cue {
                    at_ms: at,
                    kind: CueKind::Portrait {
                        variant: variant.clone(),
                    },
                }),
                Marker::ScreenShake { amp, ms } => d.cues.push(Cue {
                    at_ms: at,
                    kind: CueKind::Shake { amp: *amp, ms: *ms },
                }),
                Marker::Vts(action) => d.cues.push(Cue {
                    at_ms: at,
                    kind: CueKind::Vts(action.clone()),
                }),
            }
        }
    });
    d.clock = d.clock.saturating_add(pending_delay);
    close(&mut d, SegmentEnd::LineEnd, ordinal, &mut segments);
    Timeline {
        unit_count: ordinal,
        segments,
    }
}
