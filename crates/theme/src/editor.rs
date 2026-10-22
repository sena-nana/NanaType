//! Package editor model: project templates, the file inventory, checked
//! imports, formatting, source locations, metadata, data and pipeline edits,
//! derived copies and drafts. Everything is a pure function over [`PackageProject`]; the host owns
//! windows, threads, signing keys and where drafts live. Nothing here runs Lua.
use crate::{
    DataFormat, DataSpec, FileError, INTEGRITY_FILE, PACKAGE_MANIFEST, PackageCapability,
    PackageManifest, PackageProject, PipelineSpec, SkinDiagnostic, SkinFiles, SkinRegistry,
    ZipFiles, clean_path, load::MANIFEST, verify_integrity,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

fn error(message: impl ToString) -> FileError {
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
const LUA_TEMPLATE: &str = r#"-- Runs only in an isolated plugin worker, after explicit consent.
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
        .map(|c| format!("{:?}", format!("{c:?}").to_lowercase()))
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

/// What a file is, by declaration first and then by checked content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AssetKind {
    Manifest,
    Integrity,
    Skin,
    Lua,
    Shader,
    Data(DataFormat),
    Image,
    Audio,
    Font,
    Text,
    Binary,
}
impl AssetKind {
    pub fn text(self) -> bool {
        matches!(
            self,
            Self::Manifest
                | Self::Integrity
                | Self::Skin
                | Self::Lua
                | Self::Shader
                | Self::Text
                | Self::Data(DataFormat::Text | DataFormat::Json)
        )
    }
}

/// One row of the editor's file tree.
#[derive(Debug, Clone, PartialEq)]
pub struct FileInfo {
    pub path: String,
    pub bytes: u64,
    /// BLAKE3 of the content, as the seal records it.
    pub hash: String,
    pub kind: AssetKind,
    /// Human-readable content format, e.g. `PNG 64×64` or `nthz v1`.
    pub format: String,
    pub editable: bool,
    pub protected: bool,
    /// Bundled from this dependency; never edited here.
    pub dependency: Option<String>,
    /// The `[data]` declaration naming this file.
    pub data: Option<(String, DataSpec)>,
}

impl FileInfo {
    pub fn source_extension(&self) -> String {
        match self.kind {
            AssetKind::Data(format) => format.name().to_owned(),
            _ => extension(&self.path),
        }
    }
}

fn extension(path: &str) -> String {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn utf8_text(bytes: &[u8]) -> bool {
    !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok()
}

/// Describe file content without trusting its extension.
pub fn describe(path: &str, bytes: &[u8]) -> (AssetKind, String) {
    let ext = extension(path);
    if path == PACKAGE_MANIFEST || path.ends_with(&format!("/{PACKAGE_MANIFEST}")) {
        return (AssetKind::Manifest, "TOML".into());
    }
    if path == INTEGRITY_FILE {
        return (AssetKind::Integrity, "JSON".into());
    }
    if path == MANIFEST || path.ends_with(&format!("/{MANIFEST}")) {
        return (AssetKind::Skin, "TOML".into());
    }
    if let Ok(kind) = imagesize::image_type(bytes)
        && matches!(
            kind,
            imagesize::ImageType::Png
                | imagesize::ImageType::Jpeg
                | imagesize::ImageType::Gif
                | imagesize::ImageType::Webp
        )
        && let Ok(size) = imagesize::blob_size(bytes)
    {
        return (
            AssetKind::Image,
            format!("{kind:?} {}×{}", size.width, size.height).to_uppercase(),
        );
    }
    let audio = if bytes.starts_with(b"OggS") {
        Some("Ogg")
    } else if bytes.len() > 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WAVE" {
        Some("WAV")
    } else if bytes.starts_with(b"fLaC") {
        Some("FLAC")
    } else if bytes.starts_with(b"ID3")
        || (bytes.len() > 1 && bytes[0] == 0xff && bytes[1] & 0xe0 == 0xe0 && ext == "mp3")
    {
        Some("MP3")
    } else {
        None
    };
    if let Some(audio) = audio {
        return (AssetKind::Audio, audio.into());
    }
    if ttf_parser::fonts_in_collection(bytes).is_some() || ttf_parser::Face::parse(bytes, 0).is_ok()
    {
        let name = if bytes.starts_with(b"OTTO") {
            "OpenType"
        } else {
            "TrueType"
        };
        return (AssetKind::Font, name.into());
    }
    if bytes.starts_with(b"NTHZ") {
        return (AssetKind::Data(DataFormat::Nthz), "NTHZ".into());
    }
    if utf8_text(bytes) {
        return match ext.as_str() {
            "lua" => (AssetKind::Lua, "Lua".into()),
            "wgsl" => (AssetKind::Shader, "WGSL".into()),
            "json" => (AssetKind::Text, "JSON".into()),
            "toml" => (AssetKind::Text, "TOML".into()),
            _ => (AssetKind::Text, "UTF-8".into()),
        };
    }
    (AssetKind::Binary, "binary".into())
}

/// The editor's file tree: every file with its size, hash, format, protection
/// and data declaration.
pub fn inventory(project: &PackageProject) -> Vec<FileInfo> {
    inventory_reusing(project, &[], "")
}

/// [`inventory`] after only `changed` was edited: other rows keep their
/// hashes, so editing a manifest does not rehash large assets.
pub fn inventory_reusing(
    project: &PackageProject,
    previous: &[FileInfo],
    changed: &str,
) -> Vec<FileInfo> {
    let known: BTreeMap<&str, &FileInfo> = previous
        .iter()
        .filter(|f| f.path != changed)
        .map(|f| (f.path.as_str(), f))
        .collect();
    let seal = verify_integrity(project);
    let manifest = project.manifest().ok();
    let declared: BTreeMap<&str, (&String, &DataSpec)> = manifest
        .iter()
        .flat_map(|m| {
            m.data
                .iter()
                .map(|(id, spec)| (spec.path.as_str(), (id, spec)))
        })
        .collect();
    project
        .files
        .iter()
        .map(|(path, bytes)| {
            let dependency = path
                .strip_prefix("dependencies/")
                .and_then(|p| p.split_once('/'))
                .map(|(id, _)| id.to_owned());
            let data = dependency
                .is_none()
                .then(|| declared.get(path.as_str()))
                .flatten()
                .map(|(id, spec)| ((*id).clone(), (*spec).clone()));
            let (mut kind, mut format) = describe(path, bytes);
            if let Some((_, spec)) = &data {
                kind = AssetKind::Data(spec.format);
                format = format!("{} v{}", spec.format.name(), spec.format_version);
            }
            let protected = match &seal {
                Ok(Some(seal)) => seal.protected.contains(path),
                Ok(None) => false,
                Err(_) => dependency.is_none(),
            };
            let hash = known
                .get(path.as_str())
                .filter(|f| f.bytes == bytes.len() as u64)
                .map_or_else(
                    || blake3::hash(bytes).to_hex().to_string(),
                    |f| f.hash.clone(),
                );
            FileInfo {
                bytes: bytes.len() as u64,
                hash,
                editable: project.can_edit(path) && kind.text(),
                path: path.clone(),
                kind,
                format,
                protected,
                dependency,
                data,
            }
        })
        .collect()
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
        clean_path(name)?
    } else {
        clean_path(&format!("{}/{name}", folder.trim_matches('/')))?
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
    let ext = extension(&path);
    if !IMPORT_EXTENSIONS.contains(&ext.as_str()) {
        return Err(error(format!("unsupported file type .{ext}")));
    }
    let (kind, _) = describe(&path, &bytes);
    let expected = match ext.as_str() {
        "png" | "jpg" | "jpeg" | "webp" | "gif" => AssetKind::Image,
        "ogg" | "wav" | "mp3" | "flac" => AssetKind::Audio,
        "ttf" | "otf" | "ttc" => AssetKind::Font,
        "nthz" => AssetKind::Data(DataFormat::Nthz),
        "lua" => AssetKind::Lua,
        "wgsl" => AssetKind::Shader,
        _ => AssetKind::Text,
    };
    if kind != expected {
        return Err(error(format!(
            ".{ext} content does not match its extension"
        )));
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Error,
    Warning,
}

/// A problem located in a file: byte range plus 1-based line and column.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceDiagnostic {
    pub path: String,
    pub offset: usize,
    pub len: usize,
    pub line: usize,
    pub column: usize,
    pub level: Level,
    pub message: String,
}

/// 1-based line and character column of a byte offset.
pub fn line_column(text: &str, offset: usize) -> (usize, usize) {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    let before = &text[..offset];
    let line = before.matches('\n').count() + 1;
    let column = before
        .rsplit_once('\n')
        .map_or(before, |(_, l)| l)
        .chars()
        .count()
        + 1;
    (line, column)
}

fn located(
    path: &str,
    text: &str,
    offset: usize,
    len: usize,
    level: Level,
    message: String,
) -> SourceDiagnostic {
    let (line, column) = line_column(text, offset);
    SourceDiagnostic {
        path: path.into(),
        offset,
        len,
        line,
        column,
        level,
        message,
    }
}

/// Fast per-keystroke checks for one file: TOML and JSON syntax, and the
/// strict `package.toml` schema. Declared data overrides the filename.
pub fn check_source(path: &str, text: &str, data: Option<DataFormat>) -> Vec<SourceDiagnostic> {
    let ext = data.map_or_else(|| extension(path), |f| f.name().to_owned());
    let mut out = Vec::new();
    if ext == "toml" {
        if let Err(e) = text.parse::<toml_edit::Document<String>>() {
            let span = e.span().unwrap_or(0..0);
            out.push(located(
                path,
                text,
                span.start,
                span.len().max(1),
                Level::Error,
                e.message().trim().to_owned(),
            ));
        } else if path == PACKAGE_MANIFEST
            && let Err(e) = PackageManifest::parse(text)
        {
            let message = e.to_string();
            let offset = message
                .split('`')
                .nth(1)
                .and_then(|key| key_span(text, &[key]))
                .map_or(0, |s| s.start);
            out.push(located(path, text, offset, 1, Level::Error, message));
        }
    } else if ext == "json"
        && let Err(e) = serde_json::from_str::<serde::de::IgnoredAny>(text)
    {
        let offset = text
            .split_inclusive('\n')
            .take(e.line().saturating_sub(1))
            .map(str::len)
            .sum::<usize>()
            + e.column().saturating_sub(1);
        out.push(located(path, text, offset, 1, Level::Error, e.to_string()));
    }
    out
}

/// Byte span of the deepest existing key along `keys` in a TOML document.
fn key_span(text: &str, keys: &[&str]) -> Option<std::ops::Range<usize>> {
    let doc = text.parse::<toml_edit::Document<String>>().ok()?;
    let mut item = doc.as_item();
    let mut span = None;
    for key in keys {
        let Some((k, next)) = item.as_table_like().and_then(|t| t.get_key_value(key)) else {
            break;
        };
        span = k.span().or(next.span()).or(span);
        item = next;
    }
    span
}

/// Place a validation result in the file and key it is about.
pub fn locate(project: &PackageProject, diagnostic: &SkinDiagnostic) -> SourceDiagnostic {
    let level = match diagnostic.severity {
        crate::Severity::Error => Level::Error,
        crate::Severity::Warning => Level::Warning,
    };
    let mut field = diagnostic.field.clone();
    // Early package budget errors name a file; locate its data declaration.
    if field == PACKAGE_MANIFEST
        && let Some((id, _)) = project.manifest().ok().and_then(|m| {
            m.data.into_iter().find(|(_, d)| {
                Some(d.path.as_str()) == diagnostic.message.rsplit('`').nth(1)
                    && project
                        .files
                        .get(&d.path)
                        .is_some_and(|b| b.len() as u64 > d.format.max_raw())
            })
        })
    {
        field = format!("data.{id}");
    }
    let field = field.as_str();
    // Parse errors name the offending key last, in backticks.
    let quoted: Vec<&str> = diagnostic
        .message
        .rsplit('`')
        .nth(1)
        .map(|k| k.split('.').collect())
        .unwrap_or_default();
    let (path, keys): (&str, Vec<&str>) = if let Some(rest) = field.strip_prefix("data.") {
        (
            PACKAGE_MANIFEST,
            vec!["data", rest.rsplit('/').next().unwrap_or(rest)],
        )
    } else if let Some(rest) = field.strip_prefix("lua.") {
        (PACKAGE_MANIFEST, vec!["lua", rest])
    } else if field.starts_with("meta") {
        (PACKAGE_MANIFEST, quoted)
    } else if project.files.contains_key(field) {
        (field, quoted)
    } else {
        (
            MANIFEST,
            field
                .split(['.', '['])
                .map(|k| k.trim_end_matches(']'))
                .collect(),
        )
    };
    let text = project
        .files
        .get(path)
        .and_then(|b| std::str::from_utf8(b).ok())
        .unwrap_or("");
    let span = key_span(text, &keys).unwrap_or(0..0);
    let message = if field.is_empty() || field == path {
        diagnostic.message.clone()
    } else {
        format!("{field}: {}", diagnostic.message)
    };
    located(path, text, span.start, span.len().max(1), level, message)
}

/// Re-indent JSON with two spaces, keeping key order and every token as written.
pub fn format_json(text: &str) -> Result<String, String> {
    serde_json::from_str::<serde::de::IgnoredAny>(text).map_err(|e| e.to_string())?;
    let mut out = String::with_capacity(text.len());
    let mut depth = 0usize;
    let mut chars = text.chars().peekable();
    let newline = |out: &mut String, depth: usize| {
        out.push('\n');
        out.extend(std::iter::repeat_n("  ", depth));
    };
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                out.push('"');
                let mut escaped = false;
                for c in chars.by_ref() {
                    out.push(c);
                    if escaped {
                        escaped = false;
                    } else if c == '\\' {
                        escaped = true;
                    } else if c == '"' {
                        break;
                    }
                }
            }
            '{' | '[' => {
                out.push(c);
                while chars.peek().is_some_and(|c| c.is_whitespace()) {
                    chars.next();
                }
                if matches!(chars.peek(), Some('}' | ']')) {
                    out.push(chars.next().unwrap_or_default());
                } else {
                    depth += 1;
                    newline(&mut out, depth);
                }
            }
            '}' | ']' => {
                depth = depth.saturating_sub(1);
                newline(&mut out, depth);
                out.push(c);
            }
            ',' => {
                out.push(',');
                newline(&mut out, depth);
            }
            ':' => out.push_str(": "),
            c if c.is_whitespace() => {}
            c => out.push(c),
        }
    }
    out.push('\n');
    Ok(out)
}

/// Normalise spacing around `=` and trailing whitespace while keeping every
/// comment, key order and value representation.
pub fn format_toml(text: &str) -> Result<String, String> {
    let before: toml::Table = toml::from_str(text).map_err(|e| e.message().to_owned())?;
    let mut doc = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| e.message().to_owned())?;
    fn tidy(table: &mut dyn toml_edit::TableLike) {
        for (mut key, item) in table.iter_mut() {
            match item {
                toml_edit::Item::Value(value) => {
                    // Indentation right before a key; comments and blank lines stay.
                    let prefix = key
                        .leaf_decor()
                        .prefix()
                        .and_then(|p| p.as_str())
                        .map(|p| p.trim_end_matches([' ', '\t']).to_owned());
                    key.leaf_decor_mut().set_suffix(" ");
                    if let Some(prefix) = prefix {
                        key.leaf_decor_mut().set_prefix(prefix);
                    }
                    let suffix = value.decor().suffix().and_then(|s| s.as_str()).map(|s| {
                        let s = s.trim_end();
                        if s.trim_start().starts_with('#') {
                            format!(" {}", s.trim_start())
                        } else {
                            String::new()
                        }
                    });
                    value.decor_mut().set_prefix(" ");
                    if let Some(suffix) = suffix {
                        value.decor_mut().set_suffix(suffix);
                    }
                }
                toml_edit::Item::Table(t) => tidy(t),
                toml_edit::Item::ArrayOfTables(a) => a.iter_mut().for_each(|t| tidy(t)),
                toml_edit::Item::None => {}
            }
        }
    }
    tidy(doc.as_table_mut());
    let mut out: String = doc
        .to_string()
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    out = out.trim_end().to_owned();
    out.push('\n');
    let after: toml::Table = toml::from_str(&out).map_err(|e| e.message().to_owned())?;
    if after != before {
        return Err("formatting would change the document".into());
    }
    Ok(out)
}

/// The `package.toml` fields the metadata form edits.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Metadata {
    pub version: String,
    pub author: String,
    pub created_at: String,
    pub licensed_to: String,
    pub license: String,
    pub capabilities: Vec<PackageCapability>,
    pub dependencies: BTreeMap<String, String>,
}
impl Metadata {
    pub fn of(manifest: &PackageManifest) -> Self {
        Self {
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

fn manifest_doc(project: &PackageProject) -> Result<toml_edit::DocumentMut, FileError> {
    String::from_utf8(project.read(PACKAGE_MANIFEST)?)
        .map_err(error)?
        .parse()
        .map_err(error)
}

/// Write an edited `package.toml` only if it parses strictly.
fn write_manifest(
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
            doc[key] = toml_edit::value(value);
        }
    };
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
            capabilities.push(format!("{c:?}").to_lowercase());
        }
    }
    doc["capabilities"] = toml_edit::value(capabilities);
    if metadata.dependencies.is_empty() {
        doc.remove("dependencies");
    } else {
        let mut table = toml_edit::Table::new();
        for (id, version) in &metadata.dependencies {
            table[id.as_str()] = toml_edit::value(version.as_str());
        }
        doc["dependencies"] = toml_edit::Item::Table(table);
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

/// Read a `.ntpkg` without requiring it to be valid, so a broken package can
/// be opened and repaired. Budgets still apply; a failing seal leaves every
/// local file read-only.
pub fn read_archive(path: &Path) -> Result<PackageProject, FileError> {
    let project = PackageProject::read_all(&ZipFiles::open(path)?)?;
    project.check_budget()?;
    Ok(project)
}

/// Where a draft started, shown so a copy is never mistaken for its source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DraftOrigin {
    New,
    Installed { id: String },
    Archive { path: PathBuf },
    Copy { from: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftInfo {
    pub id: String,
    pub package: String,
    pub origin: DraftOrigin,
    /// Seconds since the Unix epoch.
    pub saved_at: u64,
}

fn valid_draft_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
        && !id.starts_with('.')
}

/// Save atomically to `<dir>/<id>.ntdraft` plus its `.json` description.
pub fn save_draft(dir: &Path, info: &DraftInfo, project: &PackageProject) -> Result<(), FileError> {
    if !valid_draft_id(&info.id) {
        return Err(error("invalid draft ID"));
    }
    project.check_budget()?;
    std::fs::create_dir_all(dir).map_err(error)?;
    let archive = dir.join(format!("{}.ntdraft", info.id));
    let staging = dir.join(format!(".{}.ntdraft.tmp", info.id));
    project.write_zip(&staging)?;
    std::fs::rename(&staging, &archive).map_err(error)?;
    let meta = dir.join(format!("{}.json", info.id));
    let staging = dir.join(format!(".{}.json.tmp", info.id));
    std::fs::write(&staging, serde_json::to_vec_pretty(info).map_err(error)?).map_err(error)?;
    std::fs::rename(&staging, &meta).map_err(error)
}

/// Saved drafts, newest first. Unreadable entries are skipped.
pub fn list_drafts(dir: &Path) -> Vec<DraftInfo> {
    let mut drafts: Vec<DraftInfo> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| {
            let bytes = std::fs::read(e.path()).ok()?;
            let info: DraftInfo = serde_json::from_slice(&bytes).ok()?;
            (valid_draft_id(&info.id) && dir.join(format!("{}.ntdraft", info.id)).is_file())
                .then_some(info)
        })
        .collect();
    drafts.sort_by(|a, b| b.saved_at.cmp(&a.saved_at).then(a.id.cmp(&b.id)));
    drafts
}

pub fn load_draft(dir: &Path, id: &str) -> Result<(DraftInfo, PackageProject), FileError> {
    if !valid_draft_id(id) {
        return Err(error("invalid draft ID"));
    }
    let info: DraftInfo =
        serde_json::from_slice(&std::fs::read(dir.join(format!("{id}.json"))).map_err(error)?)
            .map_err(error)?;
    Ok((info, read_archive(&dir.join(format!("{id}.ntdraft")))?))
}

/// Delete a draft and its description. Installed packages are untouched.
pub fn discard_draft(dir: &Path, id: &str) -> Result<(), FileError> {
    if !valid_draft_id(id) {
        return Err(error("invalid draft ID"));
    }
    for name in [format!("{id}.ntdraft"), format!("{id}.json")] {
        match std::fs::remove_file(dir.join(name)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(error(e)),
            _ => {}
        }
    }
    Ok(())
}

/// Every declared pipeline in the skin, parsed one by one so a malformed
/// declaration shows its error instead of hiding the others.
pub fn pipelines(project: &PackageProject) -> Vec<(String, Result<PipelineSpec, String>)> {
    let Some(table) = project
        .files
        .get(MANIFEST)
        .and_then(|b| std::str::from_utf8(b).ok())
        .and_then(|t| toml::from_str::<toml::Table>(t).ok())
    else {
        return Vec::new();
    };
    table
        .get("pipelines")
        .and_then(|p| p.as_table())
        .into_iter()
        .flatten()
        .map(|(name, spec)| {
            let spec = spec
                .clone()
                .try_into::<PipelineSpec>()
                .map_err(|e| e.message().trim().to_owned());
            (name.clone(), spec)
        })
        .collect()
}

/// Set `key` to `value`, keeping the comment and spacing the old value had.
fn set_kept(table: &mut toml_edit::Table, key: &str, mut value: toml_edit::Value) {
    if let Some(old) = table.get(key).and_then(toml_edit::Item::as_value) {
        *value.decor_mut() = old.decor().clone();
    }
    table[key] = toml_edit::Item::Value(value);
}

/// The `[<parent>.<name>]` tables: `previous` is renamed to (or replaced by)
/// `name`, keeping the comments inside it, or removed when `name` is `None`.
/// Returns the table to fill in.
fn entry_table<'a>(
    doc: &'a mut toml_edit::DocumentMut,
    parent: &str,
    previous: Option<&str>,
    name: Option<&str>,
) -> Result<Option<&'a mut toml_edit::Table>, FileError> {
    if !doc.contains_key(parent) {
        let mut table = toml_edit::Table::new();
        table.set_implicit(true);
        doc[parent] = toml_edit::Item::Table(table);
    }
    let parent_table = doc[parent]
        .as_table_mut()
        .ok_or_else(|| error(format!("`{parent}` must be a table")))?;
    let mut kept = previous.and_then(|p| parent_table.remove(p));
    let Some(name) = name else {
        if kept.is_none() {
            return Err(error(format!(
                "{parent}.{} does not exist",
                previous.unwrap_or("")
            )));
        }
        if parent_table.is_empty() {
            doc.remove(parent);
        }
        return Ok(None);
    };
    let parent_table = doc[parent]
        .as_table_mut()
        .ok_or_else(|| error(format!("`{parent}` must be a table")))?;
    if previous != Some(name) && parent_table.contains_key(name) {
        return Err(error(format!("{parent}.{name} already exists")));
    }
    let item = match kept.take() {
        Some(toml_edit::Item::Table(t)) => toml_edit::Item::Table(t),
        // An inline declaration becomes a table of its own.
        Some(toml_edit::Item::Value(toml_edit::Value::InlineTable(t))) => {
            toml_edit::Item::Table(t.into_table())
        }
        _ => toml_edit::Item::Table(toml_edit::Table::new()),
    };
    parent_table.insert(name, item);
    Ok(parent_table
        .get_mut(name)
        .and_then(toml_edit::Item::as_table_mut))
}

fn json_to_toml(value: &serde_json::Value) -> Option<toml_edit::Value> {
    Some(match value {
        serde_json::Value::Bool(b) => (*b).into(),
        serde_json::Value::Number(n) => match n.as_i64() {
            Some(i) => i.into(),
            None => n.as_f64()?.into(),
        },
        serde_json::Value::String(s) => s.as_str().into(),
        serde_json::Value::Array(items) => {
            toml_edit::Value::Array(items.iter().map(json_to_toml).collect::<Option<_>>()?)
        }
        serde_json::Value::Object(map) => {
            let mut table = toml_edit::InlineTable::new();
            for (k, v) in map {
                table.insert(k, json_to_toml(v)?);
            }
            toml_edit::Value::InlineTable(table)
        }
        serde_json::Value::Null => return None,
    })
}

/// Add, change, rename or (with `entry` = `None`) remove a `[data.<id>]`
/// declaration in `package.toml`, keeping comments and every other key. A
/// rename follows into `[lua] reads`. The project is unchanged unless the
/// result parses strictly and the declared file is part of the package.
pub fn apply_data(
    project: &mut PackageProject,
    previous: Option<&str>,
    entry: Option<(&str, &DataSpec)>,
) -> Result<(), FileError> {
    let mut doc = manifest_doc(project)?;
    if let Some((id, spec)) = entry {
        spec.validate(id).map_err(error)?;
        if spec.path.starts_with("dependencies/")
            || [PACKAGE_MANIFEST, INTEGRITY_FILE].contains(&spec.path.as_str())
            || !project.files.contains_key(&clean_path(&spec.path)?)
        {
            return Err(error(format!(
                "data.{id}: {} is not a file of this package",
                spec.path
            )));
        }
    }
    let name = entry.map(|(id, _)| id);
    if let Some(table) = entry_table(&mut doc, "data", previous, name)? {
        let (_, spec) = entry.expect("a table is only returned for an entry");
        set_kept(table, "path", spec.path.as_str().into());
        set_kept(table, "format", spec.format.name().into());
        set_kept(
            table,
            "format_version",
            i64::from(spec.format_version).into(),
        );
        set_kept(table, "source", spec.source.as_str().into());
        set_kept(table, "license", spec.license.as_str().into());
        match &spec.constraints {
            Some(c) => {
                let value = serde_json::to_value(c)
                    .ok()
                    .and_then(|v| json_to_toml(&v))
                    .ok_or_else(|| error("invalid constraints"))?;
                set_kept(table, "constraints", value);
            }
            None => {
                table.remove("constraints");
            }
        }
    }
    if let (Some(previous), Some(name)) = (previous, name)
        && previous != name
        && let Some(reads) = doc
            .get_mut("lua")
            .and_then(|l| l.get_mut("reads"))
            .and_then(toml_edit::Item::as_array_mut)
    {
        for read in reads.iter_mut() {
            if read.as_str() == Some(previous) {
                let decor = read.decor().clone();
                *read = name.into();
                *read.decor_mut() = decor;
            }
        }
    }
    write_manifest(project, &doc)
}

/// Pipeline errors of a `skin.toml` text, checked against the registered
/// processors with the skin's own materials (inherited ones are not seen).
fn pipeline_errors(text: &str) -> Result<BTreeSet<(String, String)>, FileError> {
    let table: toml::Table = toml::from_str(text).map_err(|e| error(e.message().trim()))?;
    let pipelines = match table.get("pipelines") {
        Some(p) => p
            .clone()
            .try_into()
            .map_err(|e: toml::de::Error| error(format!("pipelines: {}", e.message().trim())))?,
        None => BTreeMap::new(),
    };
    let skin = crate::Skin {
        pipelines,
        materials: table
            .get("materials")
            .cloned()
            .and_then(|m| m.try_into().ok())
            .unwrap_or_default(),
        ..Default::default()
    };
    Ok(crate::pipeline::validate(&skin, crate::PROCESSORS)
        .into_iter()
        .filter(|d| d.severity == crate::Severity::Error)
        .map(|d| (d.field, d.message))
        .collect())
}

/// Add, change, rename or (with `entry` = `None`) remove a
/// `[pipelines.<name>]` declaration in `skin.toml`, keeping comments and
/// every other key. The project is unchanged when the edit adds a pipeline
/// error (unknown processor, unbound or mistyped input, cycle, a material
/// losing its source) or names data this package does not declare.
pub fn apply_pipeline(
    project: &mut PackageProject,
    previous: Option<&str>,
    entry: Option<(&str, &PipelineSpec)>,
) -> Result<(), FileError> {
    let before = String::from_utf8(project.read(MANIFEST)?).map_err(error)?;
    let mut doc = before.parse::<toml_edit::DocumentMut>().map_err(error)?;
    if let Some((name, spec)) = entry {
        let declared = project.manifest().map(|m| m.data).unwrap_or_default();
        let signature = crate::processor(&spec.processor, spec.version);
        for (input, source) in &spec.inputs {
            let Some(reference) = &source.data else {
                continue;
            };
            let (package, id) = crate::data::split_reference(reference).map_err(error)?;
            let expected = signature
                .and_then(|s| s.inputs.iter().find(|(n, _)| n == input))
                .map(|(_, t)| *t);
            match (package, declared.get(id), expected) {
                (Some(_), _, _) => {}
                (None, None, _) => {
                    return Err(error(format!(
                        "pipelines.{name}.inputs.{input}: undeclared data `{id}`"
                    )));
                }
                (None, Some(data), Some(crate::PortType::Data { format, version }))
                    if (data.format, data.format_version) != (format, version) =>
                {
                    return Err(error(format!(
                        "pipelines.{name}.inputs.{input}: `{id}` is {} v{}, expected {} v{version}",
                        data.format.name(),
                        data.format_version,
                        format.name()
                    )));
                }
                _ => {}
            }
        }
    }
    if let Some(table) = entry_table(&mut doc, "pipelines", previous, entry.map(|(n, _)| n))? {
        let (_, spec) = entry.expect("a table is only returned for an entry");
        set_kept(table, "processor", spec.processor.as_str().into());
        set_kept(table, "version", i64::from(spec.version).into());
        let mut inputs = toml_edit::InlineTable::new();
        for (input, source) in &spec.inputs {
            let mut port = toml_edit::InlineTable::new();
            for (key, value) in [
                ("data", &source.data),
                ("pipeline", &source.pipeline),
                ("output", &source.output),
            ] {
                if let Some(value) = value {
                    port.insert(key, value.as_str().into());
                }
            }
            inputs.insert(input, toml_edit::Value::InlineTable(port));
        }
        let mut params = toml_edit::InlineTable::new();
        for (param, value) in &spec.params {
            params.insert(param, f64::from(*value).into());
        }
        for (key, value) in [("inputs", inputs), ("params", params)] {
            if value.is_empty() {
                table.remove(key);
            } else {
                set_kept(table, key, toml_edit::Value::InlineTable(value));
            }
        }
    }
    let text = doc.to_string();
    let old = pipeline_errors(&before).unwrap_or_default();
    let added: Vec<String> = pipeline_errors(&text)?
        .difference(&old)
        .map(|(field, message)| format!("{field}: {message}"))
        .collect();
    if !added.is_empty() {
        return Err(error(added.join("; ")));
    }
    let mut next = project.clone();
    next.set(MANIFEST, text.into_bytes())?;
    *project = next;
    Ok(())
}

#[cfg(test)]
mod tests;
