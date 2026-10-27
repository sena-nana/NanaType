//! One-time cleanup after the Bilibili web, Douyin and YouTube sources were
//! removed (settings schema 4): their saved logins and cached pictures stay on
//! disk otherwise, with nothing left to read or clear them.

use std::path::{Path, PathBuf};
use std::thread;

use nanatype_secrets::SecretStore;
use nanatype_settings::LoadNote;

/// The first settings schema written without those sources.
const REMOVED_IN: u32 = 4;
/// Secret store names, in the order of the failure event's `what` field.
const SECRETS: &[&str] = &["bilibili", "douyin"];
/// Chat picture prefixes in the blob folder.
const PICTURE_PREFIXES: &[&str] = &["bili-", "douyin-", "yt-"];

/// Clean up off the UI thread when the settings came from a build that still
/// had the sources.
pub fn start(note: &LoadNote, config: PathBuf, blobs: PathBuf) {
    if !matches!(note, LoadNote::Migrated { from } if *from < REMOVED_IN) {
        return;
    }
    let _ = thread::Builder::new()
        .name("nanatype-retired-cleanup".into())
        .spawn(move || {
            for (what, name) in SECRETS.iter().enumerate() {
                if let Err(e) = SecretStore::new(&config, name).clear() {
                    nanatype_diag::report!(
                        crate::diag::RETIRED_CLEANUP_FAILED,
                        what = what as u64;
                        "{e}"
                    );
                }
            }
            remove_pictures(&blobs);
        });
}

fn remove_pictures(dir: &Path) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if PICTURE_PREFIXES.iter().any(|p| name.starts_with(p)) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_only_retired_pictures() {
        let dir = tempfile::tempdir().unwrap();
        let names = [
            "bili-profile-7-00.png",
            "douyin-avatar-1.png",
            "yt-emote-2.png",
            "blive-avatar-3.png",
            "twitch-avatar-4.png",
            "0123abcd.png",
        ];
        for name in names {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }
        remove_pictures(dir.path());
        let mut left: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(
            left,
            ["0123abcd.png", "blive-avatar-3.png", "twitch-avatar-4.png"]
        );
    }
}
