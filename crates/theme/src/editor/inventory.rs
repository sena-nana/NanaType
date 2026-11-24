//! Package file inventory and content-sniffed asset classification.

use super::error;
use crate::{
    DataFormat, DataSpec, INTEGRITY_FILE, PACKAGE_MANIFEST, PackageProject, load::MANIFEST,
    verify_integrity,
};
use std::{collections::BTreeMap, path::Path};

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

pub(crate) fn extension(path: &str) -> String {
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

pub(crate) fn validate_import_content(path: &str, bytes: &[u8]) -> Result<(), crate::FileError> {
    let ext = extension(path);
    if !super::IMPORT_EXTENSIONS.contains(&ext.as_str()) {
        return Err(error(format!("unsupported file type .{ext}")));
    }
    let (kind, _) = describe(path, bytes);
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
    Ok(())
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
    inventory_inner(project, previous, changed, true)
}

/// UI inventory: resource digests are supplied by background validation.
pub fn inventory_unhashed(project: &PackageProject, previous: &[FileInfo]) -> Vec<FileInfo> {
    inventory_inner(project, previous, "", false)
}

fn inventory_inner(
    project: &PackageProject,
    previous: &[FileInfo],
    changed: &str,
    hash_missing: bool,
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
                .filter(|f| f.bytes == bytes.len() as u64 && !f.hash.is_empty())
                .map_or_else(
                    || {
                        if hash_missing {
                            project
                                .cached_hash(path)
                                .map(|h| blake3::Hash::from_bytes(h).to_hex().to_string())
                                .unwrap_or_else(|_| blake3::hash(bytes).to_hex().to_string())
                        } else {
                            String::new()
                        }
                    },
                    |f| f.hash.clone(),
                );
            FileInfo {
                bytes: bytes.len() as u64,
                hash,
                editable: dependency.is_none()
                    && path != INTEGRITY_FILE
                    && kind.text()
                    && match &seal {
                        Ok(None) => true,
                        Ok(Some(s)) => s.editable.contains(path),
                        Err(_) => false,
                    },
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
