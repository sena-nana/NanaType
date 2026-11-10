//! Where the files shipped beside the executable are: the built-in skins and
//! their fonts are folders of the package, not part of the binary.

use std::path::PathBuf;

/// The shipped folder `name`, the first of:
///
/// 1. the environment variable `env`, when set (and then nothing else);
/// 2. the packaged layout's runtime resources: `runtime/resources/<name>`
///    beside the executable, `Contents/Resources/<name>` in a macOS bundle;
/// 3. `source`, the source tree's folder, which only a development build
///    knows (see `build.rs`).
///
/// `None` when that is not a folder.
pub(crate) fn locate(env: &str, name: &str, source: Option<&str>) -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(env) {
        return Some(PathBuf::from(dir)).filter(|dir| dir.is_dir());
    }
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(bin) = exe.parent()
    {
        candidates.push(bin.join("runtime").join("resources").join(name));
        if let Some(contents) = bin.parent() {
            candidates.push(contents.join("Resources").join(name));
        }
    }
    candidates.extend(source.map(PathBuf::from));
    candidates.into_iter().find(|dir| dir.is_dir())
}
