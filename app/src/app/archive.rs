//! Saved history: lines appended to the session's file as they reach the
//! history (through the persist worker), and the Settings › History viewer's
//! listing, loading, export and deletion on background threads.
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use nanatype_model::{HistoryItem, LineId, SourceKind};

use super::App;
use crate::archive::{self, Format, Record};
use crate::msg::{FileTarget, HistoryMsg, Msg};
use crate::plugins::background;

#[derive(Default)]
pub(super) struct Archive {
    /// This session's file, named once its first line is saved.
    file: Option<PathBuf>,
    /// The newest history row already looked at.
    seen: Option<u64>,
    /// Partner lines already saved, by owner and line.
    partners: HashSet<(String, LineId)>,
    /// The session a save dialog is open for.
    export: Option<String>,
}

fn offset() -> i64 {
    crate::localtime::offset_seconds()
}

impl App {
    fn archive_dir(&self) -> PathBuf {
        self.paths.data.join(archive::DIR)
    }

    /// Delete sessions past the retention, then list the rest.
    pub(super) fn archive_start(&mut self) {
        let dir = self.archive_dir();
        let keep = self.settings.history.archive.keep_days;
        background("history-prune", &self.ctx, move || {
            archive::prune(&dir, keep, nanatype_model::unix_millis(), offset());
            Msg::History(HistoryMsg::Listed(archive::list(&dir)))
        });
    }

    fn archive_list(&self) {
        let dir = self.archive_dir();
        background("history-list", &self.ctx, move || {
            Msg::History(HistoryMsg::Listed(archive::list(&dir)))
        });
    }

    fn archive_wants(&self, item: &HistoryItem) -> bool {
        let a = &self.settings.history.archive;
        a.enabled && (item.kind == SourceKind::Typewriter || a.chat)
    }

    fn archive_save(&mut self, record: Record) {
        let dir = self.archive_dir();
        let file = self.archive.file.get_or_insert_with(|| {
            dir.join(format!(
                "{}.jsonl",
                archive::session_name(record.at, offset())
            ))
        });
        self.persist.append(file.clone(), record.line());
    }

    /// Save the history rows added since the last call. Clearing the history
    /// or dropping rows past its cap never reaches the file.
    pub(super) fn archive_rows(&mut self) {
        let seen = self.archive.seen;
        let fresh: Vec<Record> = self
            .core
            .history()
            .iter()
            .filter(|row| seen.is_none_or(|k| row.key > k))
            .filter(|row| self.archive_wants(&row.item))
            .map(|row| Record::of(&row.item, false))
            .collect();
        if let Some(newest) = self.core.history().back().map(|r| r.key) {
            self.archive.seen = Some(seen.map_or(newest, |k| k.max(newest)));
        }
        for record in fresh {
            self.archive_save(record);
        }
    }

    /// Save a co-op partner's line the first time the room shows it.
    pub(crate) fn archive_partner(&mut self, owner: &str, line: LineId, item: &HistoryItem) {
        if self.archive_wants(item) && self.archive.partners.insert((owner.to_owned(), line)) {
            self.archive_save(Record::of(item, true));
        }
    }

    /// The History page came into view: its list may be stale.
    pub(super) fn archive_page_shown(&self) {
        self.archive_list();
    }

    pub(super) fn history_msg(&mut self, msg: HistoryMsg) {
        match msg {
            HistoryMsg::Listed(sessions) => {
                let mut view = (*self.ui.archive.get_untracked()).clone();
                if view
                    .open
                    .as_ref()
                    .is_some_and(|open| !sessions.iter().any(|s| &s.name == open))
                {
                    view.open = None;
                    view.records.clear();
                }
                view.sessions = sessions;
                self.ui.archive.set(Arc::new(view));
            }
            HistoryMsg::Open(None) => {
                let mut view = (*self.ui.archive.get_untracked()).clone();
                view.open = None;
                view.records.clear();
                self.ui.archive.set(Arc::new(view));
            }
            HistoryMsg::Open(Some(name)) => {
                let dir = self.archive_dir();
                background("history-load", &self.ctx, move || {
                    let records = archive::load(&dir, &name).map_err(|e| e.to_string());
                    Msg::History(HistoryMsg::Loaded { name, records })
                });
            }
            HistoryMsg::Loaded { name, records } => match records {
                Ok(records) => {
                    let mut view = (*self.ui.archive.get_untracked()).clone();
                    view.open = Some(name);
                    view.records = records;
                    self.ui.archive.set(Arc::new(view));
                }
                Err(message) => {
                    let text = self
                        .ui
                        .tr("history-saved-failed", &[("message", message.into())]);
                    self.notice(text, true);
                }
            },
            HistoryMsg::Export(format) => {
                if let Some(name) = self.ui.archive.get_untracked().open.clone() {
                    self.archive.export = Some(name);
                    self.pick(FileTarget::ExportHistory(format));
                }
            }
            HistoryMsg::Exported(result) => {
                let text = match &result {
                    Ok(path) => self.ui.tr(
                        "history-saved-exported",
                        &[("path", path.display().to_string().into())],
                    ),
                    Err(message) => self.ui.tr(
                        "history-saved-failed",
                        &[("message", message.clone().into())],
                    ),
                };
                self.notice(text, result.is_err());
            }
            HistoryMsg::Delete(name) => {
                let dir = self.archive_dir();
                let Some(path) = archive::path(&dir, &name) else {
                    return;
                };
                // The next line starts a new file rather than reviving this one.
                if self.archive.file.as_ref() == Some(&path) {
                    self.archive.file = None;
                }
                background("history-delete", &self.ctx, move || {
                    if let Err(e) = std::fs::remove_file(&path) {
                        nanatype_diag::report!(crate::diag::PERSIST_FAILED; "{}: {e}", path.display());
                    }
                    Msg::History(HistoryMsg::Listed(archive::list(&dir)))
                });
            }
            HistoryMsg::OpenFolder => {
                let dir = self.archive_dir();
                let _ = std::fs::create_dir_all(&dir);
                crate::services::reveal(&dir);
            }
        }
    }

    /// The save dialog of a history export: the open session's file name.
    pub(super) fn archive_export_name(&self, format: Format) -> String {
        let name = self.archive.export.clone().unwrap_or_default();
        format!("NanaType {name}.{}", format.ext())
    }

    /// Write the session the dialog was opened for to `path`.
    pub(super) fn archive_export(&mut self, format: Format, path: PathBuf) {
        let Some(name) = self.archive.export.take() else {
            return;
        };
        let dir = self.archive_dir();
        background("history-export", &self.ctx, move || {
            let result = archive::load(&dir, &name)
                .and_then(|records| std::fs::write(&path, format.render(&records, offset())))
                .map(|()| path)
                .map_err(|e| e.to_string());
            Msg::History(HistoryMsg::Exported(result))
        });
    }
}
