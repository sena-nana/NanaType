//! Package editor model: project templates, the file inventory, checked
//! imports, formatting, source locations, metadata, data and pipeline edits,
//! derived copies and drafts. Everything is a pure function over [`PackageProject`]; the host owns
//! windows, threads, signing keys and where drafts live. Nothing here runs Lua.

use crate::{
    FileError, INTEGRITY_FILE, PACKAGE_MANIFEST, PackageCapability, PackageManifest,
    PackageProject, SkinRegistry, load::MANIFEST, verify_integrity,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

mod authoring;
mod diagnostics;
mod drafts;
mod format;
mod inventory;
mod resources;
#[cfg(test)]
mod tests;

pub use crate::load::authored_layout;
pub use authoring::*;
pub use diagnostics::*;
pub use drafts::*;
pub use format::*;
pub use inventory::*;
pub use resources::*;

pub(super) fn error(message: impl ToString) -> FileError {
    FileError::Io(message.to_string())
}

/// Largest text file the source editor opens for editing; larger text stays
/// visible as file information only.
pub const MAX_EDIT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectKind {
    Skin,
    Resources,
    Lua,
    Sounds,
    Mixed,
}
impl ProjectKind {
    pub const ALL: [Self; 5] = [
        Self::Skin,
        Self::Resources,
        Self::Lua,
        Self::Sounds,
        Self::Mixed,
    ];
    pub fn key(self) -> &'static str {
        match self {
            Self::Skin => "skin",
            Self::Resources => "resources",
            Self::Lua => "lua",
            Self::Sounds => "sounds",
            Self::Mixed => "mixed",
        }
    }
    pub fn capabilities(self) -> Vec<PackageCapability> {
        match self {
            Self::Skin => vec![PackageCapability::Skin],
            Self::Resources => vec![PackageCapability::Resources],
            Self::Lua => vec![PackageCapability::Lua],
            Self::Sounds => vec![PackageCapability::Sounds],
            Self::Mixed => vec![
                PackageCapability::Skin,
                PackageCapability::Resources,
                PackageCapability::Lua,
            ],
        }
    }
    pub fn of(manifest: &PackageManifest) -> Self {
        match manifest.capabilities.as_slice() {
            [PackageCapability::Skin] => Self::Skin,
            [PackageCapability::Resources] => Self::Resources,
            [PackageCapability::Lua] => Self::Lua,
            [PackageCapability::Sounds] => Self::Sounds,
            _ => Self::Mixed,
        }
    }
}

const SKIN_TEMPLATE: &str =
    "# Dialogue, history and input styling. See docs/skins.md.\n[chrome]\naccent = \"#7c6cf0\"\n";
const SOUNDS_TEMPLATE: &str = "# Typing, line and effect sounds that replace a skin's. See docs/skins.md.\n# Add audio files to the package and name them here, for example:\n# [sounds.typing]\n# files = [\"sounds/key-1.wav\", \"sounds/key-2.wav\"]\n[sounds]\n";
const NOTES_TEMPLATE: &str = "first line\nsecond line\n";
pub(super) const LUA_TEMPLATE: &str = r#"-- Runs only in an isolated plugin worker, after explicit consent.
local last = 'waiting for a line'
return {
    on_event = function(event)
        if event.type == 'line_start' then last = event.text end
    end,
    view = function()
        return {views = {{slot = 'panel', root = {kind = 'text', id = 'last', text = last}}}}
    end,
}
"#;
const MIXED_LUA_TEMPLATE: &str = r#"-- Runs only in an isolated plugin worker, after explicit consent.
local notes, pending = 'loading', nil
return {
    on_event = function(event)
        if event.type == 'init' then
            pending = nana.data.request('notes', {kind = 'lines', start = 0, count = 1})
        elseif event.type == 'data' then
            for _, r in ipairs(event.results) do
                if r.token == pending then notes = r.error or r.value[1] end
            end
        end
    end,
    view = function()
        return {views = {{slot = 'panel', root = {kind = 'text', id = 'notes', text = notes}}}}
    end,
}
"#;

/// A minimal valid project of `kind`. Templates contain no executable code
/// beyond the Lua entry, which still needs consent before it ever runs.
pub fn template(kind: ProjectKind, id: &str) -> Result<PackageProject, FileError> {
    if !crate::package::valid_id(id) {
        return Err(error("invalid package ID"));
    }
    let capabilities = kind
        .capabilities()
        .iter()
        .map(|c| format!("\"{}\"", c.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    let mut manifest = format!(
        "format = 1\nschema = 3\nid = \"{id}\"\nversion = \"1.0.0\"\nmin_app = \"0.1.0\"\ncapabilities = [{capabilities}]\nauthor = \"\"\nlicense = \"\"\n\n[name]\nzh-CN = \"{id}\"\nen-US = \"{id}\"\n"
    );
    let mut project = PackageProject::default();
    let data = matches!(kind, ProjectKind::Resources | ProjectKind::Mixed);
    if matches!(kind, ProjectKind::Lua | ProjectKind::Mixed) {
        manifest.push_str("\n[lua]\nentry = \"main.lua\"\n");
        if data {
            manifest.push_str("reads = [\"notes\"]\n");
        }
        let source = if data {
            MIXED_LUA_TEMPLATE
        } else {
            LUA_TEMPLATE
        };
        project.files.insert("main.lua".into(), source.into());
    }
    if data {
        manifest.push_str("\n[data.notes]\npath = \"data/notes.txt\"\nformat = \"text\"\nformat_version = 1\nsource = \"original work\"\nlicense = \"CC0-1.0\"\n");
        project
            .files
            .insert("data/notes.txt".into(), NOTES_TEMPLATE.into());
    }
    if matches!(kind, ProjectKind::Skin | ProjectKind::Mixed) {
        project.files.insert(MANIFEST.into(), SKIN_TEMPLATE.into());
    }
    if kind == ProjectKind::Sounds {
        project
            .files
            .insert(crate::SOUNDS_MANIFEST.into(), SOUNDS_TEMPLATE.into());
    }
    project
        .files
        .insert(PACKAGE_MANIFEST.into(), manifest.into());
    project.manifest()?;
    Ok(project)
}

/// Extensions a user may import, each checked against the file's content.
pub const IMPORT_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "webp", "gif", "ogg", "wav", "mp3", "flac", "ttf", "otf", "ttc", "wgsl",
    "lua", "toml", "json", "txt", "md", "csv", "nthz",
];

/// Copy a user-chosen file into `folder` after checking its name, size and
/// that its content really is what its extension says.
pub fn import_asset(
    project: &mut PackageProject,
    folder: &str,
    name: &str,
    bytes: Vec<u8>,
) -> Result<String, FileError> {
    let name = Path::new(name)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| error("invalid file name"))?;
    let path = if folder.trim_matches('/').is_empty() {
        crate::clean_path(name)?
    } else {
        crate::clean_path(&format!("{}/{name}", folder.trim_matches('/')))?
    };
    if path.starts_with("dependencies/")
        || [PACKAGE_MANIFEST, INTEGRITY_FILE, MANIFEST].contains(&path.as_str())
    {
        return Err(error("reserved package path"));
    }
    if project.files.contains_key(&path) {
        return Err(error(format!("{path} already exists")));
    }
    if bytes.len() as u64 > crate::MAX_FILE_BYTES {
        return Err(FileError::TooLarge {
            path,
            bytes: bytes.len() as u64,
        });
    }
    inventory::validate_import_content(&path, &bytes)?;
    project.set(&path, bytes)?;
    Ok(path)
}

/// Remove an editable file. The manifest and bundled dependencies stay.
pub fn remove_file(project: &mut PackageProject, path: &str) -> Result<(), FileError> {
    if path == PACKAGE_MANIFEST || !project.can_edit(path) || verify_integrity(project)?.is_some() {
        return Err(error(
            "file is protected; create an unsigned copy to edit it",
        ));
    }
    project
        .files
        .remove(path)
        .map(|_| ())
        .ok_or_else(|| FileError::Missing(path.into()))
}

/// The `package.toml` fields the metadata form edits.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Metadata {
    pub name: crate::Localized,
    pub description: crate::Localized,
    pub min_app: String,
    pub base: String,
    pub preview: String,
    pub tags: Vec<String>,
    pub version: String,
    pub author: String,
    pub created_at: String,
    pub licensed_to: String,
    pub license: String,
    pub capabilities: Vec<PackageCapability>,
    pub dependencies: BTreeMap<String, String>,
}

/// Parse every dependency row; malformed or duplicate rows must never disappear.
pub fn parse_dependencies(text: &str) -> Result<BTreeMap<String, String>, String> {
    let mut result = BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let fail = |detail: &str| format!("{}: {detail} ({line})", index + 1);
        let (id, version) = line
            .split_once('@')
            .ok_or_else(|| fail("expected package.id@exact.version"))?;
        let (id, version) = (id.trim(), version.trim());
        if !crate::package::valid_id(id) || version.is_empty() || version.contains('@') {
            return Err(fail("invalid package ID or exact version"));
        }
        // Reuse the manifest's version and dependency validation.
        let probe = format!(
            "format=1\nschema=3\nid='editor.dependencies'\nversion='1.0.0'\ncapabilities=['skin']\n[dependencies]\n{id:?}={version:?}\n"
        );
        PackageManifest::parse(&probe).map_err(|e| fail(&e.to_string()))?;
        if result.insert(id.to_owned(), version.to_owned()).is_some() {
            return Err(fail("duplicate package ID"));
        }
    }
    Ok(result)
}

impl Metadata {
    pub fn of(manifest: &PackageManifest) -> Self {
        Self {
            name: manifest.meta.name.clone(),
            description: manifest.meta.description.clone(),
            min_app: manifest.meta.min_app.clone(),
            base: manifest.meta.base.clone().unwrap_or_default(),
            preview: manifest.meta.preview.clone().unwrap_or_default(),
            tags: manifest.meta.tags.clone(),
            version: manifest.meta.version.clone(),
            author: manifest.meta.author.clone(),
            created_at: manifest.created_at.clone(),
            licensed_to: manifest.licensed_to.clone(),
            license: manifest.meta.license.clone(),
            capabilities: manifest.capabilities.clone(),
            dependencies: manifest.dependencies.clone(),
        }
    }
}

pub(super) fn manifest_doc(project: &PackageProject) -> Result<toml_edit::DocumentMut, FileError> {
    project.text(PACKAGE_MANIFEST)?.parse().map_err(error)
}

/// Write an edited `package.toml` only if it parses strictly.
pub(super) fn write_manifest(
    project: &mut PackageProject,
    doc: &toml_edit::DocumentMut,
) -> Result<(), FileError> {
    let text = doc.to_string();
    PackageManifest::parse(&text)?;
    project.set(PACKAGE_MANIFEST, text.into_bytes())
}

/// Write `metadata` into `package.toml`, keeping comments and every other
/// key, and add starter content for a newly enabled skin or Lua capability.
/// The project is unchanged unless the result parses strictly.
pub fn apply_metadata(project: &mut PackageProject, metadata: &Metadata) -> Result<(), FileError> {
    let mut doc = manifest_doc(project)?;
    let string = |doc: &mut toml_edit::DocumentMut, key: &str, value: &str| {
        if value.is_empty() && matches!(key, "created_at" | "licensed_to") {
            doc.remove(key);
        } else {
            let mut next = toml_edit::Value::from(value);
            if let Some(previous) = doc.get(key).and_then(toml_edit::Item::as_value) {
                *next.decor_mut() = previous.decor().clone();
            }
            doc[key] = toml_edit::Item::Value(next);
        }
    };
    string(&mut doc, "min_app", &metadata.min_app);
    for (key, value) in [("base", &metadata.base), ("preview", &metadata.preview)] {
        if value.trim().is_empty() {
            doc.remove(key);
        } else {
            string(&mut doc, key, value.trim());
        }
    }
    for (key, values) in [
        ("name", &metadata.name),
        ("description", &metadata.description),
    ] {
        if !doc.contains_key(key) {
            doc[key] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        for (locale, value) in values {
            doc[key][locale] = toml_edit::value(value.as_str());
        }
    }
    let mut tags = toml_edit::Array::new();
    for tag in &metadata.tags {
        tags.push(tag.as_str());
    }
    doc["tags"] = toml_edit::value(tags);
    string(&mut doc, "version", &metadata.version);
    string(&mut doc, "author", &metadata.author);
    string(&mut doc, "created_at", &metadata.created_at);
    string(&mut doc, "licensed_to", &metadata.licensed_to);
    string(&mut doc, "license", &metadata.license);
    let mut capabilities = toml_edit::Array::new();
    for c in [
        PackageCapability::Skin,
        PackageCapability::Resources,
        PackageCapability::Lua,
        PackageCapability::Sounds,
    ] {
        if metadata.capabilities.contains(&c) {
            capabilities.push(c.as_str());
        }
    }
    doc["capabilities"] = toml_edit::value(capabilities);
    if metadata.dependencies.is_empty() {
        doc.remove("dependencies");
    } else {
        if !doc.contains_key("dependencies") {
            doc["dependencies"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let table = doc["dependencies"]
            .as_table_like_mut()
            .ok_or_else(|| error("dependencies must be a table"))?;
        let removed = table
            .iter()
            .map(|(k, _)| k.to_owned())
            .filter(|k| !metadata.dependencies.contains_key(k))
            .collect::<Vec<_>>();
        for key in removed {
            table.remove(&key);
        }
        for (id, version) in &metadata.dependencies {
            let mut value = toml_edit::Value::from(version.as_str());
            if let Some(old) = table.get(id).and_then(toml_edit::Item::as_value) {
                *value.decor_mut() = old.decor().clone();
            }
            table.insert(id, toml_edit::Item::Value(value));
        }
    }
    let lua = metadata.capabilities.contains(&PackageCapability::Lua);
    let mut next = project.clone();
    if lua && !doc.contains_key("lua") {
        let entry = if next.files.contains_key("main.lua") {
            "main.lua"
        } else {
            next.set("main.lua", LUA_TEMPLATE.into())?;
            "main.lua"
        };
        let mut table = toml_edit::Table::new();
        table["entry"] = toml_edit::value(entry);
        doc["lua"] = toml_edit::Item::Table(table);
    } else if !lua {
        doc.remove("lua");
    }
    if metadata.capabilities.contains(&PackageCapability::Skin)
        && !next.files.contains_key(MANIFEST)
    {
        next.set(MANIFEST, SKIN_TEMPLATE.into())?;
    }
    if metadata.capabilities.contains(&PackageCapability::Sounds)
        && !next.files.contains_key(crate::SOUNDS_MANIFEST)
    {
        next.set(crate::SOUNDS_MANIFEST, SOUNDS_TEMPLATE.into())?;
    }
    write_manifest(&mut next, &doc)?;
    *project = next;
    Ok(())
}

/// Replace `dependencies/` with the exact declared closure: bundled copies are
/// kept, missing ones come from installed packages. Offline only.
pub fn bundle_dependencies(
    project: &mut PackageProject,
    registry: &SkinRegistry,
) -> Result<(), FileError> {
    let resolved = registry.resolve_package(std::sync::Arc::new(project.clone()))?;
    let mut next = PackageProject::capture(&*resolved)?;
    // Signed local bytes stay exactly as they were.
    for (path, bytes) in &project.files {
        if !path.starts_with("dependencies/") {
            next.files.insert(path.clone(), bytes.clone());
        }
    }
    next.check_budget()?;
    *project = next;
    Ok(())
}

/// An unsigned copy under a new ID that records where it came from. Its
/// former signature is dropped, so nothing it ships claims the original
/// signer's protection.
pub fn derive_copy(project: &PackageProject, id: &str) -> Result<PackageProject, FileError> {
    let manifest = project.manifest()?;
    if !crate::package::valid_id(id) || id == manifest.meta.id {
        return Err(error("a derived copy needs a new package ID"));
    }
    let mut doc = manifest_doc(project)?;
    doc["id"] = toml_edit::value(id);
    doc["derived_from"] =
        toml_edit::value(format!("{}@{}", manifest.meta.id, manifest.meta.version));
    let mut copy = project.clone();
    // Unsigned now, so every local file is editable.
    copy.files.remove(INTEGRITY_FILE);
    write_manifest(&mut copy, &doc)?;
    Ok(copy)
}
