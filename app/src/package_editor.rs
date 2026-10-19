//! Package editor sessions: drafts, background validation, signing and an
//! isolated Lua debugger. Views read [`EditorView`] snapshots. Nothing here
//! changes the active skin, installed packages, consent or running plugins.
use nanatype_plugins::{PluginProcess, PluginUpdate, WorkerResponse};
use nanatype_theme::editor::{
    self, DraftInfo, DraftOrigin, FileInfo, Level, Metadata, ProjectKind, SourceDiagnostic,
};
use nanatype_theme::{
    DataSpec, PACKAGE_MANIFEST, PackageProject, PipelineSpec, SealIdentity, SealStatus, Skin,
    SkinFiles, SkinRegistry,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Files,
    Package,
    Signing,
    Preview,
    Debug,
}

/// The result of one background validation: located diagnostics and, for a
/// valid skin, what the preview renders.
#[derive(Clone, Default)]
pub struct Report {
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
    let mut report = Report::default();
    for (path, bytes) in &project.files {
        if path.starts_with("dependencies/") {
            continue;
        }
        if let Ok(text) = std::str::from_utf8(bytes) {
            report.diagnostics.extend(editor::check_source(path, text));
        }
    }
    match registry.inspect_project(project) {
        Ok((files, skin)) => {
            if let Some(loaded) = skin {
                report
                    .diagnostics
                    .extend(loaded.warnings.iter().map(|w| editor::locate(project, w)));
                report.urls = Arc::new(crate::assets::skin_preview_urls(&loaded.skin, &*files));
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

/// What the debug panel shows. Its views never reach the real plugin slots.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DebugView {
    pub trusted: bool,
    pub running: bool,
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
    process: Option<PluginProcess>,
    view: DebugView,
}
impl Debugger {
    fn new() -> Self {
        Self {
            trusted: None,
            generation: 0,
            process: None,
            view: DebugView::default(),
        }
    }
    fn stop(&mut self) {
        self.process = None;
        self.generation = 0;
        self.view.running = false;
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
    pub package: String,
    pub kind: ProjectKind,
    pub origin: DraftOrigin,
    pub derived_from: String,
    pub dirty: bool,
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
    pub info: DraftInfo,
    pub project: PackageProject,
    /// The content last saved as a draft (or as opened).
    saved: PackageProject,
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
            info: DraftInfo {
                id: draft_id(package),
                package: package.into(),
                origin,
                saved_at: 0,
            },
            saved: if unsaved {
                PackageProject::default()
            } else {
                project.clone()
            },
            project,
            stored: false,
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
        self.files = Arc::new(editor::inventory(&self.project));
        self.seal = nanatype_theme::verify_integrity(&self.project).map_err(|e| e.to_string());
        if self
            .selected
            .as_ref()
            .is_some_and(|p| !self.project.files.contains_key(p))
        {
            self.selected = Some(PACKAGE_MANIFEST.into());
            self.revision = next();
        }
        self.changed();
    }

    /// Content changed: revalidate, and revoke a debug trust bound to the old
    /// content (stopping its worker).
    fn changed(&mut self) {
        self.stale = true;
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
        self.project.files != self.saved.files
    }

    pub fn view(&self, key: Option<&SealIdentity>, key_checked: bool) -> EditorView {
        let manifest = self.project.manifest().ok();
        let (text, editable) = match self.selected.as_ref().and_then(|p| {
            let row = self.files.iter().find(|f| &f.path == p)?;
            let bytes = self.project.files.get(p)?;
            Some((row, bytes))
        }) {
            Some((row, bytes)) if row.kind.text() && bytes.len() <= editor::MAX_EDIT_BYTES => {
                (std::str::from_utf8(bytes).ok().map(Arc::from), row.editable)
            }
            _ => (None, false),
        };
        let mut syntax = match (&self.selected, &text) {
            (Some(path), Some(text)) => editor::check_source(path, text),
            _ => Vec::new(),
        };
        if let Some(location) = &self.debugger.view.location
            && self.selected.as_ref() == Some(&location.path)
        {
            syntax.push(location.clone());
        }
        EditorView {
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
            pipelines: Arc::new(editor::pipelines(&self.project)),
            debug: DebugView {
                trusted: self.debugger.trusted.is_some(),
                ..self.debugger.view.clone()
            },
            confirm_close: self.confirm_close,
        }
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
        self.project
            .set(path, text.as_bytes().to_vec())
            .map_err(|e| e.to_string())?;
        // Only this row's size and hash changed; the seal policy did not.
        if let Some(row) = Arc::make_mut(&mut self.files)
            .iter_mut()
            .find(|f| f.path == path)
        {
            row.bytes = text.len() as u64;
            row.hash = blake3::hash(text.as_bytes()).to_hex().to_string();
        }
        if path == PACKAGE_MANIFEST {
            // Declarations may have changed; asset hashes have not.
            self.files = Arc::new(editor::inventory_reusing(&self.project, &self.files, path));
            self.seal = nanatype_theme::verify_integrity(&self.project).map_err(|e| e.to_string());
        }
        self.changed();
        Ok(())
    }

    /// Reformat the selected TOML or JSON file.
    pub fn format(&mut self) -> Result<(), String> {
        let path = self.selected.clone().ok_or("no file selected")?;
        let text = String::from_utf8(self.project.read(&path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let formatted = if path.ends_with(".json") {
            editor::format_json(&text)?
        } else if path.ends_with(".toml") {
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
        let folder = self
            .selected
            .as_deref()
            .and_then(|p| p.rsplit_once('/'))
            .map(|(dir, _)| dir)
            .filter(|dir| !dir.starts_with("dependencies"))
            .unwrap_or("assets")
            .to_owned();
        let path = editor::import_asset(&mut self.project, &folder, name, bytes)
            .map_err(|e| e.to_string())?;
        self.selected = Some(path.clone());
        self.revision = next();
        self.refresh();
        Ok(path)
    }

    pub fn remove(&mut self, path: &str) -> Result<(), String> {
        editor::remove_file(&mut self.project, path).map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
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
        f(&mut self.project).map_err(|e| e.to_string())?;
        self.revision = next();
        self.refresh();
        Ok(())
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
        let from = self
            .project
            .manifest()
            .map(|m| format!("{}@{}", m.meta.id, m.meta.version))
            .map_err(|e| e.to_string())?;
        let copy = editor::derive_copy(&self.project, id).map_err(|e| e.to_string())?;
        *self = Session::new(id, DraftOrigin::Copy { from }, copy, true);
        Ok(())
    }

    pub fn save(&mut self, dir: &Path) -> Result<(), String> {
        self.info.saved_at = now_s();
        if let Ok(m) = self.project.manifest() {
            self.info.package = m.meta.id;
        }
        editor::save_draft(dir, &self.info, &self.project).map_err(|e| e.to_string())?;
        self.saved = self.project.clone();
        self.stored = true;
        self.confirm_close = false;
        Ok(())
    }

    /// Return to the last saved content.
    pub fn revert(&mut self) {
        let saved = self.saved.clone();
        let _ = self.apply(|p| {
            *p = saved;
            Ok(())
        });
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
            self.report = report;
        }
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
        post: impl Fn(u64, WorkerResponse) + Send + 'static,
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
        let answer = crate::plugins::data_answer(data);
        self.debugger.stop();
        let generation = next();
        let process = PluginProcess::spawn(
            executable,
            source,
            serde_json::json!({"type": "init", "reads": reads}),
            answer,
            move |response| post(generation, response),
        )
        .map_err(|e| e.to_string())?;
        self.debugger.process = Some(process);
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
                }
                Err(error) => self.debug_failed(error),
            },
            WorkerResponse::Error { message } => self.debug_failed(message.clone()),
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
    let bridge = |e: nanatype_bridge::BridgeEvent| serde_json::to_value(e).unwrap_or_default();
    vec![
        (
            "line_start",
            bridge(nanatype_bridge::BridgeEvent::LineStart {
                id: 1,
                text: text.into(),
                markup,
            }),
        ),
        (
            "line_end",
            bridge(nanatype_bridge::BridgeEvent::LineEnd { id: 1 }),
        ),
        (
            "speaking",
            bridge(nanatype_bridge::BridgeEvent::Speaking { speaking: true }),
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
