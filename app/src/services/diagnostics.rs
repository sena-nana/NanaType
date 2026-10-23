//! The diagnostics package behind the About page's export button, and the
//! notice after a crash (docs/logging.md). Packages stay on this computer:
//! the user decides whether to attach one to a bug report.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::paths::Paths;

/// Package recent logs, crash snapshots and `summary` into
/// `<logs>/<app>-diagnostics-<time>.zip`. Blocks for seconds: call it on a
/// background thread.
pub fn export(paths: &Paths, summary: &serde_json::Value) -> Result<PathBuf, String> {
    let staging = paths.cache.join("diagnostics");
    let dir = nanatype_diag::package::package(&staging).map_err(|e| e.to_string())?;
    let archive = (|| {
        std::fs::write(
            dir.join("summary.json"),
            serde_json::to_vec_pretty(summary)?,
        )?;
        let name = dir.file_name().map_or_else(
            || "diagnostics".into(),
            |n| n.to_string_lossy().into_owned(),
        );
        let archive = paths.logs.join(format!("{name}.zip"));
        zip_dir(&dir, &archive)?;
        Ok::<_, io::Error>(archive)
    })();
    let _ = std::fs::remove_dir_all(&dir);
    archive.map_err(|e| e.to_string())
}

/// `dir`'s files, with their relative paths, as one deflated archive.
fn zip_dir(dir: &Path, archive: &Path) -> io::Result<()> {
    let mut zip = zip::ZipWriter::new(std::fs::File::create(archive)?);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut pending = vec![dir.to_path_buf()];
    while let Some(folder) = pending.pop() {
        let mut entries: Vec<_> = std::fs::read_dir(&folder)?.collect::<Result<_, _>>()?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                pending.push(path);
                continue;
            }
            let name = path
                .strip_prefix(dir)
                .map_err(io::Error::other)?
                .to_string_lossy()
                .replace('\\', "/");
            zip.start_file(name, options).map_err(io::Error::other)?;
            zip.write_all(&std::fs::read(&path)?)?;
        }
    }
    zip.finish().map_err(io::Error::other)?;
    Ok(())
}

/// What the package says about this installation besides its logs. Only
/// facts that identify no one: no account, room, channel, name or path.
pub fn summary(settings: &nanatype_settings::Settings, locale: &str) -> serde_json::Value {
    use crate::distribution::{STEAM, UNOFFICIAL_CHAT, UPDATES};
    let i = &settings.integrations;
    serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "build": env!("NANATYPE_BUILD_ID"),
        "edition": if STEAM { "steam" } else { "standalone" },
        "features": { "updates": UPDATES, "unofficial_chat": UNOFFICIAL_CHAT },
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "locale": locale,
        "skin": settings.skin.id,
        "advanced_effects": settings.skin.advanced_effects,
        "platforms": {
            "bilibili": i.bilibili.enabled,
            "blivechat": i.blivechat.enabled,
            "douyin": i.douyin.enabled,
            "youtube": i.youtube.enabled,
            "twitch": i.twitch.enabled,
        },
        "outputs": { "vts": i.vts.enabled, "osc": i.osc.enabled, "websocket": i.websocket.enabled },
        "plugins": settings.enabled_plugins.len(),
        "verbose_logs": settings.diagnostics.verbose,
    })
}

/// A crash newer than the one already pointed out, as its Unix time.
pub fn unseen_crash(settings: &nanatype_settings::Settings) -> Option<u64> {
    nanatype_diag::package::last_crash().filter(|&at| at > settings.diagnostics.crash_seen)
}
