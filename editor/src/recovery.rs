//! Recovery is separate from the manual save point and never modifies installed packages.
//! Snapshots use a content-deduplicated ZIP layout (`blobs/<blake3>` + `recovery.json`)
//! while keeping read support for legacy `working/`/`saved/`/`disk/` recovery archives.
use crate::history::Forms;
use nanatype_theme::{PackageProject, editor::DraftInfo};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

static WRITES: Mutex<()> = Mutex::new(());
const FORMAT_DEDUP: u32 = 2;

pub struct Snapshot {
    pub info: DraftInfo,
    pub forms: Forms,
    pub conflicts: BTreeSet<String>,
    pub project: PackageProject,
    pub saved: PackageProject,
    pub disk: Option<PackageProject>,
    pub identity: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Meta {
    #[serde(default)]
    format: u32,
    info: DraftInfo,
    forms: Forms,
    #[serde(default)]
    conflicts: BTreeSet<String>,
    #[serde(default)]
    has_disk: bool,
    #[serde(default)]
    identity: Option<String>,
    #[serde(default)]
    working: BTreeMap<String, String>,
    #[serde(default)]
    saved: BTreeMap<String, String>,
    #[serde(default)]
    disk: Option<BTreeMap<String, String>>,
}

fn path(root: &Path, id: &str) -> Result<PathBuf, String> {
    if nanatype_theme::clean_path(id).as_deref() != Ok(id) || id.contains('/') {
        return Err("invalid recovery ID".into());
    }
    Ok(root.join("recovery").join(format!("{id}.ntrecover")))
}

fn index_project<'a>(
    project: &'a PackageProject,
    blobs: &mut BTreeMap<String, &'a [u8]>,
) -> Result<BTreeMap<String, String>, String> {
    project.check_budget().map_err(|e| e.to_string())?;
    let mut index = BTreeMap::new();
    for (name, bytes) in &project.files {
        let hash = project.cached_hash(name).map_err(|e| e.to_string())?;
        let hex = blake3::Hash::from_bytes(hash).to_hex().to_string();
        blobs.entry(hex.clone()).or_insert(bytes.as_slice());
        index.insert(name.clone(), hex);
    }
    Ok(index)
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
    let mut blobs: BTreeMap<String, &[u8]> = BTreeMap::new();
    let working = index_project(&snapshot.project, &mut blobs)?;
    let saved = index_project(&snapshot.saved, &mut blobs)?;
    let disk = snapshot
        .disk
        .as_ref()
        .map(|d| index_project(d, &mut blobs))
        .transpose()?;

    let mut staging = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    {
        let mut zip = zip::ZipWriter::new(staging.as_file_mut());
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let meta = serde_json::to_vec(&Meta {
            format: FORMAT_DEDUP,
            info: snapshot.info,
            forms: snapshot.forms,
            conflicts: snapshot.conflicts,
            has_disk: disk.is_some(),
            identity: snapshot.identity,
            working,
            saved,
            disk,
        })
        .map_err(|e| e.to_string())?;
        if meta.len() > 2 * 1024 * 1024 {
            return Err("recovery form budget exceeded".into());
        }
        zip.start_file("recovery.json", options)
            .map_err(|e| e.to_string())?;
        zip.write_all(&meta).map_err(|e| e.to_string())?;

        let mut previous = std::fs::File::open(&target)
            .ok()
            .and_then(|f| zip::ZipArchive::new(f).ok())
            .and_then(|mut archive| {
                metadata(&mut archive)
                    .is_ok_and(|prev| prev.format == FORMAT_DEDUP)
                    .then_some(archive)
            });

        for (hex, bytes) in blobs {
            let entry_name = format!("blobs/{hex}");
            let copied = if let Some(prev_zip) = previous.as_mut()
                && let Ok(old_entry) = prev_zip.by_name(&entry_name)
                && old_entry.size() == bytes.len() as u64
            {
                zip.raw_copy_file(old_entry).is_ok()
            } else {
                false
            };
            if !copied {
                zip.start_file(&entry_name, options)
                    .map_err(|e| e.to_string())?;
                zip.write_all(bytes).map_err(|e| e.to_string())?;
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

fn load_dedup(zip: &mut zip::ZipArchive<std::fs::File>, meta: Meta) -> Result<Snapshot, String> {
    if zip.len() > 12289 {
        return Err("invalid recovery inventory".into());
    }
    let mut referenced = BTreeSet::new();
    for map in [&meta.working, &meta.saved]
        .into_iter()
        .chain(meta.disk.as_ref())
    {
        if map.len() > 4096 {
            return Err("invalid recovery inventory".into());
        }
        for (name, hex) in map {
            nanatype_theme::clean_path(name).map_err(|e| e.to_string())?;
            if nanatype_model::codec::unhex::<32>(hex).is_none() {
                return Err("invalid recovery blob digest".into());
            }
            referenced.insert(hex.clone());
        }
    }
    let mut blobs: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut total = 0_u64;
    for hex in referenced {
        let mut entry = zip
            .by_name(&format!("blobs/{hex}"))
            .map_err(|e| e.to_string())?;
        total = total
            .checked_add(entry.size())
            .ok_or("recovery exceeds budget")?;
        if entry.size() > nanatype_theme::MAX_FILE_BYTES
            || total > nanatype_theme::MAX_PACKAGE_BYTES * 3
        {
            return Err("recovery exceeds budget".into());
        }
        let mut bytes = Vec::new();
        entry
            .by_ref()
            .take(nanatype_theme::MAX_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 != entry.size()
            || blake3::hash(&bytes).to_hex().as_str() != hex.as_str()
        {
            return Err("corrupted recovery blob".into());
        }
        blobs.insert(hex, bytes);
    }
    let rebuild = |index: BTreeMap<String, String>| -> Result<PackageProject, String> {
        let mut project = PackageProject::default();
        for (name, hex) in index {
            let name = nanatype_theme::clean_path(&name).map_err(|e| e.to_string())?;
            let bytes = blobs.get(&hex).ok_or("missing recovery blob")?.clone();
            if project.files.insert(name, bytes).is_some() {
                return Err("duplicate recovery path".into());
            }
        }
        project.check_budget().map_err(|e| e.to_string())?;
        Ok(project)
    };
    let project = rebuild(meta.working)?;
    let saved = rebuild(meta.saved)?;
    let disk = meta.disk.map(rebuild).transpose()?;
    Ok(Snapshot {
        info: meta.info,
        forms: meta.forms,
        conflicts: meta.conflicts,
        project,
        saved,
        disk,
        identity: meta.identity,
    })
}

fn load_legacy(zip: &mut zip::ZipArchive<std::fs::File>, meta: Meta) -> Result<Snapshot, String> {
    if zip.len() > 12289 {
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

pub fn load(root: &Path, id: &str) -> Result<Snapshot, String> {
    let file = std::fs::File::open(path(root, id)?).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let meta = metadata(&mut zip)?;
    if meta.info.id != id {
        return Err("invalid recovery inventory".into());
    }
    match meta.format {
        FORMAT_DEDUP => load_dedup(&mut zip, meta),
        0 | 1 => load_legacy(&mut zip, meta),
        _ => Err("unsupported recovery archive format".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nanatype_theme::editor::{self, DraftOrigin, ProjectKind};

    #[test]
    fn dedup_writer_shares_identical_blobs_and_rewrites_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = editor::template(ProjectKind::Skin, "editor.dedup").unwrap();
        project
            .files
            .insert("assets/big.bin".into(), vec![0x42; 65_536]);
        let saved = project.clone();
        let disk = Some(project.clone());
        project.files.insert(
            "skin.toml".into(),
            b"[chrome]\naccent = \"#112233\"\n".to_vec(),
        );
        project.invalidate_hashes();

        let info = DraftInfo {
            id: "dedup-1".into(),
            package: "editor.dedup".into(),
            origin: DraftOrigin::New,
            saved_at: 100,
        };
        let epoch = AtomicU64::new(1);
        write(
            dir.path(),
            Snapshot {
                info: info.clone(),
                forms: Forms::new(),
                conflicts: BTreeSet::new(),
                project: project.clone(),
                saved: saved.clone(),
                disk: disk.clone(),
                identity: Some("editor.dedup".into()),
            },
            &epoch,
            1,
        )
        .unwrap();

        let archive_path = path(dir.path(), "dedup-1").unwrap();
        let archive = zip::ZipArchive::new(std::fs::File::open(&archive_path).unwrap()).unwrap();
        // `assets/big.bin` and `package.toml` are identical across working/saved/disk,
        // while `skin.toml` has 2 versions -> 4 blobs + 1 recovery.json = 5 entries total.
        assert_eq!(archive.len(), 5);

        // Incremental second write reuses unchanged blobs via raw_copy_file.
        project.files.insert(
            "skin.toml".into(),
            b"[chrome]\naccent = \"#445566\"\n".to_vec(),
        );
        project.invalidate_hashes();
        epoch.store(2, Ordering::Release);
        write(
            dir.path(),
            Snapshot {
                info,
                forms: Forms::new(),
                conflicts: BTreeSet::new(),
                project: project.clone(),
                saved: saved.clone(),
                disk,
                identity: Some("editor.dedup".into()),
            },
            &epoch,
            2,
        )
        .unwrap();

        assert_eq!(list(dir.path()).len(), 1);
        let loaded = load(dir.path(), "dedup-1").unwrap();
        assert_eq!(loaded.project.files, project.files);
        assert_eq!(loaded.saved.files, saved.files);
        assert_eq!(loaded.disk.unwrap().files, saved.files);
    }

    #[test]
    fn legacy_zip_recovery_archives_remain_readable() {
        let dir = tempfile::tempdir().unwrap();
        let target = path(dir.path(), "legacy-1").unwrap();
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        let project = editor::template(ProjectKind::Skin, "editor.legacy").unwrap();
        {
            let file = std::fs::File::create(&target).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let opts = zip::write::SimpleFileOptions::default();
            let meta = serde_json::json!({
                "info": {
                    "id": "legacy-1",
                    "package": "editor.legacy",
                    "origin": { "kind": "new" },
                    "saved_at": 42
                },
                "forms": {},
                "conflicts": [],
                "has_disk": true,
                "identity": "editor.legacy"
            });
            zip.start_file("recovery.json", opts).unwrap();
            zip.write_all(&serde_json::to_vec(&meta).unwrap()).unwrap();
            for prefix in ["working", "saved", "disk"] {
                for (name, bytes) in &project.files {
                    zip.start_file(format!("{prefix}/{name}"), opts).unwrap();
                    zip.write_all(bytes).unwrap();
                }
            }
            zip.finish().unwrap();
        }
        let rows = list(dir.path());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "legacy-1");
        let loaded = load(dir.path(), "legacy-1").unwrap();
        assert_eq!(loaded.project.files, project.files);
        assert_eq!(loaded.saved.files, project.files);
        assert_eq!(loaded.disk.unwrap().files, project.files);
    }
}
