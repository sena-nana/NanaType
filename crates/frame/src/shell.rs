//! Handing things to the system: the file manager, the browser and the
//! native title bar.

/// Open a folder (or file) in the system file manager.
pub fn reveal(path: &std::path::Path) {
    let _ = std::fs::create_dir_all(path);
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

/// Open a web page in the default browser.
pub fn open_url(url: &str) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(url).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

/// Paint a window's system title bar with `theme`'s background and text,
/// in dark mode when the background is dark (Windows 11; Windows 10 takes
/// only the dark mode). Other systems keep their own title bar.
pub fn tint_caption(handle: &nana_ui::WindowHandle, theme: &nana_ui::CompiledTheme) {
    let p = theme.palette();
    let rgb = |c: nana_ui_core::SemanticColor| {
        let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        [b(c.r), b(c.g), b(c.b)]
    };
    let (bg, text) = (rgb(p.background), rgb(p.text));
    let luma = 0.2126 * f32::from(bg[0]) + 0.7152 * f32::from(bg[1]) + 0.0722 * f32::from(bg[2]);
    let dark = luma < 128.0;
    drop(handle.with_native_handle(move |h| {
        #[cfg(windows)]
        if let raw_window_handle::RawWindowHandle::Win32(w) = h.as_raw() {
            caption_colors(w.hwnd.get(), bg, text, dark);
        }
        #[cfg(not(windows))]
        let _ = (h, bg, text, dark);
    }));
}

#[cfg(windows)]
fn caption_colors(hwnd: isize, bg: [u8; 3], text: [u8; 3], dark: bool) {
    use windows::Win32::Foundation::{COLORREF, HWND};
    use windows::Win32::Graphics::Dwm::{
        DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute,
    };
    let hwnd = HWND(hwnd as *mut core::ffi::c_void);
    let colorref =
        |[r, g, b]: [u8; 3]| COLORREF(u32::from(r) | u32::from(g) << 8 | u32::from(b) << 16);
    // A Win32 `BOOL`.
    let dark = i32::from(dark);
    let caption = colorref(bg);
    let text = colorref(text);
    // SAFETY: `hwnd` is the live window the platform handed us on its own
    // thread; each attribute is read from a local of the documented size.
    // Failures (older Windows) leave the default title bar.
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            std::ptr::from_ref(&dark).cast(),
            std::mem::size_of_val(&dark) as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_CAPTION_COLOR,
            std::ptr::from_ref(&caption).cast(),
            std::mem::size_of_val(&caption) as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_TEXT_COLOR,
            std::ptr::from_ref(&text).cast(),
            std::mem::size_of_val(&text) as u32,
        );
    }
}
