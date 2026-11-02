//! Rewrite every `builtin/<skin>/skin.toml` in canonical form: defaults
//! omitted, responsive layouts reduced to their differences from the base
//! dialogue, short arrays inline. Rebuild afterwards; the embedded copies are
//! the ones this binary was compiled with.
//!
//! `cargo run -p nanatype-theme --example regenerate_builtin_skins`
use nanatype_theme::{SkinRegistry, standalone_manifest};
use std::{fs, path::Path};

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("builtin");
    let registry = SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
    let mut dirs: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.join("skin.toml").is_file())
        .collect();
    dirs.sort();
    for dir in dirs {
        let package: toml::Table = fs::read_to_string(dir.join("package.toml"))
            .unwrap()
            .parse()
            .unwrap();
        let id = package["id"].as_str().unwrap();
        let loaded = registry.load(id).expect("built-in skin loads");
        let mut table = standalone_manifest(&loaded.skin).unwrap();
        table.remove("meta");
        let text = toml::to_string(&table).unwrap();
        fs::write(dir.join("skin.toml"), &text).unwrap();
        println!("{id}: {} lines", text.lines().count());
    }
}
