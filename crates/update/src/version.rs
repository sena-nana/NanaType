use semver::Version;

/// This build's version.
pub fn current() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// `v1.2.3` or `1.2.3`; `None` (and a logged warning) for anything else.
pub(crate) fn parse(version: &str) -> Option<Version> {
    let parsed = Version::parse(version.trim().trim_start_matches('v'));
    if let Err(error) = &parsed {
        nanatype_diag::report!(crate::diag::VERSION_UNPARSABLE; "{version:?}: {error}");
    }
    parsed.ok()
}

/// Whether `candidate` should be offered over `installed`. Pre-releases are
/// offered only to builds that are pre-releases themselves; unparsable
/// versions are never offered.
pub fn is_newer(candidate: &str, installed: &str) -> bool {
    let (Some(candidate), Some(installed)) = (parse(candidate), parse(installed)) else {
        return false;
    };
    if !candidate.pre.is_empty() && installed.pre.is_empty() {
        return false;
    }
    candidate > installed
}
