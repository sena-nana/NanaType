//! Editor draft persistence (`.ntdraft` archives).

use super::error;
use crate::{FileError, PackageProject, ZipFiles};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Read a `.ntpkg` without requiring it to be valid, so a broken package can
/// be opened and repaired. Budgets still apply; a failing seal leaves every
/// local file read-only.
pub fn read_archive(path: &Path) -> Result<PackageProject, FileError> {
    let project = PackageProject::read_all(&ZipFiles::open(path)?)?;
    project.check_budget()?;
    Ok(project)
}

/// Where a draft started, shown so a copy is never mistaken for its source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DraftOrigin {
    Directory { path: PathBuf },
    New,
    Installed { id: String },
    Archive { path: PathBuf },
    Copy { from: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftInfo {
    pub id: String,
    pub package: String,
    pub origin: DraftOrigin,
    /// Seconds since the Unix epoch.
    pub saved_at: u64,
}

fn valid_draft_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
        && !id.starts_with('.')
}

/// Atomically save bytes and metadata together in `<dir>/<id>.ntdraft`.
pub fn save_draft(dir: &Path, info: &DraftInfo, project: &PackageProject) -> Result<(), FileError> {
    if !valid_draft_id(&info.id) {
        return Err(error("invalid draft ID"));
    }
    project.check_budget()?;
    std::fs::create_dir_all(dir).map_err(error)?;
    let archive = dir.join(format!("{}.ntdraft", info.id));
    // Metadata and bytes commit together. Older drafts with sidecar JSON remain readable.
    project.write_zip_comment(&archive, &serde_json::to_string(info).map_err(error)?)
}

/// Saved drafts, newest first. Unreadable entries are skipped.
pub fn list_drafts(dir: &Path) -> Vec<DraftInfo> {
    let mut drafts: Vec<DraftInfo> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "ntdraft"))
        .filter_map(|e| {
            let id = e.path().file_stem()?.to_str()?.to_owned();
            let info = read_draft_info(dir, &id).ok()?;
            (valid_draft_id(&info.id) && dir.join(format!("{}.ntdraft", info.id)).is_file())
                .then_some(info)
        })
        .collect();
    drafts.sort_by(|a, b| b.saved_at.cmp(&a.saved_at).then(a.id.cmp(&b.id)));
    drafts
}

pub fn load_draft(dir: &Path, id: &str) -> Result<(DraftInfo, PackageProject), FileError> {
    if !valid_draft_id(id) {
        return Err(error("invalid draft ID"));
    }
    let info = read_draft_info(dir, id)?;
    Ok((info, read_archive(&dir.join(format!("{id}.ntdraft")))?))
}

fn read_draft_info(dir: &Path, id: &str) -> Result<DraftInfo, FileError> {
    let file = std::fs::File::open(dir.join(format!("{id}.ntdraft"))).map_err(error)?;
    let zip = zip::ZipArchive::new(file).map_err(error)?;
    let info: DraftInfo = if zip.comment().is_empty() {
        serde_json::from_slice(&std::fs::read(dir.join(format!("{id}.json"))).map_err(error)?)
    } else {
        serde_json::from_slice(zip.comment())
    }
    .map_err(error)?;
    if info.id != id {
        return Err(error("draft identity differs from filename"));
    }
    Ok(info)
}

/// Delete a draft and its description. Installed packages are untouched.
pub fn discard_draft(dir: &Path, id: &str) -> Result<(), FileError> {
    if !valid_draft_id(id) {
        return Err(error("invalid draft ID"));
    }
    for name in [format!("{id}.ntdraft"), format!("{id}.json")] {
        match std::fs::remove_file(dir.join(name)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(error(e)),
            _ => {}
        }
    }
    Ok(())
}
