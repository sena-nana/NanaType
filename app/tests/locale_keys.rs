//! Every message is looked up somewhere, and only by a build that ships it:
//! removed UI leaves no dead strings for translators to maintain, and neither
//! product (nor the framework both use) asks for a string only the other has.
use std::path::Path;

/// Keys built at runtime (`format!("skins-tag-{tag}")`).
const DYNAMIC_PREFIXES: &[&str] = &["skins-tag-"];

/// Settings rows look up `{key}-hint` for their label key (`hint_key`).
fn referenced(code: &str, key: &str) -> bool {
    code.contains(&format!("\"{key}\""))
        || key
            .strip_suffix("-hint")
            .is_some_and(|base| code.contains(&format!("\"{base}\"")))
}

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

fn root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn code(dirs: &[&str]) -> String {
    let mut code = String::new();
    for dir in dirs {
        sources(&root().join(dir), &mut code);
    }
    code
}

/// NanaType's own code: the app and the crates only it links.
fn app_code() -> String {
    let mut dirs = vec!["app/src".to_owned()];
    for entry in std::fs::read_dir(root().join("crates")).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !matches!(name.as_str(), "frame" | "i18n") {
            dirs.push(format!("crates/{name}"));
        }
    }
    code(&dirs.iter().map(String::as_str).collect::<Vec<_>>())
}

/// The keys of one locale file (`crates/i18n/locales/<sub>en-US.ftl`).
fn keys(sub: &str) -> Vec<String> {
    let ftl = std::fs::read_to_string(root().join(format!("crates/i18n/locales/{sub}en-US.ftl")))
        .unwrap();
    ftl.lines()
        .filter_map(|line| line.split_once(" = ").map(|(k, _)| k.trim()))
        .filter(|k| !k.is_empty() && !k.starts_with('#') && !k.starts_with(' '))
        .map(str::to_owned)
        .collect()
}

fn unused(keys: &[String], code: &str) -> Vec<String> {
    keys.iter()
        .filter(|k| !DYNAMIC_PREFIXES.iter().any(|p| k.starts_with(p)))
        .filter(|k| !referenced(code, k))
        .cloned()
        .collect()
}

fn borrowed(keys: &[String], code: &str) -> Vec<String> {
    keys.iter()
        .filter(|k| code.contains(&format!("\"{k}\"")))
        .cloned()
        .collect()
}

#[test]
fn every_locale_key_is_referenced_by_a_build_that_ships_it() {
    let (frame, app, editor) = (
        code(&["crates/frame/src"]),
        app_code(),
        code(&["editor/src"]),
    );
    // The string tests may name any key; they ship no UI.
    let strings = code(&["crates/i18n/src"]);
    let shared = keys("");
    let all = format!("{frame}{app}{editor}");
    assert_eq!(
        unused(&shared, &all),
        Vec::<String>::new(),
        "unused shared keys"
    );
    assert_eq!(
        unused(&keys("app/"), &format!("{app}{strings}")),
        Vec::<String>::new(),
        "unused app keys"
    );
    assert_eq!(
        unused(&keys("editor/"), &editor),
        Vec::<String>::new(),
        "unused editor keys"
    );
    // A key the other build ships would show as its raw id here.
    let only_app = keys("app/");
    let only_editor = keys("editor/");
    assert_eq!(
        borrowed(&only_app, &format!("{frame}{editor}")),
        Vec::<String>::new(),
        "the framework or the editor uses NanaType-only keys"
    );
    assert_eq!(
        borrowed(&only_editor, &format!("{frame}{app}")),
        Vec::<String>::new(),
        "the framework or NanaType uses editor-only keys"
    );
}
