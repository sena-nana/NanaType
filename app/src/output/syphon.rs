//! Syphon servers for the dialogue and history windows.
//!
//! `window_output_frame` runs after that frame's command buffer was submitted,
//! so a blit on the same Metal queue is ordered after the paint.

use std::collections::HashMap;

use nana_ui::wgpu;
use nana_ui::{DeviceGeneration, GpuContext, WindowOutputFrame};
use nanalive_syphon::{MTLCommandBuffer, Published, Server as SyphonServer};
use objc2_metal::MTLCommandQueue;

use crate::windows::WindowKind;

const START: &str = super::SYPHON_ERROR_KEYS[0];
const PUBLISH: &str = super::SYPHON_ERROR_KEYS[1];

#[derive(Default)]
pub struct Outputs {
    servers: HashMap<WindowKind, Server>,
    failed: HashMap<WindowKind, &'static str>,
}

struct Server {
    device: DeviceGeneration,
    server: SyphonServer,
}

impl Outputs {
    /// Drop servers that were turned off or renamed, and forget their errors.
    pub fn sync(&mut self, live: &[(WindowKind, String)]) {
        self.servers.retain(|kind, server| {
            let keep = live
                .iter()
                .any(|(live_kind, name)| live_kind == kind && name == server.server.name());
            if !keep {
                self.failed.remove(kind);
            }
            keep
        });
        self.failed
            .retain(|kind, _| live.iter().any(|(live_kind, _)| live_kind == kind));
    }

    pub fn failure(&self, kind: WindowKind) -> Option<&'static str> {
        self.failed.get(&kind).copied()
    }

    /// Publish `frame` under `name`. A busy server skips the frame without an error.
    pub fn publish(
        &mut self,
        kind: WindowKind,
        name: &str,
        gpu: &GpuContext,
        frame: &WindowOutputFrame,
    ) -> Option<(&'static str, String)> {
        let size = frame.extent();
        if size.contains(&0) {
            return None;
        }
        match self.publish_texture(kind, name, gpu, frame.texture().wgpu(), size) {
            Ok(published) => {
                self.failed.remove(&kind);
                // Retained scenes need a retry even for an animation's last frame.
                if published == Published::Busy {
                    frame.request_resend();
                }
                None
            }
            Err(error) => {
                self.failed.insert(kind, error.0);
                Some(error)
            }
        }
    }

    fn publish_texture(
        &mut self,
        kind: WindowKind,
        name: &str,
        gpu: &GpuContext,
        texture: &wgpu::Texture,
        size: [u32; 2],
    ) -> Result<Published, (&'static str, String)> {
        let interop = gpu.wgpu();
        let device = interop.device();
        let queue = interop.queue();
        // SAFETY: the guards only borrow wgpu's Metal objects for this call.
        // Nothing here destroys them; Syphon encodes a blit and we commit it
        // before the guards drop. NanaUI keeps the texture for later frames.
        let hal_device = unsafe { device.as_hal::<wgpu::hal::api::Metal>() }
            .ok_or_else(|| (PUBLISH, "output device is not Metal".to_string()))?;
        let hal_queue = unsafe { queue.as_hal::<wgpu::hal::api::Metal>() }
            .ok_or_else(|| (PUBLISH, "output queue is not Metal".to_string()))?;
        let hal_texture = unsafe { texture.as_hal::<wgpu::hal::api::Metal>() }
            .ok_or_else(|| (PUBLISH, "output texture is not Metal".to_string()))?;

        // Device recovery needs a new server for the new Metal device.
        if self
            .servers
            .get(&kind)
            .is_none_or(|server| server.server.name() != name || server.device != gpu.generation())
        {
            self.servers.remove(&kind);
            let server = SyphonServer::new(name, hal_device.raw_device())
                .map_err(|error| (START, error.to_string()))?;
            self.servers.insert(
                kind,
                Server {
                    device: gpu.generation(),
                    server,
                },
            );
        }
        let server = &self.servers[&kind].server;
        let command_buffer = hal_queue
            .as_raw()
            .commandBuffer()
            .ok_or_else(|| (PUBLISH, "Metal command buffer was not created".into()))?;
        let published = server
            .publish(hal_texture.raw_handle(), &command_buffer, size)
            .map_err(|error| (PUBLISH, error.to_string()))?;
        if published == Published::Sent {
            command_buffer.commit();
        }
        // Busy leaves the previous copy in flight without blocking the UI.
        Ok(published)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn gpu() -> GpuContext {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..wgpu::InstanceDescriptor::new_without_display_handle_from_env()
        });
        let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
            .expect("Metal adapter");
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).expect("Metal device");
        GpuContext::from_wgpu(adapter, device, queue)
    }

    fn texture(gpu: &GpuContext, size: [u32; 2], format: wgpu::TextureFormat) -> wgpu::Texture {
        gpu.wgpu()
            .device()
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("Syphon test"),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            })
    }

    fn wait_for_pixels(server: &SyphonServer) -> ([u32; 2], Vec<u8>) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(frame) = server.read_published_frame() {
                return frame;
            }
            assert!(Instant::now() < deadline, "no Syphon frame completed");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn static_server_copies_wgpu_pixels_and_survives_resize_rename_and_disable() {
        let gpu = gpu();
        let mut outputs = Outputs::default();
        for (round, size) in [[8, 4], [12, 6]].into_iter().enumerate() {
            let source = texture(&gpu, size, wgpu::TextureFormat::Bgra8Unorm);
            // Distinct rows and alpha values detect flips or pixel conversion.
            let pixels: Vec<u8> = (0..size[1])
                .flat_map(|y| (0..size[0]).flat_map(move |x| [x as u8, y as u8, 64, 128]))
                .collect();
            gpu.wgpu().queue().write_texture(
                source.as_image_copy(),
                &pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(size[0] * 4),
                    rows_per_image: Some(size[1]),
                },
                source.size(),
            );
            // Flush the upload before using the same native Metal queue.
            gpu.wgpu().queue().submit([]);
            for kind in super::super::KINDS {
                let name = format!("NanaType Test {kind:?}");
                assert_eq!(
                    outputs.publish_texture(kind, &name, &gpu, &source, size),
                    Ok(Published::Sent)
                );
                let server = &outputs.servers[&kind].server;
                assert_eq!(
                    wait_for_pixels(server),
                    (size, pixels.clone()),
                    "round {round}"
                );
            }
        }
        let kind = WindowKind::Display;
        outputs.sync(&[(kind, "NanaType Renamed".into())]);
        assert!(outputs.servers.is_empty());
        let source = texture(&gpu, [4, 2], wgpu::TextureFormat::Bgra8Unorm);
        assert_eq!(
            outputs.publish_texture(kind, "NanaType Renamed", &gpu, &source, [4, 2]),
            Ok(Published::Sent)
        );
        // Drop while the final frame may still be in flight. The static
        // library defers stop until its completion handler has finished.
        outputs.sync(&[]);
        assert!(outputs.servers.is_empty());
    }

    #[test]
    fn static_server_reports_non_blittable_textures() {
        let gpu = gpu();
        let mut outputs = Outputs::default();
        let source = texture(&gpu, [8, 4], wgpu::TextureFormat::Bgra8UnormSrgb);
        let error = outputs
            .publish_texture(
                WindowKind::Display,
                "NanaType Reject",
                &gpu,
                &source,
                [8, 4],
            )
            .unwrap_err();
        assert_eq!(error.0, PUBLISH);
        assert_eq!(
            error.1,
            nanalive_syphon::Error::TextureNotBlittable.to_string()
        );
    }
}
