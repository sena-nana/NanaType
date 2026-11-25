//! What this build ships.

/// The GitHub Releases update check.
pub const UPDATES: bool = cfg!(feature = "updates");

pub const LICENSE_LABEL: &str = "NanaType · MIT";
pub const LICENSE_TEXT: &str = concat!(
    include_str!("../../LICENSE"),
    "\n",
    include_str!("../../LICENSE-MIT")
);
