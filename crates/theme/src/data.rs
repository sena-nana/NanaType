//! Declared data resources. Only the host reads and parses them; scripts and
//! pipelines receive bounded values, never paths, handles or host objects.
use crate::{FileError, PACKAGE_MANIFEST, PackageManifest, SkinFiles, StrokeCommand, StrokeData};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, LazyLock, Mutex, OnceLock, Weak},
};

/// Largest serialized value a single query returns.
pub const MAX_QUERY_RESULT_BYTES: usize = 64 * 1024;
/// Rows a `lines` or `slice` query returns at most.
pub const MAX_QUERY_ROWS: u32 = 1024;
/// Nesting a query result may have; deeper values are refused, not truncated.
pub const MAX_RESULT_DEPTH: usize = 32;
/// Raw bytes one plugin may hold for its declared reads.
pub const MAX_ACCESS_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TEXT_LINES: usize = 1_000_000;

/// Formats are only ever taken from the declaration, never guessed from a
/// file name or content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataFormat {
    Text,
    Json,
    /// The host-registered NTHZ stroke-order container.
    Nthz,
}
impl DataFormat {
    pub fn name(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Json => "json",
            Self::Nthz => "nthz",
        }
    }
    pub fn versions(self) -> &'static [u32] {
        &[1]
    }
    /// Raw size checked before the file is read.
    pub fn max_raw(self) -> u64 {
        match self {
            Self::Text => 4 << 20,
            Self::Json => 8 << 20,
            Self::Nthz => 32 << 20,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JsonType {
    Object,
    Array,
    String,
    Number,
    Boolean,
}

/// Structural limits for a JSON resource, checked once after parsing.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct JsonConstraints {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub root: Option<JsonType>,
    /// Keys the root object must have.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub required: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_depth: Option<u32>,
    /// Values in the whole document.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_items: Option<u32>,
    /// Bytes per string or key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_string: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number_range: Option<[f64; 2]>,
}
impl JsonConstraints {
    fn depth(&self) -> usize {
        self.max_depth.unwrap_or(32) as usize
    }
    fn items(&self) -> usize {
        self.max_items.unwrap_or(1_000_000) as usize
    }
    fn string(&self) -> usize {
        self.max_string.unwrap_or(64 * 1024) as usize
    }
}

/// `[data.<id>]` in `package.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataSpec {
    /// Package-relative; never `pkg:` or an absolute path.
    pub path: String,
    pub format: DataFormat,
    pub format_version: u32,
    pub source: String,
    pub license: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub constraints: Option<JsonConstraints>,
}
impl DataSpec {
    pub fn validate(&self, id: &str) -> Result<(), String> {
        if !valid_data_id(id) {
            return Err(format!("invalid data ID `{id}`"));
        }
        if self.path.starts_with("pkg:") {
            return Err(format!("data.{id}: path must be inside this package"));
        }
        crate::clean_path(&self.path).map_err(|e| format!("data.{id}: {e}"))?;
        if !self.format.versions().contains(&self.format_version) {
            return Err(format!(
                "data.{id}: unsupported {} version {}",
                self.format.name(),
                self.format_version
            ));
        }
        for (name, value) in [("source", &self.source), ("license", &self.license)] {
            if value.trim().is_empty() || value.len() > 1024 {
                return Err(format!("data.{id}: {name} must be 1-1024 bytes"));
            }
        }
        if let Some(c) = &self.constraints {
            if self.format != DataFormat::Json {
                return Err(format!("data.{id}: constraints apply to JSON only"));
            }
            let range_ok = c
                .number_range
                .is_none_or(|[min, max]| min.is_finite() && max.is_finite() && min <= max);
            if c.required.len() > 64
                || c.required.iter().any(|k| k.len() > 256)
                || c.max_depth.is_some_and(|d| !(1..=64).contains(&d))
                || c.max_items.is_some_and(|n| n > 10_000_000)
                || c.max_string.is_some_and(|n| n > 1 << 20)
                || !range_ok
            {
                return Err(format!("data.{id}: constraints exceed host limits"));
            }
        }
        Ok(())
    }
}

pub fn valid_data_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_-".contains(&b))
}

/// `"id"` names this package's data, `"pkg:<package>/<id>"` a dependency's.
pub fn split_reference(reference: &str) -> Result<(Option<&str>, &str), String> {
    let (package, id) = match reference.strip_prefix("pkg:") {
        Some(rest) => {
            let (package, id) = rest
                .split_once('/')
                .ok_or_else(|| format!("invalid data reference `{reference}`"))?;
            if !crate::package::valid_id(package) {
                return Err(format!("invalid data reference `{reference}`"));
            }
            (Some(package), id)
        }
        None => (None, reference),
    };
    if !valid_data_id(id) {
        return Err(format!("invalid data reference `{reference}`"));
    }
    Ok((package, id))
}

/// Find a declaration through the package's own namespaces; returns the spec
/// and the resource path to read it from.
pub fn resolve_data(files: &dyn SkinFiles, reference: &str) -> Result<(DataSpec, String), String> {
    let (package, id) = split_reference(reference)?;
    let manifest = match package {
        Some(package) => resolve_manifest(files, package)?.0,
        None => PackageManifest::read(files).map_err(|e| e.to_string())?,
    };
    let spec = manifest
        .data
        .get(id)
        .cloned()
        .ok_or_else(|| format!("undeclared data `{reference}`"))?;
    let path = match package {
        Some(package) => format!("pkg:{package}/{}", spec.path),
        None => spec.path.clone(),
    };
    Ok((spec, path))
}

/// A visible dependency's manifest, read through its `pkg:` namespace.
pub(crate) fn resolve_manifest(
    files: &dyn SkinFiles,
    package: &str,
) -> Result<(PackageManifest, String), String> {
    let prefix = format!("pkg:{package}/");
    let bytes = files
        .read(&format!("{prefix}{PACKAGE_MANIFEST}"))
        .map_err(|e| format!("{package}: {e}"))?;
    if bytes.len() > 64 * 1024 {
        return Err(format!("{package}: package.toml exceeds budget"));
    }
    let text = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
    Ok((
        PackageManifest::parse(text).map_err(|e| e.to_string())?,
        prefix,
    ))
}

#[derive(Debug)]
pub struct TextData {
    pub text: Box<str>,
    /// Byte offsets where each line starts.
    starts: Vec<u32>,
}
impl TextData {
    pub fn lines(&self) -> usize {
        self.starts.len()
    }
    pub fn line(&self, index: usize) -> Option<&str> {
        let start = *self.starts.get(index)? as usize;
        let end = self
            .starts
            .get(index + 1)
            .map_or(self.text.len(), |&e| e as usize);
        Some(self.text[start..end].trim_end_matches(['\n', '\r']))
    }
}

#[derive(Debug)]
pub enum DataValue {
    Text(TextData),
    Json(serde_json::Value),
    Strokes(Arc<StrokeData>),
}

/// A parsed resource, shared by content hash across packages and skins.
#[derive(Debug)]
pub struct LoadedData {
    pub hash: [u8; 32],
    pub format: DataFormat,
    pub format_version: u32,
    pub bytes: u64,
    pub value: DataValue,
}
impl PartialEq for LoadedData {
    fn eq(&self, other: &Self) -> bool {
        (self.hash, self.format, self.format_version)
            == (other.hash, other.format, other.format_version)
    }
}

type CacheKey = ([u8; 32], DataFormat, u32, u64);
static CACHE: LazyLock<Mutex<HashMap<CacheKey, Weak<LoadedData>>>> =
    LazyLock::new(Default::default);

/// Check the raw budget, read and parse a declared resource. Imports pass
/// `full` to validate every block of block-compressed formats.
pub fn load_data(
    files: &dyn SkinFiles,
    spec: &DataSpec,
    path: &str,
    full: bool,
) -> Result<Arc<LoadedData>, String> {
    let max = spec.format.max_raw();
    let len = files.file_len(path).map_err(|e| e.to_string())?;
    if len > max {
        return Err(FileError::TooLarge {
            path: path.into(),
            bytes: len,
        }
        .to_string());
    }
    let bytes = files.read(path).map_err(|e| e.to_string())?;
    parse_data(spec, &bytes, full)
}

pub fn parse_data(spec: &DataSpec, bytes: &[u8], full: bool) -> Result<Arc<LoadedData>, String> {
    if !spec.format.versions().contains(&spec.format_version) {
        return Err(format!(
            "unsupported {} version {}",
            spec.format.name(),
            spec.format_version
        ));
    }
    if bytes.len() as u64 > spec.format.max_raw() {
        return Err(format!(
            "{} data exceeds its raw budget",
            spec.format.name()
        ));
    }
    let hash = *blake3::hash(bytes).as_bytes();
    let constraints = spec.constraints.clone().unwrap_or_default();
    let key = (
        hash,
        spec.format,
        spec.format_version,
        u64::from_le_bytes(
            blake3::hash(format!("{constraints:?}").as_bytes()).as_bytes()[..8]
                .try_into()
                .unwrap(),
        ),
    );
    if let Some(data) = CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key)
        .and_then(Weak::upgrade)
    {
        if full && let DataValue::Strokes(strokes) = &data.value {
            strokes.validate_all()?;
        }
        return Ok(data);
    }
    let value = match spec.format {
        DataFormat::Text => DataValue::Text(parse_text(bytes)?),
        DataFormat::Json => {
            let value: serde_json::Value =
                serde_json::from_slice(bytes).map_err(|e| format!("invalid JSON: {e}"))?;
            check_json(&value, &constraints)?;
            DataValue::Json(value)
        }
        DataFormat::Nthz => {
            let strokes = StrokeData::parse(bytes)?;
            if full {
                strokes.validate_all()?;
            }
            DataValue::Strokes(Arc::new(strokes))
        }
    };
    let data = Arc::new(LoadedData {
        hash,
        format: spec.format,
        format_version: spec.format_version,
        bytes: bytes.len() as u64,
        value,
    });
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    // A concurrent parse of the same bytes may have finished first; share it.
    if let Some(existing) = cache.get(&key).and_then(Weak::upgrade) {
        return Ok(existing);
    }
    cache.retain(|_, v| v.strong_count() > 0);
    cache.insert(key, Arc::downgrade(&data));
    Ok(data)
}

fn parse_text(bytes: &[u8]) -> Result<TextData, String> {
    let text = std::str::from_utf8(bytes).map_err(|e| format!("text is not UTF-8: {e}"))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut starts = vec![0];
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' && i + 1 < text.len() {
            starts.push(i as u32 + 1);
            if starts.len() > MAX_TEXT_LINES {
                return Err(format!("text exceeds {MAX_TEXT_LINES} lines"));
            }
        }
    }
    if text.is_empty() {
        starts.clear();
    }
    Ok(TextData {
        text: text.into(),
        starts,
    })
}

fn check_json(value: &serde_json::Value, c: &JsonConstraints) -> Result<(), String> {
    use serde_json::Value;
    let kind = |v: &Value| match v {
        Value::Object(_) => Some(JsonType::Object),
        Value::Array(_) => Some(JsonType::Array),
        Value::String(_) => Some(JsonType::String),
        Value::Number(_) => Some(JsonType::Number),
        Value::Bool(_) => Some(JsonType::Boolean),
        Value::Null => None,
    };
    if let Some(root) = c.root
        && kind(value) != Some(root)
    {
        return Err(format!("JSON root must be {root:?}").to_lowercase());
    }
    if !c.required.is_empty() {
        let object = value.as_object().ok_or("JSON root must be an object")?;
        if let Some(missing) = c.required.iter().find(|k| !object.contains_key(*k)) {
            return Err(format!("JSON is missing required key `{missing}`"));
        }
    }
    let (max_depth, max_items, max_string) = (c.depth(), c.items(), c.string());
    let mut items = 0usize;
    // Explicit stack: depth is checked before descending, never recursion.
    let mut stack = vec![(value, 1usize)];
    while let Some((value, depth)) = stack.pop() {
        items += 1;
        if items > max_items {
            return Err(format!("JSON exceeds {max_items} values"));
        }
        if depth > max_depth {
            return Err(format!("JSON nesting exceeds {max_depth}"));
        }
        match value {
            Value::String(s) if s.len() > max_string => {
                return Err(format!("JSON string exceeds {max_string} bytes"));
            }
            Value::Number(n) => {
                let n = n
                    .as_f64()
                    .filter(|n| n.is_finite())
                    .ok_or("non-finite JSON number")?;
                if let Some([min, max]) = c.number_range
                    && !(min..=max).contains(&n)
                {
                    return Err(format!("JSON number {n} outside {min}..={max}"));
                }
            }
            Value::Array(a) => stack.extend(a.iter().map(|v| (v, depth + 1))),
            Value::Object(o) => {
                for (k, v) in o {
                    if k.len() > max_string {
                        return Err(format!("JSON key exceeds {max_string} bytes"));
                    }
                    stack.push((v, depth + 1));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// What a script may ask of a declared resource.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DataQuery {
    // Braced so unknown fields are refused like every other variant.
    Info {},
    /// The complete value when it fits the result budget.
    Whole {},
    /// Text lines `start..start+count`.
    Lines {
        start: u32,
        count: u32,
    },
    /// A JSON value by RFC 6901 pointer.
    Pointer {
        pointer: String,
    },
    /// Array items, or object entries as `{key, value}`, below a pointer.
    Slice {
        pointer: String,
        start: u32,
        count: u32,
    },
    /// Strokes and medians of one character.
    Glyph {
        #[serde(rename = "char")]
        character: String,
    },
}

impl LoadedData {
    pub fn query(&self, query: &DataQuery) -> Result<serde_json::Value, String> {
        use serde_json::{Value, json};
        let unsupported = || format!("query not supported for {} data", self.format.name());
        let value = match (query, &self.value) {
            (DataQuery::Info {}, value) => {
                let count = match value {
                    DataValue::Text(t) => json!({"lines": t.lines()}),
                    DataValue::Json(Value::Array(a)) => json!({"items": a.len()}),
                    DataValue::Json(Value::Object(o)) => json!({"items": o.len()}),
                    DataValue::Json(_) => json!({}),
                    DataValue::Strokes(s) => json!({"glyphs": s.len()}),
                };
                let mut info = json!({
                    "format": self.format.name(),
                    "format_version": self.format_version,
                    "bytes": self.bytes,
                });
                if let (Value::Object(info), Value::Object(count)) = (&mut info, count) {
                    info.extend(count);
                }
                info
            }
            (DataQuery::Whole {}, DataValue::Text(t)) => {
                bounded(&t.text)?;
                Value::String(t.text.to_string())
            }
            (DataQuery::Whole {}, DataValue::Json(v)) => {
                bounded(v)?;
                v.clone()
            }
            (DataQuery::Lines { start, count }, DataValue::Text(t)) => {
                let count = rows(*count)?;
                let lines: Vec<_> = (*start as usize..)
                    .take(count)
                    .map_while(|i| t.line(i))
                    .collect();
                bounded(&lines)?;
                json!(lines)
            }
            (DataQuery::Pointer { pointer }, DataValue::Json(v)) => {
                let found = point(v, pointer)?;
                bounded(found)?;
                found.clone()
            }
            (
                DataQuery::Slice {
                    pointer,
                    start,
                    count,
                },
                DataValue::Json(v),
            ) => {
                let count = rows(*count)?;
                let start = *start as usize;
                match point(v, pointer)? {
                    Value::Array(a) => {
                        let items: Vec<_> = a.iter().skip(start).take(count).collect();
                        bounded(&items)?;
                        json!(items)
                    }
                    Value::Object(o) => {
                        let entries: Vec<_> = o
                            .iter()
                            .skip(start)
                            .take(count)
                            .map(|(k, v)| json!({"key": k, "value": v}))
                            .collect();
                        bounded(&entries)?;
                        json!(entries)
                    }
                    _ => return Err("slice needs an array or object".into()),
                }
            }
            (DataQuery::Glyph { character }, DataValue::Strokes(s)) => {
                let mut chars = character.chars();
                let (Some(c), None) = (chars.next(), chars.next()) else {
                    return Err("glyph needs exactly one character".into());
                };
                match s.glyph(c) {
                    None => Value::Null,
                    Some(glyph) => {
                        let strokes: Vec<Vec<Value>> = glyph
                            .strokes
                            .iter()
                            .map(|stroke| stroke.iter().map(command).collect())
                            .collect();
                        let value = json!({"strokes": strokes, "medians": glyph.medians});
                        bounded(&value)?;
                        value
                    }
                }
            }
            _ => return Err(unsupported()),
        };
        if depth(&value) > MAX_RESULT_DEPTH {
            return Err(format!("result nesting exceeds {MAX_RESULT_DEPTH}"));
        }
        Ok(value)
    }
}

fn rows(count: u32) -> Result<usize, String> {
    if count > MAX_QUERY_ROWS {
        return Err(format!("at most {MAX_QUERY_ROWS} rows per query"));
    }
    Ok(count as usize)
}

fn point<'a>(value: &'a serde_json::Value, pointer: &str) -> Result<&'a serde_json::Value, String> {
    if pointer.len() > 1024 {
        return Err("pointer exceeds 1 KiB".into());
    }
    value
        .pointer(pointer)
        .ok_or_else(|| format!("no JSON value at `{pointer}`"))
}

fn command(command: &StrokeCommand) -> serde_json::Value {
    use serde_json::json;
    match command {
        StrokeCommand::Move([x, y]) => json!(["M", x, y]),
        StrokeCommand::Line([x, y]) => json!(["L", x, y]),
        StrokeCommand::Quad([[x1, y1], [x, y]]) => json!(["Q", x1, y1, x, y]),
        StrokeCommand::Cubic([[x1, y1], [x2, y2], [x, y]]) => json!(["C", x1, y1, x2, y2, x, y]),
        StrokeCommand::Close => json!(["Z"]),
    }
}

fn depth(value: &serde_json::Value) -> usize {
    let mut max = 0;
    let mut stack = vec![(value, 1)];
    while let Some((value, d)) = stack.pop() {
        max = max.max(d);
        if d > MAX_RESULT_DEPTH {
            break;
        }
        match value {
            serde_json::Value::Array(a) => stack.extend(a.iter().map(|v| (v, d + 1))),
            serde_json::Value::Object(o) => stack.extend(o.values().map(|v| (v, d + 1))),
            _ => {}
        }
    }
    max
}

/// Serialize into a counter that stops at the result budget, so an oversized
/// value is refused before it is copied.
fn bounded(value: &impl Serialize) -> Result<(), String> {
    struct Budget(usize);
    impl std::io::Write for Budget {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0 += buf.len();
            if self.0 > MAX_QUERY_RESULT_BYTES {
                return Err(std::io::Error::other("budget"));
            }
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Budget(0), value).map_err(|_| {
        format!("result exceeds {MAX_QUERY_RESULT_BYTES} bytes; query a smaller range")
    })
}

/// What the consent screen shows for one declared read.
#[derive(Debug, Clone, PartialEq)]
pub struct DataDisclosure {
    pub reference: String,
    pub owner: String,
    pub id: String,
    pub format: DataFormat,
    pub format_version: u32,
    pub bytes: u64,
    pub source: String,
    pub license: String,
    /// Covered by the owning package's signature.
    pub protected: bool,
}

#[derive(Debug)]
struct AccessItem {
    disclosure: DataDisclosure,
    spec: DataSpec,
    bytes: Arc<[u8]>,
    loaded: OnceLock<Result<Arc<LoadedData>, String>>,
}

/// The resources a plugin was approved to read, taken from the reviewed
/// snapshot so later disk changes cannot reach it.
#[derive(Debug, Default)]
pub struct DataAccess {
    items: BTreeMap<String, AccessItem>,
}
impl PartialEq for DataAccess {
    fn eq(&self, other: &Self) -> bool {
        self.disclosures().eq(other.disclosures())
    }
}

/// A bundled dependency inside a snapshot, without copying it.
#[derive(Debug)]
struct Bundled<'a> {
    files: &'a crate::PackageProject,
    prefix: String,
}
impl SkinFiles for Bundled<'_> {
    fn read(&self, path: &str) -> Result<Vec<u8>, FileError> {
        self.files
            .read(&format!("{}{}", self.prefix, crate::clean_path(path)?))
    }
    fn file_len(&self, path: &str) -> Result<u64, FileError> {
        self.files
            .file_len(&format!("{}{}", self.prefix, crate::clean_path(path)?))
    }
    fn list(&self) -> Vec<String> {
        self.files
            .files
            .keys()
            .filter_map(|p| p.strip_prefix(&self.prefix).map(str::to_owned))
            .collect()
    }
}

impl DataAccess {
    pub fn for_plugin(
        snapshot: &crate::PackageProject,
        manifest: &PackageManifest,
    ) -> Result<Self, String> {
        let reads = manifest.lua.as_ref().map_or(&[][..], |l| &l.reads[..]);
        let mut items = BTreeMap::new();
        let mut total = 0u64;
        for reference in reads {
            let (package, id) = split_reference(reference)?;
            let owner = package.unwrap_or(&manifest.meta.id).to_owned();
            let files = Bundled {
                files: snapshot,
                prefix: match package {
                    Some(package) if package != manifest.meta.id => {
                        format!("dependencies/{package}/")
                    }
                    _ => String::new(),
                },
            };
            let owner_manifest = PackageManifest::read(&files).map_err(|e| e.to_string())?;
            let spec = owner_manifest
                .data
                .get(id)
                .cloned()
                .ok_or_else(|| format!("undeclared data `{reference}`"))?;
            let len = files.file_len(&spec.path).map_err(|e| e.to_string())?;
            total += len;
            if len > spec.format.max_raw() || total > MAX_ACCESS_BYTES {
                return Err(format!("{reference}: data exceeds the read budget"));
            }
            let bytes: Arc<[u8]> = files.read(&spec.path).map_err(|e| e.to_string())?.into();
            let protected = crate::verify_integrity(&files)
                .map_err(|e| e.to_string())?
                .is_some_and(|seal| seal.protected.contains(&spec.path));
            let disclosure = DataDisclosure {
                reference: reference.clone(),
                owner,
                id: id.into(),
                format: spec.format,
                format_version: spec.format_version,
                bytes: len,
                source: spec.source.clone(),
                license: spec.license.clone(),
                protected,
            };
            items.insert(
                reference.clone(),
                AccessItem {
                    disclosure,
                    spec,
                    bytes,
                    loaded: OnceLock::new(),
                },
            );
        }
        Ok(Self { items })
    }
    pub fn disclosures(&self) -> impl Iterator<Item = &DataDisclosure> {
        self.items.values().map(|i| &i.disclosure)
    }
    /// Answer one script request; parsing happens on first use.
    pub fn answer(
        &self,
        reference: &str,
        query: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let item = self
            .items
            .get(reference)
            .ok_or_else(|| format!("`{reference}` is not among this plugin's declared reads"))?;
        let query: DataQuery =
            serde_json::from_value(query.clone()).map_err(|e| format!("invalid query: {e}"))?;
        let loaded = item
            .loaded
            .get_or_init(|| parse_data(&item.spec, &item.bytes, false))
            .as_ref()
            .map_err(Clone::clone)?;
        loaded.query(&query)
    }
}

#[cfg(test)]
mod tests;
