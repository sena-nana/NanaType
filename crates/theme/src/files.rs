//! Where a skin's files come from: the binary, a folder, or a `.ntskin` zip.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

/// Largest single file a skin may carry.
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
/// Largest total unpacked size of a skin.
pub const MAX_PACKAGE_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileError {
    /// Absolute, `..`, drive-prefixed or otherwise escaping path.
    UnsafePath(String),
    Missing(String),
    TooLarge {
        path: String,
        bytes: u64,
    },
    Io(String),
}

impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsafePath(p) => write!(f, "unsafe path `{p}`"),
            Self::Missing(p) => write!(f, "missing file `{p}`"),
            Self::TooLarge { path, bytes } => write!(f, "`{path}` is too large ({bytes} bytes)"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for FileError {}

/// Normalize a manifest path: forward slashes, no leading `./`, and never
/// leaving the skin root.
pub fn clean_path(path: &str) -> Result<String, FileError> {
    let unsafe_path = || FileError::UnsafePath(path.to_owned());
    let normalized = path.replace('\\', "/");
    if normalized.is_empty() || normalized.starts_with('/') || normalized.contains(':') {
        return Err(unsafe_path());
    }
    let mut parts = Vec::new();
    for component in Path::new(&normalized).components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str().ok_or_else(unsafe_path)?),
            Component::CurDir => {}
            _ => return Err(unsafe_path()),
        }
    }
    if parts.is_empty() {
        return Err(unsafe_path());
    }
    Ok(parts.join("/"))
}

/// Read-only access to a skin's files by cleaned relative path.
pub trait SkinFiles: Send + Sync + std::fmt::Debug {
    fn read(&self, path: &str) -> Result<Vec<u8>, FileError>;
    fn exists(&self, path: &str) -> bool {
        self.read(path).is_ok()
    }
    /// A real directory on disk when there is one (for hot reload and "open folder").
    fn root(&self) -> Option<&Path> {
        None
    }
    /// All mutable layers contributing resources to this skin.
    fn watch_roots(&self) -> Vec<PathBuf> {
        self.root().map(Path::to_path_buf).into_iter().collect()
    }
    fn list(&self) -> Vec<String>;
}

#[derive(Debug, Default)]
pub struct MemoryFiles {
    files: BTreeMap<String, &'static [u8]>,
}

impl MemoryFiles {
    pub fn new(files: &[(&'static str, &'static [u8])]) -> Self {
        Self {
            files: files.iter().map(|(p, b)| ((*p).to_owned(), *b)).collect(),
        }
    }
}

impl SkinFiles for MemoryFiles {
    fn read(&self, path: &str) -> Result<Vec<u8>, FileError> {
        let path = clean_path(path)?;
        self.files
            .get(&path)
            .map(|b| b.to_vec())
            .ok_or(FileError::Missing(path))
    }

    fn list(&self) -> Vec<String> {
        self.files.keys().cloned().collect()
    }
}

#[derive(Debug)]
pub struct DirFiles {
    root: PathBuf,
}

impl DirFiles {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl SkinFiles for DirFiles {
    fn read(&self, path: &str) -> Result<Vec<u8>, FileError> {
        let path = clean_path(path)?;
        let root = fs::canonicalize(&self.root).map_err(|_| FileError::Missing(path.clone()))?;
        let full =
            fs::canonicalize(root.join(&path)).map_err(|_| FileError::Missing(path.clone()))?;
        if !full.starts_with(&root) {
            return Err(FileError::UnsafePath(path));
        }
        let meta = fs::metadata(&full).map_err(|_| FileError::Missing(path.clone()))?;
        if meta.len() > MAX_FILE_BYTES {
            return Err(FileError::TooLarge {
                path,
                bytes: meta.len(),
            });
        }
        fs::read(&full).map_err(|e| FileError::Io(e.to_string()))
    }

    fn root(&self) -> Option<&Path> {
        Some(&self.root)
    }

    fn list(&self) -> Vec<String> {
        fn walk(dir: &Path, prefix: &str, out: &mut Vec<String>) {
            let Ok(entries) = fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let rel = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}/{name}")
                };
                match entry.file_type() {
                    Ok(t) if t.is_dir() => walk(&entry.path(), &rel, out),
                    Ok(t) if t.is_file() => out.push(rel),
                    _ => {}
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.root, "", &mut out);
        out.sort();
        out
    }
}

/// A `.ntskin` archive read in place (preview before installing).
#[derive(Debug)]
pub struct ZipFiles {
    archive: Mutex<zip::ZipArchive<fs::File>>,
    /// Archives may wrap everything in one top-level folder.
    prefix: String,
}

impl ZipFiles {
    pub fn open(path: &Path) -> Result<Self, FileError> {
        let file = fs::File::open(path).map_err(|e| FileError::Io(e.to_string()))?;
        let mut archive = zip::ZipArchive::new(file).map_err(|e| FileError::Io(e.to_string()))?;
        let mut total = 0u64;
        let mut names = Vec::new();
        for i in 0..archive.len() {
            let entry = archive
                .by_index_raw(i)
                .map_err(|e| FileError::Io(e.to_string()))?;
            if entry.enclosed_name().is_none() {
                return Err(FileError::UnsafePath(entry.name().to_owned()));
            }
            if entry.size() > MAX_FILE_BYTES {
                return Err(FileError::TooLarge {
                    path: entry.name().to_owned(),
                    bytes: entry.size(),
                });
            }
            total += entry.size();
            names.push(entry.name().replace('\\', "/"));
        }
        if total > MAX_PACKAGE_BYTES {
            return Err(FileError::TooLarge {
                path: path.display().to_string(),
                bytes: total,
            });
        }
        let prefix = if names.iter().any(|n| n == "skin.toml") {
            String::new()
        } else {
            names
                .iter()
                .find_map(|n| {
                    n.strip_suffix("skin.toml")
                        .filter(|p| p.matches('/').count() == 1)
                })
                .unwrap_or_default()
                .to_owned()
        };
        Ok(Self {
            archive: Mutex::new(archive),
            prefix,
        })
    }
}

impl SkinFiles for ZipFiles {
    fn read(&self, path: &str) -> Result<Vec<u8>, FileError> {
        let path = clean_path(path)?;
        let mut archive = self.archive.lock().expect("zip lock");
        let mut entry = archive
            .by_name(&format!("{}{path}", self.prefix))
            .map_err(|_| FileError::Missing(path.clone()))?;
        let mut out = Vec::with_capacity(entry.size() as usize);
        entry
            .by_ref()
            .take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut out)
            .map_err(|e| FileError::Io(e.to_string()))?;
        if out.len() as u64 > MAX_FILE_BYTES {
            return Err(FileError::TooLarge {
                path,
                bytes: out.len() as u64,
            });
        }
        Ok(out)
    }

    fn list(&self) -> Vec<String> {
        let archive = self.archive.lock().expect("zip lock");
        let mut out: Vec<String> = archive
            .file_names()
            .filter_map(|n| n.strip_prefix(&self.prefix))
            .filter(|n| !n.ends_with('/') && !n.is_empty())
            .map(str::to_owned)
            .collect();
        out.sort();
        out
    }
}
