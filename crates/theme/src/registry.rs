//! Built-in skins plus the user's skin folder: scan, load, install, export.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::{collections::BTreeMap, fs};

use toml::Table;

use crate::files::{DirFiles, FileError, MemoryFiles, SkinFiles, ZipFiles, clean_path};
use crate::load::{
    BaseResolver, LoadedSkin, MANIFEST, Severity, SkinDiagnostic, SkinOrigin, builtin_preview,
    load, read_manifest,
};
use crate::manifest::{Localized, Skin};
use crate::package::{NamespaceFiles, PackageFiles, PrefixFiles, qualify_resources};
use crate::{PACKAGE_MANIFEST, PackageCapability, PackageManifest};

pub const DEFAULT_SKIN: &str = "nanatype.halo";

struct Builtin {
    id: &'static str,
    files: &'static [(&'static str, &'static [u8])],
    package_json: &'static str,
    skin_json: &'static str,
    parsed: OnceLock<Arc<(Option<Table>, PackageManifest)>>,
}

include!(concat!(env!("OUT_DIR"), "/builtin_packages.rs"));

fn builtin_files(b: &Builtin) -> Arc<dyn SkinFiles> {
    let parsed = b
        .parsed
        .get_or_init(|| {
            Arc::new((
                serde_json::from_str(b.skin_json).expect("build-validated skin table"),
                serde_json::from_str(b.package_json).expect("build-validated package metadata"),
            ))
        })
        .clone();
    Arc::new(EmbeddedPackage {
        files: MemoryFiles::new(b.files),
        parsed,
    })
}

#[derive(Debug)]
struct EmbeddedPackage {
    files: MemoryFiles,
    parsed: Arc<(Option<Table>, PackageManifest)>,
}
impl SkinFiles for EmbeddedPackage {
    fn read(&self, path: &str) -> Result<Vec<u8>, FileError> {
        self.files.read(path)
    }
    fn file_len(&self, path: &str) -> Result<u64, FileError> {
        self.files.file_len(path)
    }
    fn list(&self) -> Vec<String> {
        self.files.list()
    }
    fn cached_skin_manifest(&self) -> Option<&Table> {
        self.parsed.0.as_ref()
    }
    fn cached_package_manifest(&self) -> Option<&PackageManifest> {
        Some(&self.parsed.1)
    }
}

/// One row of the skin picker.
#[derive(Debug, Clone)]
pub struct SkinEntry {
    pub id: String,
    pub version: String,
    pub name: Localized,
    pub author: String,
    pub origin: SkinOrigin,
    /// Errors that keep the skin from loading; empty when it loads.
    pub errors: Vec<SkinDiagnostic>,
    pub warnings: Vec<SkinDiagnostic>,
    /// Static catalogue model from the last scan. Built-in GPU programs are
    /// prepared on selection, not while displaying thumbnails.
    pub skin: Option<Arc<Skin>>,
    files: Option<Arc<dyn SkinFiles>>,
    prepared: Arc<OnceLock<Result<LoadedSkin, Vec<SkinDiagnostic>>>>,
}

impl SkinEntry {
    /// An already prepared model, without triggering effect preparation.
    pub fn prepared_skin(&self) -> Option<&Arc<Skin>> {
        self.prepared
            .get()?
            .as_ref()
            .ok()
            .map(|loaded| &loaded.skin)
    }

    /// The validated catalogue's resources, including inherited files.
    pub fn files(&self) -> Option<&dyn SkinFiles> {
        self.files.as_deref()
    }

    pub fn is_builtin(&self) -> bool {
        self.origin == SkinOrigin::Builtin
    }

    pub fn loads(&self) -> bool {
        self.errors.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallOutcome {
    Installed {
        id: String,
    },
    /// An older or equal version with the same id was replaced.
    Replaced {
        id: String,
        previous: String,
    },
}

#[derive(Clone)]
pub struct SkinRegistry {
    user_dir: Option<PathBuf>,
    app_version: String,
    entries: Vec<SkinEntry>,
}

struct ScopedResolver(Arc<dyn SkinFiles>);
impl BaseResolver for ScopedResolver {
    fn base(&self, id: &str) -> Option<(Table, Arc<dyn SkinFiles>)> {
        let files: Arc<dyn SkinFiles> = Arc::new(NamespaceFiles {
            files: self.0.clone(),
            id: id.into(),
        });
        let mut table = read_manifest(&*files).ok()?;
        qualify_resources(&mut table, id);
        Some((table, files))
    }
}

fn diagnostics(error: FileError) -> Vec<SkinDiagnostic> {
    vec![SkinDiagnostic {
        severity: Severity::Error,
        field: PACKAGE_MANIFEST.into(),
        message: error.to_string(),
    }]
}

pub(crate) fn bundled_path(path: &str) -> Result<String, FileError> {
    if let Some(rest) = path.strip_prefix("pkg:") {
        let (id, path) = rest
            .split_once('/')
            .ok_or_else(|| FileError::UnsafePath(path.into()))?;
        clean_path(&format!("dependencies/{id}/{path}"))
    } else {
        clean_path(path)
    }
}

impl SkinRegistry {
    pub fn new(user_dir: Option<PathBuf>, app_version: impl Into<String>) -> Self {
        let mut registry = Self {
            user_dir,
            app_version: app_version.into(),
            entries: Vec::new(),
        };
        registry.rescan();
        registry
    }

    pub fn user_dir(&self) -> Option<&Path> {
        self.user_dir.as_deref()
    }

    pub fn entries(&self) -> &[SkinEntry] {
        &self.entries
    }

    /// All installed package capabilities, including packages without a skin.
    pub fn packages(&self) -> Vec<(PackageManifest, SkinOrigin)> {
        let mut packages = Vec::new();
        for builtin in BUILTINS {
            if let Ok(package) = PackageManifest::read(&*builtin_files(builtin)) {
                packages.push((package, SkinOrigin::Builtin));
            }
        }
        if let Some(dir) = &self.user_dir
            && let Ok(entries) = fs::read_dir(dir)
        {
            let mut paths: Vec<_> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect();
            paths.sort();
            for path in paths {
                if let Ok(package) = PackageManifest::read(&DirFiles::new(&path))
                    && !BUILTINS.iter().any(|b| b.id == package.meta.id)
                {
                    packages.push((package, SkinOrigin::Directory(path)));
                }
            }
        }
        packages
    }

    pub fn package_files(&self, id: &str) -> Result<Arc<dyn SkinFiles>, FileError> {
        self.resolve_package(
            self.raw_files_for(id)
                .ok_or_else(|| FileError::Missing(id.into()))?,
        )
    }

    pub fn inspect_package_archive(
        &self,
        archive: &Path,
    ) -> Result<Arc<dyn SkinFiles>, Vec<SkinDiagnostic>> {
        let files = self
            .resolve_package(Arc::new(ZipFiles::open(archive).map_err(diagnostics)?))
            .map_err(diagnostics)?;
        let package = PackageManifest::read(&*files).map_err(diagnostics)?;
        if package.capabilities.contains(&PackageCapability::Skin) {
            let resolver = ScopedResolver(files.clone());
            load(
                files.clone(),
                SkinOrigin::Archive(archive.into()),
                &resolver,
                &self.app_version,
            )?;
        }
        if let Some(entry) = &package.entry {
            let source = files.read(entry).map_err(diagnostics)?;
            if source.len() > 128 * 1024
                || source.first() == Some(&0x1b)
                || std::str::from_utf8(&source).is_err()
            {
                return Err(diagnostics(FileError::Io(
                    "plugin entry must be bounded UTF-8 Lua source".into(),
                )));
            }
        }
        Ok(files)
    }

    pub fn entry(&self, id: &str) -> Option<&SkinEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    fn user_folder(&self, id: &str) -> Option<PathBuf> {
        if !crate::package::valid_id(id) {
            return None;
        }
        let dir = self.user_dir.as_ref()?.join(id);
        dir.join(PACKAGE_MANIFEST).is_file().then_some(dir)
    }

    fn raw_files_for(&self, id: &str) -> Option<Arc<dyn SkinFiles>> {
        if let Some(b) = BUILTINS.iter().find(|b| b.id == id) {
            return Some(builtin_files(b));
        }
        self.user_folder(id)
            .map(|dir| Arc::new(DirFiles::new(dir)) as Arc<dyn SkinFiles>)
    }

    fn files_for(&self, id: &str) -> Option<Arc<dyn SkinFiles>> {
        self.resolve_package(self.raw_files_for(id)?).ok()
    }

    /// Resolve a closed, exactly versioned dependency graph without network access.
    pub fn resolve_package(
        &self,
        root: Arc<dyn SkinFiles>,
    ) -> Result<Arc<dyn SkinFiles>, FileError> {
        fn resolve(
            registry: &SkinRegistry,
            files: Arc<dyn SkinFiles>,
            bundle: &Arc<dyn SkinFiles>,
            stack: &mut Vec<String>,
            versions: &mut BTreeMap<String, String>,
            all: &mut BTreeMap<String, Arc<dyn SkinFiles>>,
        ) -> Result<Arc<dyn SkinFiles>, FileError> {
            let package = PackageManifest::read(&*files)?;
            package.validate_content(&*files)?;
            if let (Some(required), Some(current)) = (
                crate::load::version_tuple(&package.meta.min_app),
                crate::load::version_tuple(&registry.app_version),
            ) && required > current
            {
                return Err(FileError::Io(format!(
                    "{} requires NanaType {}",
                    package.meta.id, package.meta.min_app
                )));
            }
            if versions.len() >= 64 {
                return Err(FileError::Io(
                    "package dependency graph exceeds 64 packages".into(),
                ));
            }
            let id = package.meta.id.clone();
            if stack.len() >= 16 || stack.contains(&id) {
                return Err(FileError::Io(format!(
                    "cyclic/deep package dependency {id}"
                )));
            }
            if let Some(version) = versions.insert(id.clone(), package.meta.version.clone()) {
                if version != package.meta.version {
                    return Err(FileError::Io(format!(
                        "conflicting package versions for {id}"
                    )));
                }
                if let Some(existing) = all.get(&id) {
                    return Ok(existing.clone());
                }
            }
            stack.push(id.clone());
            let mut dependencies = BTreeMap::new();
            for (dependency, version) in &package.dependencies {
                let prefix = format!("dependencies/{dependency}");
                let dep_files: Arc<dyn SkinFiles> =
                    if bundle.exists(&format!("{prefix}/{PACKAGE_MANIFEST}")) {
                        Arc::new(PrefixFiles {
                            files: bundle.clone(),
                            prefix,
                        })
                    } else {
                        registry.raw_files_for(dependency).ok_or_else(|| {
                            FileError::Missing(format!("dependency {dependency}@{version}"))
                        })?
                    };
                let meta = PackageManifest::read(&*dep_files)?;
                if &meta.meta.id != dependency || &meta.meta.version != version {
                    return Err(FileError::Io(format!(
                        "dependency {dependency} needs exactly {version}"
                    )));
                }
                let resolved = resolve(registry, dep_files, bundle, stack, versions, all)?;
                for path in resolved.list() {
                    if let Some((nested, _)) =
                        path.strip_prefix("pkg:").and_then(|p| p.split_once('/'))
                        && let Some(files) = all.get(nested)
                    {
                        dependencies.insert(nested.into(), files.clone());
                    }
                }
                dependencies.insert(dependency.clone(), resolved);
            }
            stack.pop();
            // A dependency's explicitly declared transitive closure is visible by ID.

            let resolved: Arc<dyn SkinFiles> = Arc::new(PackageFiles {
                manifest: package,
                local: files,
                dependencies,
            });
            all.insert(id, resolved.clone());
            Ok(resolved)
        }
        let mut packages = BTreeMap::new();
        let files = resolve(
            self,
            root.clone(),
            &root,
            &mut Vec::new(),
            &mut BTreeMap::new(),
            &mut packages,
        )?;
        let paths = files.list();
        if paths.len() > 4096 {
            return Err(FileError::Io(
                "package closure exceeds 4096 resources".into(),
            ));
        }
        let mut total = 0u64;
        for path in paths {
            let bytes = files.file_len(&path)?;
            if bytes > crate::MAX_FILE_BYTES {
                return Err(FileError::TooLarge { path, bytes });
            }
            total = total
                .checked_add(bytes)
                .ok_or_else(|| FileError::Io("package size overflow".into()))?;
            if total > crate::MAX_PACKAGE_BYTES {
                return Err(FileError::TooLarge {
                    path: "package dependency closure".into(),
                    bytes: total,
                });
            }
        }
        // Reject the complete closure's size before hashing any signed payloads.
        for package in packages.values() {
            crate::verify_integrity(&**package)?;
        }
        Ok(files)
    }

    /// Re-read the user folder. Built-ins always come first.
    pub fn rescan(&mut self) {
        let mut entries = Vec::new();
        for b in BUILTINS {
            // Embedded resources cannot change during this process. Keep both
            // their static catalogue and any lazily prepared GPU programs.
            entries.push(match self.entry(b.id) {
                Some(entry) => entry.clone(),
                None => self.describe(b.id.to_owned(), builtin_files(b), SkinOrigin::Builtin),
            });
        }
        if let Some(dir) = &self.user_dir
            && let Ok(read) = fs::read_dir(dir)
        {
            let mut folders: Vec<PathBuf> = read
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.join(MANIFEST).is_file())
                .collect();
            folders.sort();
            for folder in folders {
                let id = folder
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if BUILTINS.iter().any(|b| b.id == id) {
                    continue;
                }
                let files = Arc::new(DirFiles::new(folder.clone()));
                entries.push(self.describe(id, files, SkinOrigin::Directory(folder)));
            }
        }
        self.entries = entries;
    }

    fn describe(&self, id: String, files: Arc<dyn SkinFiles>, origin: SkinOrigin) -> SkinEntry {
        let result = self
            .resolve_package(files)
            .map_err(diagnostics)
            .and_then(|files| {
                let resolver = ScopedResolver(files.clone());
                if origin == SkinOrigin::Builtin {
                    builtin_preview(files, &resolver, &self.app_version)
                } else {
                    load(files, origin.clone(), &resolver, &self.app_version)
                }
            });
        match result {
            Ok(loaded) => {
                let prepared = Arc::new(OnceLock::new());
                if origin != SkinOrigin::Builtin {
                    let _ = prepared.set(Ok(loaded.clone()));
                }
                SkinEntry {
                    id: loaded.skin.meta.id.clone(),
                    version: loaded.skin.meta.version.clone(),
                    name: loaded.skin.meta.name.clone(),
                    author: loaded.skin.meta.author.clone(),
                    origin,
                    errors: Vec::new(),
                    warnings: loaded.warnings,
                    skin: Some(loaded.skin),
                    files: Some(loaded.files),
                    prepared,
                }
            }
            Err(errors) => SkinEntry {
                id,
                version: String::new(),
                name: Localized::new(),
                author: String::new(),
                origin,
                errors: errors
                    .iter()
                    .filter(|d| d.severity == Severity::Error)
                    .cloned()
                    .collect(),
                warnings: errors
                    .into_iter()
                    .filter(|d| d.severity == Severity::Warning)
                    .collect(),
                skin: None,
                files: None,
                prepared: Arc::new(OnceLock::new()),
            },
        }
    }

    pub fn load(&self, id: &str) -> Result<LoadedSkin, Vec<SkinDiagnostic>> {
        if let Some(entry) = self.entry(id).filter(|e| e.is_builtin()) {
            return entry
                .prepared
                .get_or_init(|| {
                    if !entry.loads() {
                        return Err(entry.errors.clone());
                    }
                    let files = entry.files.clone().expect("valid catalogue has files");
                    let skin = entry.skin.clone().expect("valid catalogue has a skin");
                    let mut warnings = entry.warnings.clone();
                    let skin = if skin.shaders.is_empty()
                        && skin.materials.is_empty()
                        && skin.dialogue.reveal.handwriting.is_none()
                    {
                        skin
                    } else {
                        // The immutable manifest/resources already passed the
                        // catalogue validator; prepare only its GPU payload now.
                        let mut resolved = (*skin).clone();
                        crate::handwriting::load_handwriting(
                            &mut resolved,
                            &*files,
                            &mut warnings,
                            false,
                        );
                        crate::shader::load_shaders(&mut resolved, &*files, &mut warnings);
                        crate::material::load_materials(&mut resolved, &*files, &mut warnings);
                        if warnings.iter().any(|d| d.severity == Severity::Error) {
                            warnings.sort_by_key(|d| std::cmp::Reverse(d.severity));
                            return Err(warnings);
                        }
                        Arc::new(resolved)
                    };
                    Ok(LoadedSkin {
                        skin,
                        files,
                        origin: SkinOrigin::Builtin,
                        warnings,
                    })
                })
                .clone();
        }
        let files = self.files_for(id).ok_or_else(|| {
            vec![SkinDiagnostic {
                severity: Severity::Error,
                field: "meta.id".into(),
                message: format!("skin `{id}` is not installed"),
            }]
        })?;
        let origin = match self.user_folder(id) {
            Some(dir) if !BUILTINS.iter().any(|b| b.id == id) => SkinOrigin::Directory(dir),
            _ => SkinOrigin::Builtin,
        };
        let resolver = ScopedResolver(files.clone());
        load(files, origin, &resolver, &self.app_version)
    }

    /// Reuse this scan's validated snapshot. UI selection and watcher reloads
    /// use this path; `load` explicitly revalidates mutable authoring folders.
    pub fn load_scanned(&self, id: &str) -> Result<LoadedSkin, Vec<SkinDiagnostic>> {
        if let Some(entry) = self.entry(id).filter(|e| !e.is_builtin()) {
            return entry.prepared.get().cloned().unwrap_or_else(|| {
                let mut diagnostics = entry.errors.clone();
                diagnostics.extend(entry.warnings.clone());
                Err(diagnostics)
            });
        }
        self.load(id)
    }

    /// The requested skin, freshly validated, or the default when it no longer loads.
    pub fn load_or_default(&self, id: &str) -> (LoadedSkin, Option<Vec<SkinDiagnostic>>) {
        self.with_default(self.load(id))
    }

    /// UI startup reuses the catalogue's validation, just like selection.
    pub fn load_scanned_or_default(&self, id: &str) -> (LoadedSkin, Option<Vec<SkinDiagnostic>>) {
        self.with_default(self.load_scanned(id))
    }

    fn with_default(
        &self,
        result: Result<LoadedSkin, Vec<SkinDiagnostic>>,
    ) -> (LoadedSkin, Option<Vec<SkinDiagnostic>>) {
        match result {
            Ok(skin) => (skin, None),
            Err(errors) => (
                self.load(DEFAULT_SKIN).expect("built-in skins always load"),
                Some(errors),
            ),
        }
    }

    /// Validate an archive without installing it.
    pub fn inspect_archive(&self, archive: &Path) -> Result<LoadedSkin, Vec<SkinDiagnostic>> {
        let files = ZipFiles::open(archive).map_err(|e| {
            vec![SkinDiagnostic {
                severity: Severity::Error,
                field: archive.display().to_string(),
                message: e.to_string(),
            }]
        })?;
        let files = self.resolve_package(Arc::new(files)).map_err(diagnostics)?;
        let resolver = ScopedResolver(files.clone());
        load(
            files,
            SkinOrigin::Archive(archive.to_path_buf()),
            &resolver,
            &self.app_version,
        )
    }

    /// Validate, then unpack into `<user_dir>/<id>`. The previous copy is
    /// replaced only after the new one is completely written.
    pub fn install(&mut self, archive: &Path) -> Result<InstallOutcome, Vec<SkinDiagnostic>> {
        let err = |field: &str, message: String| {
            vec![SkinDiagnostic {
                severity: Severity::Error,
                field: field.to_owned(),
                message,
            }]
        };
        let files = self.inspect_package_archive(archive)?;
        let package = PackageManifest::read(&*files).map_err(diagnostics)?;
        let id = package.meta.id.clone();
        if BUILTINS.iter().any(|b| b.id == id) {
            return Err(err("meta.id", format!("`{id}` is a built-in skin id")));
        }
        let Some(user_dir) = self.user_dir.clone() else {
            return Err(err("meta.id", "no user skin folder".into()));
        };
        let previous = self.entry(&id).map(|e| e.version.clone());
        let staging = user_dir.join(format!(".{id}.installing"));
        let _ = fs::remove_dir_all(&staging);
        let write = || -> Result<(), FileError> {
            for path in files.list() {
                let clean = bundled_path(&path)?;
                let bytes = files.read(&path)?;
                let target = staging.join(&clean);
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).map_err(|e| FileError::Io(e.to_string()))?;
                }
                fs::write(&target, bytes).map_err(|e| FileError::Io(e.to_string()))?;
            }
            Ok(())
        };
        if let Err(e) = write() {
            let _ = fs::remove_dir_all(&staging);
            return Err(err("install", e.to_string()));
        }
        let target = user_dir.join(&id);
        let backup = user_dir.join(format!(".{id}.previous"));
        let _ = fs::remove_dir_all(&backup);
        if target.exists() {
            fs::rename(&target, &backup).map_err(|e| err("install", e.to_string()))?;
        }
        if let Err(e) = fs::rename(&staging, &target) {
            let _ = fs::rename(&backup, &target);
            return Err(err("install", e.to_string()));
        }
        let _ = fs::remove_dir_all(&backup);
        self.rescan();
        Ok(match previous {
            Some(previous) => InstallOutcome::Replaced { id, previous },
            None => InstallOutcome::Installed { id },
        })
    }

    /// Write a complete `.ntpkg` archive including dependencies and notices.
    pub fn export(&self, id: &str, dest: &Path) -> Result<(), FileError> {
        let files = self.package_files(id)?;
        if files.exists(crate::INTEGRITY_FILE) {
            return crate::PackageProject::capture(&*files)?.export(dest);
        }
        let mut package = PackageManifest::read(&*files)?;
        let manifest = if package.capabilities.contains(&PackageCapability::Skin) {
            let loaded = self.load(id).map_err(|e| FileError::Io(format!("{e:?}")))?;
            package.meta.base = None;
            let mut compact = crate::load::standalone_manifest(&loaded.skin)
                .map_err(|e| FileError::Io(e.to_string()))?;
            compact.remove("meta");
            Some(toml::to_string(&compact).map_err(|e| FileError::Io(e.to_string()))?)
        } else {
            None
        };
        let file = fs::File::create(dest).map_err(|e| FileError::Io(e.to_string()))?;
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for path in files.list() {
            let bytes = if path == MANIFEST {
                manifest
                    .as_ref()
                    .map(|s| s.as_bytes().to_vec())
                    .unwrap_or(files.read(&path)?)
            } else if path == PACKAGE_MANIFEST {
                toml::to_string_pretty(&package)
                    .map_err(|e| FileError::Io(e.to_string()))?
                    .into_bytes()
            } else {
                files.read(&path)?
            };
            zip.start_file(bundled_path(&path)?, options)
                .map_err(|e| FileError::Io(e.to_string()))?;
            zip.write_all(&bytes)
                .map_err(|e| FileError::Io(e.to_string()))?;
        }
        zip.finish().map_err(|e| FileError::Io(e.to_string()))?;
        Ok(())
    }

    /// Create an editable user skin that extends `base` and applies `overrides`
    /// (a partial `skin.toml` table).
    pub fn duplicate(
        &mut self,
        base: &str,
        new_id: &str,
        name: &Localized,
        overrides: &Table,
    ) -> Result<PathBuf, Vec<SkinDiagnostic>> {
        let err = |message: String| {
            vec![SkinDiagnostic {
                severity: Severity::Error,
                field: "meta.id".into(),
                message,
            }]
        };
        let Some(user_dir) = self.user_dir.clone() else {
            return Err(err("no user skin folder".into()));
        };
        if self.entry(new_id).is_some() {
            return Err(err(format!("`{new_id}` already exists")));
        }
        let mut table = overrides.clone();
        let mut meta = Table::new();
        meta.insert("id".into(), new_id.into());
        meta.insert("version".into(), "1.0.0".into());
        meta.insert("base".into(), base.into());
        let mut names = Table::new();
        for (k, v) in name {
            names.insert(k.clone(), v.clone().into());
        }
        meta.insert("name".into(), names.into());
        table.insert("meta".into(), meta.into());
        if !crate::package::valid_id(new_id) {
            return Err(err("invalid package ID".into()));
        }
        let parent_files = self
            .files_for(base)
            .ok_or_else(|| err("missing base package".into()))?;
        let parent = PackageManifest::read(&*parent_files).map_err(|e| err(e.to_string()))?;
        let mut metadata: crate::Meta = table["meta"]
            .clone()
            .try_into()
            .map_err(|e: toml::de::Error| err(e.to_string()))?;
        metadata.schema = crate::SKIN_SCHEMA;
        let package = PackageManifest {
            format: crate::PACKAGE_FORMAT,
            meta: metadata,
            capabilities: vec![PackageCapability::Skin],
            dependencies: BTreeMap::from([(base.into(), parent.meta.version)]),
            catalogue_order: 0,
            entry: None,
            created_at: String::new(),
            licensed_to: String::new(),
        };
        table.remove("meta");
        let dir = user_dir.join(new_id);
        fs::create_dir_all(&dir).map_err(|e| err(e.to_string()))?;
        fs::write(
            dir.join(PACKAGE_MANIFEST),
            toml::to_string_pretty(&package).map_err(|e| err(e.to_string()))?,
        )
        .map_err(|e| err(e.to_string()))?;
        fs::write(
            dir.join(MANIFEST),
            toml::to_string(&table).unwrap_or_default(),
        )
        .map_err(|e| err(e.to_string()))?;
        self.rescan();
        match self.load(new_id) {
            Ok(_) => Ok(dir),
            Err(errors) => {
                let _ = fs::remove_dir_all(&dir);
                self.rescan();
                Err(errors)
            }
        }
    }

    /// Delete a user skin. Built-ins cannot be removed.
    pub fn remove(&mut self, id: &str) -> Result<(), FileError> {
        if let Some(dir) = &self.user_dir {
            for entry in fs::read_dir(dir)
                .map_err(|e| FileError::Io(e.to_string()))?
                .flatten()
            {
                let files = DirFiles::new(entry.path());
                if let Ok(package) = PackageManifest::read(&files)
                    && package.dependencies.contains_key(id)
                {
                    return Err(FileError::Io(format!(
                        "{} depends on {id}",
                        package.meta.id
                    )));
                }
            }
        }
        let dir = self
            .user_folder(id)
            .filter(|_| !BUILTINS.iter().any(|b| b.id == id))
            .ok_or_else(|| FileError::Missing(id.to_owned()))?;
        fs::remove_dir_all(dir).map_err(|e| FileError::Io(e.to_string()))?;
        self.rescan();
        Ok(())
    }
}
