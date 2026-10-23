//! Validate a skin package folder or `.ntpkg` the way NanaType loads it, and
//! report each shader's size against the 4096-node limits.
//!
//! `cargo run -p nanatype-theme --example check_package -- <package-dir|file.ntpkg>`

use std::path::PathBuf;
use std::sync::Arc;

use nanatype_theme::{
    DirFiles, MAX_EXPRESSION_DEPTH, Severity, ShaderSpec, SkinFiles, SkinOrigin, ZipFiles, load,
    read_manifest, shader_cost,
};

fn main() {
    let Some(arg) = std::env::args().nth(1) else {
        eprintln!("usage: check_package <package-dir|file.ntpkg>");
        std::process::exit(2);
    };
    let path = PathBuf::from(&arg);
    let (files, origin): (Arc<dyn SkinFiles>, _) = if path.is_file() {
        match ZipFiles::open(&path) {
            Ok(zip) => (Arc::new(zip), SkinOrigin::Archive(path)),
            Err(e) => {
                eprintln!("{arg}: {e}");
                std::process::exit(1);
            }
        }
    } else {
        (Arc::new(DirFiles::new(&path)), SkinOrigin::Directory(path))
    };
    let diagnostics = match load(files.clone(), origin, &(), env!("CARGO_PKG_VERSION")) {
        Ok(loaded) => {
            let s = &loaded.skin;
            println!(
                "{} params, {} presets, {} placeholders",
                s.params.len(),
                s.presets.len(),
                s.param_sites.len()
            );
            loaded.warnings
        }
        Err(d) => d,
    };
    for d in &diagnostics {
        let tag = if d.severity == Severity::Error {
            "error"
        } else {
            "warning"
        };
        println!("{tag}: {d}");
    }
    // Sized from the manifest so an effect over the limits still reports.
    let shaders = read_manifest(&*files)
        .ok()
        .and_then(|m| m.get("shaders")?.as_table().cloned())
        .unwrap_or_default();
    for (id, spec) in shaders {
        let Ok(spec) = spec.try_into::<ShaderSpec>() else {
            continue;
        };
        let Ok(bytes) = files.read(&spec.source) else {
            continue;
        };
        match shader_cost(&String::from_utf8_lossy(&bytes)) {
            Ok((ir, expanded, depth)) => println!(
                "shader `{id}`: {ir} IR nodes, {expanded} expanded (limit 4096 each), \
                 expression depth {depth} (limit {MAX_EXPRESSION_DEPTH}), {} slots",
                spec.bindings.len()
            ),
            Err(e) => println!("shader `{id}`: {e}"),
        }
    }
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        std::process::exit(1);
    }
}
