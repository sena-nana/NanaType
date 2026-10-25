//! Which GPU the process renders on. The device is created once, when the
//! runtime starts, from the `WGPU_ADAPTER_NAME` it reads then; a choice made
//! in settings is applied by setting that variable before the runtime starts.

use std::sync::OnceLock;

/// The variable both of the runtime's adapter selections read.
const ADAPTER_ENV: &str = "WGPU_ADAPTER_NAME";

/// What [`choose_adapter`] found and handed the renderer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GpuChoice {
    /// The GPUs that could be chosen at startup.
    pub adapters: Vec<String>,
    /// The saved choice the renderer was given; `None` when it chose itself.
    pub applied: Option<String>,
    /// `WGPU_ADAPTER_NAME` was set outside the app, and wins.
    pub external: bool,
}

static CHOICE: OnceLock<GpuChoice> = OnceLock::new();

/// Hands the saved GPU to the renderer, when it is still present.
///
/// # Safety
/// Writes the environment: call before any other thread starts.
pub unsafe fn choose_adapter(saved: Option<&str>) {
    let adapters = adapters();
    let external = std::env::var_os(ADAPTER_ENV).is_some();
    let applied = pick(saved, external, &adapters).map(str::to_owned);
    if let Some(name) = &applied {
        // SAFETY: the caller guarantees no other thread runs yet.
        unsafe { std::env::set_var(ADAPTER_ENV, name) };
    }
    let _ = CHOICE.set(GpuChoice {
        adapters,
        applied,
        external,
    });
}

/// What [`choose_adapter`] found; empty before it ran.
pub fn gpu_choice() -> GpuChoice {
    CHOICE.get().cloned().unwrap_or_default()
}

/// The hardware GPUs this process could render on, by the name the renderer
/// reports for them, without duplicates. Empty off Windows.
fn adapters() -> Vec<String> {
    #[cfg(windows)]
    {
        windows_impl::adapters()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// A variable set from outside wins, and a name no current adapter has is
/// dropped: the renderer aborts on a name it cannot find.
fn pick<'a>(saved: Option<&'a str>, external: bool, available: &[String]) -> Option<&'a str> {
    let saved = saved.filter(|_| !external)?;
    available.iter().any(|name| name == saved).then_some(saved)
}

#[cfg(windows)]
mod windows_impl {
    use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
    use windows::Win32::Graphics::Direct3D12::{D3D12CreateDevice, ID3D12Device};
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1,
    };

    pub(super) fn adapters() -> Vec<String> {
        // SAFETY: plain COM factory creation; the factory is released on drop.
        let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
            return Vec::new();
        };
        let mut names = Vec::new();
        // EnumAdapters1 fails with DXGI_ERROR_NOT_FOUND past the last one.
        // SAFETY: `factory` is a live IDXGIFactory1.
        for adapter in (0..).map_while(|i| unsafe { factory.EnumAdapters1(i) }.ok()) {
            // SAFETY: `adapter` is a live IDXGIAdapter1.
            let Ok(desc) = (unsafe { adapter.GetDesc1() }) else {
                continue;
            };
            if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
                continue;
            }
            // A null device pointer only asks whether DX12 would accept this
            // adapter, which is what the renderer will require of it.
            // SAFETY: `adapter` is live; D3D12CreateDevice documents a null
            // output as a capability test.
            let usable = unsafe {
                D3D12CreateDevice::<_, ID3D12Device>(
                    &adapter,
                    D3D_FEATURE_LEVEL_11_0,
                    std::ptr::null_mut(),
                )
            }
            .is_ok();
            if !usable {
                continue;
            }
            let len = desc
                .Description
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(desc.Description.len());
            let name = String::from_utf16_lossy(&desc.Description[..len]);
            if !name.is_empty() && !names.contains(&name) {
                names.push(name);
            }
        }
        names
    }
}

#[cfg(test)]
mod tests {
    use super::pick;

    #[test]
    fn picks_only_a_present_adapter_nobody_else_chose() {
        let available = vec!["GPU A".to_owned(), "GPU B".to_owned()];
        assert_eq!(pick(Some("GPU B"), false, &available), Some("GPU B"));
        // Automatic: nothing to hand over.
        assert_eq!(pick(None, false, &available), None);
        // A variable set from outside wins.
        assert_eq!(pick(Some("GPU B"), true, &available), None);
        // Unplugged since it was chosen: the renderer would abort on it.
        assert_eq!(pick(Some("GPU C"), false, &available), None);
    }
}
