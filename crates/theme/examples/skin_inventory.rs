//! Capture resolved manifests and all resources for package migration/roundtrip audits.
use nanatype_theme::SkinRegistry;
use std::{fs, path::PathBuf};

fn main() {
    let root = PathBuf::from(std::env::args().nth(1).expect("output directory"));
    let registry = SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
    for entry in registry.entries() {
        let loaded = registry.load(&entry.id).expect("skin loads");
        let dir = root.join(&entry.id);
        fs::create_dir_all(&dir).unwrap();
        for path in loaded.files.list() {
            let dest = dir.join(&path);
            fs::create_dir_all(dest.parent().unwrap()).unwrap();
            fs::write(dest, loaded.files.read(&path).unwrap()).unwrap();
        }
        fs::write(
            dir.join("resolved.toml"),
            toml::to_string_pretty(&*loaded.skin).unwrap(),
        )
        .unwrap();
        let mut compact = nanatype_theme::standalone_manifest(&loaded.skin).unwrap();
        compact.remove("meta");
        fs::write(
            dir.join("compact.toml"),
            toml::to_string_pretty(&compact).unwrap(),
        )
        .unwrap();
        println!("{} {} files", entry.id, loaded.files.list().len());
    }
}
