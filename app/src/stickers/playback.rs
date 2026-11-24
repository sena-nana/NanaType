//! GPU playback of pre-baked linearly premultiplied cache frames. Each window
//! reports actual painted slots, so hidden/clipped animations do not keep windows
//! repainting, and a bounded background pool prefetches `frame + 1` into `Arc<[u8]>`.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::thread::JoinHandle;

use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use nana_ui::{
    GpuContext, GpuTexture, GpuTextureDescriptor, GpuTextureFormat, GpuTextureRegion,
    GpuTextureUsages, HostTexture, HostTextureAlphaMode, HostTextureRegistry,
};
use nana_ui_platform::WindowId;

use super::cache::{self, CachedImage};

const MAX_GPU: u64 = 128 * 1024 * 1024;
/// Slots nobody paints keep their last frame resident (a catalog's first frames
/// are its previews). Bound that idle share so a large catalog cannot consume
/// unbounded VRAM; past it, idle slots park on a 1x1 placeholder.
const MAX_IDLE_GPU: u64 = 32 * 1024 * 1024;
const PLACEHOLDER_BYTES: u64 = 4;
const PREDECODE_WORKERS: usize = 2;
const PREDECODE_QUEUE: usize = 64;

struct DecodeReq {
    slot: String,
    cache: Arc<CachedImage>,
    frame: usize,
}

struct DecodedFrame {
    slot: String,
    key: String,
    frame: usize,
    result: Result<Arc<[u8]>, String>,
}

#[derive(Default)]
struct Predecoder {
    tx: Option<Sender<DecodeReq>>,
    rx: Option<Receiver<DecodedFrame>>,
    workers: Vec<JoinHandle<()>>,
}

impl Predecoder {
    fn ensure_started(&mut self) {
        if self.tx.is_some() {
            return;
        }
        let (req_tx, req_rx) = bounded::<DecodeReq>(PREDECODE_QUEUE);
        let (res_tx, res_rx) = unbounded::<DecodedFrame>();
        for n in 0..PREDECODE_WORKERS {
            let req_rx = req_rx.clone();
            let res_tx = res_tx.clone();
            if let Ok(handle) = std::thread::Builder::new()
                .name(format!("nanatype-sticker-predecode-{n}"))
                .spawn(move || {
                    for req in req_rx {
                        let result = cache::read_premultiplied_frame(&req.cache, req.frame);
                        let _ = res_tx.send(DecodedFrame {
                            slot: req.slot,
                            key: req.cache.key.clone(),
                            frame: req.frame,
                            result,
                        });
                    }
                })
            {
                self.workers.push(handle);
            }
        }
        self.tx = Some(req_tx);
        self.rx = Some(res_rx);
    }

    fn request(&mut self, slot: &str, cache: &Arc<CachedImage>, frame: usize) -> bool {
        self.ensure_started();
        let Some(tx) = &self.tx else {
            return false;
        };
        tx.try_send(DecodeReq {
            slot: slot.to_owned(),
            cache: cache.clone(),
            frame,
        })
        .is_ok()
    }

    fn drain(&self, slots: &mut HashMap<String, Playing>) -> bool {
        let Some(rx) = &self.rx else {
            return false;
        };
        let mut any_failed = false;
        while let Ok(msg) = rx.try_recv() {
            if let Some(playing) = slots.get_mut(&msg.slot)
                && playing.cache.key == msg.key
            {
                if playing.inflight == Some(msg.frame) {
                    playing.inflight = None;
                }
                match msg.result {
                    Ok(pixels) => playing.prefetched = Some((msg.frame, pixels)),
                    Err(_) => {
                        playing.prefetched = None;
                        playing.failed = true;
                        any_failed = true;
                    }
                }
            }
        }
        any_failed
    }
}

impl Drop for Predecoder {
    fn drop(&mut self) {
        self.tx.take();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

struct Playing {
    cache: Arc<CachedImage>,
    gpu: GpuTexture,
    host: HostTexture,
    /// The frame the full-size texture holds; `None` while parked on the
    /// 1x1 placeholder.
    frame: Option<usize>,
    epoch: Option<u64>,
    bytes: u64,
    failed: bool,
    blocked: bool,
    prefetched: Option<(usize, Arc<[u8]>)>,
    inflight: Option<usize>,
}

#[derive(Default)]
pub struct Playback {
    slots: HashMap<String, Playing>,
    windows: HashMap<WindowId, HostTextureRegistry>,
    generation: Option<nana_ui::DeviceGeneration>,
    catalog_version: Option<u64>,
    rebuild: bool,
    pool: Predecoder,
}

impl Playback {
    pub fn registry(&self, id: WindowId) -> Option<HostTextureRegistry> {
        self.windows.get(&id).cloned()
    }
    pub fn discard(&mut self, id: WindowId) {
        self.windows.remove(&id);
        // No renderer can sample these slots once the last window is gone.
        // Drop them eagerly instead of keeping every resident frame until the
        // next device reset or catalog refresh.
        if self.windows.is_empty() {
            self.slots.clear();
            self.catalog_version = None;
        }
    }
    pub fn take_rebuild(&mut self) -> bool {
        std::mem::take(&mut self.rebuild)
    }

    pub fn next_at(&self, id: WindowId, now: u64, reduced: bool) -> Option<u64> {
        let registry = self.windows.get(&id)?;
        self.slots
            .iter()
            .filter(|(slot, s)| !s.failed && !s.blocked && is_painted(registry, slot))
            .filter_map(|(_, s)| {
                let elapsed = now.saturating_sub(s.epoch.unwrap_or(now));
                let (index, next) = if reduced {
                    (0, None)
                } else {
                    s.cache.frame_at(elapsed)
                };
                if s.epoch.is_none() || s.frame != Some(index) {
                    Some(now)
                } else {
                    next.map(|t| s.epoch.unwrap_or(now).saturating_add(t))
                }
            })
            .min()
    }

    pub fn prepare(&mut self, id: WindowId, gpu: &GpuContext, now: u64, reduced: bool) {
        if self.generation != Some(gpu.generation()) {
            for registry in self.windows.values() {
                for slot in self.slots.keys() {
                    registry.remove(slot);
                }
            }
            self.slots.clear();
            self.generation = Some(gpu.generation());
            self.catalog_version = None;
        }

        if self.pool.drain(&mut self.slots) {
            self.rebuild = true;
        }

        let current_catalog = cache::catalog_version();
        let new_window = !self.windows.contains_key(&id);
        if self.catalog_version != Some(current_catalog) || new_window {
            let images = cache::animations();
            let wanted: HashSet<String> = images.iter().map(|c| c.slot()).collect();
            for (slot, _) in self.slots.iter().filter(|(s, _)| !wanted.contains(*s)) {
                for registry in self.windows.values() {
                    registry.remove(slot);
                }
            }
            self.slots.retain(|slot, _| wanted.contains(slot));

            let mut idle: u64 = self
                .slots
                .iter()
                .filter(|(slot, p)| {
                    p.frame.is_some() && !self.windows.values().any(|r| is_painted(r, slot))
                })
                .map(|(_, p)| p.bytes)
                .sum();
            let mut used: u64 = self.slots.values().map(|p| p.bytes).sum();
            let registry = self.windows.entry(id).or_default();

            for cache in images {
                let slot = cache.slot();
                if !self.slots.contains_key(&slot) {
                    // Frame 0 doubles as the preview, so the first paint shows real
                    // content and a visible static sticker never uploads twice.
                    let bytes = frame_bytes(&cache);
                    let admitted = idle + bytes <= MAX_IDLE_GPU && used + bytes <= MAX_GPU;
                    let (pixels, failed) = if admitted {
                        match cache::read_premultiplied_frame(&cache, 0) {
                            Ok(px) => (Some(px), false),
                            Err(e) => {
                                nanatype_diag::report!(crate::diag::STICKER_PREVIEW_FAILED; "{e}");
                                self.rebuild = true;
                                (None, true)
                            }
                        }
                    } else {
                        (None, false)
                    };
                    let uploaded = match &pixels {
                        Some(px) => texture(gpu, cache.width, cache.height, px),
                        None => placeholder(gpu),
                    };
                    let Ok(texture) = uploaded else {
                        continue;
                    };
                    let bytes = if pixels.is_some() {
                        idle += bytes;
                        bytes
                    } else {
                        PLACEHOLDER_BYTES
                    };
                    used += bytes;
                    let host = HostTexture::new(&texture);
                    let mut inflight = None;
                    if pixels.is_some()
                        && !reduced
                        && cache.delays.len() > 1
                        && self.pool.request(&slot, &cache, 1)
                    {
                        inflight = Some(1);
                    }
                    self.slots.insert(
                        slot.clone(),
                        Playing {
                            cache: cache.clone(),
                            gpu: texture,
                            host,
                            frame: pixels.is_some().then_some(0),
                            epoch: None,
                            bytes,
                            failed,
                            blocked: false,
                            prefetched: None,
                            inflight,
                        },
                    );
                } else if let Some(playing) = self.slots.get_mut(&slot)
                    && playing.failed
                    && !Arc::ptr_eq(&playing.cache, &cache)
                {
                    playing.failed = false;
                    playing.cache = cache.clone();
                    playing.frame = None;
                    playing.prefetched = None;
                    playing.inflight = None;
                }
                let playing = &self.slots[&slot];
                registry.register(
                    slot,
                    playing.host.clone(),
                    cache.width,
                    cache.height,
                    HostTextureAlphaMode::Premultiplied,
                );
            }
            self.catalog_version = Some(current_catalog);
        }

        // Idle slots keep the frame they last showed: it is what a returning
        // consumer paints first. Park the ones past the idle budget.
        let mut idle: u64 = 0;
        for (slot, playing) in &mut self.slots {
            if playing.frame.is_none() || self.windows.values().any(|r| is_painted(r, slot)) {
                continue;
            }
            if idle + playing.bytes <= MAX_IDLE_GPU {
                idle += playing.bytes;
            } else if let Ok(texture) = placeholder(gpu) {
                playing.host.replace_texture(&texture);
                playing.gpu = texture;
                playing.frame = None;
                playing.bytes = PLACEHOLDER_BYTES;
            }
        }

        // Only the window being prepared advances what it painted. Another
        // window's demand is from its last fresh paint and stays put while it
        // is hidden or reusing a frame; advancing for it would queue uploads
        // that no submission of this window carries.
        let Some(registry) = self.windows.get(&id) else {
            return;
        };
        let mut used: u64 = self.slots.values().map(|p| p.bytes).sum();
        for (slot, playing) in &mut self.slots {
            if !is_painted(registry, slot) || playing.failed {
                continue;
            }
            let epoch = *playing.epoch.get_or_insert(now);
            let (frame, next_elapsed) = if reduced {
                (0, None)
            } else {
                playing.cache.frame_at(now.saturating_sub(epoch))
            };
            let next_frame = next_elapsed.map(|t| playing.cache.frame_at(t).0);

            if playing.frame == Some(frame) {
                if let Some(next) = next_frame
                    && next != frame
                    && !playing
                        .prefetched
                        .as_ref()
                        .is_some_and(|(idx, _)| *idx == next)
                    && playing.inflight != Some(next)
                    && self.pool.request(slot, &playing.cache, next)
                {
                    playing.inflight = Some(next);
                }
                continue;
            }
            let bytes = frame_bytes(&playing.cache);
            if playing.frame.is_none() && used - playing.bytes + bytes > MAX_GPU {
                playing.blocked = true;
                continue;
            }
            playing.blocked = false;

            let pixels = if let Some((idx, px)) = playing.prefetched.take()
                && idx == frame
            {
                px
            } else {
                match cache::read_premultiplied_frame(&playing.cache, frame) {
                    Ok(px) => px,
                    Err(_) => {
                        nanatype_diag::event!(crate::diag::STICKER_FRAME_REBUILD);
                        playing.failed = true;
                        self.rebuild = true;
                        continue;
                    }
                }
            };

            if playing.frame.is_none() {
                match texture(gpu, playing.cache.width, playing.cache.height, &pixels) {
                    Ok(texture) => {
                        playing.host.replace_texture(&texture);
                        playing.gpu = texture;
                        used = used - playing.bytes + bytes;
                        playing.bytes = bytes;
                    }
                    Err(e) => {
                        nanatype_diag::report!(crate::diag::STICKER_UPLOAD_FAILED; "{e}");
                        playing.failed = true;
                        continue;
                    }
                }
            } else if let Err(e) = write_pixels(
                gpu,
                &playing.gpu,
                playing.cache.width,
                playing.cache.height,
                &pixels,
            ) {
                nanatype_diag::report!(crate::diag::STICKER_UPLOAD_FAILED; "{e}");
                playing.failed = true;
                continue;
            }
            playing.host.invalidate();
            playing.frame = Some(frame);

            if let Some(next) = next_frame
                && next != frame
                && !playing
                    .prefetched
                    .as_ref()
                    .is_some_and(|(idx, _)| *idx == next)
                && playing.inflight != Some(next)
                && self.pool.request(slot, &playing.cache, next)
            {
                playing.inflight = Some(next);
            }
        }
    }
}

fn is_painted(registry: &HostTextureRegistry, slot: &str) -> bool {
    registry
        .painted_extent(slot)
        .is_some_and(|s| s[0] > 0 && s[1] > 0)
}

fn frame_bytes(cache: &CachedImage) -> u64 {
    u64::from(cache.width) * u64::from(cache.height) * 4
}

fn placeholder(gpu: &GpuContext) -> Result<GpuTexture, String> {
    texture(gpu, 1, 1, &[0, 0, 0, 0])
}

fn texture(gpu: &GpuContext, width: u32, height: u32, pixels: &[u8]) -> Result<GpuTexture, String> {
    let texture = gpu
        .create_texture(&GpuTextureDescriptor {
            label: Some("sticker frame"),
            width,
            height,
            // Scene samples a host texture as premultiplied linear light: the
            // sRGB view decodes what `cache::premultiply_linear` stored.
            format: GpuTextureFormat::RGBA8_UNORM_SRGB,
            usage: GpuTextureUsages::SAMPLED | GpuTextureUsages::COPY_DST,
        })
        .map_err(|e| e.to_string())?;
    write_pixels(gpu, &texture, width, height, pixels)?;
    Ok(texture)
}

fn write_pixels(
    gpu: &GpuContext,
    texture: &GpuTexture,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> Result<(), String> {
    gpu.write_texture(
        texture,
        GpuTextureRegion {
            x: 0,
            y: 0,
            width,
            height,
        },
        pixels,
        width * 4,
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::Predecoder;
    use crate::stickers::cache;
    use std::fs;
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, Instant};

    #[test]
    fn predecoder_pool_prefetches_frame_into_shared_buffer() {
        let base = std::env::temp_dir().join(format!("nanatype-predecode-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let source = base.join("anim.gif");
        let mut writer = image::codecs::gif::GifEncoder::new(fs::File::create(&source).unwrap());
        for color in [[255, 0, 0, 255], [0, 255, 0, 255]] {
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
        let cached = cache::prepare(&source, &base.join("cache"), &AtomicBool::new(false)).unwrap();
        let mut pool = Predecoder::default();
        assert!(pool.request(&cached.slot(), &cached, 1));
        let rx = pool.rx.as_ref().unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let msg = loop {
            if let Ok(msg) = rx.try_recv() {
                break msg;
            }
            assert!(Instant::now() < deadline, "predecoder timed out");
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(msg.frame, 1);
        let pixels = msg.result.unwrap();
        assert_eq!(&pixels[..4], &[0, 255, 0, 255]);
        let _ = fs::remove_dir_all(base);
    }
}
