//! Package editor sessions: drafts, background validation, signing and an
//! isolated Lua debugger. Views read [`EditorView`] snapshots. Nothing here
//! changes the active skin, installed packages, consent or running plugins.
use crate::history::{Change, Forms, History};
use nanatype_plugins::{PluginUpdate, WorkerResponse};
use nanatype_theme::editor::{
    self, DraftInfo, DraftOrigin, FileInfo, Level, Metadata, ProjectKind, SourceDiagnostic,
};
use nanatype_theme::{
    DataSpec, PACKAGE_MANIFEST, PackageCapability, PackageProject, PipelineSpec, SealIdentity,
    SealStatus, Skin, SkinFiles, SkinRegistry,
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

static NEXT: AtomicU64 = AtomicU64::new(1);
fn next() -> u64 {
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Where the host keeps the package signing key. The key never enters a
/// package, a log, the resource interface or a plugin process.
pub trait KeyStore: Send + Sync {
    fn load(&self) -> Result<Option<SealIdentity>, String>;
    fn save(&self, identity: &SealIdentity) -> Result<(), String>;
}

/// The platform's protected storage (`crates/secrets`).
pub struct SecretKeys(nanatype_secrets::SecretStore);
impl SecretKeys {
    pub fn new(directory: &Path) -> Self {
        Self(nanatype_secrets::SecretStore::new(
            directory,
            "package-signing",
        ))
    }
}
impl KeyStore for SecretKeys {
    fn load(&self) -> Result<Option<SealIdentity>, String> {
        match self.0.load().map_err(|e| e.to_string())? {
            None => Ok(None),
            Some(bytes) => <[u8; 32]>::try_from(bytes.as_slice())
                .map(|b| Some(SealIdentity::from_bytes(b)))
                .map_err(|_| "stored signing key is malformed".to_owned()),
        }
    }
    fn save(&self, identity: &SealIdentity) -> Result<(), String> {
        self.0
            .save(&identity.secret_bytes())
            .map_err(|e| e.to_string())
    }
}

/// In-memory keys, for tests and platforms without protected storage.
#[derive(Default)]
pub struct MemoryKeys(Mutex<Option<[u8; 32]>>);
impl KeyStore for MemoryKeys {
    fn load(&self) -> Result<Option<SealIdentity>, String> {
        Ok(self
            .0
            .lock()
            .map_err(|e| e.to_string())?
            .map(SealIdentity::from_bytes))
    }
    fn save(&self, identity: &SealIdentity) -> Result<(), String> {
        *self.0.lock().map_err(|e| e.to_string())? = Some(identity.secret_bytes());
        Ok(())
    }
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

/// The result of one background validation: located diagnostics and, for a
/// valid skin, what the preview renders.
#[derive(Clone, Default)]
pub struct Report {
    pub pipelines: Arc<Vec<(String, Result<PipelineSpec, String>)>>,
    pub properties: Arc<nanatype_theme::TomlTable>,
    pub references: Arc<Vec<editor::Reference>>,
    pub inventory: Arc<Vec<FileInfo>>,
    pub diagnostics: Vec<SourceDiagnostic>,
    pub skin: Option<Arc<Skin>>,
    pub files: Option<Arc<dyn SkinFiles>>,
    pub urls: Arc<HashMap<String, String>>,
}
impl Report {
    pub fn errors(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.level == Level::Error)
            .count()
    }
}

/// Validate exactly like an import: syntax of every local text file, then
/// the dependency closure, signatures, skin and data. Nothing executes.
pub fn validate(registry: &SkinRegistry, project: &PackageProject) -> Report {
    let mut report = Report {
        pipelines: Arc::new(editor::pipelines(project)),
        references: Arc::new(editor::references(project)),
        inventory: Arc::new(editor::inventory(project)),
        ..Report::default()
    };
    let manifest = project.manifest().ok();
    for (path, bytes) in &project.files {
        if path.starts_with("dependencies/") {
            continue;
        }
        if let Ok(text) = std::str::from_utf8(bytes) {
            let format = manifest
                .as_ref()
                .and_then(|m| m.data.values().find(|d| d.path == *path))
                .map(|d| d.format);
            report
                .diagnostics
                .extend(editor::check_source(path, text, format));
        }
    }
    match registry.inspect_project(project) {
        Ok((files, skin)) => {
            if let Some(loaded) = skin {
                report
                    .diagnostics
                    .extend(loaded.warnings.iter().map(|w| editor::locate(project, w)));
                report.urls = Arc::new(nanatype_frame::assets::skin_preview_urls(
                    &loaded.skin,
                    &*files,
                ));
                report.properties = loaded.authored.clone();
                report.skin = Some(loaded.skin);
            }
            report.files = Some(files);
        }
        Err(errors) => {
            if report.diagnostics.iter().all(|d| d.level != Level::Error) {
                report
                    .diagnostics
                    .extend(errors.iter().map(|e| editor::locate(project, e)));
            }
        }
    }
    report
}

/// Import checks plus compiler diagnostics. User code is never evaluated here.
pub fn validate_with_worker(
    registry: &SkinRegistry,
    project: &PackageProject,
    executable: &Path,
) -> Report {
    let mut report = validate(registry, project);
    if let Some(entry) = project.manifest().ok().and_then(|m| m.lua.map(|l| l.entry))
        && let Some(source) = project
            .files
            .get(&entry)
            .and_then(|b| std::str::from_utf8(b).ok())
        && let Err(message) =
            nanatype_plugins::check_source_in_worker(executable, source.to_owned())
    {
        let line = message
            .split_once("\"plugin\"]:")
            .and_then(|(_, s)| s.split(':').next()?.parse::<usize>().ok())
            .unwrap_or(1)
            .min(source.lines().count().max(1));
        let offset = source
            .split_inclusive('\n')
            .take(line.saturating_sub(1))
            .map(str::len)
            .sum();
        report.diagnostics.push(SourceDiagnostic {
            path: entry,
            line,
            column: 1,
            offset,
            len: 1,
            level: Level::Error,
            message,
        });
    }
    report
}

/// What the debug panel shows. Its views never reach the real plugin slots.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DebugView {
    pub trusted: bool,
    pub running: bool,
    pub replaying: bool,
    pub update: Option<PluginUpdate>,
    pub error: Option<String>,
    /// Where in the entry file the error happened, when Lua says so.
    pub location: Option<SourceDiagnostic>,
    /// Recent events sent and results received, newest last.
    pub log: Vec<String>,
}

const LOG_LINES: usize = 40;

/// A script under test, in its own sandboxed worker process.
struct Debugger {
    /// The project digest the user trusted; any edit revokes it.
    trusted: Option<String>,
    generation: u64,
    process: Option<nanatype_frame::plugin_host::Worker>,
    view: DebugView,
    events: std::collections::VecDeque<serde_json::Value>,
}
impl Debugger {
    fn new() -> Self {
        Self {
            trusted: None,
            generation: 0,
            process: None,
            view: DebugView::default(),
            events: Default::default(),
        }
    }
    fn stop(&mut self) {
        self.process = None;
        self.generation = 0;
        self.view.running = false;
        self.view.replaying = false;
        self.events.clear();
    }
    fn log(&mut self, line: String) {
        self.view.log.push(line.chars().take(400).collect());
        let excess = self.view.log.len().saturating_sub(LOG_LINES);
        self.view.log.drain(..excess);
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
    pub recovery_epoch: Arc<AtomicU64>,
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
}

pub fn now_s() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn draft_id(package: &str) -> String {
    format!("{package}-{}-{}", now_s(), next())
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
            identity: project.manifest().ok().map(|m| m.meta.id),
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
        };
        session.refresh();
        session
    }

    /// Recompute the file list and the seal after a structural change.
    fn refresh(&mut self) {
        if let Ok(manifest) = self.project.manifest() {
            self.identity = Some(manifest.meta.id);
        }
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
        self.dirty_files = self
            .project
            .files
            .keys()
            .chain(self.disk_baseline().files.keys())
            .filter(|p| self.project.files.get(*p) != self.disk_baseline().files.get(*p))
            .cloned()
            .collect();
        self.changed();
    }

    /// Content changed: revalidate, and revoke a debug trust bound to the old
    /// content (stopping its worker).
    fn changed(&mut self) {
        self.debugger.view.location = None;
        self.debugger.view.error = None;
        self.stale = true;
        self.preview_stale = true;
        self.version = next();
        self.last_changed = nanatype_frame::clock::now_ms();
        self.recovery_epoch.store(self.version, Ordering::Release);
        // No rehash per keystroke: any change revokes the trust.
        if self.debugger.trusted.is_some() {
            self.debugger.trusted = None;
            if self.debugger.process.is_some() {
                self.debugger.stop();
                self.debugger.log("■ stopped: the package changed".into());
            }
        }
    }

    pub fn dirty(&self) -> bool {
        self.forms.keys().any(|k| !k.ends_with(".editing")) || !self.dirty_files.is_empty()
    }

    pub fn view(&self, key: Option<&SealIdentity>, key_checked: bool) -> EditorView {
        let manifest = self.project.manifest().ok();
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
        if let Some(location) = &self.debugger.view.location
            && self.selected.as_ref() == Some(&location.path)
        {
            syntax.push(location.clone());
        }
        EditorView {
            conflicts: Arc::new(self.conflicts.clone()),
            documents: Arc::new(
                [
                    PACKAGE_MANIFEST,
                    nanatype_theme::MANIFEST,
                    nanatype_theme::SOUNDS_MANIFEST,
                ]
                .into_iter()
                .filter_map(|p| {
                    self.project
                        .files
                        .get(p)
                        .and_then(|b| std::str::from_utf8(b).ok())
                        .map(|t| (p.into(), t.into()))
                })
                .collect(),
            ),
            properties: self.report.properties.clone(),
            session: self.id,
            version: self.version,
            forms: Arc::new(self.forms.clone()),
            undo: self.history.available().0,
            redo: self.history.available().1,
            saving: self.saving,
            preview_stale: self.preview_stale,
            package: manifest
                .as_ref()
                .map_or_else(|| self.info.package.clone(), |m| m.meta.id.clone()),
            kind: manifest
                .as_ref()
                .map_or(ProjectKind::Mixed, ProjectKind::of),
            origin: self.info.origin.clone(),
            derived_from: manifest
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
            metadata: manifest.as_ref().map(Metadata::of),
            data: Arc::new(
                manifest
                    .as_ref()
                    .map(|m| m.data.clone())
                    .unwrap_or_default(),
            ),
            pipelines: if self
                .project
                .files
                .get(nanatype_theme::MANIFEST)
                .is_some_and(|b| b.len() > 64 * 1024)
            {
                self.report.pipelines.clone()
            } else {
                Arc::new(editor::pipelines(&self.project))
            },
            debug: DebugView {
                trusted: self.debugger.trusted.is_some(),
                ..self.debugger.view.clone()
            },
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
        let identity = if path == PACKAGE_MANIFEST {
            nanatype_theme::PackageManifest::parse(text)
                .ok()
                .map(|m| m.meta.id)
        } else {
            None
        };
        if let (Some(expected), Some(next)) = (&self.identity, &identity)
            && expected != next
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
        if identity.is_some() {
            self.identity = identity;
        }
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
            // Declarations may have changed; asset hashes have not.
            self.files = Arc::new(editor::inventory_unhashed(&self.project, &self.files));
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
        let folder = self.import_folder();
        self.import_into(&folder, name, bytes)
    }

    pub fn import_folder(&self) -> String {
        self.selected
            .as_deref()
            .and_then(|p| p.rsplit_once('/'))
            .map(|(dir, _)| dir)
            .filter(|dir| !dir.starts_with("dependencies"))
            .unwrap_or("assets")
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
        if let Ok(m) = self.project.manifest() {
            self.info.package = m.meta.id;
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
                Ok(())
            })?;
        }
        self.conflicts.remove(path);
        self.changed();
        Ok(())
    }
    pub fn ensure_forms_applied(&self) -> Result<(), String> {
        if let (Some(expected), Ok(manifest)) = (&self.identity, self.project.manifest())
            && *expected != manifest.meta.id
        {
            return Err("package ID changes require a derived copy".into());
        }
        if !self.conflicts.is_empty() {
            return Err("resolve disk conflicts before saving or exporting".into());
        }
        if self.forms.keys().all(|k| k.ends_with(".editing")) {
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
        self.recovery_epoch.store(self.version, Ordering::Release);
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
        self.dirty_files = self
            .project
            .files
            .keys()
            .chain(self.disk_baseline().files.keys())
            .filter(|p| self.project.files.get(*p) != self.disk_baseline().files.get(*p))
            .cloned()
            .collect();
    }

    // ---- isolated Lua debugging ---------------------------------------------

    /// Trust is bound to this exact content and needs a fresh confirmation
    /// after any edit.
    pub fn trust(&mut self, trusted: bool) {
        self.debugger.trusted = trusted.then(|| self.project.digest());
        if !trusted {
            self.debugger.stop();
        }
    }

    /// Start the script in its own sandboxed worker. Data answers come from
    /// this snapshot only; results stay in the editor.
    pub fn start_debug(
        &mut self,
        executable: &Path,
        post: impl Fn(u64, WorkerResponse) + Send + Sync + 'static,
    ) -> Result<(), String> {
        let digest = self.project.digest();
        if self.debugger.trusted.as_deref() != Some(digest.as_str()) {
            return Err("confirm that you trust this script first".into());
        }
        let manifest = self.project.manifest().map_err(|e| e.to_string())?;
        let entry = manifest.entry().ok_or("the package has no Lua entry")?;
        let bytes = self.project.files.get(entry).ok_or("missing Lua source")?;
        if bytes.len() > nanatype_plugins::MAX_SCRIPT_BYTES {
            return Err("plugin source exceeds budget".into());
        }
        let source = String::from_utf8(bytes.clone()).map_err(|e| e.to_string())?;
        let data = Arc::new(
            nanatype_theme::DataAccess::for_plugin(&self.project, &manifest)
                .map_err(|e| e.to_string())?,
        );
        let reads: Vec<_> = data.disclosures().map(|d| d.reference.clone()).collect();
        let answer = nanatype_frame::plugin_host::data_answer(data);
        self.debugger.stop();
        let generation = next();
        self.debugger.process = Some(nanatype_frame::plugin_host::Worker::spawn(
            executable,
            source,
            serde_json::json!({"type": "init", "reads": reads}),
            answer,
            move |response| post(generation, response),
        )?);
        self.debugger.generation = generation;
        self.debugger.view = DebugView {
            running: true,
            log: std::mem::take(&mut self.debugger.view.log),
            ..Default::default()
        };
        self.debugger.log("▶ init".into());
        Ok(())
    }

    pub fn stop_debug(&mut self) {
        if self.debugger.process.is_some() {
            self.debugger.stop();
            self.debugger.log("■ stopped".into());
        }
    }

    pub fn debug_event(&mut self, event: serde_json::Value) -> Result<(), String> {
        if self.debugger.view.replaying {
            return Err("wait for the current replay to finish".into());
        }
        self.send_debug_event(event)
    }
    pub fn debug_sequence(&mut self, events: Vec<serde_json::Value>) -> Result<(), String> {
        if self.debugger.view.update.is_none() || !self.debugging() || self.debugger.view.replaying
        {
            return Err("wait for the debugger to be ready".into());
        }
        if events.is_empty()
            || events.len() > 32
            || events
                .iter()
                .any(|e| !e.get("type").is_some_and(|v| v.is_string()))
        {
            return Err("expected 1–32 event objects with a string type".into());
        }
        self.debugger.events = events.into();
        self.debugger.view.replaying = true;
        self.replay_next()
    }
    fn replay_next(&mut self) -> Result<(), String> {
        if let Some(event) = self.debugger.events.pop_front() {
            self.send_debug_event(event)
        } else {
            self.debugger.view.replaying = false;
            Ok(())
        }
    }
    fn send_debug_event(&mut self, event: serde_json::Value) -> Result<(), String> {
        let process = self
            .debugger
            .process
            .as_ref()
            .ok_or("the script is not running")?;
        let line = format!("→ {event}");
        process.event(event).map_err(|e| e.to_string())?;
        self.debugger.log(line);
        Ok(())
    }

    pub fn clear_debug(&mut self) {
        self.debugger.view.error = None;
        self.debugger.view.location = None;
        self.debugger.view.log.clear();
    }

    /// A result from the debug worker; stale generations are ignored.
    pub fn debug_result(&mut self, generation: u64, response: &WorkerResponse) {
        if generation == 0 || generation != self.debugger.generation {
            return;
        }
        match response {
            WorkerResponse::Ready => {}
            WorkerResponse::Update { update, .. } => match update.validate() {
                Ok(()) => {
                    self.debugger.log(format!(
                        "← {} view(s), {} command(s)",
                        update.views.len(),
                        update.commands.len()
                    ));
                    self.debugger.view.update = Some(update.clone());
                    if self.debugger.view.replaying
                        && let Err(e) = self.replay_next()
                    {
                        self.debug_failed(e);
                    }
                }
                Err(error) => self.debug_failed(error),
            },
            WorkerResponse::Error { message } => self.debug_failed(message.clone()),
            // What the script wrote with `nana.log`, every level: this is the
            // author's own console.
            WorkerResponse::Log { lines, dropped } => {
                for line in lines {
                    let level = ["debug", "info", "warn", "error"]
                        .get(usize::from(line.level))
                        .unwrap_or(&"error");
                    self.debugger.log(format!("▸ {level}: {}", line.message));
                }
                if *dropped > 0 {
                    self.debugger
                        .log(format!("▸ {dropped} more line(s) over the budget"));
                }
            }
        }
    }
    fn debug_failed(&mut self, message: String) {
        self.debugger.log(format!("✕ {message}"));
        self.debugger.view.location = self.lua_location(&message);
        self.debugger.view.error = Some(message);
        self.debugger.stop();
    }
    /// The script is loaded as the chunk `plugin`, so `[string "plugin"]:N:`
    /// is line N of the entry file.
    fn lua_location(&self, message: &str) -> Option<SourceDiagnostic> {
        let (_, rest) = message.split_once("\"plugin\"]:")?;
        // The worker wraps the source in one line: `local plugin=(function()`.
        let line = rest
            .split(':')
            .next()?
            .trim()
            .parse::<usize>()
            .ok()?
            .checked_sub(1)
            .filter(|l| *l > 0)?;
        let entry = self.project.manifest().ok()?.lua?.entry;
        let text = std::str::from_utf8(self.project.files.get(&entry)?).ok()?;
        // An unclosed block is reported on the wrapper's closing line.
        let line = line.min(text.split('\n').count());
        let offset: usize = text
            .split_inclusive('\n')
            .take(line.saturating_sub(1))
            .map(str::len)
            .sum();
        let len = text[offset.min(text.len())..]
            .find('\n')
            .unwrap_or(text.len() - offset.min(text.len()))
            .max(1);
        Some(SourceDiagnostic {
            path: entry,
            offset,
            len,
            line,
            column: 1,
            level: Level::Error,
            message: message.to_owned(),
        })
    }
    pub fn debugging(&self) -> bool {
        self.debugger.process.is_some()
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

/// Sample events the debug panel can send, as the host would produce them.
pub fn sample_events(text: &str) -> Vec<(&'static str, serde_json::Value)> {
    let markup = nanatype_markup::serialize(&nanatype_markup::parse(text).doc);
    let bridge = |e: nanatype_model::BridgeEvent| serde_json::to_value(e).unwrap_or_default();
    vec![
        (
            "line_start",
            bridge(nanatype_model::BridgeEvent::LineStart {
                id: 1,
                text: text.into(),
                markup,
            }),
        ),
        (
            "line_end",
            bridge(nanatype_model::BridgeEvent::LineEnd { id: 1 }),
        ),
        (
            "speaking",
            bridge(nanatype_model::BridgeEvent::Speaking { speaking: true }),
        ),
        (
            "chat_message",
            serde_json::json!({"type": "chat_message", "platform": "bilibili", "author": "viewer", "text": text}),
        ),
    ]
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
    ProjectPrepared {
        session: u64,
        version: u64,
        prefix: String,
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
        self.recovery_epoch.store(0, Ordering::Release);
    }
}
