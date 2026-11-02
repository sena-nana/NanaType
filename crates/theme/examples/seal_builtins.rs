//! Seal every `builtin/<package>/` with the built-in key: each file the
//! build embeds is protected, and `package.integrity.json` is rewritten
//! beside it. Run it after any change to a built-in package (including
//! `regenerate_builtin_skins`); the theme tests fail on a stale seal.
//!
//! `NANATYPE_PACKAGE_KEY=<secret hex> cargo run -p nanatype-theme --example seal_builtins`
//!
//! The secret must match `BUILTIN_SEAL_KEY`. Keep it offline. Ed25519 seals
//! are deterministic, so unchanged packages reseal to identical bytes.
use nanatype_theme::{
    BUILTIN_SEAL_KEY, INTEGRITY_FILE, PackageProject, SealIdentity, SkinRegistry,
};
use std::{collections::BTreeSet, fs, path::Path};

fn main() {
    let secret = std::env::var("NANATYPE_PACKAGE_KEY").unwrap_or_else(|_| {
        eprintln!("set NANATYPE_PACKAGE_KEY to the built-in seal secret (64 hex digits)");
        std::process::exit(2);
    });
    let secret = secret.trim();
    let mut bytes = [0u8; 32];
    if secret.len() != 64 || !secret.is_ascii() {
        eprintln!("NANATYPE_PACKAGE_KEY must be 64 hex digits");
        std::process::exit(2);
    }
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&secret[i * 2..i * 2 + 2], 16).unwrap_or_else(|_| {
            eprintln!("NANATYPE_PACKAGE_KEY must be 64 hex digits");
            std::process::exit(2);
        });
    }
    let identity = SealIdentity::from_bytes(bytes);
    if identity.public_key() != BUILTIN_SEAL_KEY {
        eprintln!(
            "this secret's public key {} is not BUILTIN_SEAL_KEY",
            identity.public_key()
        );
        std::process::exit(1);
    }

    // The embedded copies are exactly the files the build ships (dotfiles and
    // SVG masters stay out); this binary was built from the current tree.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("builtin");
    let registry = SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
    let mut dirs: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.join("package.toml").is_file())
        .collect();
    dirs.sort();
    for dir in dirs {
        let package: toml::Table = fs::read_to_string(dir.join("package.toml"))
            .unwrap()
            .parse()
            .unwrap();
        let id = package["id"].as_str().unwrap();
        let files = registry.package_files(id).expect("built-in package");
        let mut project = PackageProject::read_all(&*files).unwrap();
        project.files.remove(INTEGRITY_FILE);
        let protected: BTreeSet<String> = project.files.keys().cloned().collect();
        let status = project.seal(&identity, &protected).unwrap();
        let seal = &project.files[INTEGRITY_FILE];
        let target = dir.join(INTEGRITY_FILE);
        let changed = fs::read(&target).ok().as_ref() != Some(seal);
        if changed {
            fs::write(&target, seal).unwrap();
        }
        println!(
            "{id}: {} files{}",
            status.protected.len(),
            if changed { ", resealed" } else { "" }
        );
    }
}
