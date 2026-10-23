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

struct Playing {
    cache: Arc<CachedImage>,
    gpu: GpuTexture,
    host: HostTexture,
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
        let registry = self.windows.entry(id).or_default();
        for cache in images {
            let slot = cache.slot();
            if !self.slots.contains_key(&slot) {
                // Tiny first-frame preview gives the first paint useful content;
                // the full frame is uploaded only after a visible consumer draws.
                let (preview, failed) = match read(&cache.dir.join("thumbnail.png")) {
                    Ok(p) => (p, false),
                    Err(e) => {
                        nanatype_diag::report!(crate::diag::STICKER_PREVIEW_FAILED; "{e}");
                        self.rebuild = true;
                        (image::RgbaImage::new(1, 1), true)
                    }
                };
                let Ok(texture) = texture(gpu, &preview) else {
                    continue;
                };
                let host = HostTexture::new(&texture);
                self.slots.insert(
                    slot.clone(),
                    Playing {
                        cache: cache.clone(),
                        gpu: texture,
                        host,
                        frame: None,
                        epoch: None,
                        bytes: u64::from(preview.width()) * u64::from(preview.height()) * 4,
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
        // Drop full-size textures with no consumers before admitting new ones.
        for (slot, playing) in &mut self.slots {
            if !visible.contains(slot)
                && playing.frame.is_some()
                && let Ok(preview) = read(&playing.cache.dir.join("thumbnail.png"))
                && let Ok(texture) = texture(gpu, &preview)
            {
                playing.host.replace_texture(&texture);
                playing.gpu = texture;
                playing.frame = None;
                playing.bytes = u64::from(preview.width()) * u64::from(preview.height()) * 4;
            }
        }
        let mut used: u64 = self.slots.values().map(|p| p.bytes).sum();
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
            let bytes = u64::from(playing.cache.width) * u64::from(playing.cache.height) * 4;
            if playing.frame.is_none() && used - playing.bytes + bytes > MAX_GPU {
                playing.blocked = true;
                continue;
            }
            playing.blocked = false;
            let image = match read(&playing.cache.frame_path(frame)) {
                Ok(p) if p.width() == playing.cache.width && p.height() == playing.cache.height => {
                    p
                }
                _ => {
                    nanatype_diag::event!(crate::diag::STICKER_FRAME_REBUILD);
                    playing.failed = true;
                    self.rebuild = true;
                    continue;
                }
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
            format: GpuTextureFormat::RGBA8_UNORM,
            usage: GpuTextureUsages::SAMPLED | GpuTextureUsages::COPY_DST,
        })
        .map_err(|e| e.to_string())?;
    upload(gpu, &texture, image)?;
    Ok(texture)
}
fn upload(gpu: &GpuContext, texture: &GpuTexture, image: &image::RgbaImage) -> Result<(), String> {
    let mut pixels = image.as_raw().clone();
    for pixel in pixels.as_chunks_mut::<4>().0 {
        for i in 0..3 {
            pixel[i] = ((u16::from(pixel[i]) * u16::from(pixel[3]) + 127) / 255) as u8;
        }
    }
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
