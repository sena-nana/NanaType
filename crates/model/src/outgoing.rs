//! Turning a typed line into the plain chat text a live platform accepts.
//! Shared by every integration that sends; platform limits stay with them.

use std::fmt;

use unicode_segmentation::UnicodeSegmentation;

use crate::{Document, Inline};

/// Where one outgoing line stands in a platform's send queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryState {
    Queued,
    Sending,
    Submitted,
    Failed,
    Uncertain,
    Cancelled,
}

/// Why a line cannot become chat text; nothing was sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutgoingError {
    /// Pictures have no plain-text form.
    Images,
    /// Nothing readable is left after removing performance.
    Empty,
    /// The platform's limit is zero.
    InvalidLimit,
    /// One combined character is longer than the limit.
    Unsplittable,
}

impl fmt::Display for OutgoingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Images => "images unavailable during danmaku synchronization",
            Self::Empty => "no readable text",
            Self::InvalidLimit => "invalid danmaku length",
            Self::Unsplittable => "one combined character exceeds the danmaku limit",
        })
    }
}

impl std::error::Error for OutgoingError {}

pub fn image_count(doc: &Document) -> usize {
    doc.paragraphs
        .iter()
        .flat_map(|p| &p.inlines)
        .filter(|i| matches!(i, Inline::Atom(_)))
        .count()
}

pub fn plain_text(doc: &Document) -> Result<String, OutgoingError> {
    if image_count(doc) > 0 {
        return Err(OutgoingError::Images);
    }
    let text = doc
        .plain_text()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if text.is_empty() {
        return Err(OutgoingError::Empty);
    }
    Ok(text)
}

/// Count Unicode scalars while preserving extended grapheme clusters.
pub fn split(text: &str, limit: usize) -> Result<Vec<String>, OutgoingError> {
    if limit == 0 {
        return Err(OutgoingError::InvalidLimit);
    }
    let gs: Vec<&str> = text.graphemes(true).collect();
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < gs.len() {
        let mut end = start;
        let mut count = 0;
        let mut punctuation = None;
        while end < gs.len() && count + gs[end].chars().count() <= limit {
            count += gs[end].chars().count();
            end += 1;
            if gs[end - 1].chars().any(|c| "。！？；，,.!?; ".contains(c)) {
                punctuation = Some(end);
            }
        }
        if end == start {
            return Err(OutgoingError::Unsplittable);
        }
        if end < gs.len()
            && let Some(p) = punctuation.filter(|p| *p > start)
        {
            end = p;
        }
        let chunk = gs[start..end].concat();
        if !chunk.trim().is_empty() {
            chunks.push(chunk);
        }
        start = end;
    }
    Ok(chunks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AssetRef, Atom, AtomKind};

    #[test]
    fn chunks_prefer_punctuation_and_keep_every_character() {
        let chunks = split("hello, world and more", 10).unwrap();
        assert_eq!(chunks, ["hello, ", "world and ", "more"]);
        assert_eq!(split("", 5).unwrap(), Vec::<String>::new());
        assert_eq!(split("abc", 0), Err(OutgoingError::InvalidLimit));
    }

    #[test]
    fn pictures_and_blank_lines_have_no_chat_text() {
        assert_eq!(
            plain_text(&Document::plain("  \n ")),
            Err(OutgoingError::Empty)
        );
        let doc = Document::from_inlines(vec![Inline::Atom(Atom {
            kind: AtomKind::Image,
            asset: AssetRef::Blob("fixture".into()),
            height_em: 1.0,
            alt: "picture".into(),
            effects: Default::default(),
        })]);
        assert_eq!(image_count(&doc), 1);
        assert_eq!(plain_text(&doc), Err(OutgoingError::Images));
    }
}
