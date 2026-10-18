//! Live mirror: the typewriter follows the draft as it is typed.
//!
//! Units are compared by text and attributes, never deduplicated, so typing
//! the same character twice reveals it twice.

use nanatype_model::{Document, Inline, Unit};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MirrorDelta {
    /// Units unchanged at the start; they stay revealed.
    pub common_prefix: u32,
    /// Units of `prev` after the prefix that disappeared.
    pub removed: u32,
    /// Units of `next` after the prefix that should reveal.
    pub appended: u32,
}

fn key<'a>(doc: &'a Document, unit: &Unit) -> (&'a str, Option<&'a Inline>) {
    let inline = doc
        .paragraphs
        .get(unit.para)
        .and_then(|p| p.inlines.get(unit.inline));
    (doc.unit_text(unit), inline)
}

fn same(prev: &Document, a: &Unit, next: &Document, b: &Unit) -> bool {
    a.para == b.para && same_content(prev, a, next, b)
}

fn same_content(prev: &Document, a: &Unit, next: &Document, b: &Unit) -> bool {
    let (ta, ia) = key(prev, a);
    let (tb, ib) = key(next, b);
    if ta != tb {
        return false;
    }
    match (ia, ib) {
        (Some(Inline::Text(x)), Some(Inline::Text(y))) => x.same_attrs(y),
        (Some(Inline::Ruby(x)), Some(Inline::Ruby(y))) => {
            x.annotation == y.annotation && x.base.same_attrs(&y.base)
        }
        (Some(Inline::Atom(x)), Some(Inline::Atom(y))) => x == y,
        (Some(Inline::Mention(x)), Some(Inline::Mention(y))) => x == y,
        _ => false,
    }
}

pub fn mirror_delta(prev: &Document, next: &Document) -> MirrorDelta {
    let a = prev.units();
    let b = next.units();
    let common = a
        .iter()
        .zip(&b)
        .take_while(|(x, y)| same(prev, x, next, y))
        .count();
    MirrorDelta {
        common_prefix: common as u32,
        removed: (a.len() - common) as u32,
        appended: (b.len() - common) as u32,
    }
}

/// Match retained units through middle edits without restarting the suffix.
/// Trim the common edges first and bound the middle diff's memory cost.
pub fn mirror_matches(prev: &Document, next: &Document) -> Vec<Option<usize>> {
    let a = prev.units();
    let b = next.units();
    let mut out = vec![None; b.len()];
    let mut first = 0;
    while first < a.len().min(b.len()) && same_content(prev, &a[first], next, &b[first]) {
        out[first] = Some(first);
        first += 1;
    }
    let (mut ae, mut be) = (a.len(), b.len());
    while ae > first && be > first && same_content(prev, &a[ae - 1], next, &b[be - 1]) {
        ae -= 1;
        be -= 1;
        out[be] = Some(ae);
    }
    let (n, m) = (ae - first, be - first);
    if n.saturating_mul(m) <= 262_144 {
        let mut lengths = vec![0u32; (n + 1) * (m + 1)];
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                lengths[i * (m + 1) + j] = if same_content(prev, &a[first + i], next, &b[first + j])
                {
                    lengths[(i + 1) * (m + 1) + j + 1] + 1
                } else {
                    lengths[(i + 1) * (m + 1) + j].max(lengths[i * (m + 1) + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n && j < m {
            if same_content(prev, &a[first + i], next, &b[first + j]) {
                out[first + j] = Some(first + i);
                i += 1;
                j += 1;
            } else if lengths[(i + 1) * (m + 1) + j] >= lengths[i * (m + 1) + j + 1] {
                i += 1;
            } else {
                j += 1;
            }
        }
    } else {
        // A bounded lookahead for wholesale long-document replacements.
        let mut i = first;
        for j in first..be {
            if let Some(k) = (i..ae.min(i + 64)).find(|&k| same_content(prev, &a[k], next, &b[j])) {
                out[j] = Some(k);
                i = k + 1;
            }
        }
    }
    out
}

/// Reveal offsets for the appended units: a short stagger so a paste or an
/// IME commit ripples in instead of popping, capped so a long paste does not
/// lag behind typing.
pub fn mirror_offsets(appended: u32, stagger_ms: u32, cap_ms: u32) -> Vec<u32> {
    let step = if appended > 1 {
        stagger_ms.min(cap_ms / appended.max(1))
    } else {
        stagger_ms
    };
    (0..appended).map(|i| i * step).collect()
}

#[cfg(test)]
mod retained_tests {
    use super::*;
    #[test]
    fn unicode_edits_and_repeated_characters_keep_the_unchanged_suffix() {
        let before = Document::plain("Ae\u{301}👩‍💻B");
        let after = Document::plain("A中e\u{301}👩‍💻B");
        assert_eq!(
            mirror_matches(&before, &after),
            vec![Some(0), None, Some(1), Some(2), Some(3)]
        );
        assert_eq!(
            mirror_matches(&Document::plain("AAAA"), &Document::plain("AAxAA")),
            vec![Some(0), Some(1), None, Some(2), Some(3)]
        );
        assert_eq!(
            mirror_matches(&Document::plain("原文"), &Document::plain("新\n原文")),
            vec![None, Some(0), Some(1)]
        );
    }
    #[test]
    fn wholesale_long_replacement_bounds_diff_memory() {
        assert!(
            mirror_matches(
                &Document::plain(&"a".repeat(2000)),
                &Document::plain(&"b".repeat(2000))
            )
            .iter()
            .all(Option::is_none)
        );
    }
}
