//! Bounded, file-delta history. Unchanged binary assets are never copied into history.
use nanatype_theme::PackageProject;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub type Forms = BTreeMap<String, serde_json::Value>;
struct FileChange {
    path: String,
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
}
pub struct Change {
    files: Vec<FileChange>,
    before: Forms,
    after: Forms,
}
impl Change {
    pub fn between(
        before: &PackageProject,
        after: &PackageProject,
        forms: Forms,
        next: Forms,
    ) -> Self {
        let keys: BTreeSet<_> = before.files.keys().chain(after.files.keys()).collect();
        let files = keys
            .into_iter()
            .filter(|p| before.files.get(*p) != after.files.get(*p))
            .map(|p| FileChange {
                path: p.clone(),
                before: before.files.get(p).cloned(),
                after: after.files.get(p).cloned(),
            })
            .collect();
        Self {
            files,
            before: forms,
            after: next,
        }
    }
    pub fn text(path: &str, before: Vec<u8>, after: Vec<u8>, forms: Forms) -> Self {
        Self {
            files: vec![FileChange {
                path: path.into(),
                before: Some(before),
                after: Some(after),
            }],
            before: forms.clone(),
            after: forms,
        }
    }
    fn bytes(&self) -> usize {
        self.files
            .iter()
            .map(|f| f.before.as_ref().map_or(0, Vec::len) + f.after.as_ref().map_or(0, Vec::len))
            .sum::<usize>()
            + form_bytes(&self.before)
            + form_bytes(&self.after)
    }
    fn apply(&self, project: &mut PackageProject, forms: &mut Forms, forward: bool) {
        for f in &self.files {
            match if forward { &f.after } else { &f.before } {
                Some(bytes) => {
                    project.files.insert(f.path.clone(), bytes.clone());
                }
                None => {
                    project.files.remove(&f.path);
                }
            }
        }
        *forms = if forward {
            self.after.clone()
        } else {
            self.before.clone()
        };
    }
}

fn form_bytes(forms: &Forms) -> usize {
    fn bytes(value: &serde_json::Value) -> usize {
        match value {
            serde_json::Value::String(s) => s.len(),
            serde_json::Value::Array(values) => values.iter().map(bytes).sum(),
            serde_json::Value::Object(values) => values
                .iter()
                .map(|(key, value)| key.len() + bytes(value))
                .sum(),
            _ => 16,
        }
    }
    forms
        .iter()
        .map(|(key, value)| key.len() + bytes(value))
        .sum()
}

#[derive(Default)]
pub struct History {
    undo: VecDeque<Change>,
    redo: Vec<Change>,
}
impl History {
    pub fn push(&mut self, change: Change) {
        if change.files.is_empty() && change.before == change.after {
            return;
        }
        self.redo.clear();
        self.undo.push_back(change);
        while self.undo.len() > 100
            || (self.undo.len() > 1
                && self.undo.iter().map(Change::bytes).sum::<usize>() > 512 * 1024 * 1024)
        {
            self.undo.pop_front();
        }
    }
    pub fn after_forms(&mut self, forms: Forms) {
        if let Some(c) = self.undo.back_mut() {
            c.after = forms;
        }
    }
    pub fn available(&self) -> (bool, bool) {
        (!self.undo.is_empty(), !self.redo.is_empty())
    }
    pub fn undo(&mut self, project: &mut PackageProject, forms: &mut Forms) {
        if let Some(change) = self.undo.pop_back() {
            change.apply(project, forms, false);
            self.redo.push(change);
        }
    }
    pub fn redo(&mut self, project: &mut PackageProject, forms: &mut Forms) {
        if let Some(change) = self.redo.pop() {
            change.apply(project, forms, true);
            self.undo.push_back(change);
        }
    }
}
