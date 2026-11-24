use std::collections::VecDeque;

use nanatype_model::{Document, LineMeta};

/// Lines kept for Ctrl+↑ recall.
const RECALL_CAP: usize = 100;

/// Editor draft and its `[@...]` header metadata kept atomically together.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Draft {
    pub doc: Document,
    pub meta: LineMeta,
}

#[derive(Default)]
pub(crate) struct DraftBox {
    current: Draft,
    sent: VecDeque<Draft>,
    recall: Option<usize>,
    stash: Option<Draft>,
}

impl DraftBox {
    pub fn doc(&self) -> &Document {
        &self.current.doc
    }

    pub fn meta(&self) -> &LineMeta {
        &self.current.meta
    }

    /// Update the draft document, returning `(previous, new)` if it changed.
    pub fn set_doc(&mut self, doc: Document) -> Option<(Document, Document)> {
        let doc = doc.normalized();
        if doc == self.current.doc {
            return None;
        }
        let previous = std::mem::replace(&mut self.current.doc, doc.clone());
        if self.recall.is_some() && Some(&doc) != self.recalled_doc() {
            // Typing after a recall keeps the recalled text as a new draft.
            self.recall = None;
            self.stash = None;
        }
        Some((previous, doc))
    }

    pub fn set_meta(&mut self, meta: LineMeta) {
        self.current.meta = meta;
    }

    fn recalled_doc(&self) -> Option<&Document> {
        self.recall
            .and_then(|i| self.sent.get(i))
            .map(|draft| &draft.doc)
    }

    /// Step through sent drafts (`-1` older, `+1` newer), returning
    /// `(previous_doc, restored_draft)` when recall moved.
    pub fn recall(&mut self, dir: i32) -> Option<(Document, Draft)> {
        if self.sent.is_empty() {
            return None;
        }
        let last = self.sent.len() - 1;
        let next = match (self.recall, dir.signum()) {
            (None, -1) => Some(last),
            (None, _) => return None,
            (Some(i), -1) => Some(i.saturating_sub(1)),
            (Some(i), _) if i >= last => None,
            (Some(i), _) => Some(i + 1),
        };
        if self.recall.is_none() {
            self.stash = Some(self.current.clone());
        }
        self.recall = next;
        let draft = match next {
            Some(i) => self.sent[i].clone(),
            None => self.stash.take().unwrap_or_default(),
        };
        let previous = std::mem::replace(&mut self.current.doc, draft.doc.clone());
        self.current.meta = draft.meta.clone();
        Some((previous, draft))
    }

    /// Commit the current non-blank draft into the recall ring and clear the editor.
    pub fn take_for_send(&mut self) -> Option<Draft> {
        if self.current.doc.is_blank() {
            return None;
        }
        let committed = std::mem::take(&mut self.current);
        self.sent.push_back(committed.clone());
        while self.sent.len() > RECALL_CAP {
            self.sent.pop_front();
        }
        self.recall = None;
        self.stash = None;
        Some(committed)
    }
}
