//! One-time cleanup after retired integrations were removed from settings:
//! - schema 4: Bilibili web, Douyin and YouTube sources (saved logins and cached pictures);
//! - schema 9: Translated subtitles (OpenAI & DeepL API keys).
//!
//! Their saved secrets or cached pictures would stay on disk otherwise, with
//! nothing left to read or clear them.

use std::path::{Path, PathBuf};
use std::thread;

use nanatype_secrets::SecretStore;
use nanatype_settings::LoadNote;

/// The first settings schema written without those chat sources.
const SOURCES_REMOVED_IN: u32 = 4;
/// The first settings schema written without translated subtitles.
const TRANSLATE_REMOVED_IN: u32 = 9;
/// Secret store names and their failure event `what` codes.
const ALL_RETIRED_SECRETS: &[(u64, &str)] = &[
    (0, "bilibili"),
    (1, "douyin"),
    (2, "translate-openai"),
    (3, "translate-deepl"),
];
const TRANSLATE_SECRETS: &[(u64, &str)] = &[(2, "translate-openai"), (3, "translate-deepl")];
/// Chat picture prefixes in the blob folder.
const PICTURE_PREFIXES: &[&str] = &["bili-", "douyin-", "yt-"];

fn plan(note: &LoadNote) -> Option<(&'static [(u64, &'static str)], bool)> {
    match note {
        LoadNote::Migrated { from } if *from < SOURCES_REMOVED_IN => {
            Some((ALL_RETIRED_SECRETS, true))
        }
        LoadNote::Migrated { from } if *from < TRANSLATE_REMOVED_IN => {
            Some((TRANSLATE_SECRETS, false))
        }
        _ => None,
    }
}

/// Clean up off the UI thread when the settings came from a build that still
/// had retired secrets or cached chat pictures.
pub fn start(note: &LoadNote, config: PathBuf, blobs: PathBuf) {
    let Some((secrets, pictures)) = plan(note) else {
        return;
    };
    let _ = thread::Builder::new()
        .name("nanatype-retired-cleanup".into())
        .spawn(move || {
            for &(what, name) in secrets {
                if let Err(e) = SecretStore::new(&config, name).clear() {
                    nanatype_diag::report!(
                        crate::diag::RETIRED_CLEANUP_FAILED,
                        what = what;
                        "{e}"
                    );
                }
            }
            if pictures {
                remove_pictures(&blobs);
            }
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

    #[test]
    fn plans_retired_secrets_by_origin_schema() {
        assert_eq!(plan(&LoadNote::Loaded), None);
        assert_eq!(plan(&LoadNote::Migrated { from: 9 }), None);
        assert_eq!(
            plan(&LoadNote::Migrated { from: 8 }),
            Some((TRANSLATE_SECRETS, false))
        );
        assert_eq!(
            plan(&LoadNote::Migrated { from: 3 }),
            Some((ALL_RETIRED_SECRETS, true))
        );
    }
}
