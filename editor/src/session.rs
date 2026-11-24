//! Package editor sessions: drafts, cached manifest/pipeline state, signing,
//! undo/redo history, and isolated Lua debugging. Views read [`EditorView`] snapshots.
use crate::history::{Change, Forms, History};
use nanatype_plugins::WorkerResponse;
use nanatype_theme::editor::{
    self, DraftInfo, DraftOrigin, FileInfo, Metadata, ProjectKind, SourceDiagnostic,
};
use nanatype_theme::{
    DataSpec, MANIFEST, PACKAGE_MANIFEST, PackageCapability, PackageManifest, PackageProject,
    PipelineSpec, SOUNDS_MANIFEST, SealIdentity, SealStatus, SkinFiles, SkinRegistry,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

pub use crate::debugger::{DebugView, Debugger, sample_events};
pub use crate::keys::{KeyStore, MemoryKeys, SecretKeys};
pub use crate::validation::{Report, validate, validate_with_worker};

static NEXT: AtomicU64 = AtomicU64::new(1);
fn next() -> u64 {
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// The workspace the activity rail shows. A mode the package lacks the
/// capability for falls back to [`Mode::Files`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Layers, the live canvas and the inspector.
    #[default]
    Design,
    Files,
    Sounds,
    /// Data declarations and pipelines.
    Data,
    Lua,
    Package,
    Signing,
}

impl Mode {
    pub const ALL: [Mode; 7] = [
        Mode::Design,
        Mode::Files,
        Mode::Sounds,
        Mode::Data,
        Mode::Lua,
        Mode::Package,
        Mode::Signing,
    ];

    /// Whether the open package has anything for this mode to edit.
    pub fn available(self, view: &EditorView) -> bool {
        let has = |c| {
            view.metadata
                .as_ref()
                .is_some_and(|m| m.capabilities.contains(&c))
        };
        match self {
            Mode::Design => has(PackageCapability::Skin),
            Mode::Sounds => has(PackageCapability::Skin) || has(PackageCapability::Sounds),
            Mode::Lua => has(PackageCapability::Lua),
            Mode::Files | Mode::Data | Mode::Package | Mode::Signing => true,
        }
    }

    /// `self`, or Files when the package cannot use it.
    pub fn effective(self, view: &EditorView) -> Mode {
        if self.available(view) {
            self
        } else {
            Mode::Files
        }
    }
}

/// Everything the editor views render.
#[derive(Clone)]
pub struct EditorView {
    pub conflicts: Arc<BTreeSet<String>>,
    pub documents: Arc<BTreeMap<String, String>>,
    pub properties: Arc<nanatype_theme::TomlTable>,
    pub session: u64,
    pub version: u64,
    pub forms: Arc<Forms>,
    pub undo: bool,
    pub redo: bool,
    pub saving: bool,
    pub preview_stale: bool,
    pub package: String,
    pub kind: ProjectKind,
    pub origin: DraftOrigin,
    pub derived_from: String,
    pub dirty: bool,
    pub can_revert: bool,
    pub stored: bool,
    pub files: Arc<Vec<FileInfo>>,
    pub selected: Option<String>,
    /// Text of the selected file when it is UTF-8 and small enough to show.
    pub text: Option<Arc<str>>,
    pub editable: bool,
    /// Bumps whenever the program (not typing) replaces the shown text.
    pub revision: u64,
    /// Requests the source editor to select this byte range.
    pub jump: Option<(u64, usize, usize)>,
    pub syntax: Arc<Vec<SourceDiagnostic>>,
    pub report: Arc<Report>,
    pub validating: bool,
    pub seal: Result<Option<SealStatus>, String>,
    /// Fingerprint of the host signing key, once loaded.
    pub key: Option<String>,
    pub key_checked: bool,
    pub metadata: Option<Metadata>,
    /// `[data.<id>]` as declared, including entries whose file is missing.
    pub data: Arc<BTreeMap<String, DataSpec>>,
    /// `[pipelines.<name>]`, or why a declaration does not parse.
    pub pipelines: Arc<Vec<(String, Result<PipelineSpec, String>)>>,
    pub debug: DebugView,
    /// Close was requested with unsaved changes.
    pub confirm_close: bool,
}

/// One open project.
pub struct Session {
    pub id: u64,
    pub version: u64,
    pub forms: Forms,
    history: History,
    dirty_files: BTreeSet<String>,
    pub conflicts: BTreeSet<String>,
    pub saving: bool,
    pub last_changed: u64,
    pub recovered_version: u64,
    pub recovering: bool,
    recovery_epoch: Arc<AtomicU64>,
    preview_stale: bool,
    pub info: DraftInfo,
    pub project: PackageProject,
    /// The content last saved as a draft (or as opened).
    saved: PackageProject,
    /// Latest acknowledged disk snapshot, independent of the manual save point.
    disk: Option<PackageProject>,
    identity: Option<String>,
    stored: bool,
    selected: Option<String>,
    revision: u64,
    jump: Option<(u64, usize, usize)>,
    files: Arc<Vec<FileInfo>>,
    seal: Result<Option<SealStatus>, String>,
    report: Arc<Report>,
    ticket: u64,
    validating: bool,
    stale: bool,
    debugger: Debugger,
    confirm_close: bool,
    // Incremental caches refreshed only when the corresponding file changes.
    manifest: Option<PackageManifest>,
    metadata: Option<Metadata>,
    data: Arc<BTreeMap<String, DataSpec>>,
    pipelines: Arc<Vec<(String, Result<PipelineSpec, String>)>>,
    documents: Arc<BTreeMap<String, String>>,
}

pub fn now_s() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn draft_id(package: &str) -> String {
    format!("{package}-{}-{}", now_s(), next())
}

/// Compare a single file across two snapshots using cached BLAKE3 digests when
/// lengths match.
fn file_differs(a: &PackageProject, b: &PackageProject, path: &str) -> bool {
    match (a.files.get(path), b.files.get(path)) {
        (None, None) => false,
        (Some(ab), Some(bb)) => {
            ab.len() != bb.len() || a.cached_hash(path).ok() != b.cached_hash(path).ok()
        }
        _ => true,
    }
}

fn compute_dirty_files(project: &PackageProject, baseline: &PackageProject) -> BTreeSet<String> {
    project
        .files
        .keys()
        .chain(baseline.files.keys())
        .filter(|p| file_differs(project, baseline, p))
        .cloned()
        .collect()
}

fn build_documents(project: &PackageProject) -> Arc<BTreeMap<String, String>> {
    Arc::new(
        [PACKAGE_MANIFEST, MANIFEST, SOUNDS_MANIFEST]
            .into_iter()
            .filter_map(|p| {
                project
                    .files
                    .get(p)
                    .and_then(|b| std::str::from_utf8(b).ok())
                    .map(|t| (p.into(), t.into()))
            })
            .collect(),
    )
}

fn inline_pipelines(project: &PackageProject) -> Arc<Vec<(String, Result<PipelineSpec, String>)>> {
    if project
        .files
        .get(MANIFEST)
        .is_some_and(|b| b.len() <= 64 * 1024)
    {
        Arc::new(editor::pipelines(project))
    } else {
        Arc::default()
    }
}

/// Opening never runs anything: it reads, checks budgets and lists files.
pub fn open_installed(registry: &SkinRegistry, id: &str) -> Result<Session, String> {
    let files = registry.package_files(id).map_err(|e| e.to_string())?;
    let project = PackageProject::capture(&*files).map_err(|e| e.to_string())?;
    let origin = DraftOrigin::Installed { id: id.into() };
    Ok(Session::new(id, origin, project, false))
}

/// An unsigned derived copy of an installed or built-in package.
pub fn copy_installed(registry: &SkinRegistry, id: &str, new_id: &str) -> Result<Session, String> {
    let mut session = open_installed(registry, id)?;
    session.derive(new_id)?;
    Ok(session)
}

pub fn open_archive(path: &Path) -> Result<Session, String> {
    let project = editor::read_archive(path).map_err(|e| e.to_string())?;
    let package = project
        .manifest()
        .map_or_else(|_| "package".into(), |m| m.meta.id);
    let origin = DraftOrigin::Archive { path: path.into() };
    Ok(Session::new(&package, origin, project, false))
}

pub fn open_directory(path: &Path, installed: &Path, data: &Path) -> Result<Session, String> {
    let root = path.canonicalize().map_err(|e| e.to_string())?;
    if installed
        .canonicalize()
        .ok()
        .is_some_and(|p| root.starts_with(p))
    {
        return Err("installed packages are read-only".into());
    }
    crate::directory::recover(&root, data)?;
    let project = crate::directory::read(&root, installed)?;
    let package = project
        .manifest()
        .map_or_else(|_| "package".into(), |m| m.meta.id);
    Ok(Session::new(
        &package,
        DraftOrigin::Directory { path: root },
        project,
        false,
    ))
}

pub fn create(kind: ProjectKind, id: &str) -> Result<Session, String> {
    let project = editor::template(kind, id).map_err(|e| e.to_string())?;
    Ok(Session::new(id, DraftOrigin::New, project, true))
}

pub fn open_draft(dir: &Path, id: &str) -> Result<Session, String> {
    let (info, project) = editor::load_draft(dir, id).map_err(|e| e.to_string())?;
    let mut session = Session::new(&info.package, info.origin.clone(), project, false);
    session.info = info;
    session.stored = true;
    Ok(session)
}

/// A derived project opens through the same unsaved transition as other projects.
pub fn derived(project: &PackageProject, id: &str) -> Result<Session, String> {
    let from = project
        .manifest()
        .map(|m| format!("{}@{}", m.meta.id, m.meta.version))
        .map_err(|e| e.to_string())?;
    let copy = editor::derive_copy(project, id).map_err(|e| e.to_string())?;
    Ok(Session::new(id, DraftOrigin::Copy { from }, copy, true))
}

/// A free `<id>-copy[-n]` package ID.
pub fn copy_id(registry: &SkinRegistry, id: &str) -> String {
    let taken: BTreeSet<String> = registry
        .packages()
        .into_iter()
        .map(|(m, _)| m.meta.id)
        .collect();
    let stem: String = id.chars().take(56).collect();
    (0..)
        .map(|n| match n {
            0 => format!("{stem}-copy"),
            n => format!("{stem}-copy-{n}"),
        })
        .find(|candidate| !taken.contains(candidate))
        .unwrap_or_default()
}

impl Session {
    /// `unsaved`: the content exists only in memory (a new project or copy),
    /// so closing it asks first.
    fn new(package: &str, origin: DraftOrigin, project: PackageProject, unsaved: bool) -> Self {
        let selected = project
            .files
            .contains_key(PACKAGE_MANIFEST)
            .then(|| PACKAGE_MANIFEST.to_owned());
        let manifest = project.manifest().ok();
        let metadata = manifest.as_ref().map(Metadata::of);
        let data = Arc::new(
            manifest
                .as_ref()
                .map(|m| m.data.clone())
                .unwrap_or_default(),
        );
        let pipelines = inline_pipelines(&project);
        let documents = build_documents(&project);
        let mut session = Self {
            id: next(),
            version: next(),
            forms: Forms::new(),
            history: History::default(),
            dirty_files: BTreeSet::new(),
            conflicts: BTreeSet::new(),
            saving: false,
            last_changed: 0,
            recovered_version: 0,
            recovering: false,
            recovery_epoch: Arc::new(AtomicU64::new(0)),
            preview_stale: false,
            info: DraftInfo {
                id: draft_id(package),
                package: package.into(),
                origin: origin.clone(),
                saved_at: 0,
            },
            identity: manifest.as_ref().map(|m| m.meta.id.clone()),
            disk: matches!(&origin, DraftOrigin::Directory { .. }).then(|| project.clone()),
            saved: if unsaved {
                PackageProject::default()
            } else {
                project.clone()
            },
            project,
            stored: matches!(&origin, DraftOrigin::Directory { .. }),
            selected,
            revision: next(),
            jump: None,
            files: Arc::default(),
            seal: Ok(None),
            report: Arc::default(),
            ticket: 0,
            validating: false,
            stale: true,
            debugger: Debugger::new(),
            confirm_close: false,
            manifest,
            metadata,
            data,
            pipelines,
            documents,
        };
        session.refresh();
        session
    }

    fn set_manifest_cache(&mut self, manifest: Option<PackageManifest>) {
        if let Some(ref m) = manifest {
            self.identity = Some(m.meta.id.clone());
        }
        self.metadata = manifest.as_ref().map(Metadata::of);
        self.data = Arc::new(
            manifest
                .as_ref()
                .map(|m| m.data.clone())
                .unwrap_or_default(),
        );
        self.manifest = manifest;
    }

    /// Recompute the file list, caches, and the seal after a structural change.
    fn refresh(&mut self) {
        self.set_manifest_cache(self.project.manifest().ok());
        self.pipelines = inline_pipelines(&self.project);
        self.documents = build_documents(&self.project);
        self.files = Arc::new(editor::inventory_unhashed(&self.project, &self.files));
        self.seal = nanatype_theme::verify_integrity(&self.project).map_err(|e| e.to_string());
        if self
            .selected
            .as_ref()
            .is_some_and(|p| !self.project.files.contains_key(p))
        {
            self.selected = Some(PACKAGE_MANIFEST.into());
            self.revision = next();
        }
        let baseline = self.disk.as_ref().unwrap_or(&self.saved);
        self.dirty_files = compute_dirty_files(&self.project, baseline);
        self.changed();
    }

    /// Content changed: revalidate, and revoke a debug trust bound to the old
    /// content (stopping its worker).
    fn changed(&mut self) {
        self.stale = true;
        self.preview_stale = true;
        self.version = next();
        self.last_changed = nanatype_frame::clock::now_ms();
        self.recovery_epoch.store(self.version, Ordering::Release);
        self.debugger.on_project_changed();
    }

    pub fn recovery_token(&self) -> Arc<AtomicU64> {
        self.recovery_epoch.clone()
    }

    pub fn cancel_recovery(&self) {
        self.recovery_epoch.store(0, Ordering::Release);
    }

    pub fn mark_recovery_clean(&mut self) -> Option<String> {
        if !self.dirty() && (self.recovered_version != 0 || self.recovering) {
            self.recovered_version = 0;
            self.recovering = false;
            self.recovery_epoch.store(0, Ordering::Release);
            Some(self.info.id.clone())
        } else {
            None
        }
    }

    pub fn dirty(&self) -> bool {
        !self.forms.is_empty() || !self.dirty_files.is_empty()
    }

    pub fn view(&self, key: Option<&SealIdentity>, key_checked: bool) -> EditorView {
        let (text, editable, format) = match self.selected.as_ref().and_then(|p| {
            let row = self.files.iter().find(|f| &f.path == p)?;
            let bytes = self.project.files.get(p)?;
            Some((row, bytes))
        }) {
            Some((row, bytes)) if row.kind.text() && bytes.len() <= editor::MAX_EDIT_BYTES => (
                std::str::from_utf8(bytes).ok().map(Arc::<str>::from),
                row.editable,
                row.data.as_ref().map(|(_, d)| d.format),
            ),
            _ => (None, false, None),
        };
        let mut syntax = match (&self.selected, &text) {
            (Some(path), Some(text)) if text.len() <= 64 * 1024 => {
                editor::check_source(path, text, format)
            }
            _ => Vec::new(),
        };
        if let Some(location) = self.debugger.location()
            && self.selected.as_ref() == Some(&location.path)
        {
            syntax.push(location.clone());
        }
        EditorView {
            conflicts: Arc::new(self.conflicts.clone()),
            documents: self.documents.clone(),
            properties: self.report.properties.clone(),
            session: self.id,
            version: self.version,
            forms: Arc::new(self.forms.clone()),
            undo: self.history.available().0,
            redo: self.history.available().1,
            saving: self.saving,
            preview_stale: self.preview_stale,
            package: self
                .manifest
                .as_ref()
                .map_or_else(|| self.info.package.clone(), |m| m.meta.id.clone()),
            kind: self
                .manifest
                .as_ref()
                .map_or(ProjectKind::Mixed, ProjectKind::of),
            origin: self.info.origin.clone(),
            derived_from: self
                .manifest
                .as_ref()
                .map(|m| m.derived_from.clone())
                .unwrap_or_default(),
            dirty: self.dirty(),
            can_revert: !self.saved.files.is_empty(),
            stored: self.stored,
            files: self.files.clone(),
            selected: self.selected.clone(),
            text,
            editable,
            revision: self.revision,
            jump: self.jump,
            syntax: Arc::new(syntax),
            report: self.report.clone(),
            validating: self.validating || self.stale,
            seal: self.seal.clone(),
            key: key.map(SealIdentity::fingerprint),
            key_checked,
            metadata: self.metadata.clone(),
            data: self.data.clone(),
            pipelines: if self
                .project
                .files
                .get(MANIFEST)
                .is_some_and(|b| b.len() > 64 * 1024)
            {
                self.report.pipelines.clone()
            } else {
                self.pipelines.clone()
            },
            debug: self.debugger.view(),
            confirm_close: self.confirm_close,
        }
    }

    pub(crate) fn selected_path(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    pub fn select(&mut self, path: &str) {
        if self.project.files.contains_key(path) && self.selected.as_deref() != Some(path) {
            self.selected = Some(path.into());
            self.revision = next();
            self.jump = None;
        }
    }

    /// Typing into an editable file. Rejected edits (protected files, budget)
    /// leave the project as it was.
    pub fn edit(&mut self, path: &str, text: &str) -> Result<(), String> {
        if self.project.files.get(path).map(Vec::as_slice) == Some(text.as_bytes()) {
            return Ok(());
        }
        if !self.files.iter().any(|f| f.path == path && f.editable) {
            return Err("file is protected; create an unsigned copy to edit it".into());
        }
        let parsed_manifest = if path == PACKAGE_MANIFEST {
            PackageManifest::parse(text).ok()
        } else {
            None
        };
        if let (Some(expected), Some(next_manifest)) = (&self.identity, &parsed_manifest)
            && expected != &next_manifest.meta.id
        {
            return Err("package ID changes require a derived copy".into());
        }
        let old_len = self.project.files.get(path).ok_or("missing file")?.len();
        let total = self
            .project
            .files
            .values()
            .map(|b| b.len() as u64)
            .sum::<u64>();
        if text.len() as u64 > nanatype_theme::MAX_FILE_BYTES
            || total - old_len as u64 + text.len() as u64 > nanatype_theme::MAX_PACKAGE_BYTES
        {
            return Err("project resource budget exceeded".into());
        }
        // The immutable inventory was verified when opened/refreshed. Replacing
        // an allowed text file does not change the signed policy or protected
        // bytes, so typing must not rehash all protected binary assets.
        let before = self
            .project
            .files
            .insert(path.into(), text.as_bytes().to_vec())
            .expect("checked existing file");
        self.project.invalidate_hashes();
        self.history.push(Change::text(
            path,
            before,
            text.as_bytes().to_vec(),
            self.forms.clone(),
        ));
        // Only this row's size and hash changed; the seal policy did not.
        if let Some(row) = Arc::make_mut(&mut self.files)
            .iter_mut()
            .find(|f| f.path == path)
        {
            row.bytes = text.len() as u64;
            row.hash.clear();
        }
        if path == PACKAGE_MANIFEST {
            self.set_manifest_cache(parsed_manifest);
            // Declarations may have changed; asset hashes have not.
            self.files = Arc::new(editor::inventory_unhashed(&self.project, &self.files));
        } else if path == MANIFEST {
            self.pipelines = inline_pipelines(&self.project);
        }
        if matches!(path, PACKAGE_MANIFEST | MANIFEST | SOUNDS_MANIFEST) {
            let mut docs = (*self.documents).clone();
            docs.insert(path.into(), text.into());
            self.documents = Arc::new(docs);
        }
        if self.project.files.get(path) == self.disk_baseline().files.get(path) {
            self.dirty_files.remove(path);
        } else {
            self.dirty_files.insert(path.into());
        }
        self.changed();
        Ok(())
    }

    pub fn reject_edit(&mut self) {
        self.revision = next();
    }

    /// Reformat the selected TOML or JSON file.
    pub fn replace_text(
        &mut self,
        needle: &str,
        replacement: &str,
        index: usize,
        all: bool,
    ) -> Result<(), String> {
        let path = self.selected.clone().ok_or("no source selected")?;
        let mut text = String::from_utf8(
            self.project
                .files
                .get(&path)
                .ok_or("missing source")?
                .clone(),
        )
        .map_err(|e| e.to_string())?;
        let matches = editor::find_matches(&text, needle);
        if matches.is_empty() {
            return Ok(());
        }
        if all {
            for (offset, len) in matches.into_iter().rev() {
                text.replace_range(offset..offset + len, replacement);
            }
        } else {
            let (offset, len) = matches[index % matches.len()];
            text.replace_range(offset..offset + len, replacement);
        }
        self.edit(&path, &text)?;
        self.revision = next();
        Ok(())
    }

    pub fn format(&mut self) -> Result<(), String> {
        let path = self.selected.clone().ok_or("no file selected")?;
        let text = String::from_utf8(self.project.read(&path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let ext = self
            .files
            .iter()
            .find(|f| f.path == path)
            .map(FileInfo::source_extension)
            .unwrap_or_default();
        let formatted = if ext == "json" {
            editor::format_json(&text)?
        } else if ext == "toml" {
            editor::format_toml(&text)?
        } else {
            return Err("only TOML and JSON can be formatted".into());
        };
        self.edit(&path, &formatted)?;
        self.revision = next();
        Ok(())
    }

    pub fn reveal(&mut self, diagnostic: &SourceDiagnostic) {
        self.select(&diagnostic.path);
        self.revision = next();
        self.jump = Some((
            next(),
            diagnostic.offset,
            diagnostic.offset + diagnostic.len,
        ));
    }

    pub fn import(&mut self, name: &str, bytes: Vec<u8>) -> Result<String, String> {
        let folder = self.import_folder(name);
        self.import_into(&folder, name, bytes)
    }

    /// Where an imported file goes: beside the selected one, else in the
    /// folder its kind is kept in.
    pub fn import_folder(&self, name: &str) -> String {
        let beside = self
            .selected
            .as_deref()
            .and_then(|p| p.rsplit_once('/'))
            .map(|(dir, _)| dir)
            .filter(|dir| !dir.starts_with("dependencies"));
        let extension = name
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();
        let kind = match extension.as_str() {
            "wav" | "ogg" | "mp3" | "flac" => "sounds",
            "ttf" | "otf" | "woff" | "woff2" => "fonts",
            _ => "assets",
        };
        // A sound dropped while an image is selected still lands with the sounds.
        match beside {
            Some(dir) if kind == "assets" || dir.split('/').any(|part| part == kind) => dir,
            _ => kind,
        }
        .to_owned()
    }

    pub fn import_into(
        &mut self,
        folder: &str,
        name: &str,
        bytes: Vec<u8>,
    ) -> Result<String, String> {
        let mut path = String::new();
        self.apply(|p| {
            path = editor::import_asset(p, folder, name, bytes)?;
            Ok(())
        })?;
        self.selected = Some(path.clone());
        self.revision = next();
        self.refresh();
        Ok(path)
    }

    pub fn remove(&mut self, path: &str) -> Result<(), String> {
        self.apply(|p| editor::remove_file(p, path))
    }

    pub fn apply_metadata(
        &mut self,
        metadata: &Metadata,
        registry: &SkinRegistry,
    ) -> Result<(), String> {
        self.apply(|p| {
            editor::apply_metadata(p, metadata)?;
            // A dependency that is neither bundled nor installed is reported
            // by validation; the manifest edit itself still applies.
            let _ = editor::bundle_dependencies(p, registry);
            Ok(())
        })
    }

    /// Add, change, rename or remove a `[data.<id>]` declaration. The
    /// project is unchanged unless the manifest still parses strictly.
    pub fn apply_data(
        &mut self,
        previous: Option<&str>,
        entry: Option<(&str, &DataSpec)>,
    ) -> Result<(), String> {
        self.apply(|p| editor::apply_data(p, previous, entry))
    }

    /// Add, change, rename or remove a `[pipelines.<name>]` declaration. The
    /// project is unchanged when the edit adds a pipeline error.
    pub fn apply_pipeline(
        &mut self,
        previous: Option<&str>,
        entry: Option<(&str, &PipelineSpec)>,
    ) -> Result<(), String> {
        self.apply(|p| editor::apply_pipeline(p, previous, entry))
    }

    fn apply(
        &mut self,
        f: impl FnOnce(&mut PackageProject) -> Result<(), nanatype_theme::FileError>,
    ) -> Result<(), String> {
        let before = self.project.clone();
        if let Err(e) = f(&mut self.project) {
            self.project = before;
            return Err(e.to_string());
        }
        if before.files == self.project.files {
            return Ok(());
        }
        self.history.push(Change::between(
            &before,
            &self.project,
            self.forms.clone(),
            self.forms.clone(),
        ));
        for row in Arc::make_mut(&mut self.files) {
            if before.files.get(&row.path) != self.project.files.get(&row.path) {
                row.hash.clear();
            }
        }
        self.revision = next();
        self.refresh();
        Ok(())
    }

    pub fn replace_project(&mut self, project: PackageProject) -> Result<(), String> {
        self.apply(|p| {
            *p = project;
            Ok(())
        })
    }

    pub fn sign(&mut self, key: &SealIdentity, protected: &BTreeSet<String>) -> Result<(), String> {
        self.apply(|p| p.seal(key, protected).map(|_| ()))
    }

    /// Drop a seal made with the host's own key so it can be edited and
    /// signed again. A seal from another key needs a derived copy instead.
    pub fn unseal(&mut self, key: Option<&SealIdentity>) -> Result<(), String> {
        let owner = matches!((&self.seal, key), (Ok(Some(seal)), Some(key)) if seal.fingerprint == key.fingerprint());
        if !owner {
            return Err("only the signer's own key can remove this signature".into());
        }
        self.apply(|p| {
            p.files.remove(nanatype_theme::INTEGRITY_FILE);
            p.invalidate_hashes();
            Ok(())
        })
    }

    /// Replace this session with an unsigned derived copy under `id`.
    pub fn derive(&mut self, id: &str) -> Result<(), String> {
        *self = derived(&self.project, id)?;
        Ok(())
    }

    pub fn save(&mut self, dir: &Path) -> Result<(), String> {
        self.ensure_forms_applied()?;
        self.info.saved_at = now_s();
        if let Some(m) = &self.manifest {
            self.info.package = m.meta.id.clone();
        }
        editor::save_draft(dir, &self.info, &self.project).map_err(|e| e.to_string())?;
        self.saved = self.project.clone();
        if self.directory().is_some() {
            self.disk = Some(self.project.clone());
        }
        self.dirty_files.clear();
        self.stored = true;
        self.confirm_close = false;
        Ok(())
    }

    /// Return to the last saved content.
    pub fn revert(&mut self) {
        if self.saved.files.is_empty() && !self.stored {
            return;
        }
        let saved = self.saved.clone();
        let _ = self.apply(|p| {
            *p = saved;
            Ok(())
        });
        self.conflicts.clear();
        self.applied_forms("");
    }

    pub fn request_close(&mut self) -> bool {
        if self.dirty() {
            self.confirm_close = true;
            false
        } else {
            true
        }
    }

    pub fn cancel_close(&mut self) {
        self.confirm_close = false;
    }

    /// A validation to run in the background, if one is due and none is running.
    pub fn validation_due(&mut self) -> Option<(u64, PackageProject)> {
        if !self.stale || self.validating {
            return None;
        }
        self.stale = false;
        self.validating = true;
        self.ticket = next();
        Some((self.ticket, self.project.clone()))
    }

    /// A finished validation; a newer edit makes another one due.
    pub fn validated(&mut self, ticket: u64, report: Arc<Report>) {
        if ticket != self.ticket {
            return;
        }
        self.validating = false;
        // An edit since this run started leaves it stale; the next run follows.
        if !self.stale {
            self.files = report.inventory.clone();
            self.preview_stale = report.errors() > 0;
            if self.preview_stale && self.report.skin.is_some() {
                self.report = Arc::new(Report {
                    pipelines: report.pipelines.clone(),
                    references: report.references.clone(),
                    properties: self.report.properties.clone(),
                    inventory: report.inventory.clone(),
                    skin: self.report.skin.clone(),
                    files: self.report.files.clone(),
                    urls: self.report.urls.clone(),
                    diagnostics: report.diagnostics.clone(),
                });
            } else {
                self.report = report;
            }
        }
    }

    pub fn directory(&self) -> Option<&Path> {
        if let DraftOrigin::Directory { path } = &self.info.origin {
            Some(path)
        } else {
            None
        }
    }

    pub fn disk_baseline(&self) -> &PackageProject {
        self.disk.as_ref().unwrap_or(&self.saved)
    }

    pub fn merge_disk(&mut self, disk: PackageProject) {
        let previous = self.conflicts.clone();
        let baseline = self.disk_baseline().clone();
        self.conflicts = crate::directory::merge(&baseline, &mut self.project, &disk);
        self.conflicts.extend(
            previous
                .into_iter()
                .filter(|p| self.project.files.get(p) != disk.files.get(p)),
        );
        self.disk = Some(disk);
        for row in Arc::make_mut(&mut self.files) {
            row.hash.clear();
        }
        self.revision = next();
        self.refresh();
    }

    pub fn resolve_conflict(&mut self, path: &str, keep: bool) -> Result<(), String> {
        if !self.conflicts.contains(path) {
            return Err("this conflict is no longer current".into());
        }
        if !keep {
            let bytes = self.disk_baseline().files.get(path).cloned();
            self.apply(|p| {
                if let Some(bytes) = bytes {
                    p.files.insert(path.into(), bytes);
                } else {
                    p.files.remove(path);
                }
                p.invalidate_hashes();
                Ok(())
            })?;
        }
        self.conflicts.remove(path);
        self.changed();
        Ok(())
    }

    pub fn ensure_forms_applied(&self) -> Result<(), String> {
        if let (Some(expected), Some(manifest)) = (&self.identity, &self.manifest)
            && *expected != manifest.meta.id
        {
            return Err("package ID changes require a derived copy".into());
        }
        if !self.conflicts.is_empty() {
            return Err("resolve disk conflicts before saving or exporting".into());
        }
        if self.forms.is_empty() {
            Ok(())
        } else {
            Err("Apply or discard the pending form changes before saving or exporting.".into())
        }
    }

    pub fn buffer(&mut self, key: String, value: Option<serde_json::Value>) {
        let before = self.forms.clone();
        if let Some(value) = value {
            self.forms.insert(key, value);
        } else {
            self.forms.remove(&key);
        }
        self.history.push(Change::between(
            &PackageProject::default(),
            &PackageProject::default(),
            before,
            self.forms.clone(),
        ));
        self.last_changed = nanatype_frame::clock::now_ms();
        self.version = next();
        self.recovery_epoch.store(self.version, Ordering::Release);
    }

    pub fn applied_forms(&mut self, prefix: &str) {
        self.discard_forms(prefix);
        self.history.after_forms(self.forms.clone());
    }

    pub fn discard_forms(&mut self, prefix: &str) {
        let count = self.forms.len();
        self.forms.retain(|k, _| !k.starts_with(prefix));
        self.revision = next();
        if count != self.forms.len() {
            self.version = next();
            self.last_changed = nanatype_frame::clock::now_ms();
            self.recovery_epoch.store(self.version, Ordering::Release);
        }
    }

    pub fn recovery_at(&self) -> Option<u64> {
        (self.dirty() && !self.recovering && self.recovered_version != self.version)
            .then_some(self.last_changed + 2000)
    }

    pub fn recovery(&self) -> crate::recovery::Snapshot {
        crate::recovery::Snapshot {
            info: self.info.clone(),
            forms: self.forms.clone(),
            conflicts: self.conflicts.clone(),
            project: self.project.clone(),
            saved: self.saved.clone(),
            disk: self.disk.clone(),
            identity: self.identity.clone(),
        }
    }

    pub fn restore(snapshot: crate::recovery::Snapshot) -> Self {
        let mut s = Self::new(
            &snapshot.info.package,
            snapshot.info.origin.clone(),
            snapshot.project,
            true,
        );
        s.info = snapshot.info;
        s.disk = snapshot.disk.or_else(|| {
            matches!(&s.info.origin, DraftOrigin::Directory { .. }).then(|| snapshot.saved.clone())
        });
        s.identity = snapshot.identity.or(s.identity.take());
        s.saved = snapshot.saved;
        s.forms = snapshot.forms;
        s.conflicts = snapshot.conflicts;
        s.stored = s.info.saved_at > 0 || matches!(&s.info.origin, DraftOrigin::Directory { .. });
        s.refresh();
        s
    }

    pub fn undo(&mut self) {
        for row in Arc::make_mut(&mut self.files) {
            row.hash.clear();
        }
        self.history.undo(&mut self.project, &mut self.forms);
        self.revision = next();
        self.refresh();
    }

    pub fn redo(&mut self) {
        for row in Arc::make_mut(&mut self.files) {
            row.hash.clear();
        }
        self.history.redo(&mut self.project, &mut self.forms);
        self.revision = next();
        self.refresh();
    }

    pub fn validation_at(&self) -> Option<u64> {
        (self.stale && !self.validating).then_some(
            if self.report.files.is_none() && self.ticket == 0 {
                0
            } else {
                self.last_changed + 300
            },
        )
    }

    pub fn saved_snapshot(&mut self, info: DraftInfo, project: PackageProject) {
        self.info = info;
        if self.directory().is_some() {
            self.disk = Some(project.clone());
        }
        self.saved = project;
        self.stored = true;
        self.saving = false;
        let baseline = self.disk.as_ref().unwrap_or(&self.saved);
        self.dirty_files = compute_dirty_files(&self.project, baseline);
        if !self.dirty() {
            self.confirm_close = false;
        }
    }

    // ---- isolated Lua debugging ---------------------------------------------

    pub fn trust(&mut self, trusted: bool) {
        self.debugger.trust(&self.project, trusted);
    }

    pub fn start_debug(
        &mut self,
        executable: &Path,
        post: impl Fn(u64, WorkerResponse) + Send + Sync + 'static,
    ) -> Result<(), String> {
        self.debugger.start(&self.project, executable, next(), post)
    }

    pub fn stop_debug(&mut self) {
        self.debugger.stop();
    }

    pub fn debug_event(&mut self, event: serde_json::Value) -> Result<(), String> {
        self.debugger.event(event)
    }

    pub fn debug_sequence(&mut self, events: Vec<serde_json::Value>) -> Result<(), String> {
        self.debugger.sequence(events)
    }

    pub fn clear_debug(&mut self) {
        self.debugger.clear();
    }

    pub fn debug_result(&mut self, generation: u64, response: &WorkerResponse) {
        self.debugger.result(&self.project, generation, response);
    }

    pub fn debugging(&self) -> bool {
        self.debugger.running()
    }
}

/// Validate the whole package and its dependency closure exactly like an
/// import, then write the `.ntpkg`. Nothing is written when a check fails.
pub fn export(
    registry: &SkinRegistry,
    project: &PackageProject,
    path: &Path,
) -> Result<PathBuf, String> {
    let mut project = project.clone();
    editor::bundle_dependencies(&mut project, registry).map_err(|e| e.to_string())?;
    registry.inspect_project(&project).map_err(|errors| {
        errors
            .iter()
            .map(|e| format!("{}: {}", e.field, e.message))
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    project.export(path).map_err(|e| e.to_string())?;
    Ok(path.to_owned())
}

/// Drafts and the signing key for one profile.
pub struct Editor {
    pub drafts: PathBuf,
    pub keys: Arc<dyn KeyStore>,
    /// Loaded only when the user asks to sign; kept in memory, never shown.
    pub key: Option<SealIdentity>,
    pub key_checked: bool,
    pub session: Option<Session>,
}

impl Editor {
    pub fn new(drafts: PathBuf, keys: Arc<dyn KeyStore>) -> Self {
        Self {
            drafts,
            keys,
            key: None,
            key_checked: false,
            session: None,
        }
    }
}

/// Requests from the editor views and results from its background work.
pub enum Action {
    ReloadDirectory,
    OpenFinished {
        ticket: u64,
        result: Result<Box<Session>, String>,
    },
    DebugSequence(Vec<serde_json::Value>),
    ReplaceText {
        needle: String,
        replacement: String,
        index: usize,
        all: bool,
    },
    CanvasList {
        element: String,
        operation: String,
        target: Vec<String>,
    },
    CanvasEdit(crate::canvas::Edit),
    CopyConflict(String),
    ConflictCopied {
        session: u64,
        version: u64,
        path: String,
        result: Result<(), String>,
    },
    CreateFile(String),
    RenameFile {
        from: String,
        to: String,
    },
    ReplaceFile,
    AssetPrepared {
        session: u64,
        version: u64,
        path: String,
        result: Result<Arc<crate::asset::Preview>, String>,
    },
    PlayAsset,
    /// Select a sound and play it once it is decoded.
    Audition(String),
    StopAudio,
    AudioStatus(String),
    DirectoryPoll(u64),
    DirectoryScanned {
        session: u64,
        version: u64,
        result: Result<PackageProject, String>,
    },
    ResolveConflict {
        path: String,
        keep: bool,
    },
    SetDeclaration {
        file: String,
        path: Vec<String>,
        value: Option<String>,
        unbind: bool,
    },
    TxFinished {
        session: u64,
        tx_id: u64,
        prefix: String,
        rename: Option<(crate::ui::DataTarget, crate::ui::DataTarget)>,
        refresh_asset: bool,
        result: Result<PackageProject, String>,
    },
    Recoveries(Vec<DraftInfo>),
    RestoreRecovery(String),
    DiscardRecovery(String),
    RecoveryWritten {
        session: u64,
        version: u64,
        result: Result<(), String>,
    },
    Undo,
    Redo,
    Buffer {
        session: u64,
        key: String,
        value: Option<serde_json::Value>,
    },
    DiscardForms(String),
    Exit,
    SaveFinished {
        session: u64,
        info: DraftInfo,
        project: PackageProject,
        result: Result<(), String>,
    },
    KeyLoaded(Result<Option<SealIdentity>, String>),
    Imported {
        session: u64,
        folder: String,
        name: String,
        result: Result<Vec<u8>, String>,
    },
    New {
        kind: ProjectKind,
        id: String,
    },
    OpenInstalled(String),
    CopyInstalled(String),
    OpenDraft(String),
    DiscardDraft(String),
    Opened(Result<Box<Session>, String>),
    Drafts(Vec<DraftInfo>),
    Close,
    /// Answer to the unsaved-changes prompt.
    ConfirmClose {
        save: bool,
    },
    CancelClose,
    Save,
    Revert,
    Export,
    Exported(Result<PathBuf, String>),
    Select(String),
    Edit {
        path: String,
        text: String,
    },
    Format,
    Reveal(SourceDiagnostic),
    Remove(String),
    Validated {
        ticket: u64,
        report: Arc<Report>,
    },
    ApplyMetadata(Metadata),
    /// Replace the declaration `previous` (or add one) with `entry`, or
    /// remove it when `entry` is `None`.
    ApplyData {
        previous: Option<String>,
        entry: Option<(String, DataSpec)>,
    },
    ApplyPipeline {
        previous: Option<String>,
        entry: Option<(String, PipelineSpec)>,
    },
    LoadKey,
    CreateKey,
    Sign(BTreeSet<String>),
    Unseal,
    Derive(String),
    Trust(bool),
    DebugStart,
    DebugStop,
    DebugEvent(serde_json::Value),
    DebugClear,
    DebugResult {
        generation: u64,
        response: WorkerResponse,
    },
}

impl Drop for Session {
    fn drop(&mut self) {
        self.cancel_recovery();
    }
}
