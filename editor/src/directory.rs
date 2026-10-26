//! Directory projects: bounded snapshots, three-way merging and recoverable writes.
use nanatype_theme::{PACKAGE_MANIFEST, PackageProject, clean_path};
use std::{
    collections::BTreeSet,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};
static WRITES: Mutex<()> = Mutex::new(());
pub struct Watch {
    _watcher: notify::RecommendedWatcher,
}
impl Watch {
    pub fn start(root: &Path, post: impl Fn() + Send + 'static) -> Result<Self, String> {
        use notify::Watcher;
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                if event.is_ok_and(|e| {
                    !e.kind.is_access()
                        && e.paths.iter().any(|p| {
                            !p.components().any(|c| {
                                matches!(c.as_os_str().to_str(), Some(".git" | ".hg" | ".svn"))
                            })
                        })
                }) {
                    let _ = tx.send(());
                }
            })
            .map_err(err)?;
        watcher
            .watch(root, notify::RecursiveMode::Recursive)
            .map_err(err)?;
        std::thread::Builder::new()
            .name("editor-directory-watch".into())
            .spawn(move || {
                while rx.recv().is_ok() {
                    loop {
                        match rx.recv_timeout(std::time::Duration::from_millis(300)) {
                            Ok(()) => {}
                            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                            Err(_) => return,
                        }
                    }
                    post();
                }
            })
            .map_err(err)?;
        Ok(Self { _watcher: watcher })
    }
}
fn err(e: impl ToString) -> String {
    e.to_string()
}

/// Copies must not overwrite the conflicting source or an installed package.
pub fn check_copy_destination(
    path: &Path,
    source: Option<&Path>,
    installed: &Path,
) -> Result<(), String> {
    fn resolved(path: &Path) -> Result<PathBuf, String> {
        path.canonicalize()
            .or_else(|_| {
                let parent = path
                    .parent()
                    .ok_or_else(|| std::io::Error::other("missing destination directory"))?
                    .canonicalize()?;
                Ok::<_, std::io::Error>(
                    parent.join(
                        path.file_name()
                            .ok_or_else(|| std::io::Error::other("missing file name"))?,
                    ),
                )
            })
            .map_err(err)
    }
    let path = resolved(path)?;
    if source.is_some_and(|s| resolved(s).is_ok_and(|s| s == path)) {
        return Err("choose a different path for the conflict copy".into());
    }
    if installed
        .canonicalize()
        .is_ok_and(|root| path.starts_with(root))
    {
        return Err("installed packages are read-only".into());
    }
    Ok(())
}

pub fn read(root: &Path, installed: &Path) -> Result<PackageProject, String> {
    let root = root.canonicalize().map_err(err)?;
    if installed
        .canonicalize()
        .ok()
        .is_some_and(|p| root.starts_with(p))
    {
        return Err("installed packages are read-only; open a draft copy instead".into());
    }
    fn walk(
        root: &Path,
        dir: &Path,
        project: &mut PackageProject,
        total: &mut u64,
    ) -> Result<(), String> {
        for entry in std::fs::read_dir(dir).map_err(err)? {
            let entry = entry.map_err(err)?;
            let name = entry.file_name();
            if [".git", ".hg", ".svn"].iter().any(|s| name == *s) {
                continue;
            }
            let path = entry.path();
            let kind = entry.file_type().map_err(err)?;
            if kind.is_symlink() {
                return Err(format!(
                    "symbolic links are not package files: {}",
                    path.display()
                ));
            }
            let rel = path
                .strip_prefix(root)
                .map_err(err)?
                .to_str()
                .ok_or("non-UTF-8 package path")?
                .replace('\\', "/");
            clean_path(&rel).map_err(err)?;
            if kind.is_dir() {
                walk(root, &path, project, total)?;
            } else if kind.is_file() {
                let size = entry.metadata().map_err(err)?.len();
                *total += size;
                if size > nanatype_theme::MAX_FILE_BYTES
                    || *total > nanatype_theme::MAX_PACKAGE_BYTES
                    || project.files.len() >= 4096
                {
                    return Err("directory exceeds package budget".into());
                }
                let mut bytes = Vec::new();
                std::fs::File::open(&path)
                    .map_err(err)?
                    .take(nanatype_theme::MAX_FILE_BYTES + 1)
                    .read_to_end(&mut bytes)
                    .map_err(err)?;
                if bytes.len() as u64 != size {
                    return Err("directory changed during snapshot; retry".into());
                }
                project.files.insert(rel, bytes);
            }
        }
        Ok(())
    }
    let mut project = PackageProject::default();
    walk(&root, &root, &mut project, &mut 0)?;
    if !project.files.contains_key(PACKAGE_MANIFEST) {
        return Err("select a package root containing package.toml".into());
    }
    project.check_budget().map_err(err)?;
    Ok(project)
}

/// Unchanged local files follow disk. Both sides changing differently is a conflict.
pub fn merge(
    base: &PackageProject,
    local: &mut PackageProject,
    disk: &PackageProject,
) -> BTreeSet<String> {
    let keys: BTreeSet<_> = base
        .files
        .keys()
        .chain(local.files.keys())
        .chain(disk.files.keys())
        .cloned()
        .collect();
    let mut conflicts = BTreeSet::new();
    for key in keys {
        let (b, l, d) = (
            base.files.get(&key),
            local.files.get(&key),
            disk.files.get(&key),
        );
        if l == b || l == d {
            if let Some(bytes) = d {
                local.files.insert(key, bytes.clone());
            } else {
                local.files.remove(&key);
            }
        } else if d != b {
            conflicts.insert(key);
        }
    }
    conflicts
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Journal {
    root: PathBuf,
    committed: bool,
    changes: Vec<(String, Option<String>)>,
}
fn transaction(root: &Path, data: &Path) -> PathBuf {
    data.join("directory-transactions").join(
        blake3::hash(root.to_string_lossy().as_bytes())
            .to_hex()
            .as_str(),
    )
}
fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("missing parent directory")?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(err)?;
    file.write_all(bytes).map_err(err)?;
    file.as_file().sync_all().map_err(err)?;
    file.persist(path).map_err(err)?;
    Ok(())
}
fn destination(root: &Path, name: &str) -> Result<PathBuf, String> {
    let name = clean_path(name).map_err(err)?;
    let mut path = root.to_owned();
    let parts = name.split('/').collect::<Vec<_>>();
    for (i, part) in parts.iter().enumerate() {
        path.push(part);
        match std::fs::symlink_metadata(&path) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err("refusing to write through a symbolic link".into());
            }
            Ok(m) if i + 1 < parts.len() && !m.is_dir() => {
                return Err("parent is not a directory".into());
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if i + 1 < parts.len() {
                    std::fs::create_dir(&path).map_err(err)?;
                }
            }
            Err(e) => return Err(err(e)),
        }
    }
    Ok(path)
}
fn put(root: &Path, name: &str, bytes: Option<&Vec<u8>>) -> Result<(), String> {
    let path = destination(root, name)?;
    if let Some(bytes) = bytes {
        write_file(&path, bytes)
    } else {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(err(e)),
        }
    }
}
fn current(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(err(e)),
    };
    if file.metadata().map_err(err)?.len() > nanatype_theme::MAX_FILE_BYTES {
        return Err("external file exceeds package budget".into());
    }
    let mut bytes = Vec::new();
    file.take(nanatype_theme::MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(err)?;
    if bytes.len() as u64 > nanatype_theme::MAX_FILE_BYTES {
        return Err("external file exceeds package budget".into());
    }
    Ok(Some(bytes))
}
fn rollback(root: &Path, txn: &Path, journal: &Journal) -> Result<(), String> {
    let before = nanatype_theme::editor::read_archive(&txn.join("before.ntpkg")).map_err(err)?;
    // Check all paths before restoring any, so recovery never tramples external edits.
    for (name, after) in &journal.changes {
        let target = destination(root, name)?;
        let actual = current(&target)?.map(|b| blake3::hash(&b).to_hex().to_string());
        let original = before
            .files
            .get(name)
            .map(|b| blake3::hash(b).to_hex().to_string());
        if actual != *after && actual != original {
            return Err(format!(
                "interrupted save conflicts with external changes: {name}; recovery retained at {}",
                txn.display()
            ));
        }
    }
    for (name, _) in &journal.changes {
        put(root, name, before.files.get(name))?;
    }
    std::fs::remove_dir_all(txn).map_err(err)
}
fn recover_inner(root: &Path, data: &Path) -> Result<(), String> {
    let txn = transaction(root, data);
    let journal = txn.join("intent.json");
    if !journal.exists() {
        return Ok(());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(journal)
        .map_err(err)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(err)?;
    if bytes.len() > 1024 * 1024 {
        return Err("invalid directory write journal".into());
    }
    let journal: Journal = serde_json::from_slice(&bytes).map_err(err)?;
    if journal.root != root || journal.changes.len() > 4096 {
        return Err("directory write journal identity mismatch".into());
    }
    if journal.committed {
        std::fs::remove_dir_all(txn).map_err(err)
    } else {
        rollback(root, &txn, &journal)
    }
}
pub fn recover(root: &Path, data: &Path) -> Result<(), String> {
    let _lock = WRITES.lock().map_err(err)?;
    recover_inner(&root.canonicalize().map_err(err)?, data)
}

pub fn save(
    root: &Path,
    installed: &Path,
    data: &Path,
    baseline: &PackageProject,
    next: &PackageProject,
) -> Result<(), String> {
    let _lock = WRITES.lock().map_err(err)?;
    let root = root.canonicalize().map_err(err)?;
    if installed.canonicalize().is_ok_and(|p| root.starts_with(p)) {
        return Err("installed packages are read-only".into());
    }
    recover_inner(&root, data)?;
    next.check_budget().map_err(err)?;
    if read(&root, installed)?.files != baseline.files {
        return Err("disk files changed; reload and resolve conflicts before saving".into());
    }
    let keys: BTreeSet<_> = baseline
        .files
        .keys()
        .chain(next.files.keys())
        .cloned()
        .collect();
    let changes = keys
        .into_iter()
        .filter(|p| baseline.files.get(p) != next.files.get(p))
        .map(|p| {
            let hash = next
                .files
                .get(&p)
                .map(|b| blake3::hash(b).to_hex().to_string());
            (p, hash)
        })
        .collect::<Vec<_>>();
    if baseline.files.get(nanatype_theme::INTEGRITY_FILE)
        == next.files.get(nanatype_theme::INTEGRITY_FILE)
        && let Some(seal) = nanatype_theme::verify_integrity(baseline).map_err(err)?
        && (baseline.files.keys().ne(next.files.keys())
            || seal
                .protected
                .iter()
                .any(|p| baseline.files.get(p) != next.files.get(p)))
    {
        return Err("signed file inventory and protected files are read-only".into());
    }
    if changes.iter().any(|(p, _)| p.starts_with("dependencies/")) {
        // Dependency selection may replace the bundled closure. Reconstruct it
        // from the acknowledged bytes and the new declaration, never from the
        // caller's potentially edited dependency payloads.
        let mut expected = baseline.clone();
        let manifest = next
            .files
            .get(PACKAGE_MANIFEST)
            .ok_or("missing package.toml")?;
        expected
            .files
            .insert(PACKAGE_MANIFEST.into(), manifest.clone());
        let registry = nanatype_theme::SkinRegistry::new(
            Some(installed.to_owned()),
            env!("CARGO_PKG_VERSION"),
        );
        nanatype_theme::editor::bundle_dependencies(&mut expected, &registry).map_err(err)?;
        let dependencies = |project: &PackageProject| {
            project
                .files
                .iter()
                .filter(|(p, _)| p.starts_with("dependencies/"))
                .map(|(p, b)| (p.clone(), b.clone()))
                .collect::<std::collections::BTreeMap<_, _>>()
        };
        if dependencies(next) != dependencies(&expected) {
            return Err("bundled dependencies are read-only; use dependency selection".into());
        }
    }
    let mut changes = changes;
    changes.sort_by_key(|(_, after)| after.is_some());
    let txn = transaction(&root, data);
    std::fs::create_dir_all(&txn).map_err(err)?;
    baseline.write_zip(&txn.join("before.ntpkg")).map_err(err)?;
    let mut journal = Journal {
        root: root.clone(),
        committed: false,
        changes,
    };
    write_file(
        &txn.join("intent.json"),
        &serde_json::to_vec(&journal).map_err(err)?,
    )?;
    let result = (|| {
        for (name, _) in &journal.changes {
            // Compare immediately before each replacement as well as at transaction start.
            let target = destination(&root, name)?;
            let actual = current(&target)?;
            if actual.as_ref() != baseline.files.get(name) {
                return Err(format!("file changed during save: {name}"));
            }
            put(&root, name, next.files.get(name))?;
        }
        journal.committed = true;
        write_file(
            &txn.join("intent.json"),
            &serde_json::to_vec(&journal).map_err(err)?,
        )?;
        Ok(())
    })();
    if let Err(e) = result {
        let rollback = rollback(&root, &txn, &journal);
        return Err(match rollback {
            Ok(()) => e,
            Err(r) => format!("{e}\n{r}"),
        });
    }
    // The commit marker makes cleanup failure safe to recover on the next open.
    let _ = std::fs::remove_dir_all(txn);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn interrupted() -> (tempfile::TempDir, PathBuf, PathBuf, PackageProject) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let data = dir.path().join("data");
        let mut before = nanatype_theme::editor::template(
            nanatype_theme::editor::ProjectKind::Skin,
            "editor.interrupted",
        )
        .unwrap();
        before
            .files
            .insert("notes.txt".into(), b"original".to_vec());
        for (p, b) in &before.files {
            std::fs::write(root.join(p), b).unwrap();
        }
        let txn = transaction(&root, &data);
        std::fs::create_dir_all(&txn).unwrap();
        before.write_zip(&txn.join("before.ntpkg")).unwrap();
        let journal = Journal {
            root: root.clone(),
            committed: false,
            changes: vec![(
                "notes.txt".into(),
                Some(blake3::hash(b"new").to_hex().to_string()),
            )],
        };
        write_file(
            &txn.join("intent.json"),
            &serde_json::to_vec(&journal).unwrap(),
        )
        .unwrap();
        std::fs::write(root.join("notes.txt"), b"new").unwrap();
        (dir, root, data, before)
    }
    #[test]
    fn interrupted_write_rolls_back_before_open() {
        let (_dir, root, data, before) = interrupted();
        recover(&root, &data).unwrap();
        assert_eq!(
            read(&root, &data.join("installed")).unwrap().files,
            before.files
        );
        assert!(!transaction(&root, &data).exists());
    }
    #[test]
    fn interrupted_write_keeps_external_changes_and_journal() {
        let (_dir, root, data, _) = interrupted();
        std::fs::write(root.join("notes.txt"), "external after crash").unwrap();
        assert!(recover(&root, &data).is_err());
        assert_eq!(
            std::fs::read(root.join("notes.txt")).unwrap(),
            b"external after crash"
        );
        assert!(transaction(&root, &data).join("intent.json").exists());
    }
    #[cfg(unix)]
    #[test]
    fn directory_projects_never_follow_symbolic_links() {
        let (dir, root, data, _) = interrupted();
        recover(&root, &data).unwrap();
        let external = dir.path().join("external.txt");
        std::fs::write(&external, b"private").unwrap();
        std::os::unix::fs::symlink(external, root.join("link.txt")).unwrap();
        assert!(read(&root, &data.join("installed")).is_err());
    }
}
