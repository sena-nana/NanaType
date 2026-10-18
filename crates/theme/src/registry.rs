//! Built-in skins plus the user's skin folder: scan, load, install, export.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use toml::Table;

use crate::files::{DirFiles, FileError, MemoryFiles, SkinFiles, ZipFiles, clean_path};
use crate::load::{
    BaseResolver, LoadedSkin, MANIFEST, Severity, SkinDiagnostic, SkinOrigin, builtin_preview,
    load, read_manifest,
};
use crate::manifest::{Localized, Skin};

pub const DEFAULT_SKIN: &str = "nanatype.halo";

struct Builtin {
    id: &'static str,
    manifest: &'static str,
}

const BUILTINS: &[Builtin] = &[
    Builtin {
        id: "nanatype.crayon",
        manifest: include_str!("../builtin/crayon.toml"),
    },
    Builtin {
        id: "nanatype.gothic",
        manifest: include_str!("../builtin/gothic.toml"),
    },
    Builtin {
        id: "nanatype.celestial",
        manifest: include_str!("../builtin/celestial.toml"),
    },
    Builtin {
        id: "nanatype.forest",
        manifest: include_str!("../builtin/forest.toml"),
    },
    Builtin {
        id: "nanatype.tidal",
        manifest: include_str!("../builtin/tidal.toml"),
    },
    Builtin {
        id: "nanatype.lantern",
        manifest: include_str!("../builtin/lantern.toml"),
    },
    Builtin {
        id: "nanatype.citrus",
        manifest: include_str!("../builtin/citrus.toml"),
    },
    Builtin {
        id: "nanatype.halo",
        manifest: include_str!("../builtin/halo.toml"),
    },
    Builtin {
        id: "nanatype.night",
        manifest: include_str!("../builtin/night.toml"),
    },
    Builtin {
        id: "nanatype.parchment",
        manifest: include_str!("../builtin/parchment.toml"),
    },
    Builtin {
        id: "nanatype.adv",
        manifest: include_str!("../builtin/adv.toml"),
    },
    Builtin {
        id: "nanatype.rpg",
        manifest: include_str!("../builtin/rpg.toml"),
    },
    Builtin {
        id: "nanatype.vanilla",
        manifest: include_str!("../builtin/vanilla.toml"),
    },
    Builtin {
        id: "nanatype.bubble",
        manifest: include_str!("../builtin/bubble.toml"),
    },
    Builtin {
        id: "nanatype.subtitle",
        manifest: include_str!("../builtin/subtitle.toml"),
    },
    Builtin {
        id: "nanatype.livechat",
        manifest: include_str!("../builtin/livechat.toml"),
    },
    Builtin {
        id: "nanatype.comic",
        manifest: include_str!("../builtin/comic.toml"),
    },
    Builtin {
        id: "nanatype.terminal",
        manifest: include_str!("../builtin/terminal.toml"),
    },
    Builtin {
        id: "nanatype.pixel",
        manifest: include_str!("../builtin/pixel.toml"),
    },
    Builtin {
        id: "nanatype.sakura",
        manifest: include_str!("../builtin/sakura.toml"),
    },
    Builtin {
        id: "nanatype.ink",
        manifest: include_str!("../builtin/ink.toml"),
    },
    Builtin {
        id: "nanatype.echo_vanilla",
        manifest: include_str!("../builtin/echo_vanilla.toml"),
    },
    Builtin {
        id: "nanatype.echo_glass",
        manifest: include_str!("../builtin/echo_glass.toml"),
    },
    Builtin {
        id: "nanatype.echo_bubble",
        manifest: include_str!("../builtin/echo_bubble.toml"),
    },
    Builtin {
        id: "nanatype.echo_void",
        manifest: include_str!("../builtin/echo_void.toml"),
    },
];

fn builtin_files(b: &Builtin) -> Arc<dyn SkinFiles> {
    let mut files: Vec<(&'static str, &'static [u8])> = vec![(MANIFEST, b.manifest.as_bytes())];
    macro_rules! plate {
        ($name:literal) => {
            files.extend_from_slice(&[
                (
                    concat!($name, "/artwork.wgsl"),
                    include_bytes!(concat!("../builtin/", $name, "/artwork.wgsl")),
                ),
                (
                    concat!($name, "/still.png"),
                    include_bytes!(concat!("../builtin/", $name, "/still.png")),
                ),
            ])
        };
    }
    macro_rules! still {
        ($name:literal) => {
            files.push((
                concat!($name, "/still.png"),
                include_bytes!(concat!("../builtin/", $name, "/still.png")),
            ))
        };
    }
    match b.id {
        "nanatype.crayon" => files.extend_from_slice(&[
            (
                "crayon/write.wgsl",
                include_bytes!("../builtin/crayon/write.wgsl"),
            ),
            (
                "crayon/erase.wgsl",
                include_bytes!("../builtin/crayon/erase.wgsl"),
            ),
            (
                "crayon/paper.wgsl",
                include_bytes!("../builtin/crayon/paper.wgsl"),
            ),
            (
                "crayon/pencil.wgsl",
                include_bytes!("../builtin/crayon/pencil.wgsl"),
            ),
            (
                "crayon/still.png",
                include_bytes!("../builtin/crayon/still.png"),
            ),
            (
                "crayon/bubble.png",
                include_bytes!("../builtin/crayon/bubble.png"),
            ),
        ]),
        "nanatype.halo" => still!("halo"),
        "nanatype.night" => plate!("night"),
        "nanatype.parchment" => {
            still!("parchment");
            files.push((
                "parchment/ornament.png",
                include_bytes!("../builtin/parchment/ornament.png"),
            ));
        }
        "nanatype.adv" => still!("adv"),
        "nanatype.rpg" => plate!("rpg"),
        "nanatype.vanilla" => still!("vanilla"),
        "nanatype.bubble" => plate!("bubble"),
        "nanatype.livechat" => plate!("livechat"),
        "nanatype.comic" => still!("comic"),
        "nanatype.terminal" => plate!("terminal"),
        "nanatype.pixel" => plate!("pixel"),
        "nanatype.sakura" => {
            plate!("sakura");
            files.push((
                "sakura/ornament.png",
                include_bytes!("../builtin/sakura/ornament.png"),
            ));
        }
        "nanatype.ink" => still!("ink"),
        "nanatype.echo_vanilla" => still!("echo_vanilla"),
        "nanatype.echo_bubble" => still!("echo_bubble"),
        _ => {}
    }
    if b.id == "nanatype.ink" {
        files.extend_from_slice(&[
            (
                "ink/bamboo.png",
                include_bytes!("../builtin/ink/bamboo.png"),
            ),
            (
                "ink/ART-LICENSE.txt",
                include_bytes!("../builtin/ink/ART-LICENSE.txt"),
            ),
        ]);
    }
    match b.id {
        "nanatype.gothic" => files.extend_from_slice(&[
            (
                "gothic/artwork.wgsl",
                include_bytes!("../builtin/gothic/artwork.wgsl"),
            ),
            (
                "gothic/still.png",
                include_bytes!("../builtin/gothic/still.png"),
            ),
            (
                "gothic/ornament.png",
                include_bytes!("../builtin/gothic/ornament.png"),
            ),
        ]),
        "nanatype.celestial" => files.extend_from_slice(&[
            (
                "celestial/artwork.wgsl",
                include_bytes!("../builtin/celestial/artwork.wgsl"),
            ),
            (
                "celestial/still.png",
                include_bytes!("../builtin/celestial/still.png"),
            ),
            (
                "celestial/ornament.png",
                include_bytes!("../builtin/celestial/ornament.png"),
            ),
        ]),
        "nanatype.forest" => files.extend_from_slice(&[
            (
                "forest/artwork.wgsl",
                include_bytes!("../builtin/forest/artwork.wgsl"),
            ),
            (
                "forest/still.png",
                include_bytes!("../builtin/forest/still.png"),
            ),
            (
                "forest/ornament.png",
                include_bytes!("../builtin/forest/ornament.png"),
            ),
        ]),
        "nanatype.tidal" => files.extend_from_slice(&[
            (
                "tidal/artwork.wgsl",
                include_bytes!("../builtin/tidal/artwork.wgsl"),
            ),
            (
                "tidal/still.png",
                include_bytes!("../builtin/tidal/still.png"),
            ),
            (
                "tidal/ornament.png",
                include_bytes!("../builtin/tidal/ornament.png"),
            ),
        ]),
        "nanatype.lantern" => files.extend_from_slice(&[
            (
                "lantern/artwork.wgsl",
                include_bytes!("../builtin/lantern/artwork.wgsl"),
            ),
            (
                "lantern/still.png",
                include_bytes!("../builtin/lantern/still.png"),
            ),
            (
                "lantern/ornament.png",
                include_bytes!("../builtin/lantern/ornament.png"),
            ),
        ]),
        "nanatype.citrus" => files.extend_from_slice(&[
            (
                "citrus/artwork.wgsl",
                include_bytes!("../builtin/citrus/artwork.wgsl"),
            ),
            (
                "citrus/still.png",
                include_bytes!("../builtin/citrus/still.png"),
            ),
            (
                "citrus/ornament.png",
                include_bytes!("../builtin/citrus/ornament.png"),
            ),
        ]),
        _ => {}
    }
    Arc::new(MemoryFiles::new(&files))
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

struct Resolver<'a>(&'a SkinRegistry);

impl BaseResolver for Resolver<'_> {
    fn base(&self, id: &str) -> Option<(Table, Arc<dyn SkinFiles>)> {
        let files = self.0.files_for(id)?;
        let table = read_manifest(&*files).ok()?;
        Some((table, files))
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

    pub fn entry(&self, id: &str) -> Option<&SkinEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    fn user_folder(&self, id: &str) -> Option<PathBuf> {
        let dir = self.user_dir.as_ref()?.join(id);
        dir.join(MANIFEST).is_file().then_some(dir)
    }

    fn files_for(&self, id: &str) -> Option<Arc<dyn SkinFiles>> {
        if let Some(b) = BUILTINS.iter().find(|b| b.id == id) {
            if let Some(files) = self.entry(id).and_then(|e| e.files.clone()) {
                return Some(files);
            }
            return Some(builtin_files(b));
        }
        self.user_folder(id)
            .map(|dir| Arc::new(DirFiles::new(dir)) as Arc<dyn SkinFiles>)
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
        let result = if origin == SkinOrigin::Builtin {
            builtin_preview(files, &Resolver(self), &self.app_version)
        } else {
            load(files, origin.clone(), &Resolver(self), &self.app_version)
        };
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
                    let skin = if skin.shaders.is_empty() && skin.materials.is_empty() {
                        skin
                    } else {
                        // The immutable manifest/resources already passed the
                        // catalogue validator; prepare only its GPU payload now.
                        let mut resolved = (*skin).clone();
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
        load(files, origin, &Resolver(self), &self.app_version)
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
        load(
            Arc::new(files),
            SkinOrigin::Archive(archive.to_path_buf()),
            &Resolver(self),
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
        let loaded = self.inspect_archive(archive)?;
        let id = loaded.skin.meta.id.clone();
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
            for path in loaded.files.list() {
                let clean = clean_path(&path)?;
                let bytes = loaded.files.read(&clean)?;
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

    /// Write a `.ntskin` archive, including built-in artwork and its notices.
    pub fn export(&self, id: &str, dest: &Path) -> Result<(), FileError> {
        let mut files = self
            .files_for(id)
            .ok_or_else(|| FileError::Missing(id.to_owned()))?;
        // GPU skins must travel with inherited WGSL/textures, including when
        // their authoring base was another user folder. Flatten only these
        // packages; existing static exports retain their original manifests.
        let mut manifest = None;
        if let Ok(loaded) = self.load(id)
            && (!loaded.skin.shaders.is_empty() || !loaded.skin.materials.is_empty())
        {
            let compact = crate::load::standalone_manifest(&loaded.skin)
                .map_err(|e| FileError::Io(e.to_string()))?;
            manifest =
                Some(toml::to_string_pretty(&compact).map_err(|e| FileError::Io(e.to_string()))?);
            files = loaded.files;
        }
        let file = fs::File::create(dest).map_err(|e| FileError::Io(e.to_string()))?;
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for path in files.list() {
            let bytes = if path == MANIFEST {
                match &manifest {
                    Some(s) => s.as_bytes().to_vec(),
                    None => files.read(&path)?,
                }
            } else {
                files.read(&path)?
            };
            zip.start_file(path.as_str(), options)
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
        let dir = user_dir.join(new_id);
        fs::create_dir_all(&dir).map_err(|e| err(e.to_string()))?;
        fs::write(
            dir.join(MANIFEST),
            toml::to_string_pretty(&table).unwrap_or_default(),
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
        let dir = self
            .user_folder(id)
            .filter(|_| !BUILTINS.iter().any(|b| b.id == id))
            .ok_or_else(|| FileError::Missing(id.to_owned()))?;
        fs::remove_dir_all(dir).map_err(|e| FileError::Io(e.to_string()))?;
        self.rescan();
        Ok(())
    }
}
