use semver::Version;

/// This build's version.
pub fn current() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Whether `candidate` should be offered over `installed`. Pre-releases are
/// offered only to builds that are pre-releases themselves; unparsable
/// versions are never offered.
pub fn is_newer(candidate: &str, installed: &str) -> bool {
    let parse = |v: &str| Version::parse(v.trim().trim_start_matches('v')).ok();
    let (Some(candidate), Some(installed)) = (parse(candidate), parse(installed)) else {
        return false;
    };
    if !candidate.pre.is_empty() && installed.pre.is_empty() {
        return false;
    }
    candidate > installed
}
