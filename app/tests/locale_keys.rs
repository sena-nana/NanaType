//! Every message in the source locale is looked up somewhere: removed UI
//! leaves no dead strings for translators to maintain.
use std::path::Path;

/// Keys built at runtime (`format!("skins-tag-{tag}")`).
const DYNAMIC_PREFIXES: &[&str] = &["skins-tag-"];

fn sources(dir: &Path, out: &mut String) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push_str(&std::fs::read_to_string(&path).unwrap());
        }
    }
}

#[test]
fn every_source_locale_key_is_referenced() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut code = String::new();
    for dir in ["app/src", "crates"] {
        sources(&root.join(dir), &mut code);
    }
    let ftl = std::fs::read_to_string(root.join("crates/i18n/locales/en-US.ftl")).unwrap();
    let unused: Vec<_> = ftl
        .lines()
        .filter_map(|line| line.split_once(" = ").map(|(k, _)| k.trim()))
        .filter(|k| !k.is_empty() && !k.starts_with('#') && !k.starts_with(' '))
        .filter(|k| !DYNAMIC_PREFIXES.iter().any(|p| k.starts_with(p)))
        .filter(|k| !code.contains(&format!("\"{k}\"")))
        .collect();
    assert!(unused.is_empty(), "unused locale keys: {unused:?}");
}
