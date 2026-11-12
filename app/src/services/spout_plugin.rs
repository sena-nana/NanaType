//! Fetching Off World Live's OBS Spout2 plugin installer and starting it.
//!
//! The installer is pinned by size and SHA-256, so the GitHub file proxies
//! tried after github.com need not be trusted. Bump all four constants
//! together from the release at
//! <https://github.com/Off-World-Live/obs-spout2-plugin/releases>.

use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const VERSION: &str = "1.15.0";
const NAME: &str = "OBS_Spout2_Plugin_Install_v1.15.0.exe";
const SIZE: u64 = 405_904;
const SHA256: &str = "b49480a6c1f0c41eb1881ba21a4356961b4ee8b75394a0793c6005285b2ee8c6";

/// Public GitHub file proxies after github.com itself; the same ones the
/// updater falls back to (crates/update/src/mirror.rs).
const PROXIES: &[&str] = &[
    "https://gh-proxy.com/",
    "https://ghfast.top/",
    "https://ghproxy.net/",
];
const TIMEOUT: Duration = Duration::from_secs(60);
const CHUNK: usize = 64 * 1024;

/// What the OBS steps show beside the plugin step.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Idle,
    Downloading {
        done: u64,
        total: u64,
    },
    /// The installer was started (it asks for administrator rights itself).
    Launched,
    Failed(String),
}

#[derive(Debug)]
pub enum Event {
    Progress { done: u64, total: u64 },
    Done(Result<PathBuf, String>),
}

fn url() -> String {
    format!(
        "https://github.com/Off-World-Live/obs-spout2-plugin/releases/download/{VERSION}/{NAME}"
    )
}

/// Download the installer into `dir` off the UI thread, trying the user's
/// own proxy (`proxy`, may be empty), github.com, then the built-in proxies.
/// A copy already there with the right digest is reused.
pub fn download(dir: PathBuf, proxy: &str, post: impl Fn(Event) + Send + 'static) {
    let mut sources: Vec<String> = Vec::new();
    let proxy = proxy.trim();
    if proxy.starts_with("https://") && !proxy.contains(char::is_whitespace) {
        let mut prefix = proxy.to_owned();
        if !prefix.ends_with('/') {
            prefix.push('/');
        }
        sources.push(prefix);
    }
    sources.push(String::new());
    sources.extend(PROXIES.iter().map(|p| (*p).to_owned()));
    std::thread::spawn(move || {
        let result = fetch(&dir, &sources, &post);
        post(Event::Done(result));
    });
}

fn fetch(dir: &Path, sources: &[String], post: &impl Fn(Event)) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join(NAME);
    if digest_of(&path).is_some_and(|d| d == SHA256) {
        return Ok(path);
    }
    let partial = dir.join(format!("{NAME}.part"));
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .user_agent(concat!("NanaType/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let url = url();
    let mut failures = Vec::new();
    for prefix in sources {
        match fetch_one(&agent, &format!("{prefix}{url}"), &partial, post) {
            Ok(()) => {
                std::fs::rename(&partial, &path).map_err(|e| e.to_string())?;
                return Ok(path);
            }
            Err(e) => {
                let _ = std::fs::remove_file(&partial);
                let from = if prefix.is_empty() { "GitHub" } else { prefix };
                failures.push(format!("{from}: {e}"));
            }
        }
    }
    Err(failures.join("; "))
}

fn fetch_one(
    agent: &ureq::Agent,
    url: &str,
    partial: &Path,
    post: &impl Fn(Event),
) -> Result<(), String> {
    let response = agent.get(url).call().map_err(|e| e.to_string())?;
    // One byte over the size, so an oversized body is seen, not truncated.
    let mut body = response
        .into_body()
        .into_with_config()
        .limit(SIZE + 1)
        .reader();
    let mut file = std::fs::File::create(partial).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0; CHUNK];
    let mut done = 0u64;
    post(Event::Progress { done, total: SIZE });
    loop {
        let n = body.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        done += n as u64;
        if done > SIZE {
            return Err("larger than published".into());
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        post(Event::Progress { done, total: SIZE });
    }
    file.sync_all().map_err(|e| e.to_string())?;
    if done != SIZE {
        return Err("shorter than published".into());
    }
    if hex(&hasher.finalize()) != SHA256 {
        return Err("checksum mismatch".into());
    }
    Ok(())
}

fn digest_of(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    (bytes.len() as u64 == SIZE).then(|| hex(&Sha256::digest(&bytes)))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Start the verified installer. It asks for administrator rights, which a
/// plain process spawn cannot grant, so it goes through the shell (UAC).
pub fn launch(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        use windows::core::{HSTRING, PCWSTR, w};
        let file = HSTRING::from(path.as_os_str());
        // SAFETY: every string outlives the call.
        let code = unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                &file,
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        // Values above 32 mean success; 5 is a declined UAC prompt.
        let code = code.0 as isize;
        if code > 32 {
            Ok(())
        } else {
            Err(format!("ShellExecute failed ({code})"))
        }
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err("the Spout2 plugin is for Windows only".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_digest_is_lowercase_hex() {
        assert_eq!(SHA256.len(), 64);
        assert!(
            SHA256
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
        assert!(NAME.contains(VERSION));
    }

    #[test]
    fn reuses_a_verified_copy_and_rejects_others() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(NAME);
        std::fs::write(&path, vec![0u8; SIZE as usize]).unwrap();
        assert_ne!(digest_of(&path).as_deref(), Some(SHA256));
        std::fs::write(&path, b"short").unwrap();
        assert_eq!(digest_of(&path), None);
    }
}
