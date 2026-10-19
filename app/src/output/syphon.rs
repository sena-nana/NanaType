//! Syphon servers for the dialogue and history windows.
//!
//! `window_output_frame` runs after that frame's command buffer was submitted,
//! so a blit on the same Metal queue is ordered after the paint.

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_void};
use std::sync::OnceLock;

use nana_ui::wgpu;
use nana_ui::{GpuContext, WindowOutputFrame};
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use crate::windows::WindowKind;

const MISSING: &str = super::SYPHON_ERROR_KEYS[0];
const PUBLISH: &str = super::SYPHON_ERROR_KEYS[1];

/// `MTLPixelFormatBGRA8Unorm`.
const MTL_PIXEL_FORMAT_BGRA8_UNORM: usize = 80;

const RTLD_NOW: i32 = 2;
const RTLD_GLOBAL: i32 = 8;

unsafe extern "C" {
    fn dlopen(path: *const i8, mode: i32) -> *mut c_void;
    fn dlerror() -> *mut i8;
}

#[derive(Default)]
pub struct Outputs {
    servers: HashMap<WindowKind, Server>,
    failed: HashMap<WindowKind, &'static str>,
}

struct Server {
    name: String,
    /// The `MTLDevice` the server was made for (it retains it, so the
    /// address cannot be reused while the server lives).
    device: usize,
    server: Retained<AnyObject>,
}

impl Drop for Server {
    fn drop(&mut self) {
        unsafe {
            let _: () = msg_send![&*self.server, stop];
        }
    }
}

impl Outputs {
    /// Drop servers that were turned off or renamed, and forget their errors.
    pub fn sync(&mut self, live: &[(WindowKind, String)]) {
        self.servers.retain(|kind, server| {
            let keep = live
                .iter()
                .any(|(live_kind, name)| live_kind == kind && name == &server.name);
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

    /// Publish `frame` under `name`. `None` means the frame was handed to Syphon.
    pub fn publish(
        &mut self,
        kind: WindowKind,
        name: &str,
        gpu: &GpuContext,
        frame: &WindowOutputFrame,
    ) -> Option<(&'static str, String)> {
        if let Err(error) = load_framework() {
            self.failed.insert(kind, MISSING);
            return Some((MISSING, error));
        }
        let [width, height] = frame.extent();
        if width == 0 || height == 0 {
            return None;
        }
        let result = self
            .publish_frame(kind, name, gpu, frame, width, height)
            .err()
            .map(|error| (PUBLISH, error));
        match &result {
            Some((key, _)) => {
                self.failed.insert(kind, key);
            }
            None => {
                self.failed.remove(&kind);
            }
        }
        result
    }

    fn publish_frame(
        &mut self,
        kind: WindowKind,
        name: &str,
        gpu: &GpuContext,
        frame: &WindowOutputFrame,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        let interop = gpu.wgpu();
        let device = interop.device();
        let queue = interop.queue();
        let texture = frame.texture().wgpu();
        // SAFETY: the guards only borrow wgpu's Metal objects for this call.
        // Nothing here destroys them; Syphon encodes a blit and we commit it
        // before the guards drop. NanaUI keeps the texture for later frames.
        let hal_device = unsafe { device.as_hal::<wgpu::hal::api::Metal>() }
            .ok_or_else(|| "output device is not Metal".to_string())?;
        let hal_queue = unsafe { queue.as_hal::<wgpu::hal::api::Metal>() }
            .ok_or_else(|| "output queue is not Metal".to_string())?;
        let hal_texture = unsafe { texture.as_hal::<wgpu::hal::api::Metal>() }
            .ok_or_else(|| "output texture is not Metal".to_string())?;

        let device_ptr = object_ptr(hal_device.raw_device());
        // A recreated wgpu device (device loss, GPU switch) needs a new
        // server: publishing a texture from another device is undefined.
        if self
            .servers
            .get(&kind)
            .is_none_or(|server| server.name != name || server.device != device_ptr as usize)
        {
            self.servers.remove(&kind);
            let server = make_server(name, device_ptr)?;
            self.servers.insert(
                kind,
                Server {
                    name: name.to_owned(),
                    device: device_ptr as usize,
                    server,
                },
            );
        }
        let Some(server) = self.servers.get(&kind) else {
            return Err("Syphon server was not created".into());
        };
        let queue_ptr = object_ptr(hal_queue.as_raw());
        let texture_ptr = object_ptr(hal_texture.raw_handle());
        // The bundled Syphon server only blits when the texture is BGRA8,
        // single-sample, and not framebuffer-only. Anything else would publish
        // an empty frame, because the shader redraw path is not in this build.
        let format: usize = unsafe { msg_send![texture_ptr, pixelFormat] };
        let samples: usize = unsafe { msg_send![texture_ptr, sampleCount] };
        let framebuffer_only: bool = unsafe { msg_send![texture_ptr, framebufferOnly] };
        if framebuffer_only || samples != 1 || format != MTL_PIXEL_FORMAT_BGRA8_UNORM {
            return Err(format!(
                "texture cannot be blitted (format {format}, samples {samples}, framebufferOnly {framebuffer_only})"
            ));
        }
        let command_buffer: *mut AnyObject = unsafe { msg_send![queue_ptr, commandBuffer] };
        if command_buffer.is_null() {
            return Err("Metal command buffer was not created".into());
        }
        let region = NSRect {
            origin: NSPoint { x: 0.0, y: 0.0 },
            size: NSSize {
                width: f64::from(width),
                height: f64::from(height),
            },
        };
        unsafe {
            let _: () = msg_send![
                &*server.server,
                publishFrameTexture: texture_ptr,
                onCommandBuffer: command_buffer,
                imageRegion: region,
                flipped: false
            ];
            let _: () = msg_send![command_buffer, commit];
        }
        // `commandBuffer` returned +0 (autoreleased). Syphon retains it for the
        // completed-handler, and `commit` keeps it until the GPU is done.
        Ok(())
    }
}

/// `ProtocolObject` is a transparent wrapper over the Objective-C object.
fn object_ptr<T: ?Sized>(object: &objc2::runtime::ProtocolObject<T>) -> *mut AnyObject {
    std::ptr::from_ref(object).cast::<AnyObject>().cast_mut()
}

fn make_server(name: &str, device: *mut AnyObject) -> Result<Retained<AnyObject>, String> {
    let class = AnyClass::get(c"SyphonMetalServer").ok_or("SyphonMetalServer is not registered")?;
    let ns_name = NSString::from_str(name);
    let server: *mut AnyObject = unsafe {
        let allocated: *mut AnyObject = msg_send![class, alloc];
        msg_send![allocated, initWithName: &*ns_name, device: device, options: std::ptr::null::<AnyObject>()]
    };
    if server.is_null() {
        return Err("SyphonMetalServer init failed".into());
    }
    // `alloc`/`init` already return a +1 reference.
    Ok(unsafe { Retained::from_raw(server) }.expect("non-null server"))
}

fn load_framework() -> Result<(), String> {
    static LOADED: OnceLock<Result<(), String>> = OnceLock::new();
    LOADED
        .get_or_init(|| {
            let path = framework_binary().ok_or_else(|| {
                "Syphon.framework was not found beside the app or in packaging/macos".to_string()
            })?;
            let c_path = CString::new(path).map_err(|e| e.to_string())?;
            let handle = unsafe { dlopen(c_path.as_ptr(), RTLD_NOW | RTLD_GLOBAL) };
            if handle.is_null() {
                let message = unsafe { dlerror() };
                if message.is_null() {
                    Err("dlopen failed".into())
                } else {
                    Err(unsafe { CStr::from_ptr(message) }
                        .to_string_lossy()
                        .into_owned())
                }
            } else {
                Ok(())
            }
        })
        .clone()
}

fn framework_binary() -> Option<String> {
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        candidates.push(dir.join("Syphon.framework/Syphon"));
        candidates.push(dir.join("../Frameworks/Syphon.framework/Syphon"));
    }
    // Development builds only: a shipped binary never loads code from the
    // build machine's checkout path.
    #[cfg(debug_assertions)]
    candidates.push(std::path::PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../packaging/macos/Syphon.framework/Syphon"
    )));
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .map(|path| path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::{load_framework, make_server};

    #[link(name = "Metal", kind = "framework")]
    unsafe extern "C" {
        fn MTLCreateSystemDefaultDevice() -> *mut objc2::runtime::AnyObject;
    }

    #[test]
    fn the_framework_can_open_a_server() {
        load_framework().expect("Syphon.framework");
        let device = unsafe { MTLCreateSystemDefaultDevice() };
        assert!(!device.is_null(), "no Metal device");
        let server = make_server("NanaType Test", device).expect("server");
        drop(server);
    }
}
