//! Skin-owned, offline stroke-order resources. Geometry keeps its upstream order.
mod packed;
use crate::{Severity, Skin, SkinDiagnostic, SkinFiles};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{Arc, LazyLock, Mutex, Weak},
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandwritingSpec {
    pub data: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StrokeCommand {
    Move([f32; 2]),
    Line([f32; 2]),
    Quad([[f32; 2]; 2]),
    Cubic([[f32; 2]; 3]),
    Close,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StrokeGlyph {
    pub strokes: Vec<Vec<StrokeCommand>>,
    pub medians: Vec<Vec<[f32; 2]>>,
}

#[derive(Debug)]
pub struct StrokeData {
    pub hash: [u8; 32],
    pub format: u32,
    pub notice: String,
    pub source: String,
    pub source_sha256: String,
    storage: packed::Packed,
}

impl PartialEq for StrokeData {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash
    }
}

impl StrokeData {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > 32 * 1024 * 1024 {
            return Err("stroke data exceeds 32 MiB".into());
        }
        if !bytes.starts_with(packed::MAGIC) {
            return Err("stroke data requires the NTHZ v1 binary container".into());
        }
        packed::Packed::parse(bytes)
    }

    pub fn len(&self) -> usize {
        self.storage.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Only the requested 64-character block is decompressed. Eight blocks are
    /// retained; already returned glyphs remain valid when their block is evicted.
    pub fn glyph(&self, character: char) -> Option<Arc<StrokeGlyph>> {
        self.storage.glyph(character).ok().flatten()
    }

    /// Imports validate every block without populating the runtime cache.
    pub fn validate_all(&self) -> Result<(), String> {
        self.storage.validate_all()
    }
}

static DATA: LazyLock<Mutex<BTreeMap<[u8; 32], Weak<StrokeData>>>> =
    LazyLock::new(Default::default);

pub(crate) fn load_handwriting(
    skin: &mut Skin,
    files: &dyn SkinFiles,
    out: &mut Vec<SkinDiagnostic>,
    validate: bool,
) {
    let Some(spec) = &skin.dialogue.reveal.handwriting else {
        return;
    };
    let result = (|| {
        let path = crate::clean_path(&spec.data).map_err(|e| e.to_string())?;
        let bytes = files.read(&path).map_err(|e| e.to_string())?;
        let hash = *blake3::hash(&bytes).as_bytes();
        let mut cache = DATA.lock().unwrap_or_else(|e| e.into_inner());
        let data = match cache.get(&hash).and_then(Weak::upgrade) {
            Some(data) => data,
            None => Arc::new(StrokeData::parse(&bytes)?),
        };
        if validate {
            data.validate_all()?;
        }
        cache.retain(|_, v| v.strong_count() > 0);
        cache.insert(hash, Arc::downgrade(&data));
        Ok::<_, String>(data)
    })();
    match result {
        Ok(data) => skin.stroke_data = Some(data),
        Err(message) => out.push(SkinDiagnostic {
            severity: Severity::Error,
            field: "dialogue.reveal.handwriting.data".into(),
            message,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BaseResolver, DirFiles, SkinOrigin, SkinRegistry, TomlTable, ZipFiles, load};
    struct Offline;
    impl BaseResolver for Offline {
        fn base(&self, _: &str) -> Option<(TomlTable, Arc<dyn SkinFiles>)> {
            None
        }
    }
    #[test]
    fn love_letter_roundtrips_all_resources_without_the_original_directory() {
        let registry = SkinRegistry::new(None, "1.0.0");
        let original = registry.load("nanatype.love_letter").unwrap();
        let data = original.skin.stroke_data.as_ref().unwrap();
        assert_eq!(data.len(), 6763);
        assert_eq!(data.glyph('心').unwrap().strokes.len(), 4);
        assert!(data.glyph('A').is_none());
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("love-letter.ntskin");
        registry.export("nanatype.love_letter", &archive).unwrap();
        let files = Arc::new(ZipFiles::open(&archive).unwrap());
        let restored = load(
            files.clone(),
            SkinOrigin::Archive(archive.clone()),
            &Offline,
            "1.0.0",
        )
        .unwrap();
        assert_eq!(restored.skin.stroke_data, original.skin.stroke_data);
        for path in [
            "love_letter/fonts/LXGWWenKai-Regular.ttf",
            "love_letter/fonts/OFL.txt",
            "love_letter/ARPHICPL.TXT",
            "love_letter/SOURCES.txt",
            "love_letter/write.wgsl",
            "love_letter/dissolve.wgsl",
        ] {
            assert!(!files.read(path).unwrap().is_empty(), "{path}");
        }
        if let Ok(destination) = std::env::var("NANATYPE_LOVE_LETTER_EXPORT") {
            std::fs::copy(archive, destination).unwrap();
        }
    }
    #[test]
    fn malformed_or_missing_stroke_resources_fail_import_and_inheritance_exports() {
        let dir = tempfile::tempdir().unwrap();
        for path in ["missing.nthz", "../outside.nthz", "C:/outside.nthz"] {
            std::fs::write(
                dir.path().join("skin.toml"),
                format!("[meta]\nid='test.stroke'\n[dialogue.reveal.handwriting]\ndata='{path}'\n"),
            )
            .unwrap();
            assert!(
                load(
                    Arc::new(DirFiles::new(dir.path())),
                    SkinOrigin::Directory(dir.path().into()),
                    &Offline,
                    "1.0.0"
                )
                .is_err()
            );
        }
        let mut registry = SkinRegistry::new(Some(dir.path().join("skins")), "1.0.0");
        let name = [("en-US".into(), "My letter".into())].into_iter().collect();
        registry
            .duplicate(
                "nanatype.love_letter",
                "test.inherited-letter",
                &name,
                &TomlTable::new(),
            )
            .unwrap();
        let archive = dir.path().join("child.ntskin");
        registry.export("test.inherited-letter", &archive).unwrap();
        let loaded = load(
            Arc::new(ZipFiles::open(&archive).unwrap()),
            SkinOrigin::Archive(archive),
            &Offline,
            "1.0.0",
        )
        .unwrap();
        assert_eq!(loaded.skin.stroke_data.as_ref().unwrap().len(), 6763);
    }

    #[test]
    fn non_packed_stroke_resources_are_rejected() {
        let legacy = br#"{"format":1,"notice":"legacy","source":"test","source_sha256":"test","characters":{"A":{"strokes":["M 0 0 L 1 1 Z"],"medians":[[[0.25,0.75],[1.5,1]]]}}}"#;
        assert_eq!(
            StrokeData::parse(legacy).unwrap_err(),
            "stroke data requires the NTHZ v1 binary container"
        );
    }
}
