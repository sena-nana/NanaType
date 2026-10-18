//! Skin packages.
//!
//! A skin is a folder (or a `.ntskin` zip of one) with a `skin.toml` and the
//! images, fonts and sounds it names. It styles the dialogue box, the name
//! plate, the portrait, the history bubbles, typing sounds and the app's own
//! controls. Skins inherit from a `base` by deep-merging TOML tables, so a
//! user skin only states what it changes. Only the backend-neutral NanaUI material contract is shared; the
//! app maps a [`Skin`] onto its widgets.

#![forbid(unsafe_code)]

mod contour;
mod files;
mod fonts;
mod geometry;
mod load;
mod manifest;
mod material;
mod registry;
pub use material::*;
mod shader;
pub use shader::*;

#[cfg(test)]
mod tests;

pub use contour::*;
pub use files::{
    DirFiles, FileError, MAX_FILE_BYTES, MAX_PACKAGE_BYTES, MemoryFiles, SkinFiles, ZipFiles,
    clean_path,
};
pub use fonts::{
    BUILTIN_FONT_PREFIX, BuiltinFont, BuiltinFonts, builtin_font, builtin_path, font_family,
    font_name, is_builtin_font,
};
pub use geometry::*;
pub use load::{
    BaseResolver, LoadedSkin, MANIFEST, MAX_DECOR, MAX_IMAGE_SIDE, MAX_SHADOWS, MAX_STOPS,
    SKIN_TAGS, Severity, SkinDiagnostic, SkinOrigin, deep_merge, load, read_manifest,
    standalone_manifest,
};
pub use manifest::*;
pub use registry::{DEFAULT_SKIN, InstallOutcome, SkinEntry, SkinRegistry};
pub use toml::Table as TomlTable;
