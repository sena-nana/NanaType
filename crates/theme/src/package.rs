//! Versioned package identity and explicit, read-only resource namespaces.
use crate::{FileError, Meta, SkinFiles, clean_path};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path, sync::Arc};

pub const PACKAGE_MANIFEST: &str = "package.toml";
pub const PACKAGE_FORMAT: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageCapability {
    Skin,
    Resources,
    Lua,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackageManifest {
    pub format: u32,
    #[serde(flatten)]
    pub meta: Meta,
    pub capabilities: Vec<PackageCapability>,
    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,
    #[serde(default)]
    pub catalogue_order: u32,
    /// Lua source entry; never a native module or bytecode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
    /// Author-declared creation date and license recipient, independent of capability.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub created_at: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub licensed_to: String,
}

impl PackageManifest {
    pub fn to_toml(&self) -> Result<String, FileError> {
        self.validate()?;
        toml::to_string_pretty(self).map_err(|e| FileError::Io(e.to_string()))
    }
    pub fn read(files: &dyn SkinFiles) -> Result<Self, FileError> {
        if let Some(package) = files.cached_package_manifest() {
            package.validate()?;
            return Ok(package.clone());
        }
        let bytes = files.read(PACKAGE_MANIFEST)?;
        if bytes.len() > 64 * 1024 {
            return Err(FileError::TooLarge {
                path: PACKAGE_MANIFEST.into(),
                bytes: bytes.len() as u64,
            });
        }
        let text = std::str::from_utf8(&bytes).map_err(|e| FileError::Io(e.to_string()))?;
        Self::parse(text)
    }

    /// Parse strictly: identity and permissions never silently drop a misspelt key.
    pub fn parse(text: &str) -> Result<Self, FileError> {
        let table: toml::Table = toml::from_str(text).map_err(|e| FileError::Io(e.to_string()))?;
        if let Some(key) = table.keys().find(|k| !ROOT_KEYS.contains(&k.as_str())) {
            return Err(FileError::Io(format!("package.toml: unknown key `{key}`")));
        }
        let package: Self = table
            .try_into()
            .map_err(|e: toml::de::Error| FileError::Io(e.to_string()))?;
        package.validate()?;
        Ok(package)
    }

    /// Declared capabilities must describe what the package actually ships.
    pub(crate) fn validate_content(&self, files: &dyn SkinFiles) -> Result<(), FileError> {
        let invalid = |message: &str| FileError::Io(format!("package.toml: {message}"));
        let has = |c| self.capabilities.contains(&c);
        // Listing, not reading: payloads stay unread until budgets are checked.
        let listed = files.list();
        let exists = |path: &str| listed.iter().any(|p| p == path);
        if has(PackageCapability::Skin) != exists(crate::load::MANIFEST) {
            return Err(invalid(
                "the skin capability and skin.toml must appear together",
            ));
        }
        if let Some(entry) = &self.entry
            && !exists(entry)
        {
            return Err(invalid("the Lua entry does not exist"));
        }
        if has(PackageCapability::Resources)
            && !listed.iter().any(|path| {
                !path.starts_with("pkg:")
                    && !path.starts_with("dependencies/")
                    && ![
                        PACKAGE_MANIFEST,
                        crate::INTEGRITY_FILE,
                        crate::load::MANIFEST,
                    ]
                    .contains(&path.as_str())
                    && self.entry.as_deref() != Some(path)
            })
        {
            return Err(invalid("the resources capability needs resource files"));
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), FileError> {
        let invalid = |message: &str| FileError::Io(format!("package.toml: {message}"));
        if self.meta.author.len() > 1024
            || self.licensed_to.len() > 1024
            || self.meta.license.len() > 1024
            || !valid_date(&self.created_at)
        {
            return Err(invalid(
                "author/license recipient too long or invalid YYYY-MM-DD creation date",
            ));
        }
        if crate::load::version_tuple(&self.meta.min_app).is_none() {
            return Err(invalid("invalid min_app version"));
        }
        if self.format != PACKAGE_FORMAT || self.meta.schema != crate::SKIN_SCHEMA {
            return Err(invalid(
                "unsupported package format/schema; legacy packages must be recreated",
            ));
        }
        if !valid_id(&self.meta.id)
            || !exact_version(&self.meta.version)
            || self.capabilities.is_empty()
            || self.capabilities.len() > 3
            || self.dependencies.len() > 32
        {
            return Err(invalid(
                "invalid package identity, exact version or capabilities",
            ));
        }
        for (id, version) in &self.dependencies {
            if !valid_id(id) || id == &self.meta.id || !exact_version(version) {
                return Err(invalid(
                    "dependencies require a different package ID and exact version",
                ));
            }
        }
        if self.capabilities.contains(&PackageCapability::Lua) {
            let entry = self
                .entry
                .as_deref()
                .ok_or_else(|| invalid("Lua package needs an entry"))?;
            clean_path(entry)?;
            if !entry.ends_with(".lua") {
                return Err(invalid("Lua entry must be source (.lua)"));
            }
        } else if self.entry.is_some() {
            return Err(invalid("entry requires the lua capability"));
        }
        Ok(())
    }
}

/// Every accepted top-level key of `package.toml`, including flattened `Meta`.
const ROOT_KEYS: &[&str] = &[
    "format",
    "capabilities",
    "dependencies",
    "catalogue_order",
    "entry",
    "created_at",
    "licensed_to",
    "schema",
    "id",
    "version",
    "min_app",
    "base",
    "name",
    "description",
    "author",
    "license",
    "preview",
    "tags",
];

fn valid_date(date: &str) -> bool {
    if date.is_empty() {
        return true;
    }
    if !date.is_ascii()
        || date.len() != 10
        || date.as_bytes()[4] != b'-'
        || date.as_bytes()[7] != b'-'
    {
        return false;
    }
    let (Ok(year), Ok(month), Ok(day)) = (
        date[..4].parse::<u32>(),
        date[5..7].parse::<u32>(),
        date[8..].parse::<u32>(),
    ) else {
        return false;
    };
    let days = match month {
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(400) || (year.is_multiple_of(4) && !year.is_multiple_of(100)) => {
            29
        }
        2 => 28,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => 0,
    };
    year > 0 && day > 0 && day <= days
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"._-".contains(&c))
        && id.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
        && !id.ends_with('.')
        && clean_path(id).is_ok()
}

fn exact_version(version: &str) -> bool {
    let parts: Vec<_> = version.split('.').collect();
    version.len() <= 32
        && parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Validate a manifest resource reference; filesystem paths use `clean_path`.
pub fn clean_resource_path(path: &str) -> Result<String, FileError> {
    if let Some(rest) = path.strip_prefix("pkg:") {
        let (id, rel) = rest
            .split_once('/')
            .ok_or_else(|| FileError::UnsafePath(path.into()))?;
        if !valid_id(id) {
            return Err(FileError::UnsafePath(path.into()));
        }
        Ok(format!("pkg:{id}/{}", clean_path(rel)?))
    } else {
        clean_path(path)
    }
}

/// A view into a bundled dependency; callers never receive its physical path.
#[derive(Debug)]
pub(crate) struct PrefixFiles {
    pub files: Arc<dyn SkinFiles>,
    pub prefix: String,
}
impl SkinFiles for PrefixFiles {
    fn file_len(&self, path: &str) -> Result<u64, FileError> {
        self.files
            .file_len(&format!("{}/{}", self.prefix, clean_path(path)?))
    }
    fn read(&self, path: &str) -> Result<Vec<u8>, FileError> {
        self.files
            .read(&format!("{}/{}", self.prefix, clean_path(path)?))
    }
    fn list(&self) -> Vec<String> {
        let prefix = format!("{}/", self.prefix);
        self.files
            .list()
            .into_iter()
            .filter_map(|p| p.strip_prefix(&prefix).map(str::to_owned))
            .collect()
    }
    fn watch_roots(&self) -> Vec<std::path::PathBuf> {
        self.files.watch_roots()
    }
}

#[derive(Debug)]
pub(crate) struct NamespaceFiles {
    pub files: Arc<dyn SkinFiles>,
    pub id: String,
}
impl SkinFiles for NamespaceFiles {
    fn file_len(&self, path: &str) -> Result<u64, FileError> {
        if path.starts_with("pkg:") {
            self.files.file_len(path)
        } else {
            self.files
                .file_len(&format!("pkg:{}/{}", self.id, clean_path(path)?))
        }
    }
    fn read(&self, path: &str) -> Result<Vec<u8>, FileError> {
        if path.starts_with("pkg:") {
            self.files.read(path)
        } else {
            self.files
                .read(&format!("pkg:{}/{}", self.id, clean_path(path)?))
        }
    }
    fn list(&self) -> Vec<String> {
        let prefix = format!("pkg:{}/", self.id);
        self.files
            .list()
            .into_iter()
            .filter_map(|p| p.strip_prefix(&prefix).map(str::to_owned))
            .collect()
    }
    fn watch_roots(&self) -> Vec<std::path::PathBuf> {
        self.files.watch_roots()
    }
}

#[derive(Debug)]
pub(crate) struct PackageFiles {
    pub manifest: PackageManifest,
    pub local: Arc<dyn SkinFiles>,
    pub dependencies: BTreeMap<String, Arc<dyn SkinFiles>>,
}
impl SkinFiles for PackageFiles {
    fn cached_skin_manifest(&self) -> Option<&toml::Table> {
        self.local.cached_skin_manifest()
    }
    fn cached_package_manifest(&self) -> Option<&PackageManifest> {
        Some(&self.manifest)
    }
    fn file_len(&self, path: &str) -> Result<u64, FileError> {
        let path = clean_resource_path(path)?;
        if let Some(rest) = path.strip_prefix("pkg:") {
            let (id, relative) = rest.split_once('/').unwrap();
            if id == self.manifest.meta.id {
                self.local.file_len(relative)
            } else {
                self.dependencies
                    .get(id)
                    .ok_or_else(|| FileError::Missing(id.into()))?
                    .file_len(relative)
            }
        } else {
            self.local.file_len(&path)
        }
    }
    fn read(&self, path: &str) -> Result<Vec<u8>, FileError> {
        let clean = clean_resource_path(path)?;
        if let Some(rest) = clean.strip_prefix("pkg:") {
            let (id, rel) = rest.split_once('/').unwrap();
            if id == self.manifest.meta.id {
                return self.local.read(rel);
            }
            self.dependencies
                .get(id)
                .ok_or_else(|| FileError::Missing(format!("undeclared dependency {id}")))?
                .read(rel)
        } else {
            self.local.read(&clean)
        }
    }
    fn list(&self) -> Vec<String> {
        let mut paths: Vec<_> = self
            .local
            .list()
            .into_iter()
            .filter(|p| !p.starts_with("dependencies/"))
            .collect();
        for (id, files) in &self.dependencies {
            paths.extend(
                files
                    .list()
                    .into_iter()
                    .filter(|p| !p.starts_with("dependencies/") && !p.starts_with("pkg:"))
                    .map(|p| format!("pkg:{id}/{p}")),
            );
        }
        paths.sort();
        paths.dedup();
        paths
    }
    fn root(&self) -> Option<&Path> {
        self.local.root()
    }
    fn watch_roots(&self) -> Vec<std::path::PathBuf> {
        let mut roots = self.local.watch_roots();
        roots.extend(self.dependencies.values().flat_map(|f| f.watch_roots()));
        roots.sort();
        roots.dedup();
        roots
    }
}

/// Qualify inherited file references with their original package owner.
pub(crate) fn qualify_resources(table: &mut toml::Table, owner: &str) {
    /// Keys whose string (or string array) value is a file reference.
    const FILE_KEYS: &[&str] = &[
        "file",
        "latin_file",
        "source",
        "image",
        "background",
        "preview",
        "data",
        "textures",
        "files",
        "open",
        "advance",
    ];
    /// Tables whose every string value is a file reference.
    const FILE_MAPS: &[&str] = &["variants", "se"];
    fn visit(value: &mut toml::Value, key: &str, in_map: bool, owner: &str) {
        match value {
            toml::Value::String(s) if in_map || FILE_KEYS.contains(&key) => {
                if !s.is_empty()
                    && !s.starts_with('#')
                    && !s.starts_with("builtin:")
                    && !s.starts_with("pkg:")
                {
                    *s = format!("pkg:{owner}/{s}");
                }
            }
            toml::Value::Table(t) => {
                let map = FILE_MAPS.contains(&key);
                for (k, v) in t {
                    visit(v, k, map, owner);
                }
            }
            toml::Value::Array(a) => {
                for v in a {
                    visit(v, key, in_map, owner);
                }
            }
            _ => {}
        }
    }
    for (key, value) in table {
        visit(value, key, false, owner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DirFiles, SKIN_SCHEMA, SkinRegistry};
    use std::fs;
    fn put(
        root: &Path,
        id: &str,
        capabilities: Vec<PackageCapability>,
        dependencies: &[(&str, &str)],
        base: Option<&str>,
        skin: Option<&str>,
    ) {
        let dir = root.join(id);
        fs::create_dir_all(&dir).unwrap();
        let package = PackageManifest {
            format: PACKAGE_FORMAT,
            meta: Meta {
                id: id.into(),
                version: "1.0.0".into(),
                base: base.map(str::to_owned),
                ..Default::default()
            },
            capabilities,
            dependencies: dependencies
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            catalogue_order: 0,
            entry: None,
            created_at: String::new(),
            licensed_to: String::new(),
        };
        fs::write(
            dir.join(PACKAGE_MANIFEST),
            toml::to_string_pretty(&package).unwrap(),
        )
        .unwrap();
        if let Some(skin) = skin {
            fs::write(dir.join("skin.toml"), skin).unwrap();
        }
    }
    #[test]
    fn namespaces_require_declared_exact_dependencies_and_never_escape_roots() {
        let dir = tempfile::tempdir().unwrap();
        put(
            dir.path(),
            "shared",
            vec![PackageCapability::Resources],
            &[],
            None,
            None,
        );
        fs::write(dir.path().join("shared/note.txt"), "shared").unwrap();
        put(
            dir.path(),
            "consumer",
            vec![PackageCapability::Skin],
            &[("shared", "1.0.0")],
            None,
            Some(""),
        );
        let registry = SkinRegistry::new(Some(dir.path().into()), "1.0.0");
        let files = registry.package_files("consumer").unwrap();
        assert_eq!(files.read("pkg:shared/note.txt").unwrap(), b"shared");
        for path in [
            "pkg:other/note.txt",
            "pkg:shared/../note.txt",
            "pkg:shared/C:/x",
            "https://example.com/file",
            "\\\\server\\file",
        ] {
            assert!(files.read(path).is_err(), "{path}");
        }
        assert!(
            registry
                .package_files("shared")
                .unwrap()
                .read("pkg:consumer/skin.toml")
                .is_err()
        );
        put(
            dir.path(),
            "consumer",
            vec![PackageCapability::Skin],
            &[("shared", "2.0.0")],
            None,
            Some(""),
        );
        assert!(registry.package_files("consumer").is_err());
    }
    #[test]
    fn cycles_missing_dependencies_and_legacy_packages_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        put(
            dir.path(),
            "a",
            vec![PackageCapability::Resources],
            &[("b", "1.0.0")],
            None,
            None,
        );
        put(
            dir.path(),
            "b",
            vec![PackageCapability::Resources],
            &[("a", "1.0.0")],
            None,
            None,
        );
        for id in ["a", "b"] {
            fs::write(dir.path().join(id).join("note.txt"), id).unwrap();
        }
        let registry = SkinRegistry::new(Some(dir.path().into()), "1.0.0");
        let error = registry.package_files("a").unwrap_err().to_string();
        assert!(error.contains("cyclic"), "{error}");
        put(
            dir.path(),
            "a",
            vec![PackageCapability::Resources],
            &[("absent", "1.0.0")],
            None,
            None,
        );
        assert!(registry.package_files("a").is_err());
        let mut package = PackageManifest::read(&DirFiles::new(dir.path().join("b"))).unwrap();
        package.meta.schema = SKIN_SCHEMA - 1;
        assert!(package.validate().is_err());
    }
    #[test]
    fn full_exports_reinstall_offline_and_can_be_exported_again() {
        let dir = tempfile::tempdir().unwrap();
        let sources = dir.path().join("sources");
        put(
            &sources,
            "shared",
            vec![PackageCapability::Resources],
            &[],
            None,
            None,
        );
        fs::write(sources.join("shared/note.txt"), "portable").unwrap();
        put(
            &sources,
            "consumer",
            vec![PackageCapability::Skin],
            &[("shared", "1.0.0")],
            None,
            Some(""),
        );
        let registry = SkinRegistry::new(Some(sources), "1.0.0");
        let archive = dir.path().join("consumer.ntpkg");
        registry.export("consumer", &archive).unwrap();
        let mut offline = SkinRegistry::new(Some(dir.path().join("offline")), "1.0.0");
        offline.install(&archive).unwrap();
        assert_eq!(
            offline
                .load("consumer")
                .unwrap()
                .files
                .read("pkg:shared/note.txt")
                .unwrap(),
            b"portable"
        );
        let again = dir.path().join("again.ntpkg");
        offline.export("consumer", &again).unwrap();
        offline.inspect_archive(&again).unwrap();
    }
    #[test]
    fn inherited_resources_keep_their_owner_despite_local_filename_collisions() {
        let dir = tempfile::tempdir().unwrap();
        put(
            dir.path(),
            "base",
            vec![PackageCapability::Skin],
            &[],
            None,
            Some("[dialogue]\nbackground='same.png'\n"),
        );
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::new(4, 4)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        fs::write(dir.path().join("base/same.png"), png.into_inner()).unwrap();
        put(
            dir.path(),
            "child",
            vec![PackageCapability::Skin],
            &[("base", "1.0.0")],
            Some("base"),
            Some(""),
        );
        fs::write(dir.path().join("child/same.png"), b"not the base artwork").unwrap();
        let mut registry = SkinRegistry::new(Some(dir.path().into()), "1.0.0");
        let loaded = registry.load("child").unwrap();
        assert_eq!(
            loaded.skin.dialogue.background.as_deref(),
            Some("pkg:base/same.png")
        );
        assert_ne!(
            loaded.files.read("same.png").unwrap(),
            loaded.files.read("pkg:base/same.png").unwrap()
        );
        assert!(registry.remove("base").is_err());
    }
    #[test]
    fn renaming_a_skin_never_changes_its_rendering_contract() {
        let dir = tempfile::tempdir().unwrap();
        let registry = SkinRegistry::new(None, "1.0.0");
        for entry in registry.entries() {
            let original = registry.load(&entry.id).unwrap();
            let new_id = format!("renamed.{}", entry.id);
            let root = dir.path().join(&new_id);
            fs::create_dir_all(&root).unwrap();
            for path in original.files.list() {
                let dest = root.join(&path);
                fs::create_dir_all(dest.parent().unwrap()).unwrap();
                fs::write(dest, original.files.read(&path).unwrap()).unwrap();
            }
            let mut package = PackageManifest::read(&DirFiles::new(&root)).unwrap();
            package.meta.id = new_id;
            fs::write(
                root.join(PACKAGE_MANIFEST),
                toml::to_string_pretty(&package).unwrap(),
            )
            .unwrap();
            let mut expected = (*original.skin).clone();
            expected.meta.id = package.meta.id.clone();
            let loaded = crate::load(
                Arc::new(DirFiles::new(&root)),
                crate::SkinOrigin::Directory(root.clone()),
                &(),
                "1.0.0",
            )
            .unwrap();
            assert_eq!(*loaded.skin, expected, "{}", entry.id);
        }
    }

    #[test]
    fn wrapped_function_packages_import_without_execution_or_skin_entries() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("wrapped.ntpkg");
        let mut zip = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
        let metadata = "format=1\nschema=3\nid='example.function'\nversion='1.0.0'\ncapabilities=['lua']\nentry='main.lua'\n";
        for (path, bytes) in [
            ("wrapper/package.toml", metadata),
            ("wrapper/main.lua", "while true do end"),
        ] {
            zip.start_file(path, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        let mut registry = SkinRegistry::new(Some(dir.path().join("installed")), "1.0.0");
        registry.install(&archive).unwrap();
        assert!(
            registry
                .packages()
                .iter()
                .any(|(p, _)| p.meta.id == "example.function")
        );
        assert!(registry.entry("example.function").is_none());
        assert_eq!(
            registry
                .package_files("example.function")
                .unwrap()
                .read("main.lua")
                .unwrap(),
            b"while true do end"
        );
    }

    #[test]
    fn archives_reject_case_collisions_and_windows_device_aliases() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        for names in [
            ["image.png", "IMAGE.PNG"],
            ["CON.txt", "normal.txt"],
            ["folder./a", "normal.txt"],
        ] {
            let archive = dir.path().join("bad.ntpkg");
            let mut zip = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
            for name in names {
                zip.start_file(name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                zip.write_all(b"data").unwrap();
            }
            zip.finish().unwrap();
            assert!(crate::ZipFiles::open(&archive).is_err());
        }
        for id in [".other.installing", "...", "nul", "con.example", "valid."] {
            assert!(!valid_id(id), "{id}");
        }
    }

    #[test]
    fn package_budgets_are_checked_before_reading_large_resource_payloads() {
        #[derive(Debug)]
        struct Oversized;
        impl SkinFiles for Oversized {
            fn read(&self, path: &str) -> Result<Vec<u8>, FileError> {
                assert_eq!(
                    path, PACKAGE_MANIFEST,
                    "oversized payload must never be read"
                );
                Ok(b"format=1\nschema=3\nid='large'\nversion='1.0.0'\ncapabilities=['resources']\n".to_vec())
            }
            fn file_len(&self, path: &str) -> Result<u64, FileError> {
                Ok(if path == PACKAGE_MANIFEST {
                    100
                } else {
                    crate::MAX_FILE_BYTES + 1
                })
            }
            fn list(&self) -> Vec<String> {
                vec![PACKAGE_MANIFEST.into(), "payload.bin".into()]
            }
        }
        assert!(
            SkinRegistry::new(None, "1.0.0")
                .resolve_package(Arc::new(Oversized))
                .is_err()
        );
    }

    #[test]
    fn unknown_manifest_keys_are_rejected() {
        let valid = "format=1\nschema=3\nid='x'\nversion='1.0.0'\ncapabilities=['resources']\n";
        assert!(PackageManifest::parse(valid).is_ok());
        for typo in [
            "licenced_to='Someone'",
            "capability=['lua']",
            "[dependency]\n'y'='1.0.0'",
        ] {
            let error = PackageManifest::parse(&format!("{valid}{typo}\n"))
                .unwrap_err()
                .to_string();
            assert!(error.contains("unknown key"), "{typo}: {error}");
        }
    }
    #[test]
    fn builtin_manifests_parse_strictly() {
        // Built-ins ship pre-parsed manifests, so check the authored bytes directly.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("builtin");
        let mut count = 0;
        for dir in fs::read_dir(root).unwrap() {
            let path = dir.unwrap().path().join(PACKAGE_MANIFEST);
            if path.exists() {
                let text = fs::read_to_string(&path).unwrap();
                PackageManifest::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                count += 1;
            }
        }
        assert_eq!(count, SkinRegistry::new(None, "1.0.0").entries().len());
    }
    #[test]
    fn capabilities_must_match_content() {
        let dir = tempfile::tempdir().unwrap();
        let registry = SkinRegistry::new(Some(dir.path().into()), "1.0.0");
        let fails = |id: &str| registry.package_files(id).unwrap_err().to_string();
        put(
            dir.path(),
            "styleless",
            vec![PackageCapability::Skin],
            &[],
            None,
            None,
        );
        assert!(fails("styleless").contains("skin.toml"));
        put(
            dir.path(),
            "undeclared",
            vec![PackageCapability::Resources],
            &[],
            None,
            Some(""),
        );
        assert!(fails("undeclared").contains("skin.toml"));
        put(
            dir.path(),
            "empty",
            vec![PackageCapability::Resources],
            &[],
            None,
            None,
        );
        assert!(fails("empty").contains("resource files"));
        fs::write(dir.path().join("empty/note.txt"), "now shipped").unwrap();
        assert!(registry.package_files("empty").is_ok());
        fs::create_dir_all(dir.path().join("script")).unwrap();
        fs::write(
            dir.path().join("script").join(PACKAGE_MANIFEST),
            "format=1\nschema=3\nid='script'\nversion='1.0.0'\ncapabilities=['lua']\nentry='main.lua'\n",
        )
        .unwrap();
        assert!(fails("script").contains("entry"));
        fs::write(dir.path().join("script/main.lua"), "return {}").unwrap();
        assert!(registry.package_files("script").is_ok());
    }
    #[test]
    fn inherited_sounds_and_portrait_variants_keep_their_owner() {
        let dir = tempfile::tempdir().unwrap();
        put(
            dir.path(),
            "base",
            vec![PackageCapability::Skin],
            &[],
            None,
            Some(
                "[sounds]\nopen='open.wav'\nadvance='next.wav'\nse={ding='ding.wav'}\n\
                 [sounds.typing]\nfiles=['key.wav']\n\
                 [dialogue.portrait]\nvariants={smile='smile.png'}\n",
            ),
        );
        let mut table: toml::Table =
            toml::from_str(&fs::read_to_string(dir.path().join("base/skin.toml")).unwrap())
                .unwrap();
        qualify_resources(&mut table, "base");
        let sounds = &table["sounds"];
        assert_eq!(sounds["open"].as_str(), Some("pkg:base/open.wav"));
        assert_eq!(sounds["advance"].as_str(), Some("pkg:base/next.wav"));
        assert_eq!(sounds["se"]["ding"].as_str(), Some("pkg:base/ding.wav"));
        assert_eq!(
            sounds["typing"]["files"][0].as_str(),
            Some("pkg:base/key.wav")
        );
        assert_eq!(
            table["dialogue"]["portrait"]["variants"]["smile"].as_str(),
            Some("pkg:base/smile.png")
        );
    }
}
