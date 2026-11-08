//! Persistent, content-addressed preprocessing. Original GIFs are never decoded
//! by the renderer; image's animation decoder composites disposal/alpha here.
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, RwLock, Weak};

use image::{AnimationDecoder, ImageDecoder};
use serde::{Deserialize, Serialize};

// Bumping this version moves the cache to a new folder; [`open`] deletes the
// previous one.
const VERSION: u32 = 5;
pub const MAX_SOURCE: u64 = 8 * 1024 * 1024;
/// One decoded frame serves both the list preview and actual rendering.
const MAX_DIMENSION: u32 = 128;
const MAX_FRAMES: usize = 1024;
const MAX_PIXELS: u64 = 64 * 1024 * 1024;
pub const MAX_DISK: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CachedImage {
    pub version: u32,
    pub key: String,
    pub width: u32,
    pub height: u32,
    pub delays: Vec<u64>,
    /// Total cycles; None means infinite.
    pub cycles: Option<u32>,
    pub hashes: Vec<String>,
    #[serde(skip)]
    pub dir: PathBuf,
}

impl CachedImage {
    pub fn frame_path(&self, frame: usize) -> PathBuf {
        self.dir.join(format!("{frame:04}.png"))
    }
    pub fn url(&self) -> String {
        crate::assets::file_url(&self.frame_path(0))
    }
    /// The first decoded frame is shared by the list preview and renderer.
    pub fn thumbnail(&self) -> String {
        crate::assets::file_url(&self.frame_path(0))
    }
    pub fn slot(&self) -> String {
        format!("sticker:{}", self.key)
    }
    pub fn animated(&self) -> bool {
        self.delays.len() > 1
    }
    /// Index and next absolute elapsed time; a finished animation stops waking.
    pub fn frame_at(&self, elapsed: u64) -> (usize, Option<u64>) {
        let total: u64 = self.delays.iter().sum();
        if !self.animated() || total == 0 {
            return (0, None);
        }
        if self.cycles.is_some_and(|n| elapsed / total >= u64::from(n)) {
            return (self.delays.len() - 1, None);
        }
        let t = elapsed % total;
        let mut end = 0;
        for (i, delay) in self.delays.iter().enumerate() {
            end += delay;
            if t < end {
                return (i, Some(elapsed - t + end));
            }
        }
        (0, None)
    }
    fn complete(&self) -> bool {
        if self.version != VERSION
            || self.delays.is_empty()
            || self.delays.len() > MAX_FRAMES
            || self.width == 0
            || self.height == 0
            || self.width > MAX_DIMENSION
            || self.height > MAX_DIMENSION
            || self.delays.iter().any(|d| *d < 10 || *d > 600_000)
            || self.hashes.len() != self.delays.len()
        {
            return false;
        }
        let mut disk_bytes = 0u64;
        let frame_pixels = u64::from(self.width)
            .checked_mul(u64::from(self.height))
            .and_then(|pixels| pixels.checked_mul(4));
        let Some(frame_pixels) = frame_pixels else {
            return false;
        };
        let mut pixels = 0u64;
        for (index, hash) in self.hashes.iter().enumerate() {
            let Some(bytes) = checked_file_bytes(
                &self.frame_path(index),
                hash,
                Some((self.width, self.height)),
            ) else {
                return false;
            };
            disk_bytes = disk_bytes.saturating_add(bytes);
            pixels = pixels.saturating_add(frame_pixels);
            if disk_bytes > MAX_DISK || pixels > MAX_PIXELS {
                return false;
            }
        }
        true
    }
}

// Like rich::OBJECTS, this is render metadata only. It is replaced on the UI
// thread after a completed import, never published partially by a worker.
static IMAGES: LazyLock<RwLock<HashMap<String, Arc<CachedImage>>>> =
    LazyLock::new(Default::default);
static LIVE: LazyLock<Mutex<HashMap<String, Vec<Weak<CachedImage>>>>> =
    LazyLock::new(Default::default);

/// The cache folder of this [`VERSION`] under `root`, after deleting every
/// other generation (older `v*` folders, and the entries and marker of the
/// flat layout before them). Content keys outlive a preprocessing policy
/// change, so a new policy gets a new folder rather than a per-entry check.
/// Call once, on the worker that owns the cache, before any [`prepare`].
/// Cleanup is best effort: whatever survives is retried on the next start.
pub fn open(root: &Path) -> PathBuf {
    let current = format!("v{VERSION}");
    for entry in fs::read_dir(root).into_iter().flatten().flatten() {
        if entry.file_name().to_str() == Some(current.as_str()) {
            continue;
        }
        let path = entry.path();
        let removed = match fs::symlink_metadata(&path) {
            Ok(m) if m.is_dir() => fs::remove_dir_all(&path),
            Ok(_) => fs::remove_file(&path),
            Err(e) => Err(e),
        };
        if let Err(e) = removed {
            nanatype_diag::report!(crate::diag::STICKER_CACHE_CLEANUP_FAILED; "{}: {e}", path.display());
        }
    }
    root.join(current)
}

fn tracked(image: CachedImage) -> Arc<CachedImage> {
    let image = Arc::new(image);
    let mut live = LIVE.lock().unwrap_or_else(|e| e.into_inner());
    live.entry(image.key.clone())
        .or_default()
        .push(Arc::downgrade(&image));
    image
}
fn valid_hash(path: &Path, hash: &str) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() <= MAX_SOURCE)
        && fs::read(path).is_ok_and(|bytes| blake3::hash(&bytes).to_hex().as_str() == hash)
}

/// Validate both the content hash and the decoded image dimensions before a
/// persisted cache entry is trusted. Hashes alone do not protect against a
/// caller that rewrites both metadata and files.
fn checked_file_bytes(path: &Path, hash: &str, dimensions: Option<(u32, u32)>) -> Option<u64> {
    let size = fs::metadata(path)
        .ok()
        .filter(|m| m.is_file() && m.len() <= MAX_SOURCE)?
        .len();
    if !valid_hash(path, hash) {
        return None;
    }
    let actual = image::image_dimensions(path).ok()?;
    if dimensions.is_some_and(|expected| actual != expected) {
        return None;
    }
    Some(size)
}
pub fn lookup(url: &str) -> Option<Arc<CachedImage>> {
    IMAGES.read().ok()?.get(url).cloned()
}
pub fn publish(images: impl Iterator<Item = Arc<CachedImage>>) {
    let mut catalog = IMAGES.write().unwrap_or_else(|e| e.into_inner());
    *catalog = images.map(|c| (c.url(), c)).collect();
    // Rich text draws the animated ones from their texture slots.
    nanatype_frame::ui::rich::publish_animated(
        catalog
            .iter()
            .filter(|(_, c)| c.animated())
            .map(|(url, c)| {
                let picture = nanatype_frame::ui::rich::AnimatedPicture {
                    slot: c.slot(),
                    width: c.width,
                    height: c.height,
                };
                (url.clone(), picture)
            })
            .collect(),
    );
}
pub fn animations() -> Vec<Arc<CachedImage>> {
    IMAGES
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .values()
        .filter(|c| c.animated())
        .cloned()
        .collect()
}
fn limits() -> image::Limits {
    crate::assets::image_limits(4096, MAX_PIXELS)
}

pub fn prepare(
    path: &Path,
    base: &Path,
    cancelled: &AtomicBool,
) -> Result<Arc<CachedImage>, String> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(MAX_SOURCE + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_SOURCE {
        return Err("Sticker image exceeds 8 MiB".into());
    }
    let mut hash = blake3::Hasher::new();
    hash.update(&VERSION.to_le_bytes());
    hash.update(&bytes);
    let key = hash.finalize().to_hex().to_string();
    let dir = base.join(&key);
    if let Ok(bytes) = fs::read(dir.join("image.json"))
        && let Ok(mut cached) = serde_json::from_slice::<CachedImage>(&bytes)
    {
        cached.dir = dir.clone();
        if cached.key == key && cached.complete() {
            return Ok(tracked(cached));
        }
    }
    fs::create_dir_all(base).map_err(|e| e.to_string())?;
    // A single import worker owns this cache. No two writers use this staging path.
    let stage = base.join(format!("{key}.part"));
    if stage.exists() {
        fs::remove_dir_all(&stage).map_err(|e| e.to_string())?;
    }
    fs::create_dir(&stage).map_err(|e| e.to_string())?;
    let result = transcode(&bytes, &stage, &key, cancelled);
    let mut cached = match result {
        Ok(c) => c,
        Err(e) => {
            let _ = fs::remove_dir_all(&stage);
            return Err(e);
        }
    };
    fs::write(
        stage.join("image.json"),
        serde_json::to_vec(&cached).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    fs::rename(&stage, &dir).map_err(|e| e.to_string())?;
    cached.dir = dir;
    Ok(tracked(cached))
}

fn transcode(
    bytes: &[u8],
    stage: &Path,
    key: &str,
    cancelled: &AtomicBool,
) -> Result<CachedImage, String> {
    let mut cached = CachedImage {
        version: VERSION,
        key: key.into(),
        width: 0,
        height: 0,
        delays: Vec::new(),
        cycles: Some(1),
        hashes: Vec::new(),
        dir: stage.into(),
    };
    let mut pixels = 0;
    let mut disk_bytes = 0;
    let mut save = |image: image::RgbaImage, delay: u64| -> Result<(), String> {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Cancelled".into());
        }
        let image = if image.width() > MAX_DIMENSION || image.height() > MAX_DIMENSION {
            image::DynamicImage::ImageRgba8(image)
                .thumbnail(MAX_DIMENSION, MAX_DIMENSION)
                .into_rgba8()
        } else {
            image
        };
        pixels += u64::from(image.width()) * u64::from(image.height()) * 4;
        if cached.delays.len() >= MAX_FRAMES || pixels > MAX_PIXELS {
            return Err("Decoded sticker exceeds frame/memory limit".into());
        }
        if cached.delays.is_empty() {
            cached.width = image.width();
            cached.height = image.height();
        }
        let path = cached.frame_path(cached.delays.len());
        image.save(&path).map_err(|e| e.to_string())?;
        cached.hashes.push(
            blake3::hash(&fs::read(&path).map_err(|e| e.to_string())?)
                .to_hex()
                .to_string(),
        );
        disk_bytes += fs::metadata(path).map_err(|e| e.to_string())?.len();
        if disk_bytes > MAX_DISK / 2 {
            return Err(format!(
                "Sticker frame cache exceeds {} MiB",
                MAX_DISK / 2 / (1024 * 1024)
            ));
        }
        cached.delays.push(delay.clamp(10, 600_000));
        Ok(())
    };
    let cycles;
    if bytes.starts_with(&[0x1f, 0x8b]) {
        super::formats::tgs(bytes, &mut save)?;
        cycles = None;
    } else if bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        super::formats::webm(bytes, &mut save)?;
        cycles = None;
    } else if image::guess_format(bytes).map_err(|e| e.to_string())? == image::ImageFormat::Gif {
        let mut decoder =
            image::codecs::gif::GifDecoder::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
        decoder.set_limits(limits()).map_err(|e| e.to_string())?;
        // gif::Repeat keeps the distinction between no loop extension (once)
        // and an explicit infinite loop. Finite values count repetitions.
        let info = gif::DecodeOptions::new()
            .read_info(Cursor::new(bytes))
            .map_err(|e| e.to_string())?;
        cycles = match info.repeat() {
            gif::Repeat::Infinite => None,
            gif::Repeat::Finite(n) => Some(u32::from(n) + 1),
        };
        for frame in decoder.into_frames() {
            let frame = frame.map_err(|e| e.to_string())?;
            let (num, den) = frame.delay().numer_denom_ms();
            save(frame.into_buffer(), u64::from(num) / u64::from(den.max(1)))?;
        }
    } else {
        let format = image::guess_format(bytes).map_err(|e| e.to_string())?;
        let mut reader = image::ImageReader::with_format(Cursor::new(bytes), format);
        reader.limits(limits());
        save(
            reader.decode().map_err(|e| e.to_string())?.into_rgba8(),
            100,
        )?;
        cycles = Some(1);
    }
    cached.cycles = cycles;
    if cached.delays.is_empty() {
        return Err("Image has no frames".into());
    }
    Ok(cached)
}

/// Active source revisions stay pinned. Reject an oversized active working set
/// instead of deleting frames that a window may still be sampling.
pub fn prune(base: &Path) -> Result<(), String> {
    let mut protected = HashSet::new();
    // Includes completed imports still queued for UI publication, and frames
    // retained by a renderer while an edited source is being rebuilt.
    let mut live = LIVE.lock().unwrap_or_else(|e| e.into_inner());
    live.retain(|key, images| {
        images.retain(|image| image.strong_count() > 0);
        if !images.is_empty() {
            protected.insert(key.clone());
            true
        } else {
            false
        }
    });
    drop(live);
    let mut dirs = Vec::new();
    for entry in fs::read_dir(base).into_iter().flatten().flatten() {
        let path = entry.path();
        if !path.is_dir()
            || path
                .symlink_metadata()
                .is_ok_and(|m| m.file_type().is_symlink())
        {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.len() != 64 || !name.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        let bytes: u64 = fs::read_dir(&path)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|f| f.metadata().ok())
            .map(|m| m.len())
            .sum();
        let modified = entry.metadata().and_then(|m| m.modified()).ok();
        dirs.push((path, name, bytes, modified));
    }
    let mut total: u64 = dirs.iter().map(|d| d.2).sum();
    dirs.sort_by_key(|d| d.3);
    for (path, name, bytes, _) in dirs {
        if total <= MAX_DISK {
            return Ok(());
        }
        if !protected.contains(&name) && fs::remove_dir_all(path).is_ok() {
            total = total.saturating_sub(bytes);
        }
    }
    if total > MAX_DISK {
        Err(format!(
            "Active sticker cache exceeds {} MiB",
            MAX_DISK / (1024 * 1024)
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opening_the_cache_deletes_other_generations() {
        let root =
            std::env::temp_dir().join(format!("nanatype-cache-version-{}", std::process::id()));
        let flat = root.join("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        let older = root.join("v4");
        let current = root.join(format!("v{VERSION}")).join("kept");
        for dir in [&flat, &older, &current] {
            fs::create_dir_all(dir).unwrap();
        }
        fs::write(root.join(".cache-version"), "3").unwrap();
        fs::write(flat.join("old-frame.png"), b"old").unwrap();

        assert_eq!(open(&root), root.join(format!("v{VERSION}")));

        assert!(!flat.exists() && !older.exists());
        assert!(!root.join(".cache-version").exists());
        assert!(current.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn oversized_frames_are_downscaled_to_shared_preview_size() {
        let base =
            std::env::temp_dir().join(format!("nanatype-gif-downscale-{}", std::process::id()));
        fs::create_dir_all(&base).unwrap();
        let source = base.join("large.gif");
        let mut writer = image::codecs::gif::GifEncoder::new(fs::File::create(&source).unwrap());
        writer
            .encode_frame(image::Frame::from_parts(
                image::RgbaImage::from_fn(256, 192, |x, y| {
                    image::Rgba([(x % 256) as u8, (y % 256) as u8, 128, 255])
                }),
                0,
                0,
                image::Delay::from_numer_denom_ms(40, 1),
            ))
            .unwrap();
        drop(writer);

        let cached = prepare(&source, &base.join("cache"), &AtomicBool::new(false)).unwrap();
        assert_eq!((cached.width, cached.height), (128, 96));
        assert_eq!(
            image::image_dimensions(cached.frame_path(0)).unwrap(),
            (128, 96)
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn timing_stops_at_finite_loop_end() {
        let c = CachedImage {
            version: VERSION,
            key: "test".into(),
            width: 2,
            height: 2,
            delays: vec![30, 70],
            cycles: Some(2),
            hashes: Vec::new(),
            dir: PathBuf::new(),
        };
        assert_eq!(c.frame_at(0), (0, Some(30)));
        assert_eq!(c.frame_at(30), (1, Some(100)));
        assert_eq!(c.frame_at(100), (0, Some(130)));
        assert_eq!(c.frame_at(200), (1, None));
    }
    #[test]
    fn composited_gif_cache_reuse_and_missing_frame_rebuild() {
        let base = std::env::temp_dir().join(format!("nanatype-gif-cache-{}", std::process::id()));
        fs::create_dir_all(&base).unwrap();
        let source = base.join("test.gif");
        let mut writer = image::codecs::gif::GifEncoder::new(fs::File::create(&source).unwrap());
        writer
            .set_repeat(image::codecs::gif::Repeat::Infinite)
            .unwrap();
        for color in [[255, 0, 0, 255], [0, 255, 0, 0]] {
            writer
                .encode_frame(image::Frame::from_parts(
                    image::RgbaImage::from_pixel(2, 2, image::Rgba(color)),
                    0,
                    0,
                    image::Delay::from_numer_denom_ms(40, 1),
                ))
                .unwrap();
        }
        drop(writer);
        let cache = base.join("cache");
        let c = prepare(&source, &cache, &AtomicBool::new(false)).unwrap();
        assert!(c.animated());
        assert_eq!(c.delays, [40, 40]);
        assert_eq!(c.cycles, None);
        assert_eq!(c.thumbnail(), crate::assets::file_url(&c.frame_path(0)));
        assert!(!c.dir.join("thumbnail.png").exists());
        let modified = fs::metadata(c.frame_path(0)).unwrap().modified().unwrap();
        let reused = prepare(&source, &cache, &AtomicBool::new(false)).unwrap();
        assert_eq!(
            fs::metadata(reused.frame_path(0))
                .unwrap()
                .modified()
                .unwrap(),
            modified
        );
        fs::remove_file(c.frame_path(1)).unwrap();
        assert!(
            prepare(&source, &cache, &AtomicBool::new(false))
                .unwrap()
                .frame_path(1)
                .is_file()
        );
        fs::write(c.frame_path(1), b"corrupt cached frame").unwrap();
        let repaired = prepare(&source, &cache, &AtomicBool::new(false)).unwrap();
        assert!(image::open(repaired.frame_path(1)).is_ok());
        let frame = image::open(c.frame_path(0)).unwrap().into_rgba8();
        assert_eq!(frame.get_pixel(0, 0).0, [255, 0, 0, 255]);
        let mut old: CachedImage =
            serde_json::from_slice(&fs::read(c.dir.join("image.json")).unwrap()).unwrap();
        old.version = 0;
        fs::write(c.dir.join("image.json"), serde_json::to_vec(&old).unwrap()).unwrap();
        assert_eq!(
            prepare(&source, &cache, &AtomicBool::new(false))
                .unwrap()
                .version,
            VERSION
        );
        let _ = fs::remove_dir_all(base);
    }
}
