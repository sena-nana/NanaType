//! Static skin materials. The framework owns their geometry and GPU resources.
use super::Ui;
use nana_ui_core::{MaterialInstance, MaterialProgram, MaterialRef, MaterialTexture};
use nanatype_theme::Skin;
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex, Weak},
};

static INSTANCES: LazyLock<Mutex<HashMap<blake3::Hash, Weak<MaterialInstance>>>> =
    LazyLock::new(Default::default);
pub fn enabled(ui: Ui) -> bool {
    ui.settings.with(|s| s.skin.advanced_effects)
}
/// Preserve the manifest and fallback artwork while disabling every material
/// slot together, including panels, nameplates and decorations.
pub fn render_skin(ui: Ui, skin: Arc<Skin>) -> Arc<Skin> {
    if enabled(ui) || skin.material_programs.is_empty() {
        return skin;
    }
    let mut fallback = (*skin).clone();
    fallback.material_programs.clear();
    Arc::new(fallback)
}
pub fn resolve(skin: &Skin, name: Option<&str>) -> Option<MaterialRef> {
    let name = name?;
    let spec = skin.materials.get(name)?;
    let program = skin.material_programs.get(name)?;
    let mut hash = blake3::Hasher::new();
    hash.update(program.hash.as_bytes());
    for parameter in &spec.params {
        hash.update(&parameter.to_le_bytes());
    }
    let key = hash.finalize();
    let mut instances = INSTANCES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(material) = instances.get(&key).and_then(MaterialRef::upgrade) {
        return Some(material);
    }
    let mut params = [[0.; 4]; 2];
    for (index, value) in spec.params.iter().enumerate() {
        params[index / 4][index % 4] = *value;
    }
    let textures: Arc<[MaterialTexture]> = program
        .textures
        .iter()
        .map(|t| MaterialTexture {
            size: t.size,
            rgba: t.bytes.clone(),
        })
        .collect();
    let material = MaterialRef::new(MaterialProgram::new(program.source.clone()))
        .with_parameters(Arc::from(params))
        .with_textures(textures);
    instances.retain(|_, m| m.strong_count() > 0);
    instances.insert(key, material.downgrade());
    Some(material)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn static_materials_share_instances_and_survive_reduced_motion() {
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        let skin = registry.load("nanatype.crayon").unwrap().skin;
        let ui = Ui::new(
            Arc::new(Default::default()),
            skin.clone(),
            nanatype_i18n::Locale::ZhCn,
        );
        let a = resolve(&skin, Some("pencil")).unwrap();
        let b = resolve(&skin, Some("pencil")).unwrap();
        assert_eq!(a.key(), b.key());
        ui.display.update(|d| Arc::make_mut(d).empty = false);
        assert!(!super::super::shader_effects::animating(ui));
        ui.reduced_motion.set(true);
        assert!(enabled(ui));
        assert!(render_skin(ui, skin.clone()).material_programs.len() > 0);
        assert!(
            super::super::dialogue::Base::editor(&skin)
                .material
                .is_none()
        );
        ui.settings
            .update(|s| Arc::make_mut(s).skin.advanced_effects = false);
        assert!(!enabled(ui));
        assert!(render_skin(ui, skin).material_programs.is_empty());
    }
}
