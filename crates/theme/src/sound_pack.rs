//! Sound packs: a package whose `sounds.toml` holds a `[sounds]` table shaped
//! like a skin's. Chosen in the settings, a pack replaces the active skin's
//! typing, line-open and continue sounds where it defines them and adds or
//! replaces named effects; the skin keeps everything else, so `[se=name]`
//! markup written for the skin still plays.

use std::sync::Arc;

use serde::Deserialize;

use crate::files::{FileError, SkinFiles};
use crate::load::{SkinDiagnostic, SkinOrigin};
use crate::manifest::{Localized, Sounds};
use crate::package::clean_resource_path;

pub const SOUNDS_MANIFEST: &str = "sounds.toml";

const MAX_MANIFEST_BYTES: usize = 64 * 1024;
const MAX_TYPING_FILES: usize = 255;
const MAX_EFFECTS: usize = 256;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SoundsManifest {
    sounds: Sounds,
}

/// Read and check `sounds.toml`: every file it names must be in the package.
pub fn read_sounds(files: &dyn SkinFiles) -> Result<Sounds, FileError> {
    let invalid = |message: String| FileError::Io(format!("{SOUNDS_MANIFEST}: {message}"));
    let bytes = files.read(SOUNDS_MANIFEST)?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(FileError::TooLarge {
            path: SOUNDS_MANIFEST.into(),
            bytes: bytes.len() as u64,
        });
    }
    let text = std::str::from_utf8(&bytes).map_err(|e| invalid(e.to_string()))?;
    let sounds = toml::from_str::<SoundsManifest>(text)
        .map_err(|e| invalid(e.to_string()))?
        .sounds;
    if sounds.typing.files.len() > MAX_TYPING_FILES || sounds.se.len() > MAX_EFFECTS {
        return Err(invalid(format!(
            "at most {MAX_TYPING_FILES} typing sounds and {MAX_EFFECTS} effects"
        )));
    }
    let named = sounds
        .typing
        .files
        .iter()
        .chain(&sounds.open)
        .chain(&sounds.advance)
        .chain(sounds.se.values());
    for path in named {
        let clean = clean_resource_path(path)?;
        if !files.exists(&clean) {
            return Err(FileError::Missing(path.clone()));
        }
    }
    if sounds.se.keys().any(|name| name.trim().is_empty()) {
        return Err(invalid("effect names must not be empty".into()));
    }
    Ok(sounds)
}

/// The sounds that play with `pack` chosen over a skin's own.
pub fn layer(skin: &Sounds, pack: &Sounds) -> Sounds {
    let mut sounds = skin.clone();
    if supplies_typing(pack) {
        sounds.typing = pack.typing.clone();
    }
    if pack.open.is_some() {
        sounds.open = pack.open.clone();
    }
    if pack.advance.is_some() {
        sounds.advance = pack.advance.clone();
    }
    sounds.se.extend(pack.se.clone());
    sounds
}

/// Whether `pack` replaces the typing sounds (and their rhythm) at all.
pub fn supplies_typing(pack: &Sounds) -> bool {
    !pack.typing.files.is_empty()
}

/// One row of the sound pack picker.
#[derive(Debug, Clone)]
pub struct SoundPackEntry {
    pub id: String,
    pub version: String,
    pub name: Localized,
    pub author: String,
    pub origin: SkinOrigin,
    /// The pack as validated by the scan; `None` when it does not load.
    pub pack: Option<LoadedSoundPack>,
    /// Why the pack does not load; empty when it does.
    pub errors: Vec<SkinDiagnostic>,
}

/// A sound pack ready to decode: its sounds and the files they name.
#[derive(Debug, Clone)]
pub struct LoadedSoundPack {
    pub id: String,
    pub sounds: Sounds,
    pub files: Arc<dyn SkinFiles>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::MemoryFiles;

    fn files(manifest: &'static str) -> MemoryFiles {
        MemoryFiles::new(&[
            (SOUNDS_MANIFEST, manifest.as_bytes()),
            ("click.wav", b"RIFF"),
            ("ding.ogg", b"OggS"),
        ])
    }

    #[test]
    fn a_pack_names_only_its_own_files() {
        let sounds = read_sounds(&files(
            "[sounds]\nopen = \"ding.ogg\"\n[sounds.typing]\nfiles = [\"click.wav\"]\nvolume = 0.4\n[sounds.se]\nbell = \"ding.ogg\"\n",
        ))
        .unwrap();
        assert_eq!(sounds.typing.files, ["click.wav"]);
        assert_eq!(sounds.typing.volume, 0.4);
        assert_eq!(sounds.se["bell"], "ding.ogg");

        assert!(matches!(
            read_sounds(&files("[sounds]\nadvance = \"missing.wav\"\n")),
            Err(FileError::Missing(_))
        ));
        assert!(read_sounds(&files("[sounds]\nopen = \"../click.wav\"\n")).is_err());
        assert!(read_sounds(&files("[sounds]\n[keys]\n")).is_err());
        assert!(read_sounds(&files("[sounds.se]\n\" \" = \"ding.ogg\"\n")).is_err());
    }

    #[test]
    fn a_pack_replaces_what_it_defines_and_keeps_the_skins_effects() {
        let mut skin = Sounds::default();
        skin.typing.files = vec!["skin-key.wav".into()];
        skin.open = Some("skin-open.wav".into());
        skin.advance = Some("skin-next.wav".into());
        skin.se.insert("bell".into(), "skin-bell.wav".into());
        skin.se.insert("door".into(), "skin-door.wav".into());

        let mut pack = Sounds::default();
        pack.typing.files = vec!["pack-key.wav".into()];
        pack.typing.every_n = 2;
        pack.open = Some("pack-open.wav".into());
        pack.se.insert("bell".into(), "pack-bell.wav".into());
        pack.se.insert("chime".into(), "pack-chime.wav".into());

        let layered = layer(&skin, &pack);
        assert_eq!(layered.typing, pack.typing);
        assert_eq!(layered.open.as_deref(), Some("pack-open.wav"));
        assert_eq!(layered.advance.as_deref(), Some("skin-next.wav"));
        assert_eq!(layered.se["bell"], "pack-bell.wav");
        assert_eq!(layered.se["door"], "skin-door.wav");
        assert_eq!(layered.se["chime"], "pack-chime.wav");

        // Effects alone leave the typing rhythm to the skin.
        let effects_only = Sounds {
            se: pack.se.clone(),
            ..Sounds::default()
        };
        assert_eq!(layer(&skin, &effects_only).typing, skin.typing);
    }

    #[test]
    fn installed_sound_packs_are_listed_with_what_keeps_them_from_loading() {
        let dir = tempfile::tempdir().unwrap();
        let packages = dir.path().join("packages");
        let write = |id: &str, sounds: &str| {
            let root = packages.join(id);
            std::fs::create_dir_all(root.join("sounds")).unwrap();
            std::fs::write(
                root.join("package.toml"),
                format!(
                    "format = 1\nschema = 3\nid = \"{id}\"\nversion = \"1.0.0\"\nmin_app = \"0.1.0\"\ncapabilities = [\"sounds\"]\n\n[name]\nen-US = \"Clicks\"\n"
                ),
            )
            .unwrap();
            std::fs::write(root.join(SOUNDS_MANIFEST), sounds).unwrap();
            std::fs::write(root.join("sounds/a.wav"), b"RIFF").unwrap();
        };
        write(
            "test.clicks",
            "[sounds.typing]\nfiles = [\"sounds/a.wav\"]\n",
        );
        write("test.broken", "[sounds]\nopen = \"sounds/missing.wav\"\n");
        let registry = crate::SkinRegistry::new(Some(packages), "1.0.0");

        let ok = registry.sound_pack("test.clicks").unwrap();
        assert!(ok.errors.is_empty(), "{:?}", ok.errors);
        assert_eq!(ok.name["en-US"], "Clicks");
        assert_eq!(
            ok.pack.as_ref().unwrap().sounds.typing.files,
            ["sounds/a.wav"]
        );
        let pack = ok.pack.as_ref().unwrap();
        assert_eq!(pack.files.read("sounds/a.wav").unwrap(), b"RIFF");

        let broken = registry.sound_pack("test.broken").unwrap();
        assert!(broken.pack.is_none() && !broken.errors.is_empty());
        // Sound packs are not skins.
        assert!(registry.entry("test.clicks").is_none());
    }
}
