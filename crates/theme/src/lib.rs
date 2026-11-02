//! Skin packages.
//!
//! A package has a `package.toml`, optional `skin.toml`, and explicitly owned
//! resources. Skins style dialogue, history and input content; operator controls
//! keep the independent application palette. Inheritance merges configuration
//! while retaining each resource's package owner. The renderer interprets the
//! same generic schema for every skin and never selects behavior by skin ID.

#![forbid(unsafe_code)]

mod contour;
mod data;
pub use data::{
    DataAccess, DataDisclosure, DataFormat, DataQuery, DataSpec, DataValue, JsonConstraints,
    JsonType, LoadedData, MAX_QUERY_RESULT_BYTES, TextData, load_data, parse_data, resolve_data,
    split_reference,
};
#[cfg(feature = "editor")]
pub mod editor;
mod files;
mod fonts;
mod geometry;
mod handwriting;
pub use handwriting::{StrokeCommand, StrokeData, StrokeGlyph};
mod load;
mod manifest;
mod material;
mod pipeline;
pub use pipeline::{
    BoundInput, BoundNode, MAX_PIPELINE_TEXTURE_SIDE, MaterialBinding, PROCESSORS, ParamSig,
    PipelineOutput, PipelineSpec, PortSource, PortType, ProcessorSignature, TextureSource,
    processor,
};
mod package;
mod params;
pub use params::{
    EnumOption, MAX_PARAMS, ParamCheck, ParamError, ParamGroup, ParamGroupListing, ParamSite,
    ParamSpec, ParamType, PresetSpec, Resolved, coerce_values, resolve as resolve_params,
    retain_valid as retain_valid_params, text_len,
};
mod project;
pub use package::{
    LuaSpec, PACKAGE_FORMAT, PACKAGE_MANIFEST, PackageCapability, PackageManifest,
    clean_resource_path,
};
pub use project::{
    INTEGRITY_FILE, PackageProject, PackageSeal, SealIdentity, SealStatus, verify_integrity,
};
mod registry;
mod responsive;
pub use material::*;
pub use responsive::*;
mod shader;
pub use shader::*;
mod sound_pack;
pub use sound_pack::{
    LoadedSoundPack, SOUNDS_MANIFEST, SoundPackEntry, layer as layer_sounds, read_sounds,
    supplies_typing,
};

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
pub use registry::{
    DEFAULT_SKIN, ExternalProblem, ExternalRoot, ExternalStatus, InstallOutcome, SkinEntry,
    SkinRegistry,
};
pub use toml::Table as TomlTable;
