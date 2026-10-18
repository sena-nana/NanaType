//! Pictures referenced by documents: sticker libraries, the active skin's
//! files, picked files and pasted images. NanaUI decodes and caches images
//! itself, so resolving an [`AssetRef`] means finding a `file://` URL it is
//! allowed to read; [`crate::services::media`] makes one for pictures
//! outside its base.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use nanatype_model::AssetRef;
use nanatype_theme::LoadedSkin;

use crate::services::media;

const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sticker {
    pub lib: String,
    pub name: String,
    pub path: PathBuf,
    /// The sticker folder from the settings that it was found in.
    pub root: PathBuf,
}

impl Sticker {
    pub fn asset(&self) -> AssetRef {
        AssetRef::Library {
            lib: self.lib.clone(),
            name: self.name.clone(),
        }
    }

    /// A URL NanaUI will load, or `None` when the picture cannot be shown.
    pub fn url(&self) -> Option<String> {
        media::url(&self.path, Some(&self.root))
    }
}

#[derive(Debug, Default)]
pub struct AssetLibrary {
    pub stickers: Vec<Sticker>,
    blob_dir: PathBuf,
}

pub fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| IMAGE_EXTS.contains(&e.to_ascii_lowercase().as_str()))
}

/// `skin:<path>` → URL for local, embedded and inherited skin artwork.
pub fn skin_urls(files: &dyn nanatype_theme::SkinFiles) -> HashMap<String, String> {
    skin_image_urls(files, files.list())
}

/// Static picker artwork only. GPU input textures are decoded by the effect
/// loader and need not also be materialized into the UI's image cache.
pub fn skin_preview_urls(
    skin: &nanatype_theme::Skin,
    files: &dyn nanatype_theme::SkinFiles,
) -> HashMap<String, String> {
    let mut paths: Vec<_> = skin
        .referenced_files()
        .into_iter()
        .filter(|(field, _, kind)| {
            matches!(
                kind,
                nanatype_theme::FileKind::Image | nanatype_theme::FileKind::NineSlice(_)
            ) && !matches!(*field, "shaders.textures" | "materials.textures")
        })
        .map(|(_, path, _)| path.to_owned())
        .collect();
    paths.sort();
    paths.dedup();
    skin_image_urls(files, paths)
}

fn skin_image_urls(
    files: &dyn nanatype_theme::SkinFiles,
    paths: Vec<String>,
) -> HashMap<String, String> {
    paths
        .into_iter()
        .filter(|path| is_image(Path::new(path)))
        .filter_map(|path| {
            // SkinFiles enforces its resource jail. Immutable content URLs
            // invalidate the painter when a local image is saved in place.
            let bytes = files.read(&path).ok()?;
            let url = media::global()?.bytes_url(&path, &bytes)?;
            Some((format!("skin:{path}"), url))
        })
        .collect()
}

/// URL of the skin file `path` (as the manifest names it) under `root`.
pub fn skin_file_url(root: &Path, path: &str) -> Option<String> {
    let clean = nanatype_theme::clean_path(path).ok()?;
    media::url(&root.join(clean), Some(root))
}

pub fn file_url(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let encoded: String = s
        .chars()
        .flat_map(|c| match c {
            ' ' => "%20".chars().collect::<Vec<_>>(),
            '#' => "%23".chars().collect(),
            '%' => "%25".chars().collect(),
            '?' => "%3F".chars().collect(),
            c => vec![c],
        })
        .collect();
    if encoded.starts_with('/') {
        format!("file://{encoded}")
    } else {
        format!("file:///{encoded}")
    }
}

impl AssetLibrary {
    pub fn new(blob_dir: PathBuf) -> Self {
        Self {
            stickers: Vec::new(),
            blob_dir,
        }
    }

    /// Scan sticker folders: images directly inside a folder belong to the
    /// library named after it; sub-folders become their own libraries.
    pub fn scan(&mut self, dirs: &[String]) {
        let mut out = Vec::new();
        for dir in dirs {
            let root = PathBuf::from(dir);
            let lib = root
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "stickers".into());
            collect(&root, &root, &lib, &mut out, 0);
        }
        out.sort_by(|a, b| (&a.lib, &a.name).cmp(&(&b.lib, &b.name)));
        out.dedup_by(|a, b| a.lib == b.lib && a.name == b.name);
        self.stickers = out;
    }

    pub fn resolve(&self, asset: &AssetRef, skin: Option<&LoadedSkin>) -> Option<PathBuf> {
        self.locate(asset, skin).map(|(path, _)| path)
    }

    /// The file of `asset` and the folder it must stay inside, if any.
    fn locate(
        &self,
        asset: &AssetRef,
        skin: Option<&LoadedSkin>,
    ) -> Option<(PathBuf, Option<PathBuf>)> {
        match asset {
            // A file the user picked or typed: no folder to stay in.
            AssetRef::File(path) => Some(PathBuf::from(path))
                .filter(|p| p.is_file())
                .map(|p| (p, None)),
            AssetRef::Blob(hash) => {
                let path = self.blob_dir.join(format!("{hash}.png"));
                path.is_file().then(|| (path, Some(self.blob_dir.clone())))
            }
            AssetRef::Library { lib, name } => self
                .stickers
                .iter()
                .find(|s| &s.lib == lib && &s.name == name)
                .map(|s| (s.path.clone(), Some(s.root.clone()))),
            AssetRef::Skin(path) => {
                let skin = skin?;
                let root = skin.files.root()?;
                let clean = nanatype_theme::clean_path(path).ok()?;
                let full = root.join(clean);
                full.is_file().then(|| (full, Some(root.to_path_buf())))
            }
        }
    }

    /// A URL NanaUI will load for `asset`, or `None` when it cannot be shown.
    pub fn url(&self, asset: &AssetRef, skin: Option<&LoadedSkin>) -> Option<String> {
        if let AssetRef::Skin(path) = asset {
            return skin_image_urls(&*skin?.files, vec![path.clone()]).remove(&asset.to_string());
        }
        let (path, root) = self.locate(asset, skin)?;
        media::url(&path, root.as_deref())
    }

    /// Store pasted RGBA pixels as a PNG named by content.
    pub fn store_rgba(&self, width: u32, height: u32, rgba: &[u8]) -> Result<AssetRef, String> {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for b in rgba
            .iter()
            .chain(&width.to_le_bytes())
            .chain(&height.to_le_bytes())
        {
            hash ^= u64::from(*b);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        let name = format!("{hash:016x}");
        let path = self.blob_dir.join(format!("{name}.png"));
        if !path.exists() {
            fs::create_dir_all(&self.blob_dir).map_err(|e| e.to_string())?;
            let img = image::RgbaImage::from_raw(width, height, rgba.to_vec())
                .ok_or_else(|| "bad image size".to_string())?;
            img.save(&path).map_err(|e| e.to_string())?;
        }
        Ok(AssetRef::Blob(name))
    }
}

fn collect(root: &Path, dir: &Path, lib: &str, out: &mut Vec<Sticker>, depth: u32) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth < 2 {
                let sub = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                collect(root, &path, &sub, out, depth + 1);
            }
        } else if is_image(&path) {
            let name = path
                .file_stem()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            out.push(Sticker {
                lib: lib.to_owned(),
                name,
                path,
                root: root.to_path_buf(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_urls_escape_spaces_and_hashes() {
        let url = file_url(Path::new(r"C:\My Stickers\#1.png"));
        assert_eq!(url, "file:///C:/My%20Stickers/%231.png");
        assert_eq!(file_url(Path::new("/tmp/a b.png")), "file:///tmp/a%20b.png");
    }

    #[test]
    fn sticker_folders_become_libraries() {
        let tmp = std::env::temp_dir().join(format!("nanatype-stickers-{}", std::process::id()));
        let cats = tmp.join("cats");
        fs::create_dir_all(cats.join("sub")).unwrap();
        fs::write(cats.join("hello.png"), b"x").unwrap();
        fs::write(cats.join("notes.txt"), b"x").unwrap();
        fs::write(cats.join("sub").join("wave.gif"), b"x").unwrap();
        let mut lib = AssetLibrary::new(tmp.join("blobs"));
        lib.scan(&[cats.display().to_string()]);
        let names: Vec<_> = lib
            .stickers
            .iter()
            .map(|s| format!("{}/{}", s.lib, s.name))
            .collect();
        assert_eq!(names, vec!["cats/hello", "sub/wave"]);
        let asset: AssetRef = "lib:cats/hello".parse().unwrap();
        assert!(lib.resolve(&asset, None).is_some());
        let blob = lib.store_rgba(1, 1, &[255, 0, 0, 255]).unwrap();
        assert!(lib.resolve(&blob, None).is_some());
        let _ = fs::remove_dir_all(&tmp);
    }
}
