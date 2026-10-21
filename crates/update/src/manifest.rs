use crate::Error;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Bumped only for changes old clients cannot read.
pub const MANIFEST_SCHEMA: u32 = 1;

/// Largest file a manifest may announce.
const MAX_ASSET_BYTES: u64 = 1 << 31;

/// What a release offers, as signed by the release workflow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    pub version: String,
    /// The git tag, which also names the release's download folder.
    pub tag: String,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    pub name: String,
    pub target: Target,
    pub kind: Kind,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Target {
    #[serde(rename = "windows-x86_64")]
    WindowsX86_64,
    #[serde(rename = "macos-aarch64")]
    MacosAarch64,
    #[serde(rename = "macos-x86_64")]
    MacosX86_64,
    /// A platform this build does not know; never selected.
    #[serde(other)]
    Unknown,
}

impl Target {
    /// The download for the running build. Windows on Arm runs the x64 build.
    pub fn current() -> Option<Self> {
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("windows", _) => Some(Self::WindowsX86_64),
            ("macos", "aarch64") => Some(Self::MacosAarch64),
            ("macos", "x86_64") => Some(Self::MacosX86_64),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// Windows Inno Setup installer.
    Installer,
    /// Windows zip of the program folder.
    Portable,
    /// macOS disk image.
    Dmg,
    /// macOS zip of the app bundle.
    AppZip,
    #[serde(other)]
    Unknown,
}

impl Manifest {
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        let bad = |why: String| Error::BadManifest(why);
        let mut manifest: Self = serde_json::from_slice(bytes).map_err(|e| bad(e.to_string()))?;
        if manifest.schema != MANIFEST_SCHEMA {
            return Err(bad(format!("unsupported schema {}", manifest.schema)));
        }
        if semver::Version::parse(&manifest.version).is_err()
            || manifest.tag != format!("v{}", manifest.version)
        {
            return Err(bad("version and tag disagree".into()));
        }
        for asset in &manifest.assets {
            if !safe_name(&asset.name)
                || asset.size == 0
                || asset.size > MAX_ASSET_BYTES
                || crate::unhex::<32>(&asset.sha256).is_none()
            {
                return Err(bad(format!("invalid asset {:?}", asset.name)));
            }
        }
        for asset in &mut manifest.assets {
            asset.sha256.make_ascii_lowercase();
        }
        Ok(manifest)
    }

    /// The first asset for `target` in the order of `kinds`.
    pub fn select(&self, target: Target, kinds: &[Kind]) -> Option<&Asset> {
        kinds.iter().find_map(|kind| {
            self.assets
                .iter()
                .find(|a| a.target == target && a.kind == *kind)
        })
    }

    pub fn notes_url(&self) -> String {
        format!(
            "https://github.com/{}/releases/tag/{}",
            crate::REPO,
            self.tag
        )
    }

    /// The manifest for the packaged files in `dir`, named as
    /// scripts/package.ps1 and scripts/package-macos.sh name them.
    pub fn from_dir(dir: &Path, version: &str) -> Result<Self, Error> {
        use sha2::{Digest, Sha256};
        let mut assets = Vec::new();
        let mut entries: Vec<_> = std::fs::read_dir(dir)?
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some((target, kind)) = classify(&name, version) else {
                continue;
            };
            let mut file = std::fs::File::open(entry.path())?;
            let mut hasher = Sha256::new();
            let size = std::io::copy(&mut file, &mut hasher)?;
            assets.push(Asset {
                name,
                target,
                kind,
                size,
                sha256: crate::hex(&hasher.finalize()),
            });
        }
        let manifest = Self {
            schema: MANIFEST_SCHEMA,
            version: version.into(),
            tag: format!("v{version}"),
            assets,
        };
        Self::parse(&serde_json::to_vec(&manifest).map_err(|e| Error::Io(e.to_string()))?)
    }
}

/// A plain file name, safe to join onto the download folder.
fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

fn classify(name: &str, version: &str) -> Option<(Target, Kind)> {
    let rest = name.strip_prefix(&format!("NanaType-{version}-"))?;
    Some(match rest {
        "setup.exe" => (Target::WindowsX86_64, Kind::Installer),
        "portable.zip" => (Target::WindowsX86_64, Kind::Portable),
        "macos-arm64.dmg" => (Target::MacosAarch64, Kind::Dmg),
        "macos-arm64.zip" => (Target::MacosAarch64, Kind::AppZip),
        "macos-x86_64.dmg" => (Target::MacosX86_64, Kind::Dmg),
        "macos-x86_64.zip" => (Target::MacosX86_64, Kind::AppZip),
        _ => return None,
    })
}
