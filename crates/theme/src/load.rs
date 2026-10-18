//! Parse, inherit and validate a skin. Validation fails closed: a pack with an
//! error is refused as a whole, the way NanaUI refuses a theme that does not
//! compile. Unknown fields only warn so newer packs still load.

use std::path::PathBuf;
use std::sync::{Arc, LazyLock};

use toml::{Table, Value};

use crate::files::{FileError, SkinFiles, clean_path};
use crate::fonts::{BuiltinFonts, builtin_path, is_builtin_font};
use crate::manifest::{
    DecorMotionKind, DecorSpec, FileKind, GradientSpec, Len, SKIN_SCHEMA, ShadowSpec, ShapeSpec,
    Skin,
};

pub const MANIFEST: &str = "skin.toml";
pub const MAX_IMAGE_SIDE: usize = 8192;
const MAX_BASE_DEPTH: usize = 8;

/// [`Skin::default`] as a TOML table, the bottom layer every skin merges onto.
static DEFAULT_TABLE: LazyLock<Table> = LazyLock::new(|| match Value::try_from(Skin::default()) {
    Ok(Value::Table(t)) => t,
    other => panic!("the default skin serializes to a table: {other:?}"),
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkinDiagnostic {
    pub severity: Severity,
    /// Dotted manifest path (`dialogue.box.frame`) or a file path.
    pub field: String,
    pub message: String,
}

impl SkinDiagnostic {
    fn error(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            field: field.into(),
            message: message.into(),
        }
    }

    fn warning(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            field: field.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for SkinDiagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkinOrigin {
    Builtin,
    /// An installed or developer folder.
    Directory(PathBuf),
    /// Previewed straight from a `.ntskin` file.
    Archive(PathBuf),
}

/// Files of a skin layered over the files of its bases.
#[derive(Debug)]
pub struct LayeredFiles {
    layers: Vec<Arc<dyn SkinFiles>>,
}

impl SkinFiles for LayeredFiles {
    fn read(&self, path: &str) -> Result<Vec<u8>, FileError> {
        let mut last = FileError::Missing(path.to_owned());
        for layer in &self.layers {
            match layer.read(path) {
                Ok(bytes) => return Ok(bytes),
                Err(FileError::Missing(p)) => last = FileError::Missing(p),
                Err(e) => return Err(e),
            }
        }
        Err(last)
    }

    fn root(&self) -> Option<&std::path::Path> {
        self.layers.first().and_then(|l| l.root())
    }

    fn watch_roots(&self) -> Vec<PathBuf> {
        let mut roots: Vec<_> = self.layers.iter().flat_map(|l| l.watch_roots()).collect();
        roots.sort();
        roots.dedup();
        roots
    }

    fn list(&self) -> Vec<String> {
        let mut all: Vec<String> = self.layers.iter().flat_map(|l| l.list()).collect();
        all.sort();
        all.dedup();
        all
    }
}

#[derive(Debug, Clone)]
pub struct LoadedSkin {
    pub skin: Arc<Skin>,
    pub files: Arc<dyn SkinFiles>,
    pub origin: SkinOrigin,
    pub warnings: Vec<SkinDiagnostic>,
}

/// Look up a base skin's raw manifest table and files by id.
pub trait BaseResolver {
    fn base(&self, id: &str) -> Option<(Table, Arc<dyn SkinFiles>)>;
}

impl BaseResolver for () {
    fn base(&self, _: &str) -> Option<(Table, Arc<dyn SkinFiles>)> {
        None
    }
}

pub fn read_manifest(files: &dyn SkinFiles) -> Result<Table, SkinDiagnostic> {
    let bytes = files
        .read(MANIFEST)
        .map_err(|e| SkinDiagnostic::error(MANIFEST, e.to_string()))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| SkinDiagnostic::error(MANIFEST, "skin.toml is not UTF-8"))?;
    text.parse::<Table>()
        .map_err(|e| SkinDiagnostic::error(MANIFEST, e.to_string().trim().to_owned()))
}

/// `over` wins; tables merge recursively, everything else replaces.
pub fn deep_merge(base: &mut Table, over: &Table) {
    for (key, value) in over {
        match (base.get_mut(key), value) {
            (Some(Value::Table(b)), Value::Table(o)) => deep_merge(b, o),
            _ => {
                base.insert(key.clone(), value.clone());
            }
        }
    }
}

/// A self-contained manifest with contextual defaults omitted. Only use this
/// on a resolved skin: removing defaults from a partial child would change
/// inheritance (in particular explicit empty arrays).
pub fn standalone_manifest(skin: &Skin) -> Result<Table, toml::ser::Error> {
    fn prune(table: &mut Table, defaults: &Table) {
        table.retain(|key, value| {
            let Some(default) = defaults.get(key) else {
                return true;
            };
            if value == default {
                return false;
            }
            if let (Value::Table(t), Value::Table(d)) = (value, default) {
                prune(t, d);
                return !t.is_empty();
            }
            true
        });
    }
    let mut resolved = skin.clone();
    resolved.meta.base = None;
    let Value::Table(mut table) = Value::try_from(resolved)? else {
        unreachable!()
    };
    prune(&mut table, &DEFAULT_TABLE);
    Ok(table)
}

fn base_id(table: &Table) -> Option<String> {
    table
        .get("meta")
        .and_then(Value::as_table)
        .and_then(|m| m.get("base"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// Report keys present in `raw` but absent from the canonical `known` form.
fn unknown_keys(raw: &Table, known: &Table, prefix: &str, out: &mut Vec<SkinDiagnostic>) {
    for (key, value) in raw {
        let field = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match known.get(key) {
            None => out.push(SkinDiagnostic::warning(field, "unknown field is ignored")),
            Some(k) => unknown_in_value(value, k, &field, out),
        }
    }
}

fn unknown_in_value(raw: &Value, known: &Value, field: &str, out: &mut Vec<SkinDiagnostic>) {
    match (raw, known) {
        (Value::Table(r), Value::Table(k)) => unknown_keys(r, k, field, out),
        // Arrays of tables (`[[dialogue.decor]]`) pair up item by item.
        (Value::Array(r), Value::Array(k)) => {
            for (i, (r, k)) in r.iter().zip(k).enumerate() {
                unknown_in_value(r, k, &format!("{field}.{i}"), out);
            }
        }
        _ => {}
    }
}

fn version_tuple(v: &str) -> Option<(u32, u32, u32)> {
    let mut parts = v.trim().split(['.', '-', '+']);
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

/// Load and validate. `app_version` is this build's version (`CARGO_PKG_VERSION`).
pub fn load(
    files: Arc<dyn SkinFiles>,
    origin: SkinOrigin,
    resolver: &dyn BaseResolver,
    app_version: &str,
) -> Result<LoadedSkin, Vec<SkinDiagnostic>> {
    load_impl(files, origin, resolver, app_version, true)
}

/// Only immutable, embedded catalogue entries may defer GPU preparation.
/// User folders and archives always go through the complete validator above.
pub(crate) fn builtin_preview(
    files: Arc<dyn SkinFiles>,
    resolver: &dyn BaseResolver,
    app_version: &str,
) -> Result<LoadedSkin, Vec<SkinDiagnostic>> {
    load_impl(files, SkinOrigin::Builtin, resolver, app_version, false)
}

fn load_impl(
    files: Arc<dyn SkinFiles>,
    origin: SkinOrigin,
    resolver: &dyn BaseResolver,
    app_version: &str,
    prepare_shaders: bool,
) -> Result<LoadedSkin, Vec<SkinDiagnostic>> {
    let raw = read_manifest(&*files).map_err(|d| vec![d])?;
    let mut diagnostics = Vec::new();

    // Inheritance: collect the base chain, then merge from the root down.
    let mut chain = vec![raw.clone()];
    let mut layers = vec![files.clone()];
    let mut next = base_id(&raw);
    let mut seen = Vec::new();
    while let Some(id) = next.take() {
        if seen.contains(&id) || seen.len() >= MAX_BASE_DEPTH {
            return Err(vec![SkinDiagnostic::error(
                "meta.base",
                format!("base chain loops or is deeper than {MAX_BASE_DEPTH}: {id}"),
            )]);
        }
        seen.push(id.clone());
        match resolver.base(&id) {
            Some((table, base_files)) => {
                next = base_id(&table);
                chain.push(table);
                layers.push(base_files);
            }
            None => {
                return Err(vec![SkinDiagnostic::error(
                    "meta.base",
                    format!("base skin `{id}` is not installed"),
                )]);
            }
        }
    }
    // Start from the full default skin so a partial nested table (say
    // `[history.text]` with only a colour) keeps that field's own defaults
    // rather than the generic ones of its type.
    let mut merged = DEFAULT_TABLE.clone();
    for table in chain.iter().rev() {
        deep_merge(&mut merged, table);
    }
    // The pack's identity is its own, never inherited.
    if let Some(Value::Table(meta)) = merged.get_mut("meta") {
        let own = raw.get("meta").and_then(Value::as_table);
        for key in [
            "id",
            "version",
            "name",
            "description",
            "author",
            "license",
            "preview",
        ] {
            match own.and_then(|m| m.get(key)) {
                Some(v) => {
                    meta.insert(key.into(), v.clone());
                }
                None => {
                    meta.remove(key);
                }
            }
        }
    }

    let mut skin: Skin = match Value::Table(merged).try_into() {
        Ok(skin) => skin,
        Err(e) => {
            return Err(vec![SkinDiagnostic::error(
                MANIFEST,
                e.to_string().trim().to_owned(),
            )]);
        }
    };
    // Older descendants often replace a base's geometry and artwork. Do not
    // silently impose that base's new orientation/layout contract on them.
    // An explicit (even empty) responsive table opts into inherited layouts.
    if base_id(&raw).is_some()
        && raw
            .get("dialogue")
            .and_then(Value::as_table)
            .and_then(|d| d.get("responsive"))
            .is_none()
    {
        skin.dialogue.responsive = None;
    }
    if let Ok(Value::Table(known)) = Value::try_from(&skin) {
        unknown_keys(&raw, &known, "", &mut diagnostics);
    }

    let files: Arc<dyn SkinFiles> = if layers.len() == 1 {
        files
    } else {
        Arc::new(LayeredFiles { layers })
    };
    validate(&skin, &*files, app_version, &mut diagnostics);
    if prepare_shaders {
        crate::handwriting::load_handwriting(&mut skin, &*files, &mut diagnostics, true);
        crate::shader::load_shaders(&mut skin, &*files, &mut diagnostics);
        crate::material::load_materials(&mut skin, &*files, &mut diagnostics);
    }
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        diagnostics.sort_by_key(|d| std::cmp::Reverse(d.severity));
        return Err(diagnostics);
    }
    Ok(LoadedSkin {
        skin: Arc::new(skin),
        files,
        origin,
        warnings: diagnostics,
    })
}

fn validate(skin: &Skin, files: &dyn SkinFiles, app_version: &str, out: &mut Vec<SkinDiagnostic>) {
    let meta = &skin.meta;
    if meta.schema > SKIN_SCHEMA {
        out.push(SkinDiagnostic::error(
            "meta.schema",
            format!(
                "schema {} is newer than this app ({SKIN_SCHEMA})",
                meta.schema
            ),
        ));
    }
    let id_ok = !meta.id.is_empty()
        && meta.id.len() <= 64
        && meta
            .id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_'));
    if !id_ok {
        out.push(SkinDiagnostic::error(
            "meta.id",
            "id must be 1–64 characters of a-z, 0-9, `.`, `-` or `_`",
        ));
    }
    match (version_tuple(&meta.min_app), version_tuple(app_version)) {
        (Some(need), Some(have)) if need > have => out.push(SkinDiagnostic::error(
            "meta.min_app",
            format!("needs NanaType {} or newer", meta.min_app),
        )),
        (None, _) => out.push(SkinDiagnostic::error("meta.min_app", "not a version")),
        _ => {}
    }
    if version_tuple(&meta.version).is_none() {
        out.push(SkinDiagnostic::warning("meta.version", "not a version"));
    }

    let d = &skin.dialogue;
    if let Some(spec) = &d.responsive {
        if spec.rules.len() > 16 {
            out.push(SkinDiagnostic::error(
                "dialogue.responsive.rules",
                "at most 16 layout rules",
            ));
        }
        for rule in &spec.rules {
            if !in_range(rule.max_width, 1.0, 8192.0) || !in_range(rule.max_height, 1.0, 8192.0) {
                out.push(SkinDiagnostic::error(
                    "dialogue.responsive.rules",
                    "thresholds must be finite and within 1–8192",
                ));
            }
        }
        for layout in spec.layouts() {
            if layout
                .padding
                .is_some_and(|p| p.iter().any(|v| !in_range(*v, 0.0, 4096.0)))
            {
                out.push(SkinDiagnostic::error(
                    "dialogue.responsive.padding",
                    "must be finite and within 0–4096",
                ));
            }
            let mut variant = skin.clone();
            variant.dialogue.responsive = None;
            layout.apply_to(&mut variant.dialogue);
            let v = &variant.dialogue;
            if !in_range(v.nameplate.size, 4.0, 400.0)
                || !in_range(v.continue_.size, 4.0, 400.0)
                || !in_range(v.box_.width_fraction, 0.1, 1.0)
                || v.nameplate
                    .offset
                    .iter()
                    .chain(v.portrait.offset.iter())
                    .any(|v| !in_range(*v, -4096.0, 4096.0))
                || v.nameplate
                    .padding
                    .iter()
                    .any(|v| !in_range(*v, 0.0, 1024.0))
                || v.portrait.size.iter().any(|v| !in_range(*v, 0.0, 4096.0))
            {
                out.push(SkinDiagnostic::error(
                    "dialogue.responsive",
                    "invalid label, portrait or box dimensions",
                ));
            }
            validate_shapes(&variant, out);
        }
    }
    let in_range = |v: f32, lo: f32, hi: f32| v.is_finite() && (lo..=hi).contains(&v);
    if !in_range(d.region[0], 160.0, 4096.0) || !in_range(d.region[1], 60.0, 4096.0) {
        out.push(SkinDiagnostic::error(
            "dialogue.region",
            "region must be 160–4096 × 60–4096",
        ));
    }
    if !in_range(d.overscan, 0.0, 256.0) {
        out.push(SkinDiagnostic::error("dialogue.overscan", "must be 0–256"));
    }
    for (field, size) in [
        ("dialogue.text.size", d.text.size),
        ("dialogue.nameplate.size", d.nameplate.size),
        ("history.text.size", skin.history.text.size),
        ("history.name_size", skin.history.name_size),
    ] {
        if !in_range(size, 4.0, 400.0) {
            out.push(SkinDiagnostic::error(field, "font size must be 4–400"));
        }
    }
    if !in_range(d.text.line_height, 0.8, 4.0) {
        out.push(SkinDiagnostic::error(
            "dialogue.text.line_height",
            "must be 0.8–4",
        ));
    }
    if !in_range(d.box_.width_fraction, 0.1, 1.0) {
        out.push(SkinDiagnostic::error(
            "dialogue.box.width_fraction",
            "must be 0.1–1",
        ));
    }
    if d.reveal.base_ms == 0 || d.reveal.base_ms > 2000 {
        out.push(SkinDiagnostic::error(
            "dialogue.reveal.base_ms",
            "must be 1–2000",
        ));
    }
    if let Some(light) = &d.reveal.light {
        for (field, value, lo, hi) in [
            ("dialogue.reveal.light.intensity", light.intensity, 0.0, 2.0),
            ("dialogue.reveal.light.radius", light.radius, 1.0, 32.0),
        ] {
            if !in_range(value, lo, hi) {
                out.push(SkinDiagnostic::error(field, format!("must be {lo}–{hi}")));
            }
        }
    }
    if !in_range(d.sweep.intensity, 0.0, 1.0) {
        out.push(SkinDiagnostic::error(
            "dialogue.sweep.intensity",
            "must be 0–1",
        ));
    }
    if !in_range(d.sweep.period_s, 0.1, 3600.0) {
        out.push(SkinDiagnostic::error(
            "dialogue.sweep.period_s",
            "must be 0.1–3600 seconds",
        ));
    }
    for (field, value, hi) in [
        ("effects.pulse_amount", skin.effects.pulse_amount, 2.0),
        ("effects.flicker_amount", skin.effects.flicker_amount, 1.0),
        ("effects.flicker_hz", skin.effects.flicker_hz, 60.0),
    ] {
        if !in_range(value, 0.0, hi) {
            out.push(SkinDiagnostic::error(field, "out of range or non-finite"));
        }
    }
    if !in_range(skin.effects.pulse_period_s, 0.05, 3600.0) {
        out.push(SkinDiagnostic::error(
            "effects.pulse_period_s",
            "must be 0.05–3600 seconds",
        ));
    }
    validate_shapes(skin, out);

    for (field, path, kind) in skin.referenced_files() {
        if kind == FileKind::Font && is_builtin_font(path) {
            validate_builtin_font(field, path, &BuiltinFonts::embedded(), out);
            continue;
        }
        let path = match clean_path(path) {
            Ok(p) => p,
            Err(e) => {
                out.push(SkinDiagnostic::error(field, e.to_string()));
                continue;
            }
        };
        let bytes = match files.read(&path) {
            Ok(b) => b,
            Err(e) => {
                out.push(SkinDiagnostic::error(field, e.to_string()));
                continue;
            }
        };
        match kind {
            FileKind::StrokeData => {} // Parsed after path and size validation.
            FileKind::Shader => {}     // Parsed and checked by load_shaders.
            FileKind::Image | FileKind::NineSlice(_) => match imagesize::blob_size(&bytes) {
                Ok(size) => {
                    if size.width > MAX_IMAGE_SIDE || size.height > MAX_IMAGE_SIDE {
                        out.push(SkinDiagnostic::error(
                            field,
                            format!("`{path}` is larger than {MAX_IMAGE_SIDE}px"),
                        ));
                    }
                    if let FileKind::NineSlice([top, right, bottom, left]) = kind
                        && ((top + bottom) as usize >= size.height
                            || (left + right) as usize >= size.width)
                    {
                        out.push(SkinDiagnostic::error(
                            field,
                            format!(
                                "slice insets do not fit inside the {}×{} image",
                                size.width, size.height
                            ),
                        ));
                    }
                }
                Err(_) => out.push(SkinDiagnostic::error(
                    field,
                    format!("`{path}` is not a PNG, JPEG, WebP or GIF image"),
                )),
            },
            FileKind::Font => {
                if ttf_parser::Face::parse(&bytes, 0).is_err() {
                    out.push(SkinDiagnostic::error(
                        field,
                        format!("`{path}` is not an OpenType/TrueType font"),
                    ));
                }
            }
            FileKind::Sound => {
                let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
                if !matches!(ext.as_str(), "wav" | "ogg" | "flac" | "mp3") || bytes.len() < 16 {
                    out.push(SkinDiagnostic::error(
                        field,
                        format!("`{path}` must be a WAV, OGG, FLAC or MP3 file"),
                    ));
                }
            }
        }
    }
}

/// Check a `builtin:` font reference against `fonts`. A font in
/// `crates/theme/fonts/` that this build leaves out is fine (the role falls
/// back to its families); a name that is not there at all is a warning, as
/// it is most likely a typo.
pub(crate) fn validate_builtin_font(
    field: &str,
    reference: &str,
    fonts: &BuiltinFonts,
    out: &mut Vec<SkinDiagnostic>,
) {
    if builtin_path(reference).is_none() {
        out.push(SkinDiagnostic::error(
            field,
            FileError::UnsafePath(reference.to_owned()).to_string(),
        ));
    } else if let Some(font) = fonts.resolve(reference) {
        if ttf_parser::Face::parse(font.bytes, 0).is_err() {
            out.push(SkinDiagnostic::error(
                field,
                format!("`{reference}` is not an OpenType/TrueType font"),
            ));
        }
    } else if !fonts.knows(reference) {
        out.push(SkinDiagnostic::warning(
            field,
            format!("no built-in font `{reference}`; the families are used instead"),
        ));
    }
}

/// Most shadow layers one shape can paint.
pub const MAX_SHADOWS: usize = 4;
/// Most stops one gradient can have.
pub const MAX_STOPS: usize = 8;
pub const MAX_DECOR: usize = 16;
/// The picker's tag vocabulary; the picker ignores other tags.
pub const SKIN_TAGS: &[&str] = &[
    "classic",
    "streaming",
    "game",
    "retro",
    "cute",
    "elegant",
    "minimal",
];

fn in_range(v: f32, lo: f32, hi: f32) -> bool {
    v.is_finite() && (lo..=hi).contains(&v)
}

fn check_gradient(field: &str, g: Option<&GradientSpec>, out: &mut Vec<SkinDiagnostic>) {
    let Some(g) = g else { return };
    if !(2..=MAX_STOPS).contains(&g.stops.len()) {
        out.push(SkinDiagnostic::error(
            field,
            format!("a gradient needs 2–{MAX_STOPS} stops"),
        ));
    }
    if g.stops.iter().any(|(p, _)| !in_range(*p, 0.0, 1.0))
        || g.stops.windows(2).any(|w| w[1].0 < w[0].0)
    {
        out.push(SkinDiagnostic::error(
            field,
            "stop positions must be 0–1 and must not decrease",
        ));
    }
    if !g.angle.is_finite() || !g.center.iter().all(|c| in_range(*c, -200.0, 300.0)) {
        out.push(SkinDiagnostic::error(
            field,
            "angle must be finite and center -200–300%",
        ));
    }
}

fn check_corners(field: &str, radii: [f32; 4], out: &mut Vec<SkinDiagnostic>) {
    if !radii.iter().all(|v| in_range(*v, 0.0, 2000.0)) {
        out.push(SkinDiagnostic::error(field, "radius must be 0–2000"));
    }
}

fn check_borders(field: &str, widths: [f32; 4], out: &mut Vec<SkinDiagnostic>) {
    if !widths.iter().all(|v| in_range(*v, 0.0, 64.0)) {
        out.push(SkinDiagnostic::error(field, "border width must be 0–64"));
    }
}

fn check_shadows(field: &str, layers: &[ShadowSpec], out: &mut Vec<SkinDiagnostic>) {
    if layers.len() > MAX_SHADOWS {
        out.push(SkinDiagnostic::error(
            field,
            format!(
                "at most {MAX_SHADOWS} shadow layers (inner border, glow and shadow count too)"
            ),
        ));
    }
    if layers.iter().any(|s| {
        !in_range(s.blur, 0.0, 200.0)
            || !in_range(s.spread, -500.0, 500.0)
            || !in_range(s.dx, -500.0, 500.0)
            || !in_range(s.dy, -500.0, 500.0)
    }) {
        out.push(SkinDiagnostic::error(
            field,
            "blur must be 0–200, spread and offsets -500–500",
        ));
    }
}

/// A part's gradient, corners, borders and shadow layers, reported under
/// `prefix.gradient`, `prefix.radius`, `prefix.border_width` and `prefix.shadows`.
fn check_shape(
    prefix: &str,
    shape: &ShapeSpec,
    radius: f32,
    border_width: f32,
    layers: &[ShadowSpec],
    out: &mut Vec<SkinDiagnostic>,
) {
    check_gradient(&format!("{prefix}.gradient"), shape.gradient.as_ref(), out);
    check_corners(&format!("{prefix}.radius"), shape.radii(radius), out);
    check_borders(
        &format!("{prefix}.border_width"),
        shape.border_widths(border_width),
        out,
    );
    check_shadows(&format!("{prefix}.shadows"), layers, out);
}

/// Placement, shape, polygon and glyph ranges of a decoration list.
fn check_decor(field: &str, decor: &[DecorSpec], out: &mut Vec<SkinDiagnostic>) {
    if decor.len() > MAX_DECOR {
        out.push(SkinDiagnostic::error(
            field,
            format!("at most {MAX_DECOR} decorations"),
        ));
    }
    for (i, dec) in decor.iter().enumerate() {
        let prefix = format!("{field}.{i}");
        let f = |k: &str| format!("{prefix}.{k}");
        if dec.aspect_ratio.is_some_and(|v| !in_range(v, 0.01, 100.0))
            || dec
                .max_fraction
                .is_some_and(|v| v.iter().any(|v| !in_range(*v, 0.001, 4.0)))
            || dec
                .min_window
                .is_some_and(|v| v.iter().any(|v| !in_range(*v, 0.0, 8192.0)))
        {
            out.push(SkinDiagnostic::error(
                &prefix,
                "invalid responsive ornament constraint",
            ));
        }
        if let Some(m) = dec.motion {
            if !in_range(m.period_s, 0.5, 120.0) {
                out.push(SkinDiagnostic::error(
                    f("motion.period_s"),
                    "must be 0.5–120 seconds",
                ));
            }
            let max = match m.kind {
                DecorMotionKind::Float => 32.0,
                DecorMotionKind::Breathe => 1.0,
            };
            if !in_range(m.amplitude, 0.0, max) {
                out.push(SkinDiagnostic::error(
                    f("motion.amplitude"),
                    format!("must be 0–{max}"),
                ));
            }
            if !in_range(m.phase, 0.0, 1.0) {
                out.push(SkinDiagnostic::error(
                    f("motion.phase"),
                    "must be 0–1 cycles",
                ));
            }
        }
        let size_ok = dec.size.iter().all(|s| match s {
            Len::Px(v) => in_range(*v, 0.0, 4096.0),
            Len::Percent(v) => in_range(*v, 0.0, 400.0),
        });
        if !size_ok {
            out.push(SkinDiagnostic::error(
                f("size"),
                "px sizes must be 0–4096 and percentages 0–400",
            ));
        }
        if dec
            .inset
            .is_some_and(|i| !i.iter().all(|v| in_range(*v, -4096.0, 4096.0)))
            || !dec.offset.iter().all(|v| in_range(*v, -4096.0, 4096.0))
        {
            out.push(SkinDiagnostic::error(
                f("offset"),
                "offsets and insets must be within ±4096",
            ));
        }
        check_shape(
            &prefix,
            &dec.shape,
            dec.radius,
            dec.border_width,
            &dec.shape.shadows,
            out,
        );
        if !(dec.polygon.is_empty() || (3..=8).contains(&dec.polygon.len()))
            || !dec
                .polygon
                .iter()
                .flatten()
                .all(|v| in_range(*v, -100.0, 200.0))
        {
            out.push(SkinDiagnostic::error(
                f("polygon"),
                "a polygon needs 3–8 points within -100–200%",
            ));
        }
        if !in_range(dec.rotate, -360.0, 360.0) {
            out.push(SkinDiagnostic::error(f("rotate"), "must be -360–360"));
        }
        if !in_range(dec.opacity, 0.0, 1.0) {
            out.push(SkinDiagnostic::error(f("opacity"), "must be 0–1"));
        }
        if dec.text.chars().count() > 8 {
            out.push(SkinDiagnostic::error(f("text"), "at most 8 characters"));
        }
        if !in_range(dec.text_size, 4.0, 400.0) {
            out.push(SkinDiagnostic::error(
                f("text_size"),
                "font size must be 4–400",
            ));
        }
    }
}

/// Ranges of the drawn-shape fields: gradients, corners, borders, shadows,
/// the tail, decorations, the continue indicator and the history panel.
fn validate_shapes(skin: &Skin, out: &mut Vec<SkinDiagnostic>) {
    let d = &skin.dialogue;
    let b = &d.box_;
    check_shape(
        "dialogue.box",
        &b.shape,
        b.radius,
        b.border_width,
        &b.shadow_layers(),
        out,
    );
    check_borders(
        "dialogue.box.inner_border_width",
        [b.inner_border_width; 4],
        out,
    );
    let [top, right, bottom, left] = b.margin;
    if !b.margin.iter().all(|m| in_range(*m, 0.0, 1024.0))
        || left + right >= d.region[0]
        || top + bottom >= d.region[1]
    {
        out.push(SkinDiagnostic::error(
            "dialogue.box.margin",
            "margins must be 0–1024 and leave room for the box",
        ));
    }

    let p = &d.nameplate;
    check_shape(
        "dialogue.nameplate",
        &p.shape,
        p.radius,
        p.border_width,
        &p.shape.shadows,
        out,
    );
    if !in_range(p.rotate, -360.0, 360.0) {
        out.push(SkinDiagnostic::error(
            "dialogue.nameplate.rotate",
            "must be -360–360",
        ));
    }

    if let Some(t) = &d.tail
        && (!in_range(t.at, 0.0, 1.0)
            || !in_range(t.width, 4.0, 1024.0)
            || !in_range(t.length, 2.0, 512.0)
            || !in_range(t.lean, -512.0, 512.0))
    {
        out.push(SkinDiagnostic::error(
            "dialogue.tail",
            "at must be 0–1, width 4–1024, length 2–512 and lean -512–512",
        ));
    }

    check_decor("dialogue.decor", &d.decor, out);

    let c = &d.continue_;
    if !(1..=64).contains(&c.frames) {
        out.push(SkinDiagnostic::error(
            "dialogue.continue.frames",
            "must be 1–64",
        ));
    }
    if !in_range(c.fps, 0.5, 60.0) {
        out.push(SkinDiagnostic::error(
            "dialogue.continue.fps",
            "must be 0.5–60",
        ));
    }
    if !in_range(c.period_s, 0.1, 10.0) {
        out.push(SkinDiagnostic::error(
            "dialogue.continue.period_s",
            "must be 0.1–10",
        ));
    }
    if c.image_size
        .is_some_and(|s| !s.iter().all(|v| in_range(*v, 1.0, 1024.0)))
    {
        out.push(SkinDiagnostic::error(
            "dialogue.continue.image_size",
            "must be 1–1024",
        ));
    }

    let h = &skin.history;
    check_gradient("history.bubble_gradient", h.bubble_gradient.as_ref(), out);
    check_corners(
        "history.bubble_radius",
        h.bubble_radii.unwrap_or([h.bubble_radius; 4]),
        out,
    );
    check_borders(
        "history.bubble_border_width",
        [h.bubble_border_width; 4],
        out,
    );
    check_shadows("history.bubble_shadows", &h.bubble_shadows, out);
    if let Some(np) = &h.name_plate {
        check_shape(
            "history.name_plate",
            &np.shape,
            np.radius,
            np.border_width,
            &np.shape.shadows,
            out,
        );
        if !np.padding.iter().all(|v| in_range(*v, 0.0, 1024.0)) {
            out.push(SkinDiagnostic::error(
                "history.name_plate.padding",
                "must be 0–1024",
            ));
        }
    }
    check_borders("history.avatar.ring_width", [h.avatar.ring_width; 4], out);
    if !in_range(h.name_width, 0.0, 2048.0) {
        out.push(SkinDiagnostic::error(
            "history.name_width",
            "must be 0–2048",
        ));
    }
    if !(100..=1000).contains(&h.name_weight) {
        out.push(SkinDiagnostic::error(
            "history.name_weight",
            "must be 100–1000",
        ));
    }
    if h.time_size.is_some_and(|s| !in_range(s, 4.0, 400.0)) {
        out.push(SkinDiagnostic::error(
            "history.time_size",
            "font size must be 4–400",
        ));
    }
    if let Some(p) = &h.panel {
        check_shape(
            "history.panel",
            &p.shape,
            p.radius,
            p.border_width,
            &p.shape.shadows,
            out,
        );
        if !p
            .padding
            .iter()
            .chain(&p.margin)
            .all(|v| in_range(*v, 0.0, 1024.0))
        {
            out.push(SkinDiagnostic::error(
                "history.panel.padding",
                "padding and margin must be 0–1024",
            ));
        }
    }
    if let Some(t) = &h.title
        && (!in_range(t.size, 4.0, 400.0)
            || !in_range(t.height, 0.0, 1024.0)
            || !in_range(t.padding, 0.0, 1024.0)
            || !(100..=1000).contains(&t.weight)
            || t.text.chars().count() > 64)
    {
        out.push(SkinDiagnostic::error(
            "history.title",
            "size must be 4–400, height and padding 0–1024, weight 100–1000 and text at most 64 characters",
        ));
    }
    check_decor("history.decor", &h.decor, out);

    let tags = &skin.meta.tags;
    let bad_tag = |t: &String| {
        t.is_empty()
            || t.len() > 24
            || !t
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    };
    if tags.len() > 8 || tags.iter().any(bad_tag) {
        out.push(SkinDiagnostic::warning(
            "meta.tags",
            "at most 8 tags of 1–24 characters a-z, 0-9 or `-`",
        ));
    }
}
