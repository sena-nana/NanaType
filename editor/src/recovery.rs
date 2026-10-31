//! Recovery is separate from the manual save point and never modifies installed packages.
use crate::history::Forms;
use nanatype_theme::{PackageProject, editor::DraftInfo};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

static WRITES: Mutex<()> = Mutex::new(());
pub struct Snapshot {
    pub info: DraftInfo,
    pub forms: Forms,
    pub conflicts: std::collections::BTreeSet<String>,
    pub project: PackageProject,
    pub saved: PackageProject,
    pub disk: Option<PackageProject>,
    pub identity: Option<String>,
}
#[derive(serde::Serialize, serde::Deserialize)]
struct Meta {
    info: DraftInfo,
    forms: Forms,
    #[serde(default)]
    conflicts: std::collections::BTreeSet<String>,
    #[serde(default)]
    has_disk: bool,
    #[serde(default)]
    identity: Option<String>,
}
fn path(root: &Path, id: &str) -> Result<PathBuf, String> {
    if nanatype_theme::clean_path(id).as_deref() != Ok(id) || id.contains('/') {
        return Err("invalid recovery ID".into());
    }
    Ok(root.join("recovery").join(format!("{id}.ntrecover")))
}

pub fn write(
    root: &Path,
    snapshot: Snapshot,
    epoch: &AtomicU64,
    version: u64,
) -> Result<(), String> {
    let _lock = WRITES.lock().map_err(|e| e.to_string())?;
    let target = path(root, &snapshot.info.id)?;
    let parent = target.parent().unwrap();
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut staging = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    {
        let mut zip = zip::ZipWriter::new(staging.as_file_mut());
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let meta = serde_json::to_vec(&Meta {
            info: snapshot.info,
            forms: snapshot.forms,
            conflicts: snapshot.conflicts,
            has_disk: snapshot.disk.is_some(),
            identity: snapshot.identity,
        })
        .map_err(|e| e.to_string())?;
        if meta.len() > 2 * 1024 * 1024 {
            return Err("recovery form budget exceeded".into());
        }
        zip.start_file("recovery.json", options)
            .map_err(|e| e.to_string())?;
        zip.write_all(&meta).map_err(|e| e.to_string())?;
        let mut sections = vec![("working", snapshot.project), ("saved", snapshot.saved)];
        if let Some(disk) = snapshot.disk {
            sections.push(("disk", disk));
        }
        for (prefix, project) in sections {
            project.check_budget().map_err(|e| e.to_string())?;
            for (name, bytes) in project.files {
                zip.start_file(format!("{prefix}/{name}"), options)
                    .map_err(|e| e.to_string())?;
                zip.write_all(&bytes).map_err(|e| e.to_string())?;
            }
        }
        zip.finish().map_err(|e| e.to_string())?;
    }
    staging.as_file().sync_all().map_err(|e| e.to_string())?;
    if epoch.load(Ordering::Acquire) == version {
        staging.persist(target).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn remove(root: &Path, id: &str) -> Result<(), String> {
    let _lock = WRITES.lock().map_err(|e| e.to_string())?;
    match std::fs::remove_file(path(root, id)?) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
fn metadata(zip: &mut zip::ZipArchive<std::fs::File>) -> Result<Meta, String> {
    let mut data = Vec::new();
    zip.by_name("recovery.json")
        .map_err(|e| e.to_string())?
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut data)
        .map_err(|e| e.to_string())?;
    if data.len() > 2 * 1024 * 1024 {
        return Err("recovery metadata exceeds budget".into());
    }
    serde_json::from_slice(&data).map_err(|e| e.to_string())
}
pub fn list(root: &Path) -> Vec<DraftInfo> {
    let mut rows: Vec<_> = std::fs::read_dir(root.join("recovery"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let p = entry.path();
            if p.extension()?.to_str()? != "ntrecover" {
                return None;
            }
            let mut zip = zip::ZipArchive::new(std::fs::File::open(&p).ok()?).ok()?;
            let info = metadata(&mut zip).ok()?.info;
            (path(root, &info.id).ok()? == p).then_some(info)
        })
        .collect();
    rows.sort_by_key(|a| std::cmp::Reverse(a.saved_at));
    rows
}
pub fn load(root: &Path, id: &str) -> Result<Snapshot, String> {
    let file = std::fs::File::open(path(root, id)?).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let meta = metadata(&mut zip)?;
    if meta.info.id != id || zip.len() > 12289 {
        return Err("invalid recovery inventory".into());
    }
    let mut working = PackageProject::default();
    let mut saved = PackageProject::default();
    let mut disk = PackageProject::default();
    let mut total = 0_u64;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let path = entry.name().map_err(|e| e.to_string())?.into_owned();
        if path == "recovery.json" {
            continue;
        }
        total += entry.size();
        if entry.size() > nanatype_theme::MAX_FILE_BYTES
            || total > nanatype_theme::MAX_PACKAGE_BYTES * 3
        {
            return Err("recovery exceeds budget".into());
        }
        let (prefix, name) = path.split_once('/').ok_or("invalid recovery path")?;
        let name = nanatype_theme::clean_path(name).map_err(|e| e.to_string())?;
        let project = match prefix {
            "working" => &mut working,
            "saved" => &mut saved,
            "disk" if meta.has_disk => &mut disk,
            _ => return Err("invalid recovery section".into()),
        };
        let mut bytes = Vec::new();
        entry
            .by_ref()
            .take(nanatype_theme::MAX_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 != entry.size() || project.files.insert(name, bytes).is_some() {
            return Err("invalid recovery file".into());
        }
    }
    working.check_budget().map_err(|e| e.to_string())?;
    saved.check_budget().map_err(|e| e.to_string())?;
    disk.check_budget().map_err(|e| e.to_string())?;
    Ok(Snapshot {
        info: meta.info,
        forms: meta.forms,
        conflicts: meta.conflicts,
        project: working,
        saved,
        disk: meta.has_disk.then_some(disk),
        identity: meta.identity,
    })
}
