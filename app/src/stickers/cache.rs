//! Persistent, content-addressed preprocessing. Original GIFs/WebPs are never
//! decoded by the renderer; frames are composited and linearly premultiplied here.
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, RwLock, Weak};
use std::time::{Duration, UNIX_EPOCH};

use flate2::{Compression, read::DeflateDecoder, write::DeflateEncoder};
use image::{AnimationDecoder, ImageDecoder};
use serde::{Deserialize, Serialize};

// Bumping this version moves the cache to a new folder; [`open`] deletes the
// previous one.
const VERSION: u32 = 6;
pub const MAX_SOURCE: u64 = 8 * 1024 * 1024;
/// One decoded frame serves both the list preview and actual rendering.
const MAX_DIMENSION: u32 = 128;
const MAX_FRAMES: usize = 1024;
const MAX_PIXELS: u64 = 64 * 1024 * 1024;
pub const MAX_DISK: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FileStat {
    pub size: u64,
    pub mtime_secs: u64,
    pub mtime_nanos: u32,
}

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
    #[serde(default)]
    pub(crate) preview_stat: Option<FileStat>,
    #[serde(default)]
    pub(crate) premul_stats: Vec<FileStat>,
    #[serde(skip)]
    pub dir: PathBuf,
}

impl CachedImage {
    pub fn frame_path(&self, frame: usize) -> PathBuf {
        self.dir.join(format!("{frame:04}.png"))
    }
    pub(crate) fn premul_path(&self, frame: usize) -> PathBuf {
        self.dir.join(format!("{frame:04}.premul"))
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
        let Some(frame_pixels) = u64::from(self.width)
            .checked_mul(u64::from(self.height))
            .and_then(|pixels| pixels.checked_mul(4))
        else {
            return false;
        };
        let Some(total_pixels) = frame_pixels.checked_mul(self.delays.len() as u64) else {
            return false;
        };
        if total_pixels > MAX_PIXELS {
            return false;
        }
        let Some(preview) = self.preview_stat else {
            return false;
        };
        if stat_path(&self.frame_path(0)) != Some(preview) {
            return false;
        }
        let mut disk_bytes = preview.size;
        if self.animated() {
            if self.premul_stats.len() != self.delays.len() {
                return false;
            }
            for (index, expected) in self.premul_stats.iter().enumerate() {
                if stat_path(&self.premul_path(index)) != Some(*expected) {
                    return false;
                }
                disk_bytes = disk_bytes.saturating_add(expected.size);
                if disk_bytes > MAX_DISK {
                    return false;
                }
            }
        }
        disk_bytes <= MAX_DISK
    }
}

// Like rich::OBJECTS, this is render metadata only. It is replaced on the UI
// thread after a completed import, never published partially by a worker.
static IMAGES: LazyLock<RwLock<HashMap<String, Arc<CachedImage>>>> =
    LazyLock::new(Default::default);
static LIVE: LazyLock<Mutex<HashMap<String, Vec<Weak<CachedImage>>>>> =
    LazyLock::new(Default::default);
static CATALOG_VERSION: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Serialize, Deserialize)]
struct PackIndex {
    version: u32,
    entries: Vec<PackEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PackEntry {
    rel_path: PathBuf,
    source_stat: FileStat,
    cached: CachedImage,
}

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

fn stat_metadata(m: &fs::Metadata) -> Option<FileStat> {
    if !m.is_file() || m.len() > MAX_SOURCE {
        return None;
    }
    let modified = m.modified().ok()?;
    let since = modified
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO);
    Some(FileStat {
        size: m.len(),
        mtime_secs: since.as_secs(),
        mtime_nanos: since.subsec_nanos(),
    })
}

fn stat_path(path: &Path) -> Option<FileStat> {
    fs::metadata(path).ok().and_then(|m| stat_metadata(&m))
}

fn write_with_stat(path: &Path, bytes: &[u8]) -> Result<FileStat, String> {
    let mut file = fs::File::create(path).map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    stat_metadata(&meta).ok_or_else(|| "Written cache file exceeds size limit".to_string())
}

pub fn lookup(url: &str) -> Option<Arc<CachedImage>> {
    IMAGES.read().ok()?.get(url).cloned()
}
pub fn publish(images: impl Iterator<Item = Arc<CachedImage>>) {
    let mut catalog = IMAGES.write().unwrap_or_else(|e| e.into_inner());
    *catalog = images.map(|c| (c.url(), c)).collect();
    CATALOG_VERSION.fetch_add(1, Ordering::Release);
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
pub(crate) fn catalog_version() -> u64 {
    CATALOG_VERSION.load(Ordering::Acquire)
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

/// Straight-alpha sRGB bytes to sRGB-encoded premultiplied linear light, so
/// an sRGB view samples premultiplied linear colour. Opaque texels keep their
/// bytes; only partially covered ones are weighted in linear light.
pub(crate) fn premultiply_linear(pixels: &mut [u8]) {
    static DECODE: std::sync::OnceLock<[f32; 256]> = std::sync::OnceLock::new();
    let decode = DECODE.get_or_init(|| {
        std::array::from_fn(|i| {
            let v = i as f32 / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        })
    });
    let encode = |v: f32| {
        let v = if v <= 0.003_130_8 {
            v * 12.92
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        };
        (v * 255.0 + 0.5) as u8
    };
    for pixel in pixels.as_chunks_mut::<4>().0 {
        match pixel[3] {
            255 => {}
            0 => pixel[..3].fill(0),
            a => {
                let a = f32::from(a) / 255.0;
                for c in &mut pixel[..3] {
                    *c = encode(decode[usize::from(*c)] * a);
                }
            }
        }
    }
}

/// Read a pre-baked linearly premultiplied frame into a shared buffer.
pub(crate) fn read_premultiplied_frame(
    cached: &CachedImage,
    index: usize,
) -> Result<Arc<[u8]>, String> {
    if index >= cached.delays.len() {
        return Err("Cached frame index out of bounds".into());
    }
    let expected = (cached.width as usize)
        .checked_mul(cached.height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or("Cached frame dimensions overflow")?;
    if !cached.animated() {
        let path = cached.frame_path(index);
        if fs::metadata(&path).map_err(|e| e.to_string())?.len() > MAX_SOURCE {
            return Err("Cached frame exceeds size limit".into());
        }
        let mut reader = image::ImageReader::open(&path).map_err(|e| e.to_string())?;
        reader.limits(crate::assets::image_limits(1024, 16 * 1024 * 1024));
        let image = reader.decode().map_err(|e| e.to_string())?.into_rgba8();
        if image.dimensions() != (cached.width, cached.height) {
            return Err("Cached frame size does not match its sticker".into());
        }
        let mut pixels = image.into_raw();
        premultiply_linear(&mut pixels);
        return Ok(Arc::from(pixels));
    }
    let path = cached.premul_path(index);
    let len = fs::metadata(&path).map_err(|e| e.to_string())?.len();
    if len == 0 || len > MAX_SOURCE {
        return Err("Cached premultiplied frame exceeds size limit".into());
    }
    let bytes = fs::read(&path).map_err(|e| e.to_string())?;
    match bytes.first() {
        Some(0) => {
            let payload = &bytes[1..];
            if payload.len() != expected {
                return Err("Cached premultiplied frame size mismatch".into());
            }
            Ok(Arc::from(payload))
        }
        Some(1) => {
            let mut pixels = Vec::with_capacity(expected);
            DeflateDecoder::new(&bytes[1..])
                .take(expected as u64 + 1)
                .read_to_end(&mut pixels)
                .map_err(|e| e.to_string())?;
            if pixels.len() != expected {
                return Err("Cached compressed frame size mismatch".into());
            }
            Ok(Arc::from(pixels))
        }
        _ => Err("Invalid cached premultiplied frame format".into()),
    }
}

/// Prepare every sticker entry in a pack using a per-pack `(mtime, byte_size)`
/// manifest index in `base/packs/`, falling back to full [`prepare`] on misses.
pub(crate) fn prepare_pack(
    pack_root: &Path,
    entries: &[PathBuf],
    base: &Path,
    verify_source: bool,
    cancelled: &AtomicBool,
    mut progress: impl FnMut(usize, usize),
) -> Result<Vec<Arc<CachedImage>>, String> {
    let pack_hash = blake3::hash(pack_root.to_string_lossy().as_bytes())
        .to_hex()
        .to_string();
    let idx_dir = base.join("packs");
    let idx_path = idx_dir.join(format!("{pack_hash}.idx"));
    let loaded = if verify_source {
        None
    } else {
        fs::read(&idx_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<PackIndex>(&bytes).ok())
            .filter(|idx| idx.version == VERSION)
    };
    let existing_len = loaded.as_ref().map_or(0, |idx| idx.entries.len());
    let existing: HashMap<&Path, &PackEntry> = loaded
        .as_ref()
        .map(|idx| {
            idx.entries
                .iter()
                .map(|e| (e.rel_path.as_path(), e))
                .collect()
        })
        .unwrap_or_default();

    let mut images = Vec::with_capacity(entries.len());
    let mut next_entries = Vec::with_capacity(entries.len());
    let mut dirty = verify_source || loaded.is_none();

    for (i, rel_path) in entries.iter().enumerate() {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Cancelled".into());
        }
        progress(i, entries.len());
        let source = super::definition::contained(pack_root, &pack_root.join(rel_path))?;
        let src_stat = stat_path(&source);
        if !verify_source
            && let Some(src_stat) = src_stat
            && let Some(entry) = existing.get(rel_path.as_path())
            && entry.source_stat == src_stat
        {
            let mut cached = entry.cached.clone();
            cached.dir = base.join(&cached.key);
            if cached.complete() {
                next_entries.push(PackEntry {
                    rel_path: rel_path.clone(),
                    source_stat: src_stat,
                    cached: cached.clone(),
                });
                images.push(tracked(cached));
                continue;
            }
        }
        dirty = true;
        let cached = prepare(&source, base, cancelled)
            .map_err(|e| format!("{}: {e}", rel_path.display()))?;
        if let Some(src_stat) = stat_path(&source) {
            next_entries.push(PackEntry {
                rel_path: rel_path.clone(),
                source_stat: src_stat,
                cached: (*cached).clone(),
            });
        }
        images.push(cached);
    }

    if (dirty || next_entries.len() != existing_len)
        && let Ok(encoded) = serde_json::to_vec(&PackIndex {
            version: VERSION,
            entries: next_entries,
        })
        && fs::create_dir_all(&idx_dir).is_ok()
    {
        let part = idx_dir.join(format!("{pack_hash}.idx.part"));
        if fs::write(&part, &encoded).is_ok() {
            let _ = fs::rename(&part, &idx_path);
        }
    }

    Ok(images)
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

fn encode_png_in_memory(image: &image::RgbaImage) -> Result<Vec<u8>, String> {
    let mut png_bytes = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut png_bytes), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(png_bytes)
}

fn encode_premul_in_memory(image: image::RgbaImage) -> Result<Vec<u8>, String> {
    let mut raw = image.into_raw();
    premultiply_linear(&mut raw);
    if raw.len() > 64 {
        let mut compressed = Vec::with_capacity(raw.len() / 2 + 16);
        compressed.push(1);
        {
            let mut encoder = DeflateEncoder::new(&mut compressed, Compression::fast());
            encoder.write_all(&raw).map_err(|e| e.to_string())?;
            encoder.finish().map_err(|e| e.to_string())?;
        }
        if compressed.len() < 1 + raw.len() {
            return Ok(compressed);
        }
    }
    let mut plain = Vec::with_capacity(1 + raw.len());
    plain.push(0);
    plain.extend_from_slice(&raw);
    Ok(plain)
}

fn check_disk_budget(disk_bytes: u64) -> Result<(), String> {
    if disk_bytes > MAX_DISK / 2 {
        Err(format!(
            "Sticker frame cache exceeds {} MiB",
            MAX_DISK / 2 / (1024 * 1024)
        ))
    } else {
        Ok(())
    }
}

fn write_preview_png(
    cached: &mut CachedImage,
    image: &image::RgbaImage,
    disk_bytes: &mut u64,
) -> Result<(FileStat, String), String> {
    let png_bytes = encode_png_in_memory(image)?;
    let hash = blake3::hash(&png_bytes).to_hex().to_string();
    let stat = write_with_stat(&cached.frame_path(0), &png_bytes)?;
    *disk_bytes = disk_bytes.saturating_add(stat.size);
    check_disk_budget(*disk_bytes)?;
    cached.preview_stat = Some(stat);
    Ok((stat, hash))
}

fn write_premul_frame(
    cached: &mut CachedImage,
    image: image::RgbaImage,
    delay: u64,
    disk_bytes: &mut u64,
) -> Result<(), String> {
    let index = cached.delays.len();
    let encoded = encode_premul_in_memory(image)?;
    let hash = blake3::hash(&encoded).to_hex().to_string();
    let stat = write_with_stat(&cached.premul_path(index), &encoded)?;
    *disk_bytes = disk_bytes.saturating_add(stat.size);
    check_disk_budget(*disk_bytes)?;
    cached.hashes.push(hash);
    cached.premul_stats.push(stat);
    cached.delays.push(delay);
    Ok(())
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
        preview_stat: None,
        premul_stats: Vec::new(),
        dir: stage.into(),
    };
    let mut pixels = 0;
    let mut disk_bytes = 0;
    let mut first_frame: Option<(image::RgbaImage, u64)> = None;
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
        let frame_count = cached.delays.len() + usize::from(first_frame.is_some());
        pixels += u64::from(image.width()) * u64::from(image.height()) * 4;
        if frame_count >= MAX_FRAMES || pixels > MAX_PIXELS {
            return Err("Decoded sticker exceeds frame/memory limit".into());
        }
        let delay = delay.clamp(10, 600_000);
        if frame_count == 0 {
            cached.width = image.width();
            cached.height = image.height();
            first_frame = Some((image, delay));
            return Ok(());
        }
        if image.dimensions() != (cached.width, cached.height) {
            return Err("Animated frame dimensions do not match".into());
        }
        if let Some((first, first_delay)) = first_frame.take() {
            write_preview_png(&mut cached, &first, &mut disk_bytes)?;
            write_premul_frame(&mut cached, first, first_delay, &mut disk_bytes)?;
        }
        write_premul_frame(&mut cached, image, delay, &mut disk_bytes)
    };
    let cycles;
    if bytes.starts_with(&[0x1f, 0x8b]) {
        super::formats::tgs(bytes, &mut save)?;
        cycles = None;
    } else if bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        super::formats::webm(bytes, &mut save)?;
        cycles = None;
    } else {
        let format = image::guess_format(bytes).map_err(|e| e.to_string())?;
        if format == image::ImageFormat::Gif {
            let mut decoder = image::codecs::gif::GifDecoder::new(Cursor::new(bytes))
                .map_err(|e| e.to_string())?;
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
        } else if format == image::ImageFormat::WebP {
            let mut decoder = image::codecs::webp::WebPDecoder::new(Cursor::new(bytes))
                .map_err(|e| e.to_string())?;
            decoder.set_limits(limits()).map_err(|e| e.to_string())?;
            let (w, h) = decoder.dimensions();
            if w == 0
                || h == 0
                || w > 4096
                || h > 4096
                || u64::from(w) * u64::from(h) * 4 > MAX_PIXELS
            {
                return Err("WebP dimensions exceed limits".into());
            }
            if decoder.has_animation() {
                cycles = match decoder.loop_count() {
                    image::metadata::LoopCount::Infinite => None,
                    image::metadata::LoopCount::Finite(n) => Some(n.get()),
                };
                for frame in decoder.into_frames() {
                    let frame = frame.map_err(|e| e.to_string())?;
                    let (num, den) = frame.delay().numer_denom_ms();
                    save(frame.into_buffer(), u64::from(num) / u64::from(den.max(1)))?;
                }
            } else {
                let mut reader = image::ImageReader::with_format(Cursor::new(bytes), format);
                reader.limits(limits());
                save(
                    reader.decode().map_err(|e| e.to_string())?.into_rgba8(),
                    100,
                )?;
                cycles = Some(1);
            }
        } else {
            let mut reader = image::ImageReader::with_format(Cursor::new(bytes), format);
            reader.limits(limits());
            save(
                reader.decode().map_err(|e| e.to_string())?.into_rgba8(),
                100,
            )?;
            cycles = Some(1);
        }
    }
    drop(save);
    if let Some((first, first_delay)) = first_frame {
        let (_, hash) = write_preview_png(&mut cached, &first, &mut disk_bytes)?;
        cached.hashes.push(hash);
        cached.delays.push(first_delay);
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
            preview_stat: None,
            premul_stats: Vec::new(),
            dir: PathBuf::new(),
        };
        assert_eq!(c.frame_at(0), (0, Some(30)));
        assert_eq!(c.frame_at(30), (1, Some(100)));
        assert_eq!(c.frame_at(100), (0, Some(130)));
        assert_eq!(c.frame_at(200), (1, None));
    }

    #[test]
    fn premultiplies_in_linear_light() {
        let mut pixels = [
            119, 60, 200, 255, // opaque: bytes unchanged
            200, 10, 90, 0, // transparent: colour cleared
            255, 255, 255, 128, // half-covered white: half its light
        ];
        premultiply_linear(&mut pixels);
        assert_eq!(pixels[..8], [119, 60, 200, 255, 0, 0, 0, 0]);
        // OETF(128 / 255) is 188, where a gamma-space premultiply gives 128.
        assert_eq!(pixels[8..], [188, 188, 188, 128]);
    }

    #[test]
    fn composited_gif_cache_reuse_and_missing_frame_rebuild() {
        let base = std::env::temp_dir().join(format!("nanatype-gif-cache-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
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
        assert!(c.premul_path(0).is_file());
        assert!(c.premul_path(1).is_file());
        let modified = fs::metadata(c.frame_path(0)).unwrap().modified().unwrap();
        let reused = prepare(&source, &cache, &AtomicBool::new(false)).unwrap();
        assert_eq!(
            fs::metadata(reused.frame_path(0))
                .unwrap()
                .modified()
                .unwrap(),
            modified
        );
        fs::remove_file(c.premul_path(1)).unwrap();
        assert!(
            prepare(&source, &cache, &AtomicBool::new(false))
                .unwrap()
                .premul_path(1)
                .is_file()
        );
        fs::write(c.premul_path(1), b"corrupt cached frame").unwrap();
        let repaired = prepare(&source, &cache, &AtomicBool::new(false)).unwrap();
        assert!(read_premultiplied_frame(&repaired, 1).is_ok());
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

    fn animated_webp_bytes(frames: &[([u8; 4], u32)], loop_count: u16) -> Vec<u8> {
        let mut body = Vec::new();
        let push_chunk = |out: &mut Vec<u8>, tag: &[u8; 4], payload: &[u8]| {
            out.extend_from_slice(tag);
            out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            out.extend_from_slice(payload);
            if payload.len() % 2 == 1 {
                out.push(0);
            }
        };
        let mut vp8x = [0u8; 10];
        vp8x[0] = 0x12; // animation + alpha
        vp8x[4] = 1; // width - 1 = 1 (2px)
        vp8x[7] = 1; // height - 1 = 1 (2px)
        push_chunk(&mut body, b"VP8X", &vp8x);

        let mut anim = [0u8; 6];
        anim[4..6].copy_from_slice(&loop_count.to_le_bytes());
        push_chunk(&mut body, b"ANIM", &anim);

        for (rgba, delay_ms) in frames {
            let mut single = Vec::new();
            image::codecs::webp::WebPEncoder::new_lossless(&mut single)
                .encode(&rgba.repeat(4), 2, 2, image::ExtendedColorType::Rgba8)
                .unwrap();
            let mut anmf = Vec::new();
            anmf.extend_from_slice(&[0; 6]); // x/2, y/2
            anmf.extend_from_slice(&[1, 0, 0, 1, 0, 0]); // width-1, height-1
            let d = delay_ms.to_le_bytes();
            anmf.extend_from_slice(&d[..3]);
            anmf.push(0x02); // do not blend
            anmf.extend_from_slice(&single[12..]); // VP8L chunk
            push_chunk(&mut body, b"ANMF", &anmf);
        }

        let mut riff = Vec::new();
        riff.extend_from_slice(b"RIFF");
        riff.extend_from_slice(&((4 + body.len()) as u32).to_le_bytes());
        riff.extend_from_slice(b"WEBP");
        riff.extend_from_slice(&body);
        riff
    }

    #[test]
    fn animated_webp_decodes_frames_delays_loops_and_premultiplies() {
        let base = std::env::temp_dir().join(format!("nanatype-anim-webp-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let source = base.join("anim.webp");
        fs::write(
            &source,
            animated_webp_bytes(&[([255, 255, 255, 128], 50), ([0, 255, 0, 255], 80)], 3),
        )
        .unwrap();

        let cache = base.join("cache");
        let cached = prepare(&source, &cache, &AtomicBool::new(false)).unwrap();
        assert!(cached.animated());
        assert_eq!(cached.delays, [50, 80]);
        assert_eq!(cached.cycles, Some(3));
        // Frame 0 preview PNG preserves straight alpha for UI/file:// consumers.
        let preview = image::open(cached.frame_path(0)).unwrap().into_rgba8();
        assert_eq!(preview.get_pixel(0, 0).0, [255, 255, 255, 128]);
        // Pre-baked frames are linearly premultiplied for GPU upload.
        let f0 = read_premultiplied_frame(&cached, 0).unwrap();
        assert_eq!(&f0[..4], &[188, 188, 188, 128]);
        let f1 = read_premultiplied_frame(&cached, 1).unwrap();
        assert_eq!(&f1[..4], &[0, 255, 0, 255]);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn pack_index_reuses_entries_by_mtime_and_size_without_rehash() {
        let base = std::env::temp_dir().join(format!("nanatype-pack-idx-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let pack = base.join("pack");
        let cache = base.join("cache");
        fs::create_dir_all(&pack).unwrap();
        let img = pack.join("a.png");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]))
            .save(&img)
            .unwrap();
        let entries = vec![PathBuf::from("a.png")];
        let first = prepare_pack(
            &pack,
            &entries,
            &cache,
            false,
            &AtomicBool::new(false),
            |_, _| {},
        )
        .unwrap();
        let key_before = first[0].key.clone();
        // Second call uses PackIndex (mtime + size) and returns the same key.
        let second = prepare_pack(
            &pack,
            &entries,
            &cache,
            false,
            &AtomicBool::new(false),
            |_, _| {},
        )
        .unwrap();
        assert_eq!(second[0].key, key_before);
        // Deleting the cached PNG triggers fallback & repair.
        fs::remove_file(second[0].frame_path(0)).unwrap();
        let repaired = prepare_pack(
            &pack,
            &entries,
            &cache,
            false,
            &AtomicBool::new(false),
            |_, _| {},
        )
        .unwrap();
        assert!(repaired[0].frame_path(0).is_file());
        let _ = fs::remove_dir_all(base);
    }
}
