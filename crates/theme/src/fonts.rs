//! Fonts shipped with the app for the built-in skins.
//!
//! A skin folder ships its own font files (`file = "fonts/X.ttf"`); the
//! built-in skins share a few, so they name a shipped font instead:
//! `file = "builtin:fonts/X.ttf"`. Those live in a folder beside the
//! executable ([`builtin_fonts_dir`]; `crates/theme/fonts/` in the source
//! tree), each font with its licence, and are read once, with the
//! `builtin-fonts` feature. A reference to a font this build does not carry
//! is not an error: the role falls back to its `families`, exactly as if the
//! skin had named no file.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::files::clean_path;
use crate::manifest::FontSpec;

/// What a skin writes before a built-in font's path.
pub const BUILTIN_FONT_PREFIX: &str = "builtin:";

/// One shipped font and the licence that ships with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinFont {
    /// `fonts/<name>.ttf`.
    pub path: &'static str,
    pub bytes: &'static [u8],
    pub license: &'static str,
}

/// A table of built-in fonts; [`BuiltinFonts::shipped`] is this process's.
#[derive(Debug, Clone, Copy)]
pub struct BuiltinFonts {
    fonts: &'static [BuiltinFont],
    known: &'static [&'static str],
}

/// The folder the built-in fonts are read from: `NANATYPE_BUILTIN_FONTS`
/// when set, else `runtime/resources/fonts` beside the executable
/// (`Contents/Resources/fonts` in a macOS bundle), else, in a development
/// build, `crates/theme/fonts` of the source tree. `None` when that is not a
/// folder; every `builtin:` role falls back to its families then.
pub fn builtin_fonts_dir() -> Option<&'static Path> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        crate::shipped::locate(
            "NANATYPE_BUILTIN_FONTS",
            "fonts",
            option_env!("NANATYPE_SOURCE_FONTS"),
        )
    })
    .as_deref()
}

/// The shipped folder's fonts, read once and kept for the process. A font
/// without its `<name>.LICENSE.txt` beside it is known but not used: the OFL
/// requires the licence to travel with the font.
fn read_shipped() -> (Vec<BuiltinFont>, Vec<&'static str>) {
    let mut fonts = Vec::new();
    let mut known = Vec::new();
    let Some(dir) = builtin_fonts_dir() else {
        return (fonts, known);
    };
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| {
            let lower = name.to_ascii_lowercase();
            lower.ends_with(".ttf") || lower.ends_with(".otf")
        })
        .collect();
    names.sort();
    for name in names {
        let path: &'static str = format!("fonts/{name}").leak();
        known.push(path);
        if !cfg!(feature = "builtin-fonts") {
            continue;
        }
        let license = dir.join(format!("{}.LICENSE.txt", &name[..name.len() - 4]));
        if let (Ok(bytes), Ok(license)) = (
            std::fs::read(dir.join(&name)),
            std::fs::read_to_string(license),
        ) {
            fonts.push(BuiltinFont {
                path,
                bytes: bytes.leak(),
                license: license.leak(),
            });
        }
    }
    (fonts, known)
}

impl BuiltinFonts {
    /// A table of `fonts`; `known` are the paths a reference may name even
    /// when their bytes are not carried.
    pub const fn new(fonts: &'static [BuiltinFont], known: &'static [&'static str]) -> Self {
        Self { fonts, known }
    }

    /// The fonts shipped with this build: empty without the `builtin-fonts`
    /// feature or when the fonts folder is missing or has no fonts.
    pub fn shipped() -> Self {
        static SHIPPED: OnceLock<(Vec<BuiltinFont>, Vec<&'static str>)> = OnceLock::new();
        let (fonts, known) = SHIPPED.get_or_init(read_shipped);
        Self::new(fonts, known)
    }

    pub fn fonts(&self) -> &'static [BuiltinFont] {
        self.fonts
    }

    /// Every font path in the fonts folder, carried or not.
    pub fn known(&self) -> &'static [&'static str] {
        self.known
    }

    /// Bytes of font data the table holds.
    pub fn total_bytes(&self) -> usize {
        self.fonts.iter().map(|f| f.bytes.len()).sum()
    }

    /// The font a `builtin:` reference names, when this table carries it.
    pub fn resolve(&self, reference: &str) -> Option<&'static BuiltinFont> {
        let path = builtin_path(reference)?;
        self.fonts.iter().find(|f| f.path == path)
    }

    /// Whether `reference` names a font in the fonts folder, carried or
    /// not. An unknown name is most likely a typo.
    pub fn knows(&self, reference: &str) -> bool {
        builtin_path(reference).is_some_and(|p| {
            self.known.contains(&p.as_str()) || self.fonts.iter().any(|f| f.path == p)
        })
    }
}

/// Whether a manifest path names a built-in font rather than a skin file.
pub fn is_builtin_font(reference: &str) -> bool {
    reference.starts_with(BUILTIN_FONT_PREFIX)
}

/// The cleaned path (`fonts/<name>.ttf`) of a `builtin:` reference.
pub fn builtin_path(reference: &str) -> Option<String> {
    clean_path(reference.strip_prefix(BUILTIN_FONT_PREFIX)?).ok()
}

/// The shipped bytes for a `builtin:` reference; `None` when the reference
/// is not one or the font is not shipped, and the role falls back.
pub fn builtin_font(reference: &str) -> Option<&'static [u8]> {
    BuiltinFonts::shipped().resolve(reference).map(|f| f.bytes)
}

/// The family a font file registers under: its typographic family name
/// (name ID 16) when it has one, else its family name (ID 1); the English
/// record before localized ones (a CJK face also names itself in Chinese).
pub fn font_family(bytes: &[u8]) -> Option<String> {
    font_name(bytes, ttf_parser::name_id::TYPOGRAPHIC_FAMILY)
        .or_else(|| font_name(bytes, ttf_parser::name_id::FAMILY))
}

/// One record of a font file's name table (`name_id`, see
/// `ttf_parser::name_id`), the English record before localized ones.
pub fn font_name(bytes: &[u8], name_id: u16) -> Option<String> {
    let face = ttf_parser::Face::parse(bytes, 0).ok()?;
    let mut found: Vec<_> = face
        .names()
        .into_iter()
        .filter(|n| n.name_id == name_id && n.is_unicode())
        .collect();
    found.sort_by_key(|n| n.language() != ttf_parser::Language::English_UnitedStates);
    found.into_iter().find_map(|n| n.to_string())
}

/// What the OFL 1.1 text says of itself, in every upstream `OFL.txt`.
const OFL_1_1_MARK: &str = "SIL OPEN FONT LICENSE Version 1.1";

impl BuiltinFont {
    /// The family the font registers under ([`font_family`]).
    pub fn family(&self) -> Option<String> {
        font_family(self.bytes)
    }

    /// Who made it: the designer (name ID 9), else the manufacturer (8).
    pub fn author(&self) -> Option<String> {
        font_name(self.bytes, ttf_parser::name_id::DESIGNER)
            .or_else(|| font_name(self.bytes, ttf_parser::name_id::MANUFACTURER))
    }

    /// The version string (name ID 5).
    pub fn version(&self) -> Option<String> {
        font_name(self.bytes, ttf_parser::name_id::VERSION)
    }

    /// The licence's name, when its text is one this crate recognises.
    pub fn license_name(&self) -> Option<&'static str> {
        self.license
            .contains(OFL_1_1_MARK)
            .then_some("SIL Open Font License 1.1")
    }
}

impl FontSpec {
    /// Whether the role brings a font file that will actually be used: a
    /// skin file (validated on load), or a built-in font this build ships.
    pub fn has_usable_file(&self) -> bool {
        match self.file.as_deref() {
            None => false,
            Some(f) if is_builtin_font(f) => builtin_font(f).is_some(),
            Some(_) => true,
        }
    }
}
