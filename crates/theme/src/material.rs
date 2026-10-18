//! Skin-owned fragment programs for NanaUI's shared material contract.
use crate::{Severity, ShaderProgram, ShaderTexture, Skin, SkinDiagnostic, SkinFiles};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{Arc, LazyLock, Mutex, Weak},
};

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MaterialSpec {
    pub source: String,
    pub params: Vec<f32>,
    pub textures: Vec<String>,
}
static VALIDATED: LazyLock<Mutex<BTreeMap<String, Weak<ShaderProgram>>>> =
    LazyLock::new(Default::default);

pub fn validate_material(source: &str) -> Result<ShaderProgram, String> {
    let (_, full) = crate::shader::validate_fragment(
        source,
        nana_ui_core::MATERIAL_ABI,
        "material",
        "MaterialInput",
        4,
    )?;
    Ok(ShaderProgram {
        hash: blake3::hash(full.as_bytes()).to_hex().to_string(),
        source: source.into(),
        textures: vec![],
    })
}

pub(crate) fn load_materials(
    skin: &mut Skin,
    files: &dyn SkinFiles,
    diagnostics: &mut Vec<SkinDiagnostic>,
) {
    let error = |field: String, message: String| SkinDiagnostic {
        field,
        message,
        severity: Severity::Error,
    };
    if skin.materials.len() > 16 || (!skin.materials.is_empty() && skin.meta.schema < 2) {
        diagnostics.push(error(
            "materials".into(),
            "materials require schema 2 and at most sixteen definitions".into(),
        ));
        return;
    }
    let mut programs = BTreeMap::new();
    let mut texture_bytes = 0usize;
    for (id, spec) in &skin.materials {
        let result = (|| -> Result<Arc<ShaderProgram>, String> {
            if id.is_empty()
                || id.len() > 64
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
            {
                return Err("invalid material id".into());
            }
            if spec.params.len() > 8
                || spec.params.iter().any(|v| !v.is_finite())
                || spec.textures.len() > 2
            {
                return Err("materials allow eight finite parameters and two textures".into());
            }
            let path = crate::clean_path(&spec.source).map_err(|e| e.to_string())?;
            let source = files.read(&path).map_err(|e| e.to_string())?;
            let encoded: Vec<_> = spec
                .textures
                .iter()
                .map(|path| {
                    let path = crate::clean_path(path).map_err(|e| e.to_string())?;
                    files.read(&path).map_err(|e| e.to_string())
                })
                .collect::<Result<_, _>>()?;
            let mut hash = blake3::Hasher::new();
            hash.update(nana_ui_core::MATERIAL_ABI.as_bytes());
            hash.update(&(source.len() as u64).to_le_bytes());
            hash.update(&source);
            for texture in &encoded {
                hash.update(&(texture.len() as u64).to_le_bytes());
                hash.update(texture);
            }
            let key = hash.finalize().to_hex().to_string();
            if let Some(program) = VALIDATED
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&key)
                .and_then(Weak::upgrade)
            {
                texture_bytes += program
                    .textures
                    .iter()
                    .map(|t| t.bytes.len())
                    .sum::<usize>();
                if texture_bytes > 32 * 1024 * 1024 {
                    return Err("material textures exceed 32 MiB decoded".into());
                }
                return Ok(program);
            }
            let mut program =
                validate_material(std::str::from_utf8(&source).map_err(|e| e.to_string())?)
                    .map_err(|e| format!("{path}: {e}"))?;
            for bytes in encoded {
                let size = imagesize::blob_size(&bytes).map_err(|e| e.to_string())?;
                if size.width == 0 || size.height == 0 || size.width > 2048 || size.height > 2048 {
                    return Err("material textures must fit 2048x2048".into());
                }
                texture_bytes += size.width * size.height * 4;
                if texture_bytes > 32 * 1024 * 1024 {
                    return Err("material textures exceed 32 MiB decoded".into());
                }
                let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
                    .with_guessed_format()
                    .map_err(|e| e.to_string())?;
                let mut limits = image::Limits::default();
                limits.max_image_width = Some(2048);
                limits.max_image_height = Some(2048);
                limits.max_alloc = Some(32 * 1024 * 1024);
                reader.limits(limits);
                let image = reader.decode().map_err(|e| e.to_string())?.to_rgba8();
                program.textures.push(ShaderTexture {
                    size: [image.width(), image.height()],
                    bytes: image.into_raw().into(),
                });
            }
            program.hash = key.clone();
            let program = Arc::new(program);
            let mut cache = VALIDATED.lock().unwrap_or_else(|e| e.into_inner());
            cache.retain(|_, p| p.strong_count() > 0);
            cache.insert(key, Arc::downgrade(&program));
            Ok(program)
        })();
        match result {
            Ok(program) => {
                programs.insert(id.clone(), program);
            }
            Err(message) => diagnostics.push(error(format!("materials.{id}"), message)),
        }
    }
    for (field, reference) in skin.material_references() {
        if !skin.materials.contains_key(reference) {
            diagnostics.push(error(
                field.into(),
                format!("unknown material `{reference}`"),
            ));
        }
    }
    skin.material_programs = programs;
}
