//! A mask can be shared by outputs; mutable draw parameters cannot. Keep a
//! small LRU of bindings per output (see [`output_key`]).
use nana_ui::DeviceGeneration;
use nana_ui::{GpuContext, SceneGpuPassContext, wgpu};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{collections::HashMap, time::Instant};
static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
pub(super) fn allocations() -> u64 {
    ALLOCATIONS.load(Ordering::Relaxed)
}

#[derive(Debug, Default)]
pub(super) struct Scales(HashMap<(DeviceGeneration, u64, [u32; 2]), (f32, Instant)>);
impl Scales {
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn retain(&mut self, mut keep: impl FnMut(&(DeviceGeneration, u64, [u32; 2])) -> bool) {
        self.0.retain(|key, _| keep(key));
    }
    pub fn record(
        &mut self,
        generation: DeviceGeneration,
        material: u64,
        dest: [u32; 2],
        scale: f32,
    ) {
        let key = (generation, material, dest);
        if !self.0.contains_key(&key)
            && self
                .0
                .keys()
                .filter(|(g, id, _)| *g == generation && *id == material)
                .count()
                >= 8
        {
            let oldest = self
                .0
                .iter()
                .filter(|((g, id, _), _)| *g == generation && *id == material)
                .min_by_key(|(_, v)| v.1)
                .map(|(k, _)| *k)
                .unwrap();
            self.0.remove(&oldest);
        }
        self.0.insert(key, (scale, Instant::now()));
    }
    pub fn get(&mut self, generation: DeviceGeneration, material: u64, dest: [u32; 2]) -> f32 {
        self.0
            .get_mut(&(generation, material, dest))
            .map(|v| {
                v.1 = Instant::now();
                v.0
            })
            .unwrap_or(1.)
    }
}

/// Which output a binding belongs to. Position and opacity are uniform
/// *values*: they are rewritten every draw, so keying on them allocated a new
/// buffer and bind group every frame while a box moved or faded. Outputs that
/// share a key also share those values within a submission (one layout drawn
/// to the window and to its same-size export).
pub(super) type OutputKey = [u32; 5];
pub(super) fn output_key(c: &SceneGpuPassContext<'_>, scale: f32) -> OutputKey {
    [
        c.dest_size[0],
        c.dest_size[1],
        c.bounds.width,
        c.bounds.height,
        scale.to_bits(),
    ]
}

#[derive(Debug)]
pub(super) struct Binding {
    pub buffer: wgpu::Buffer,
    pub group: wgpu::BindGroup,
}
#[derive(Debug, Default)]
pub(super) struct Uniforms {
    entries: Vec<(OutputKey, usize, Binding)>,
}
impl Uniforms {
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn bytes(&self) -> usize {
        self.entries.iter().map(|(_, size, _)| *size).sum()
    }
    pub fn binding(
        &mut self,
        gpu: &GpuContext,
        layout: &wgpu::BindGroupLayout,
        textures: &[wgpu::TextureView],
        sampler: &wgpu::Sampler,
        key: OutputKey,
        size: usize,
    ) -> &Binding {
        if let Some(index) = self.entries.iter().position(|(k, _, _)| *k == key) {
            let entry = self.entries.remove(index);
            self.entries.push(entry);
        } else {
            if self.entries.len() == 8 {
                self.entries.remove(0);
            }
            // Eviction drops the old handles, never rewrites an in-flight
            // binding. Encoded command buffers retain their resources.
            let device = gpu.wgpu().device();
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("effect output parameters"),
                size: size as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let mut entries = vec![
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&textures[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ];
            for (i, texture) in textures.iter().enumerate().skip(1) {
                entries.push(wgpu::BindGroupEntry {
                    binding: i as u32 + 2,
                    resource: wgpu::BindingResource::TextureView(texture),
                });
            }
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("effect output binding"),
                layout,
                entries: &entries,
            });
            self.entries.push((key, size, Binding { buffer, group }));
        }
        &self.entries.last().unwrap().2
    }
}
