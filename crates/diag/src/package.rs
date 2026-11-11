//! What a user can hand to a bug report: recent session logs and crash
//! snapshots as redacted text and JSON. Nothing leaves the machine unless
//! the user sends it.

use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use nana_diagnostics::PackageOptions;

/// The newest crash of this app, as its snapshot's modification time in
/// Unix seconds: a panic, a native crash or abort, or a session that ended
/// without shutting down. Other snapshots, such as a lost GPU device, are
/// not crashes.
pub fn last_crash() -> Option<u64> {
    let crash = nana_diagnostics::last_crash()?;
    Some(crash.time.duration_since(UNIX_EPOCH).ok()?.as_secs())
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
