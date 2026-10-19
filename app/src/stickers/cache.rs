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

const VERSION: u32 = 3;
pub const MAX_SOURCE: u64 = 8 * 1024 * 1024;
const MAX_FRAMES: usize = 1024;
const MAX_PIXELS: u64 = 256 * 1024 * 1024;
pub const MAX_DISK: u64 = 512 * 1024 * 1024;

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
    pub thumbnail_hash: String,
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
    pub fn thumbnail(&self) -> String {
        crate::assets::file_url(&self.dir.join("thumbnail.png"))
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
        self.version == VERSION
            && !self.delays.is_empty()
            && self.delays.len() <= MAX_FRAMES
            && self.width > 0
            && self.height > 0
            && self.width <= 1024
            && self.height <= 1024
            && self.delays.iter().all(|d| *d >= 10 && *d <= 600_000)
            && self.hashes.len() == self.delays.len()
            && valid_hash(&self.dir.join("thumbnail.png"), &self.thumbnail_hash)
            && (0..self.delays.len()).all(|i| valid_hash(&self.frame_path(i), &self.hashes[i]))
    }
}

// Like rich::OBJECTS, this is render metadata only. It is replaced on the UI
// thread after a completed import, never published partially by a worker.
static IMAGES: LazyLock<RwLock<HashMap<String, Arc<CachedImage>>>> =
    LazyLock::new(Default::default);
static LIVE: LazyLock<Mutex<HashMap<String, Vec<Weak<CachedImage>>>>> =
    LazyLock::new(Default::default);
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
pub fn lookup(url: &str) -> Option<Arc<CachedImage>> {
    IMAGES.read().ok()?.get(url).cloned()
}
pub fn publish(images: impl Iterator<Item = Arc<CachedImage>>) {
    let mut catalog = IMAGES.write().unwrap_or_else(|e| e.into_inner());
    *catalog = images.map(|c| (c.url(), c)).collect();
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
    crate::assets::image_limits(4096, 128 * 1024 * 1024)
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
        thumbnail_hash: String::new(),
        dir: stage.into(),
    };
    let mut pixels = 0;
    let mut disk_bytes = 0;
    let mut save = |image: image::RgbaImage, delay: u64| -> Result<(), String> {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Cancelled".into());
        }
        let image = if image.width() > 1024 || image.height() > 1024 {
            image::DynamicImage::ImageRgba8(image)
                .thumbnail(1024, 1024)
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
            image::DynamicImage::ImageRgba8(image.clone())
                .thumbnail(104, 104)
                .save(stage.join("thumbnail.png"))
                .map_err(|e| e.to_string())?;
            cached.thumbnail_hash =
                blake3::hash(&fs::read(stage.join("thumbnail.png")).map_err(|e| e.to_string())?)
                    .to_hex()
                    .to_string();
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
            return Err("Sticker frame cache exceeds 256 MiB".into());
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
        Err("Active sticker cache exceeds 512 MiB".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            thumbnail_hash: String::new(),
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
