//! Pictures for skins and documents: decoder limits and the `file://` and
//! content URLs NanaUI is allowed to read; [`crate::media`] makes one for
//! pictures outside its base.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::media;

const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp"];

/// Decoder limits for images from outside the app: a few compressed bytes
/// must not decode into hundreds of megabytes.
pub fn image_limits(max_side: u32, max_alloc: u64) -> image::Limits {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(max_side);
    limits.max_image_height = Some(max_side);
    limits.max_alloc = Some(max_alloc);
    limits
}

/// Decode untrusted image bytes under [`image_limits`].
pub fn decode_image(
    bytes: &[u8],
    max_side: u32,
    max_alloc: u64,
) -> image::ImageResult<image::DynamicImage> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    reader.limits(image_limits(max_side, max_alloc));
    reader.decode()
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

pub fn skin_image_urls(
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

/// The path of a [`file_url`]; `None` for anything else.
pub fn file_path(url: &str) -> Option<PathBuf> {
    let raw = url.strip_prefix("file://")?;
    // `file:///C:/x` carries a drive after the authority's slash.
    let raw = if raw.as_bytes().get(2) == Some(&b':') && raw.starts_with('/') {
        &raw[1..]
    } else {
        raw
    };
    // `%25` last, so an encoded `%` never re-forms another escape.
    Some(PathBuf::from(
        raw.replace("%20", " ")
            .replace("%23", "#")
            .replace("%3F", "?")
            .replace("%25", "%"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_urls_escape_spaces_and_hashes() {
        let url = file_url(Path::new(r"C:\My Stickers\#1.png"));
        assert_eq!(url, "file:///C:/My%20Stickers/%231.png");
        assert_eq!(file_url(Path::new("/tmp/a b.png")), "file:///tmp/a%20b.png");
        for path in [
            "/Users/a b/#1%3F?.png",
            "/tmp/表情.png",
            "C:/My Stickers/x.png",
        ] {
            assert_eq!(
                file_path(&file_url(Path::new(path))).unwrap(),
                Path::new(path)
            );
        }
        assert_eq!(file_path("https://example.test/a.png"), None);
    }
}
