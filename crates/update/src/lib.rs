//! Update checks against GitHub Releases.
//!
//! Each release carries `latest.json`, a manifest listing every asset with
//! its size and SHA-256, and `latest.json.sig`, an Ed25519 signature over the
//! manifest bytes. The public key is compiled in, so the manifest and the
//! files can travel through any mirror: a mirror that alters either is caught
//! by the signature or the digest, and the next source is tried.
pub mod diag;
mod error;
mod fetch;
mod manifest;
mod mirror;
mod sign;
mod version;

pub use error::Error;
pub use fetch::{Checked, Client, Progress};
pub use manifest::{Asset, Kind, MANIFEST_SCHEMA, Manifest, Target};
pub use mirror::{BUILTIN, Source};
pub use sign::{PUBLIC_KEY, hex, sign, unhex, verify};
pub use version::{current, is_newer};

/// The repository releases are published from.
pub const REPO: &str = "sena-nana/NanaType";
/// The manifest's asset name; its signature is this plus [`SIG_SUFFIX`].
pub const MANIFEST_NAME: &str = "latest.json";
pub const SIG_SUFFIX: &str = ".sig";

#[cfg(test)]
mod tests;
