//! Auditable package projects and selective, detached Ed25519 seals.
use crate::{FileError, PACKAGE_MANIFEST, PackageManifest, SkinFiles, clean_path};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::Path,
};

pub const INTEGRITY_FILE: &str = "package.integrity.json";
const DOMAIN: &[u8] = b"NanaType package seal v1\0";
fn error(message: impl ToString) -> FileError {
    FileError::Io(message.to_string())
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn unhex<const N: usize>(text: &str) -> Result<[u8; N], FileError> {
    if text.len() != N * 2 || !text.is_ascii() {
        return Err(error("invalid seal encoding"));
    }
    let mut bytes = [0; N];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).map_err(error)?;
    }
    Ok(bytes)
}
fn local(path: &str) -> bool {
    !path.starts_with("pkg:") && !path.starts_with("dependencies/") && path != INTEGRITY_FILE
}

#[derive(Clone)]
pub struct SealIdentity(SigningKey);
impl SealIdentity {
    pub fn generate() -> Result<Self, FileError> {
        let mut bytes = [0; 32];
        getrandom::fill(&mut bytes).map_err(error)?;
        Ok(Self::from_bytes(bytes))
    }
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(SigningKey::from_bytes(&bytes))
    }
    /// Host-only key persistence. Never write these bytes into a package.
    pub fn secret_bytes(&self) -> [u8; 32] {
        self.0.to_bytes()
    }
    pub fn fingerprint(&self) -> String {
        blake3::hash(self.0.verifying_key().as_bytes())
            .to_hex()
            .to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageSeal {
    pub format: u32,
    pub public_key: String,
    pub protected: BTreeMap<String, String>,
    pub editable: BTreeSet<String>,
    pub signature: String,
}
impl PackageSeal {
    fn message(&self) -> Result<Vec<u8>, FileError> {
        let mut message = DOMAIN.to_vec();
        message.extend(
            serde_json::to_vec(&(
                self.format,
                &self.public_key,
                &self.protected,
                &self.editable,
            ))
            .map_err(error)?,
        );
        Ok(message)
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealStatus {
    pub fingerprint: String,
    pub protected: BTreeSet<String>,
    pub editable: BTreeSet<String>,
}
pub fn verify_integrity(files: &dyn SkinFiles) -> Result<Option<SealStatus>, FileError> {
    if !files.list().iter().any(|p| p == INTEGRITY_FILE) {
        return Ok(None);
    }
    if files.file_len(INTEGRITY_FILE)? > 512 * 1024 {
        return Err(error("seal exceeds budget"));
    }
    let seal: PackageSeal = serde_json::from_slice(&files.read(INTEGRITY_FILE)?).map_err(error)?;
    if seal.format != 1
        || seal.protected.len() + seal.editable.len() > 4096
        || !seal.protected.contains_key(PACKAGE_MANIFEST)
    {
        return Err(error("invalid seal policy"));
    }
    let mut inventory = BTreeSet::new();
    for path in seal.protected.keys().chain(seal.editable.iter()) {
        if clean_path(path)? != *path || !local(path) || !inventory.insert(path.clone()) {
            return Err(error("invalid or overlapping seal paths"));
        }
    }
    let actual: BTreeSet<_> = files.list().into_iter().filter(|p| local(p)).collect();
    if inventory != actual {
        return Err(error("package file inventory differs from signed policy"));
    }
    let mut total = 0;
    for path in &actual {
        let bytes = files.file_len(path)?;
        total += bytes;
        if bytes > crate::MAX_FILE_BYTES || total > crate::MAX_PACKAGE_BYTES {
            return Err(error("signed package exceeds resource budget"));
        }
    }
    let key = VerifyingKey::from_bytes(&unhex::<32>(&seal.public_key)?).map_err(error)?;
    key.verify_strict(
        &seal.message()?,
        &Signature::from_bytes(&unhex::<64>(&seal.signature)?),
    )
    .map_err(|_| error("package signature is invalid"))?;
    for (path, expected) in &seal.protected {
        if files.file_len(path)? > crate::MAX_FILE_BYTES {
            return Err(error("protected file exceeds budget"));
        }
        if blake3::hash(&files.read(path)?).to_hex().as_str() != expected {
            return Err(error(format!("protected file was modified: {path}")));
        }
    }
    Ok(Some(SealStatus {
        fingerprint: blake3::hash(key.as_bytes()).to_hex().to_string(),
        protected: seal.protected.into_keys().collect(),
        editable: seal.editable,
    }))
}

/// Immutable, bounded snapshots avoid authorisation/read time-of-check races.
#[derive(Debug, Clone, Default)]
pub struct PackageProject {
    pub files: BTreeMap<String, Vec<u8>>,
}
impl SkinFiles for PackageProject {
    fn read(&self, path: &str) -> Result<Vec<u8>, FileError> {
        self.files
            .get(&clean_path(path)?)
            .cloned()
            .ok_or_else(|| FileError::Missing(path.into()))
    }
    fn file_len(&self, path: &str) -> Result<u64, FileError> {
        self.files
            .get(&clean_path(path)?)
            .map(|b| b.len() as u64)
            .ok_or_else(|| FileError::Missing(path.into()))
    }
    fn list(&self) -> Vec<String> {
        self.files.keys().cloned().collect()
    }
}
impl PackageProject {
    pub fn capture(files: &dyn SkinFiles) -> Result<Self, FileError> {
        let mut project = Self::default();
        let paths = files.list();
        if paths.len() > 4096 {
            return Err(error("too many project files"));
        }
        let mut total = 0;
        for path in paths {
            let size = files.file_len(&path)?;
            total += size;
            if size > crate::MAX_FILE_BYTES || total > crate::MAX_PACKAGE_BYTES {
                return Err(error("project resource budget exceeded"));
            }
            let target = crate::registry::bundled_path(&path)?;
            let bytes = files.read(&path)?;
            if bytes.len() as u64 != size {
                return Err(error("package changed during snapshot"));
            }
            if project.files.insert(target, bytes).is_some() {
                return Err(error("duplicate project file"));
            }
        }
        verify_integrity(&project)?;
        Ok(project)
    }
    pub fn digest(&self) -> String {
        let mut hash = blake3::Hasher::new();
        hash.update(b"NanaType activation v1\0");
        for (path, bytes) in &self.files {
            hash.update(&(path.len() as u64).to_le_bytes());
            hash.update(path.as_bytes());
            hash.update(&(bytes.len() as u64).to_le_bytes());
            hash.update(bytes);
        }
        hash.finalize().to_hex().to_string()
    }
    pub fn manifest(&self) -> Result<PackageManifest, FileError> {
        PackageManifest::read(self)
    }
    pub fn can_edit(&self, path: &str) -> bool {
        if path.starts_with("dependencies/") || path == INTEGRITY_FILE {
            return false;
        }
        match verify_integrity(self) {
            Ok(None) => true,
            Ok(Some(seal)) => seal.editable.contains(path),
            Err(_) => false,
        }
    }
    pub fn set(&mut self, path: &str, bytes: Vec<u8>) -> Result<(), FileError> {
        let path = clean_path(path)?;
        if !self.can_edit(&path) {
            return Err(error(
                "file is protected; create an unsigned copy to edit it",
            ));
        }
        let previous = self.files.insert(path.clone(), bytes);
        if let Err(error) = self.check_budget() {
            if let Some(bytes) = previous {
                self.files.insert(path, bytes);
            } else {
                self.files.remove(&path);
            }
            return Err(error);
        }
        Ok(())
    }
    pub fn check_budget(&self) -> Result<(), FileError> {
        if self.files.len() > 4096
            || self
                .files
                .values()
                .any(|b| b.len() as u64 > crate::MAX_FILE_BYTES)
            || self.files.values().map(|b| b.len() as u64).sum::<u64>() > crate::MAX_PACKAGE_BYTES
        {
            return Err(error("project resource budget exceeded"));
        }
        let mut paths = BTreeSet::new();
        for path in self.files.keys() {
            if clean_path(path)? != *path || !paths.insert(path.to_lowercase()) {
                return Err(error("unsafe or colliding project paths"));
            }
        }
        Ok(())
    }
    pub fn seal(
        &mut self,
        identity: &SealIdentity,
        protected: &BTreeSet<String>,
    ) -> Result<SealStatus, FileError> {
        self.check_budget()?;
        let mut selected = protected.clone();
        selected.insert(PACKAGE_MANIFEST.into());
        if selected
            .iter()
            .any(|p| !local(p) || !self.files.contains_key(p))
        {
            return Err(error("unknown protected file"));
        }
        let mut seal = PackageSeal {
            format: 1,
            public_key: hex(identity.0.verifying_key().as_bytes()),
            protected: BTreeMap::new(),
            editable: BTreeSet::new(),
            signature: String::new(),
        };
        for (path, bytes) in self.files.iter().filter(|(p, _)| local(p)) {
            if selected.contains(path) {
                seal.protected
                    .insert(path.clone(), blake3::hash(bytes).to_hex().to_string());
            } else {
                seal.editable.insert(path.clone());
            }
        }
        seal.signature = hex(&identity.0.sign(&seal.message()?).to_bytes());
        self.files.insert(
            INTEGRITY_FILE.into(),
            serde_json::to_vec_pretty(&seal).map_err(error)?,
        );
        verify_integrity(self)?.ok_or_else(|| error("seal was not created"))
    }
    pub fn export(&self, dest: &Path) -> Result<(), FileError> {
        self.check_budget()?;
        self.manifest()?;
        verify_integrity(self)?;
        let file = std::fs::File::create(dest).map_err(error)?;
        let mut zip = zip::ZipWriter::new(file);
        for (path, bytes) in &self.files {
            zip.start_file(
                path,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )
            .map_err(error)?;
            zip.write_all(bytes).map_err(error)?;
        }
        zip.finish().map_err(error)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn project() -> PackageProject {
        PackageProject { files:BTreeMap::from([
        (PACKAGE_MANIFEST.into(),b"format=1\nschema=3\nid='mixed.test'\nversion='1.0.0'\ncapabilities=['skin','resources','lua']\nentry='main.lua'\nauthor='Author'\ncreated_at='2026-10-19'\nlicensed_to='Studio'\n".to_vec()),
        ("skin.toml".into(),b"[chrome]\naccent='#123456'\n".to_vec()),
        ("main.lua".into(),b"return {}".to_vec()),
        ("notes.txt".into(),b"editable".to_vec())]) }
    }
    #[test]
    fn selective_seals_detect_tampering_and_preserve_editable_files() {
        let mut p = project();
        let key = SealIdentity::generate().unwrap();
        let status = p.seal(&key, &BTreeSet::from(["main.lua".into()])).unwrap();
        assert_eq!(status.fingerprint, key.fingerprint());
        p.set("notes.txt", b"changed".to_vec()).unwrap();
        assert!(verify_integrity(&p).unwrap().is_some());
        assert!(p.set("main.lua", b"return {evil=true}".to_vec()).is_err());
        p.files.insert("main.lua".into(), b"tampered".to_vec());
        assert!(verify_integrity(&p).is_err());
    }
    #[test]
    fn seals_cover_metadata_policy_and_inventory_but_never_hide_source() {
        let mut p = project();
        p.seal(&SealIdentity::generate().unwrap(), &BTreeSet::new())
            .unwrap();
        assert_eq!(p.read("main.lua").unwrap(), b"return {}");
        let mut extra = p.clone();
        extra
            .files
            .insert("extra.lua".into(), b"return {}".to_vec());
        assert!(verify_integrity(&extra).is_err());
        p.files
            .get_mut(PACKAGE_MANIFEST)
            .unwrap()
            .extend(b"# edited");
        assert!(verify_integrity(&p).is_err());
    }
    #[test]
    fn signed_mixed_packages_roundtrip_without_rewriting_signed_bytes() {
        let mut p = project();
        p.seal(
            &SealIdentity::generate().unwrap(),
            &BTreeSet::from(["main.lua".into()]),
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let zip = dir.path().join("mixed.ntpkg");
        p.export(&zip).unwrap();
        let mut registry = crate::SkinRegistry::new(Some(dir.path().join("installed")), "1.0.0");
        registry.install(&zip).unwrap();
        let exported = dir.path().join("export.ntpkg");
        registry.export("mixed.test", &exported).unwrap();
        let restored =
            PackageProject::capture(&*registry.inspect_package_archive(&exported).unwrap())
                .unwrap();
        assert_eq!(restored.files, p.files);
        assert_eq!(restored.digest(), p.digest());
    }
    #[test]
    fn metadata_dates_and_signature_policy_are_validated() {
        let mut manifest = project().manifest().unwrap();
        for valid in ["", "2024-02-29", "2026-10-19", "2000-02-29"] {
            manifest.created_at = valid.into();
            assert!(manifest.validate().is_ok(), "{valid}");
        }
        for invalid in [
            "2026-02-29",
            "1900-02-29",
            "2026-13-01",
            "2026-01-00",
            "中中ab12",
            "2026-1-01",
        ] {
            manifest.created_at = invalid.into();
            assert!(manifest.validate().is_err(), "{invalid}");
        }
        manifest.created_at.clear();
        manifest.licensed_to = "a".repeat(1025);
        assert!(manifest.validate().is_err());
        let mut p = project();
        p.seal(
            &SealIdentity::generate().unwrap(),
            &BTreeSet::from(["main.lua".into()]),
        )
        .unwrap();
        let mut seal: PackageSeal = serde_json::from_slice(&p.files[INTEGRITY_FILE]).unwrap();
        seal.protected.remove("main.lua");
        seal.editable.insert("main.lua".into());
        p.files
            .insert(INTEGRITY_FILE.into(), serde_json::to_vec(&seal).unwrap());
        assert!(
            verify_integrity(&p).is_err(),
            "changing the signed protection policy must fail"
        );
    }
    #[test]
    fn rejected_edits_leave_the_original_project_intact() {
        let mut p = project();
        let original = p.files.clone();
        assert!(
            p.set("NOTES.txt", b"conflicting filename".to_vec())
                .is_err()
        );
        assert_eq!(p.files, original);
    }
}
