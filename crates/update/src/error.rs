use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// No source answered with a manifest that passed verification.
    Unreachable(String),
    /// A manifest arrived but its signature or contents were rejected.
    BadManifest(String),
    /// No published release carries a manifest yet.
    NoRelease,
    /// The release has no file for this platform and install kind.
    NoAsset,
    /// Every source failed to deliver the file with the published digest.
    Download(String),
    Io(String),
    Cancelled,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreachable(why) => write!(f, "no update source reachable: {why}"),
            Self::BadManifest(why) => write!(f, "update manifest rejected: {why}"),
            Self::NoRelease => f.write_str("no release published"),
            Self::NoAsset => f.write_str("no download for this platform"),
            Self::Download(why) => write!(f, "download failed: {why}"),
            Self::Io(why) => write!(f, "{why}"),
            Self::Cancelled => f.write_str("cancelled"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}
