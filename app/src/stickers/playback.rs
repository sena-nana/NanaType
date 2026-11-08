//! GPU playback of already-composited cache frames. Each window reports actual
//! painted slots, so hidden/clipped animations do not keep windows repainting.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

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
}

#[derive(Default)]
pub struct Playback {
    slots: HashMap<String, Playing>,
    windows: HashMap<WindowId, HostTextureRegistry>,
    generation: Option<nana_ui::DeviceGeneration>,
    rebuild: bool,
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
        }
    }
    pub fn take_rebuild(&mut self) -> bool {
        std::mem::take(&mut self.rebuild)
    }

    pub fn next_at(&self, id: WindowId, now: u64, reduced: bool) -> Option<u64> {
        let registry = self.windows.get(&id)?;
        self.slots
            .iter()
            .filter(|(slot, s)| {
                !s.failed
                    && !s.blocked
                    && registry
                        .painted_extent(slot)
                        .is_some_and(|s| s[0] > 0 && s[1] > 0)
            })
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
        }
        let images = cache::animations();
        let wanted: HashSet<_> = images.iter().map(|c| c.slot()).collect();
        let removed: Vec<_> = self
            .slots
            .keys()
            .filter(|slot| !wanted.contains(*slot))
            .cloned()
            .collect();
        for registry in self.windows.values() {
            for slot in &removed {
                registry.remove(slot);
            }
        }
        self.slots.retain(|slot, _| wanted.contains(slot));
        let visible: HashSet<_> = self
            .windows
            .values()
            .flat_map(|r| {
                self.slots
                    .keys()
                    .filter(|slot| r.painted_extent(slot).is_some_and(|s| s[0] > 0 && s[1] > 0))
                    .cloned()
            })
            .collect();
        // Idle slots keep the frame they last showed: it is what a returning
        // consumer paints first. Park the ones past the idle budget.
        let mut idle: u64 = 0;
        for (slot, playing) in &mut self.slots {
            if visible.contains(slot) || playing.frame.is_none() {
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
        let mut used: u64 = self.slots.values().map(|p| p.bytes).sum();
        let registry = self.windows.entry(id).or_default();
        for cache in images {
            let slot = cache.slot();
            if !self.slots.contains_key(&slot) {
                // Frame 0 doubles as the preview, so the first paint shows real
                // content and a visible static sticker never uploads twice.
                let bytes = frame_bytes(&cache);
                let admitted = idle + bytes <= MAX_IDLE_GPU && used + bytes <= MAX_GPU;
                let (image, failed) = if admitted {
                    match read_frame(&cache, 0) {
                        Ok(image) => (Some(image), false),
                        Err(e) => {
                            nanatype_diag::report!(crate::diag::STICKER_PREVIEW_FAILED; "{e}");
                            self.rebuild = true;
                            (None, true)
                        }
                    }
                } else {
                    (None, false)
                };
                let uploaded = match &image {
                    Some(image) => texture(gpu, image),
                    None => placeholder(gpu),
                };
                let Ok(texture) = uploaded else {
                    continue;
                };
                let bytes = if image.is_some() {
                    idle += bytes;
                    bytes
                } else {
                    PLACEHOLDER_BYTES
                };
                used += bytes;
                let host = HostTexture::new(&texture);
                self.slots.insert(
                    slot.clone(),
                    Playing {
                        cache: cache.clone(),
                        gpu: texture,
                        host,
                        frame: image.is_some().then_some(0),
                        epoch: None,
                        bytes,
                        failed,
                        blocked: false,
                    },
                );
            } else if let Some(playing) = self.slots.get_mut(&slot)
                && playing.failed
                && !Arc::ptr_eq(&playing.cache, &cache)
            {
                playing.failed = false;
                playing.cache = cache.clone();
                playing.frame = None;
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
        for (slot, playing) in &mut self.slots {
            if !visible.contains(slot) || playing.failed {
                continue;
            }
            let epoch = *playing.epoch.get_or_insert(now);
            let (frame, _) = if reduced {
                (0, None)
            } else {
                playing.cache.frame_at(now.saturating_sub(epoch))
            };
            if playing.frame == Some(frame) {
                continue;
            }
            let bytes = frame_bytes(&playing.cache);
            if playing.frame.is_none() && used - playing.bytes + bytes > MAX_GPU {
                playing.blocked = true;
                continue;
            }
            playing.blocked = false;
            let Ok(image) = read_frame(&playing.cache, frame) else {
                nanatype_diag::event!(crate::diag::STICKER_FRAME_REBUILD);
                playing.failed = true;
                self.rebuild = true;
                continue;
            };
            if playing.frame.is_none() {
                match texture(gpu, &image) {
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
            } else if let Err(e) = upload(gpu, &playing.gpu, &image) {
                nanatype_diag::report!(crate::diag::STICKER_UPLOAD_FAILED; "{e}");
                playing.failed = true;
                continue;
            }
            playing.host.invalidate();
            playing.frame = Some(frame);
        }
    }
}

fn frame_bytes(cache: &CachedImage) -> u64 {
    u64::from(cache.width) * u64::from(cache.height) * 4
}

/// One cached frame, which must match the size the slot was registered with.
fn read_frame(cache: &CachedImage, index: usize) -> Result<image::RgbaImage, String> {
    let image = read(&cache.frame_path(index))?;
    if image.dimensions() != (cache.width, cache.height) {
        return Err("Cached frame size does not match its sticker".into());
    }
    Ok(image)
}

fn placeholder(gpu: &GpuContext) -> Result<GpuTexture, String> {
    texture(gpu, &image::RgbaImage::new(1, 1))
}

fn read(path: &std::path::Path) -> Result<image::RgbaImage, String> {
    if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > cache::MAX_SOURCE {
        return Err("Cached frame exceeds size limit".into());
    }
    let mut reader = image::ImageReader::open(path).map_err(|e| e.to_string())?;
    reader.limits(crate::assets::image_limits(1024, 16 * 1024 * 1024));
    Ok(reader.decode().map_err(|e| e.to_string())?.into_rgba8())
}

fn texture(gpu: &GpuContext, image: &image::RgbaImage) -> Result<GpuTexture, String> {
    let texture = gpu
        .create_texture(&GpuTextureDescriptor {
            label: Some("sticker frame"),
            width: image.width(),
            height: image.height(),
            // Scene samples a host texture as premultiplied linear light: the
            // sRGB view decodes what `premultiply_linear` stored.
            format: GpuTextureFormat::RGBA8_UNORM_SRGB,
            usage: GpuTextureUsages::SAMPLED | GpuTextureUsages::COPY_DST,
        })
        .map_err(|e| e.to_string())?;
    upload(gpu, &texture, image)?;
    Ok(texture)
}
fn upload(gpu: &GpuContext, texture: &GpuTexture, image: &image::RgbaImage) -> Result<(), String> {
    let mut pixels = image.as_raw().clone();
    premultiply_linear(&mut pixels);
    gpu.write_texture(
        texture,
        GpuTextureRegion {
            x: 0,
            y: 0,
            width: image.width(),
            height: image.height(),
        },
        &pixels,
        image.width() * 4,
    )
    .map_err(|e| e.to_string())
}

/// Straight-alpha sRGB bytes to sRGB-encoded premultiplied linear light, so
/// an sRGB view samples premultiplied linear colour. Opaque texels keep their
/// bytes; only partially covered ones are weighted in linear light.
fn premultiply_linear(pixels: &mut [u8]) {
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

#[cfg(test)]
mod tests {
    use super::premultiply_linear;

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
}
