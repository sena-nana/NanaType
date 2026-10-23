//! What a user can hand to a bug report: recent session logs and crash
//! snapshots as redacted text and JSON. Nothing leaves the machine unless
//! the user sends it.

use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use nana_diagnostics::PackageOptions;

/// The newest crash (panic) snapshot of this app, as its modification time
/// in Unix seconds. Other snapshots, such as a lost GPU device, are not
/// crashes.
pub fn last_crash() -> Option<u64> {
    let diagnostics = nana_diagnostics::global()?;
    let dir = diagnostics.paths().crash.clone()?;
    newest_panic(&dir, &diagnostics.metadata().app_id)
}

fn newest_panic(dir: &Path, app_id: &str) -> Option<u64> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with(app_id) && name.contains("-panic")
        })
        .filter_map(|entry| entry.metadata().ok()?.modified().ok())
        .filter_map(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|age| age.as_secs())
        .max()
}

/// Write the diagnostics package into a new directory under `dest` and
/// return it: a fresh snapshot of the last minute, then the five newest
/// sessions and snapshots as text and JSON lines, the home directory
/// redacted. The raw `.nlog` files are left out: free text in them is
/// redacted at the source, but paths are only rewritten on export. Blocks
/// for up to ten seconds; never call it on the UI thread.
pub fn package(dest: &Path) -> io::Result<PathBuf> {
    let diagnostics =
        nana_diagnostics::global().ok_or_else(|| io::Error::other("diagnostics are off"))?;
    diagnostics.export_package(
        dest,
        &PackageOptions {
            max_sessions: 5,
            max_snapshots: 5,
            include_raw: false,
            redact_home: true,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_this_apps_panic_snapshots_count_as_crashes() {
        let dir = std::env::temp_dir().join(format!("nanatype-diag-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(newest_panic(&dir, "dev.app"), None);
        for name in [
            "dev.app-20260101T000000Z-1-device-lost.nlog",
            "dev.other-20260101T000000Z-1-panic.nlog",
        ] {
            std::fs::write(dir.join(name), b"").unwrap();
        }
        assert_eq!(newest_panic(&dir, "dev.app"), None);
        std::fs::write(dir.join("dev.app-20260101T000000Z-1-panic.nlog"), b"").unwrap();
        assert!(newest_panic(&dir, "dev.app").is_some());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
