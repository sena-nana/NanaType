//! Persistent 2-worker shader compilation & rest-frame blit pipelines.
use nana_ui::{GpuTextureFormat, wgpu};
use nanatype_theme::ShaderProgram;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use super::super::effects::compiler_pool;
use super::SLOT_BYTES;

pub(super) static COMPILED: AtomicU64 = AtomicU64::new(0);
/// Compiles turned away while both compiler slots were busy; the draw retries
/// on a later frame (see `warming`).
pub(super) static REFUSED: AtomicU64 = AtomicU64::new(0);

pub(super) struct Pipeline {
    pub uploaded: AtomicBool,
    pub pipeline: wgpu::RenderPipeline,
    pub layout: wgpu::BindGroupLayout,
    pub textures: [wgpu::Texture; 2],
    pub sampler: wgpu::Sampler,
}

pub(super) struct Pending {
    pub value: Arc<OnceLock<Result<Arc<Pipeline>, String>>>,
    pub texture_bytes: usize,
}

/// Copies rest frames into the destination.
pub(super) struct Blit {
    pub pipeline: wgpu::RenderPipeline,
    pub layout: wgpu::BindGroupLayout,
    pub sampler: wgpu::Sampler,
}

pub(super) fn active_compiles() -> usize {
    compiler_pool().active_slots()
}

/// Submit a shader compilation to the persistent 2-worker pool.
/// Returns `None` if both worker slots are currently busy (`REFUSED` incremented).
pub(super) fn try_compile_async(
    device: wgpu::Device,
    format: GpuTextureFormat,
    program: Arc<ShaderProgram>,
    texture_bytes: usize,
) -> Option<Pending> {
    let value = Arc::new(OnceLock::new());
    let result = Arc::clone(&value);
    let submitted = compiler_pool().try_submit(move || {
        let compiled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            compile(&device, format, &program).map(Arc::new)
        }))
        .unwrap_or_else(|_| Err("GPU shader compiler failed".into()));
        let _ = result.set(compiled);
    });
    if !submitted {
        REFUSED.fetch_add(1, Ordering::Relaxed);
        return None;
    }
    COMPILED.fetch_add(1, Ordering::Relaxed);
    Some(Pending {
        value,
        texture_bytes,
    })
}

fn compile(
    device: &wgpu::Device,
    format: GpuTextureFormat,
    program: &ShaderProgram,
) -> Result<Pipeline, String> {
    let oom = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let result = (|| {
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("skin shader ABI 2"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: std::num::NonZeroU64::new(176),
                    },
                    count: None,
                },
                texture_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                texture_entry(3),
                texture_entry(4),
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: std::num::NonZeroU64::new(SLOT_BYTES as u64),
                    },
                    count: None,
                },
            ],
        });
        // `fx_mask` is uploaded as R8Unorm (1 byte/texel); sample its `.r` channel.
        let wrapper = nanatype_theme::SHADER_WRAPPER.replace(
            "textureSampleLevel(fx_mask, fx_sampler, uv, 0.).a",
            "textureSampleLevel(fx_mask, fx_sampler, uv, 0.).r",
        );
        let source = format!("{}\n{}", program.source, wrapper);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("validated skin shader"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("skin shader"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("skin shader"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("fx_vertex"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fx_fragment"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: format.wgpu(),
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("skin clamp sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let mut textures = vec![];
        for i in 0..2 {
            let size = program.textures.get(i).map_or([1, 1], |t| t.size);
            textures.push(device.create_texture(&wgpu::TextureDescriptor {
                label: Some("skin texture"),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            }));
        }
        Ok(Pipeline {
            uploaded: AtomicBool::new(false),
            pipeline,
            layout,
            textures: textures.try_into().map_err(|_| "texture count")?,
            sampler,
        })
    })();
    let errors = [
        pollster::block_on(scope.pop()),
        pollster::block_on(internal.pop()),
        pollster::block_on(oom.pop()),
    ];
    if let Some(e) = errors.into_iter().flatten().next() {
        return Err(e.to_string());
    }
    result
}

pub(super) fn blit_pipeline(device: &wgpu::Device, format: GpuTextureFormat) -> Blit {
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("skin rest frame"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: std::num::NonZeroU64::new(32),
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                count: None,
            },
        ],
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("skin rest frame"),
        source: wgpu::ShaderSource::Wgsl(
            format!(
                "{}\n{}",
                super::super::effects::FULLSCREEN_QUAD_WGSL,
                include_str!("../shader_rest.wgsl")
            )
            .into(),
        ),
    });
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("skin rest frame"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("skin rest frame"),
        layout: Some(&pl),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("rest_vertex"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("rest_fragment"),
            targets: &[Some(wgpu::ColorTargetState {
                format: format.wgpu(),
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("skin rest frame"),
        ..Default::default()
    });
    Blit {
        pipeline,
        layout,
        sampler,
    }
}
