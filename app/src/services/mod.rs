pub mod chat_assets;
pub mod hotkeys;
pub mod instance;
pub mod media;
pub mod overlay_menu;
pub mod persist;
pub mod sounds;
pub mod tray;
pub mod updater;
pub mod watch;

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

/// Register or remove the "launch at login" entry.
pub fn set_autostart(enabled: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows::Win32::System::Registry::{
            HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegDeleteValueW, RegOpenKeyExW,
            RegSetValueExW,
        };
        use windows::core::HSTRING;
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let value = format!("\"{}\"", exe.display());
        let mut key = Default::default();
        let path = HSTRING::from(r"Software\Microsoft\Windows\CurrentVersion\Run");
        let name = HSTRING::from("NanaType");
        unsafe {
            RegOpenKeyExW(HKEY_CURRENT_USER, &path, Some(0), KEY_SET_VALUE, &mut key)
                .ok()
                .map_err(|e| e.to_string())?;
            let result = if enabled {
                let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
                let bytes = std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide.len() * 2);
                RegSetValueExW(key, &name, Some(0), REG_SZ, Some(bytes))
            } else {
                let r = RegDeleteValueW(key, &name);
                if r.0 == 2 {
                    windows::Win32::Foundation::ERROR_SUCCESS
                } else {
                    r
                }
            };
            let _ = RegCloseKey(key);
            result.ok().map_err(|e| e.to_string())
        }
    }
    #[cfg(target_os = "macos")]
    {
        set_autostart_macos(enabled)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = enabled;
        Ok(())
    }
}

/// LaunchAgent plist for `exe`. Pure, so tests never call `launchctl`.
#[cfg(target_os = "macos")]
pub(crate) fn launch_agent_plist(exe: &std::path::Path) -> String {
    let exe = xml_escape(&exe.display().to_string());
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
</dict>
</plist>
"#
    )
}

#[cfg(target_os = "macos")]
const LABEL: &str = "dev.sena-nana.nanatype";

#[cfg(target_os = "macos")]
fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(target_os = "macos")]
fn set_autostart_macos(enabled: bool) -> Result<(), String> {
    let home = std::env::var("HOME").map_err(|e| e.to_string())?;
    let dir = std::path::PathBuf::from(home).join("Library/LaunchAgents");
    let plist_path = dir.join(format!("{LABEL}.plist"));
    let uid = user_id()?;
    let domain = format!("gui/{uid}");
    let service = format!("{domain}/{LABEL}");
    // Drop a previous registration before rewriting the plist.
    let _ = std::process::Command::new("launchctl")
        .args(["bootout", &service])
        .status();
    if !enabled {
        if plist_path.exists() {
            std::fs::remove_file(&plist_path).map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    std::fs::write(&plist_path, launch_agent_plist(&exe)).map_err(|e| e.to_string())?;
    let status = std::process::Command::new("launchctl")
        .args(["bootstrap", &domain])
        .arg(&plist_path)
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("launchctl bootstrap exited {status}"))
    }
}

#[cfg(target_os = "macos")]
fn user_id() -> Result<u32, String> {
    let output = std::process::Command::new("/usr/bin/id")
        .arg("-u")
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!("id -u exited {}", output.status));
    }
    String::from_utf8(output.stdout)
        .map_err(|e| e.to_string())?
        .trim()
        .parse()
        .map_err(|e: std::num::ParseIntError| e.to_string())
}

#[cfg(all(test, target_os = "macos"))]
mod autostart_tests {
    use super::launch_agent_plist;
    use std::path::Path;

    #[test]
    fn the_plist_names_the_executable_and_runs_at_load() {
        let plist = launch_agent_plist(Path::new(
            "/Applications/NanaType.app/Contents/MacOS/NanaType",
        ));
        assert!(plist.contains("<string>dev.sena-nana.nanatype</string>"));
        assert!(
            plist.contains("<string>/Applications/NanaType.app/Contents/MacOS/NanaType</string>")
        );
        assert!(plist.contains("<key>RunAtLoad</key>"));
        let escaped = launch_agent_plist(Path::new("/tmp/a&b<c>"));
        assert!(escaped.contains("/tmp/a&amp;b&lt;c&gt;"));
        assert!(!escaped.contains("/tmp/a&b"));
    }
}
