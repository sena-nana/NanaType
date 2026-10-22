//! Skin-declared user parameters.
//!
//! A skin lists `[params.<id>]` with a type, a default and limits. Any other
//! field may then hold `{ param = "<id>" }`, and a shader may bind params to
//! uniform slots (`shaders.<id>.bindings`). Loading compiles every placeholder
//! into a [`ParamSite`] and substitutes the default, so the skin validates as a
//! plain manifest; [`resolve`] later writes a user's values into the same sites.
//! Params never name files: placeholders under fonts, sounds, images or shader
//! sources are refused, so a user's value cannot change what the skin loads.

use std::collections::BTreeMap;
use std::sync::Arc;

use nanatype_model::{ParamValue, Rgba8, valid_param_id};
use serde::{Deserialize, Serialize};
use toml::{Table, Value};
use unicode_segmentation::UnicodeSegmentation;

use crate::load::{SkinDiagnostic, validate_values};
use crate::manifest::{ColorRef, ColorRole, Localized, Skin, localized};

pub const MAX_PARAMS: usize = 64;
const MAX_PRESETS: usize = 32;
const MAX_GROUPS: usize = 16;
const MAX_OPTIONS: usize = 16;
const MAX_CHECKS: usize = 16;
/// Upper bound for `max_len` and the default when a text param omits it.
pub const MAX_TEXT_GRAPHEMES: u32 = 64;

/// Top-level tables that declare params rather than consume them.
const SCHEMA_ROOTS: [&str; 4] = ["params", "param_groups", "presets", "param_checks"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamType {
    #[default]
    Number,
    Color,
    Bool,
    Enum,
    Text,
}

impl ParamType {
    fn name(self) -> &'static str {
        match self {
            Self::Number => "number",
            Self::Color => "color",
            Self::Bool => "bool",
            Self::Enum => "enum",
            Self::Text => "text",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct EnumOption {
    pub id: String,
    #[serde(skip_serializing_if = "Localized::is_empty")]
    pub name: Localized,
}

/// One `[params.<id>]` entry.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ParamSpec {
    #[serde(rename = "type")]
    pub kind: ParamType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<ParamValue>,
    #[serde(skip_serializing_if = "Localized::is_empty")]
    pub name: Localized,
    #[serde(skip_serializing_if = "Localized::is_empty")]
    pub description: Localized,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// Colours: allow translucency. Off, a colour must be opaque.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpha: Option<bool>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<EnumOption>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_len: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_empty: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ParamGroup {
    #[serde(skip_serializing_if = "Localized::is_empty")]
    pub name: Localized,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<i32>,
}

/// A named set of values; params it leaves out take their defaults.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PresetSpec {
    #[serde(skip_serializing_if = "Localized::is_empty")]
    pub name: Localized,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<i32>,
    pub values: BTreeMap<String, ParamValue>,
}

/// A contrast check the settings page and loader raise as a warning, never
/// a refusal.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ParamCheck {
    /// A colour param id or a colour role (`text`, `box`, …).
    pub foreground: String,
    pub background: String,
    pub min: f32,
}

/// One group of [`Skin::ordered_params`]: its id (`None` for ungrouped
/// params) and its params in order.
pub type ParamGroupListing<'a> = (Option<&'a str>, Vec<(&'a str, &'a ParamSpec)>);

/// A param's value after checking it against its spec.
#[derive(Debug, Clone, PartialEq)]
pub enum Resolved {
    Number(f32),
    Bool(bool),
    Color(Rgba8),
    Enum { id: String, index: u32 },
    Text(String),
}

impl Resolved {
    /// The settings form of this value.
    pub fn to_value(&self) -> ParamValue {
        match self {
            Self::Number(v) => ParamValue::Number(f64::from(*v)),
            Self::Bool(v) => ParamValue::Bool(*v),
            Self::Color(c) => ParamValue::Text(c.to_hex()),
            Self::Enum { id, .. } => ParamValue::Text(id.clone()),
            Self::Text(t) => ParamValue::Text(t.clone()),
        }
    }

    /// The scalar a shader slot component receives: numbers as is, bools as
    /// 0/1, enums as the option index. Colours and text have none.
    pub fn scalar(&self) -> Option<f32> {
        match self {
            Self::Number(v) => Some(*v),
            Self::Bool(v) => Some(f32::from(u8::from(*v))),
            Self::Enum { index, .. } => Some(*index as f32),
            Self::Color(_) | Self::Text(_) => None,
        }
    }

    fn leaf(&self, integer: bool) -> Value {
        match self {
            Self::Number(v) if integer => Value::Integer(v.round() as i64),
            Self::Number(v) => Value::Float(f64::from(*v)),
            Self::Bool(v) => Value::Boolean(*v),
            Self::Color(c) => Value::String(c.to_hex()),
            Self::Enum { id, .. } => Value::String(id.clone()),
            Self::Text(t) => Value::String(t.clone()),
        }
    }
}

/// Why a value does not fit its param. The settings page localizes these.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamError {
    WrongType(ParamType),
    NotANumber,
    OutOfRange { min: f64, max: f64 },
    BadColor,
    NeedsOpaque,
    UnknownOption,
    TooLong { max: u32, len: usize },
    Empty,
    ControlCharacter,
}

impl std::fmt::Display for ParamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongType(t) => write!(f, "expected a {} value", t.name()),
            Self::NotANumber => f.write_str("not a finite number"),
            Self::OutOfRange { min, max } => write!(f, "must be {min}–{max}"),
            Self::BadColor => f.write_str("not a #rrggbb(aa) colour"),
            Self::NeedsOpaque => f.write_str("must be opaque"),
            Self::UnknownOption => f.write_str("not one of the options"),
            Self::TooLong { max, len } => write!(f, "{len} characters, at most {max}"),
            Self::Empty => f.write_str("must not be empty"),
            Self::ControlCharacter => {
                f.write_str("must not contain line breaks or control characters")
            }
        }
    }
}

/// Grapheme clusters, the unit `max_len` counts.
pub fn text_len(text: &str) -> usize {
    text.graphemes(true).count()
}

impl ParamSpec {
    pub fn label(&self, id: &str, locale: &str) -> String {
        localized(&self.name, locale, id)
    }

    pub fn range(&self) -> (f64, f64) {
        (self.min.unwrap_or(0.0), self.max.unwrap_or(1.0))
    }

    pub fn max_len(&self) -> u32 {
        self.max_len.unwrap_or(MAX_TEXT_GRAPHEMES)
    }

    /// Check `value` against this spec.
    pub fn coerce(&self, value: &ParamValue) -> Result<Resolved, ParamError> {
        match self.kind {
            ParamType::Number => {
                let v = value.as_f64().ok_or(ParamError::WrongType(self.kind))?;
                if !v.is_finite() {
                    return Err(ParamError::NotANumber);
                }
                let (min, max) = self.range();
                let slack = (max - min).abs() * 1e-6;
                if v < min - slack || v > max + slack {
                    return Err(ParamError::OutOfRange { min, max });
                }
                Ok(Resolved::Number(v.clamp(min, max) as f32))
            }
            ParamType::Bool => value
                .as_bool()
                .map(Resolved::Bool)
                .ok_or(ParamError::WrongType(self.kind)),
            ParamType::Color => {
                let text = value.as_str().ok_or(ParamError::WrongType(self.kind))?;
                let color: Rgba8 = text.parse().map_err(|_| ParamError::BadColor)?;
                if color.a != 255 && self.alpha != Some(true) {
                    return Err(ParamError::NeedsOpaque);
                }
                Ok(Resolved::Color(color))
            }
            ParamType::Enum => {
                let id = value.as_str().ok_or(ParamError::WrongType(self.kind))?;
                let index = self
                    .options
                    .iter()
                    .position(|o| o.id == id)
                    .ok_or(ParamError::UnknownOption)?;
                Ok(Resolved::Enum {
                    id: id.to_owned(),
                    index: index as u32,
                })
            }
            ParamType::Text => {
                let text = value.as_str().ok_or(ParamError::WrongType(self.kind))?;
                if text.chars().any(char::is_control) {
                    return Err(ParamError::ControlCharacter);
                }
                let len = text_len(text);
                if len == 0 && self.allow_empty != Some(true) {
                    return Err(ParamError::Empty);
                }
                if len > self.max_len() as usize {
                    return Err(ParamError::TooLong {
                        max: self.max_len(),
                        len,
                    });
                }
                Ok(Resolved::Text(text.to_owned()))
            }
        }
    }

    /// Read what a user typed into this param's field.
    pub fn parse_input(&self, input: &str) -> Result<ParamValue, ParamError> {
        let value = match self.kind {
            ParamType::Number => ParamValue::Number(
                input
                    .trim()
                    .parse::<f64>()
                    .map_err(|_| ParamError::NotANumber)?,
            ),
            ParamType::Bool => ParamValue::Bool(matches!(input.trim(), "true" | "1")),
            ParamType::Color => {
                let color: Rgba8 = input.trim().parse().map_err(|_| ParamError::BadColor)?;
                ParamValue::Text(color.to_hex())
            }
            ParamType::Enum | ParamType::Text => ParamValue::Text(input.to_owned()),
        };
        self.coerce(&value).map(|r| r.to_value())
    }

    fn default_resolved(&self) -> Option<Resolved> {
        self.coerce(self.default.as_ref()?).ok()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Seg {
    Key(String),
    Index(usize),
}

/// A field holding `{ param = "<id>" }`, as a path from the manifest root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamSite {
    path: Vec<Seg>,
    pub param: String,
    /// The field is an integer in the manifest, so numbers are rounded.
    integer: bool,
}

fn path_string(path: &[Seg]) -> String {
    let mut out = String::new();
    for seg in path {
        if !out.is_empty() {
            out.push('.');
        }
        match seg {
            Seg::Key(k) => out.push_str(k),
            Seg::Index(i) => out.push_str(&i.to_string()),
        }
    }
    out
}

/// The schema tables, read from a merged manifest before it becomes a `Skin`.
#[derive(Debug, Default)]
struct Schema {
    params: BTreeMap<String, ParamSpec>,
    groups: BTreeMap<String, ParamGroup>,
    presets: BTreeMap<String, PresetSpec>,
    checks: Vec<ParamCheck>,
}

fn read_schema(merged: &Table, out: &mut Vec<SkinDiagnostic>) -> Option<Schema> {
    fn part<T: serde::de::DeserializeOwned + Default>(
        merged: &Table,
        key: &str,
        out: &mut Vec<SkinDiagnostic>,
    ) -> Option<T> {
        match merged.get(key) {
            None => Some(T::default()),
            Some(v) => match v.clone().try_into() {
                Ok(t) => Some(t),
                Err(e) => {
                    out.push(SkinDiagnostic::error(key, e.to_string().trim().to_owned()));
                    None
                }
            },
        }
    }
    Some(Schema {
        params: part(merged, "params", out)?,
        groups: part(merged, "param_groups", out)?,
        presets: part(merged, "presets", out)?,
        checks: part(merged, "param_checks", out)?,
    })
}

fn validate_schema(s: &Schema, out: &mut Vec<SkinDiagnostic>) {
    let mut err = |field: String, message: String| out.push(SkinDiagnostic::error(field, message));
    if s.params.len() > MAX_PARAMS {
        err("params".into(), format!("at most {MAX_PARAMS} params"));
    }
    if s.groups.len() > MAX_GROUPS {
        err(
            "param_groups".into(),
            format!("at most {MAX_GROUPS} groups"),
        );
    }
    if s.presets.len() > MAX_PRESETS {
        err("presets".into(), format!("at most {MAX_PRESETS} presets"));
    }
    if s.checks.len() > MAX_CHECKS {
        err(
            "param_checks".into(),
            format!("at most {MAX_CHECKS} checks"),
        );
    }
    let bad_id = "id must be 1–32 characters of a-z, 0-9 or `_`";
    for id in s.groups.keys().filter(|id| !valid_param_id(id)) {
        err(format!("param_groups.{id}"), bad_id.into());
    }
    for id in s.presets.keys().filter(|id| !valid_param_id(id)) {
        err(format!("presets.{id}"), bad_id.into());
    }
    for (id, p) in &s.params {
        let field = format!("params.{id}");
        if !valid_param_id(id) {
            err(field.clone(), bad_id.into());
        }
        if let Some(group) = &p.group
            && !s.groups.contains_key(group)
        {
            err(
                format!("{field}.group"),
                format!("group `{group}` is not declared"),
            );
        }
        match p.kind {
            ParamType::Number => {
                let (Some(min), Some(max)) = (p.min, p.max) else {
                    err(field.clone(), "a number param needs `min` and `max`".into());
                    continue;
                };
                if !min.is_finite() || !max.is_finite() || min >= max {
                    err(field.clone(), "`min` must be finite and below `max`".into());
                    continue;
                }
                if let Some(step) = p.step
                    && (!step.is_finite() || step <= 0.0 || step > max - min)
                {
                    err(
                        format!("{field}.step"),
                        "must be above 0 and at most max − min".into(),
                    );
                }
            }
            ParamType::Enum => {
                if p.options.is_empty() || p.options.len() > MAX_OPTIONS {
                    err(
                        format!("{field}.options"),
                        format!("1–{MAX_OPTIONS} options"),
                    );
                }
                for (i, o) in p.options.iter().enumerate() {
                    if !valid_param_id(&o.id) {
                        err(format!("{field}.options.{i}"), bad_id.into());
                    }
                    if p.options[..i].iter().any(|e| e.id == o.id) {
                        err(
                            format!("{field}.options.{i}"),
                            format!("duplicate option `{}`", o.id),
                        );
                    }
                }
            }
            ParamType::Text => {
                if let Some(n) = p.max_len
                    && !(1..=MAX_TEXT_GRAPHEMES).contains(&n)
                {
                    err(
                        format!("{field}.max_len"),
                        format!("must be 1–{MAX_TEXT_GRAPHEMES}"),
                    );
                }
            }
            ParamType::Color | ParamType::Bool => {}
        }
        match &p.default {
            None => err(format!("{field}.default"), "required".into()),
            Some(v) => match p.coerce(v) {
                Err(e) => err(format!("{field}.default"), e.to_string()),
                Ok(Resolved::Number(d)) => {
                    if let (Some(step), Some(min)) = (p.step, p.min) {
                        let k = (f64::from(d) - min) / step;
                        if (k - k.round()).abs() > 1e-4 {
                            err(
                                format!("{field}.default"),
                                "is not on the `step` grid".into(),
                            );
                        }
                    }
                }
                Ok(_) => {}
            },
        }
    }
    for (id, preset) in &s.presets {
        for (key, value) in &preset.values {
            let field = format!("presets.{id}.values.{key}");
            match s.params.get(key) {
                None => err(field, format!("`{key}` is not a declared param")),
                Some(p) => {
                    if let Err(e) = p.coerce(value) {
                        err(field, e.to_string());
                    }
                }
            }
        }
    }
    for (i, c) in s.checks.iter().enumerate() {
        for (side, name) in [("foreground", &c.foreground), ("background", &c.background)] {
            let colour_param = s
                .params
                .get(name)
                .is_some_and(|p| p.kind == ParamType::Color);
            if !colour_param && ColorRole::parse(name).is_none() {
                err(
                    format!("param_checks.{i}.{side}"),
                    format!("`{name}` is neither a colour param nor a colour role"),
                );
            }
        }
        if !(1.0..=21.0).contains(&c.min) {
            err(
                format!("param_checks.{i}.min"),
                "contrast must be 1–21".into(),
            );
        }
    }
}

/// True when `{ param = … }` may not stand at `path`.
fn forbidden(path: &[Seg]) -> bool {
    let key = |i: usize| match path.get(i) {
        Some(Seg::Key(k)) => Some(k.as_str()),
        _ => None,
    };
    if matches!(key(0), Some("fonts" | "sounds" | "pipelines" | "materials")) {
        return true;
    }
    if key(0) == Some("shaders") {
        let allowed = (key(2) == Some("intensity") && path.len() == 3)
            || (key(2) == Some("params") && path.len() == 4);
        return !allowed;
    }
    if key(0) == Some("dialogue") && key(1) == Some("responsive") && key(2) == Some("rules") {
        return true;
    }
    path.iter().any(|seg| {
        matches!(seg, Seg::Key(k) if matches!(k.as_str(), "image" | "file" | "source" | "textures" | "families" | "font_family"))
    })
}

/// The value at `path` in `table`, if every step exists.
fn lookup<'a>(table: &'a Table, path: &[Seg]) -> Option<&'a Value> {
    let (first, rest) = path.split_first()?;
    let Seg::Key(k) = first else { return None };
    let mut value = table.get(k)?;
    for seg in rest {
        value = match (seg, value) {
            (Seg::Key(k), Value::Table(t)) => t.get(k)?,
            (Seg::Index(i), Value::Array(a)) => a.get(*i)?,
            _ => return None,
        };
    }
    Some(value)
}

/// Write `leaf` at `path`, creating missing tables on the way.
fn assign(table: &mut Table, path: &[Seg], leaf: Value) -> bool {
    let Some((Seg::Key(first), rest)) = path.split_first() else {
        return false;
    };
    if rest.is_empty() {
        table.insert(first.clone(), leaf);
        return true;
    }
    let next = table.entry(first.clone()).or_insert_with(|| match rest[0] {
        Seg::Key(_) => Value::Table(Table::new()),
        Seg::Index(_) => Value::Array(Vec::new()),
    });
    assign_value(next, rest, leaf)
}

fn assign_value(value: &mut Value, path: &[Seg], leaf: Value) -> bool {
    match (path.split_first(), value) {
        (None, slot) => {
            *slot = leaf;
            true
        }
        (Some((Seg::Key(_), _)), Value::Table(t)) => assign(t, path, leaf),
        (Some((Seg::Index(i), rest)), Value::Array(a)) => match a.get_mut(*i) {
            Some(item) => assign_value(item, rest, leaf),
            None => false,
        },
        _ => false,
    }
}

fn kind_matches(kind: ParamType, existing: &Value) -> bool {
    match kind {
        ParamType::Number => matches!(existing, Value::Integer(_) | Value::Float(_)),
        ParamType::Bool => matches!(existing, Value::Boolean(_)),
        ParamType::Color | ParamType::Enum | ParamType::Text => {
            matches!(existing, Value::String(_))
        }
    }
}

/// Replace every placeholder in `value` with the param's default, recording
/// where each one stood.
fn walk(
    value: &mut Value,
    path: &mut Vec<Seg>,
    schema: &Schema,
    defaults: &Table,
    sites: &mut Vec<ParamSite>,
    out: &mut Vec<SkinDiagnostic>,
) {
    match value {
        Value::Table(t) if t.contains_key("param") => {
            let field = path_string(path);
            let id = match (t.len(), t.get("param")) {
                (1, Some(Value::String(id))) => id.clone(),
                (1, _) => {
                    out.push(SkinDiagnostic::error(field, "`param` must name a param id"));
                    return;
                }
                _ => {
                    out.push(SkinDiagnostic::error(
                        field,
                        "a `{ param = … }` placeholder must be the whole value, with no other keys",
                    ));
                    return;
                }
            };
            let Some(spec) = schema.params.get(&id) else {
                out.push(SkinDiagnostic::error(
                    field,
                    format!("`{id}` is not a declared param"),
                ));
                return;
            };
            if forbidden(path) {
                out.push(SkinDiagnostic::error(
                    field,
                    "params cannot set files, fonts, sounds, materials, pipelines or shader sources",
                ));
                return;
            }
            let existing = lookup(defaults, path);
            if let Some(existing) = existing
                && !kind_matches(spec.kind, existing)
            {
                out.push(SkinDiagnostic::error(
                    field,
                    format!("a {} param cannot fill this field", spec.kind.name()),
                ));
                return;
            }
            // Whole-number params stay integers where the field's type is
            // unknown (arrays, absent options): floats parse into neither
            // integer fields nor, here, anything else that a whole number
            // would not.
            let whole = spec.kind == ParamType::Number
                && [spec.min, spec.max, Some(spec.step.unwrap_or(1.0))]
                    .iter()
                    .flatten()
                    .all(|v| v.fract() == 0.0);
            let integer = match existing {
                Some(v) => matches!(v, Value::Integer(_)),
                None => whole,
            };
            if integer && spec.kind == ParamType::Number && !whole {
                out.push(SkinDiagnostic::error(
                    field,
                    format!(
                        "this field is a whole number, so `{id}` needs whole min, max and step"
                    ),
                ));
                return;
            }
            let Some(default) = spec.default_resolved() else {
                return; // Reported by validate_schema.
            };
            *value = default.leaf(integer);
            sites.push(ParamSite {
                path: path.clone(),
                param: id,
                integer,
            });
        }
        Value::Table(t) => {
            for (key, v) in t.iter_mut() {
                if path.is_empty() && (key == "meta" || SCHEMA_ROOTS.contains(&key.as_str())) {
                    continue;
                }
                path.push(Seg::Key(key.clone()));
                walk(v, path, schema, defaults, sites, out);
                path.pop();
            }
        }
        Value::Array(a) => {
            for (i, v) in a.iter_mut().enumerate() {
                path.push(Seg::Index(i));
                walk(v, path, schema, defaults, sites, out);
                path.pop();
            }
        }
        _ => {}
    }
}

/// Validate the param schema in `merged` and replace its placeholders with
/// defaults. Run between responsive expansion and deserialization.
pub(crate) fn compile(
    merged: &mut Table,
    defaults: &Table,
    out: &mut Vec<SkinDiagnostic>,
) -> Arc<[ParamSite]> {
    let Some(schema) = read_schema(merged, out) else {
        return Arc::from([]);
    };
    validate_schema(&schema, out);
    let mut sites = Vec::new();
    let mut root = Value::Table(std::mem::take(merged));
    walk(
        &mut root,
        &mut Vec::new(),
        &schema,
        defaults,
        &mut sites,
        out,
    );
    if let Value::Table(t) = root {
        *merged = t;
    }
    sites.into()
}

impl Skin {
    /// Each declared param at its default.
    pub fn param_defaults(&self) -> BTreeMap<String, Resolved> {
        self.params
            .iter()
            .filter_map(|(id, p)| Some((id.clone(), p.default_resolved()?)))
            .collect()
    }

    /// Params bound by any shader slot.
    fn bound_params(&self) -> impl Iterator<Item = &str> {
        self.shaders.values().flat_map(|s| s.bound_params())
    }

    /// `name` as a colour: a colour param's current value or a colour role.
    fn check_color(&self, name: &str, values: &BTreeMap<String, Resolved>) -> Option<Rgba8> {
        match values.get(name) {
            Some(Resolved::Color(c)) => Some(*c),
            Some(_) => None,
            None => ColorRole::parse(name).map(|r| self.color(ColorRef::Role(r))),
        }
    }

    /// Contrast checks that fail for `values`, with the measured ratio.
    pub fn failed_checks(&self, values: &BTreeMap<String, Resolved>) -> Vec<(&ParamCheck, f32)> {
        self.param_checks
            .iter()
            .filter_map(|c| {
                let fg = self.check_color(&c.foreground, values)?;
                let bg = self.check_color(&c.background, values)?;
                let ratio = fg.contrast(bg);
                (ratio + 1e-3 < c.min).then_some((c, ratio))
            })
            .collect()
    }

    /// Presets in display order.
    pub fn ordered_presets(&self) -> Vec<(&str, &PresetSpec)> {
        let mut v: Vec<_> = self.presets.iter().map(|(k, p)| (k.as_str(), p)).collect();
        v.sort_by_key(|(id, p)| (p.order.unwrap_or(i32::MAX), *id));
        v
    }

    /// Groups in display order, each with its params in order. Params without
    /// a group come last under `None`.
    pub fn ordered_params(&self) -> Vec<ParamGroupListing<'_>> {
        let mut groups: Vec<_> = self.param_groups.iter().collect();
        groups.sort_by_key(|(id, g)| (g.order.unwrap_or(i32::MAX), id.as_str()));
        let mut out = Vec::new();
        let sorted = |group: Option<&str>| {
            let mut v: Vec<_> = self
                .params
                .iter()
                .filter(|(_, p)| p.group.as_deref() == group)
                .map(|(k, p)| (k.as_str(), p))
                .collect();
            v.sort_by_key(|(id, p)| (p.order.unwrap_or(i32::MAX), *id));
            v
        };
        for (id, _) in groups {
            let params = sorted(Some(id));
            if !params.is_empty() {
                out.push((Some(id.as_str()), params));
            }
        }
        let rest = sorted(None);
        if !rest.is_empty() {
            out.push((None, rest));
        }
        out
    }

    /// A preset's full value set: its own values over the defaults.
    pub fn preset_values(&self, id: &str) -> Option<BTreeMap<String, Resolved>> {
        let preset = self.presets.get(id)?;
        let mut values = self.param_defaults();
        for (key, value) in &preset.values {
            if let (Some(spec), Some(slot)) = (self.params.get(key), values.get_mut(key))
                && let Ok(v) = spec.coerce(value)
            {
                *slot = v;
            }
        }
        Some(values)
    }
}

/// Coerce stored values against the skin's schema. Unknown ids and values
/// that no longer fit fall back to the default with a warning.
pub fn coerce_values(
    skin: &Skin,
    values: &BTreeMap<String, ParamValue>,
    out: &mut Vec<SkinDiagnostic>,
) -> BTreeMap<String, Resolved> {
    let mut resolved = skin.param_defaults();
    for (id, value) in values {
        let field = format!("params.{id}");
        match (skin.params.get(id), resolved.get_mut(id)) {
            (Some(spec), Some(slot)) => match spec.coerce(value) {
                Ok(v) => *slot = v,
                Err(e) => out.push(SkinDiagnostic::warning(
                    field,
                    format!("saved value is invalid ({e}); using the default"),
                )),
            },
            _ => out.push(SkinDiagnostic::warning(
                field,
                "saved value is for a param this skin no longer has",
            )),
        }
    }
    resolved
}

/// Keep only the entries of `values` that fit `skin`'s schema and differ
/// from the default.
pub fn retain_valid(skin: &Skin, values: &mut BTreeMap<String, ParamValue>) {
    let defaults = skin.param_defaults();
    values.retain(|id, value| {
        skin.params
            .get(id)
            .and_then(|spec| spec.coerce(value).ok())
            .is_some_and(|v| defaults.get(id) != Some(&v))
    });
}

/// `skin` with `values` written into its param sites.
fn apply(skin: &Skin, values: &BTreeMap<String, Resolved>) -> Result<Skin, String> {
    let mut out = skin.clone();
    if !skin.param_sites.is_empty() {
        let Value::Table(mut table) = Value::try_from(skin).map_err(|e| e.to_string())? else {
            unreachable!("a skin serializes to a table")
        };
        for site in skin.param_sites.iter() {
            let Some(value) = values.get(&site.param) else {
                continue;
            };
            if !assign(&mut table, &site.path, value.leaf(site.integer)) {
                return Err(format!(
                    "{}: the field no longer exists",
                    path_string(&site.path)
                ));
            }
        }
        out = Value::Table(table)
            .try_into()
            .map_err(|e: toml::de::Error| e.to_string().trim().to_owned())?;
        out.material_bindings = skin.material_bindings.clone();
        out.material_programs = skin.material_programs.clone();
        out.shader_programs = skin.shader_programs.clone();
        out.param_sites = skin.param_sites.clone();
    }
    out.param_values = Arc::new(values.clone());
    Ok(out)
}

/// `skin` with a user's `values`. Values that do not fit fall back to the
/// default; a combination the skin cannot take leaves `skin` as it was. The
/// shader programs are shared, never recompiled.
pub fn resolve(
    skin: &Skin,
    values: &BTreeMap<String, ParamValue>,
    app_version: &str,
) -> (Skin, Vec<SkinDiagnostic>) {
    let mut diagnostics = Vec::new();
    if skin.params.is_empty() {
        return (skin.clone(), diagnostics);
    }
    let resolved = coerce_values(skin, values, &mut diagnostics);
    if *skin.param_values == resolved {
        return (skin.clone(), diagnostics);
    }
    match apply(skin, &resolved) {
        Ok(out) => {
            let mut errors = Vec::new();
            validate_values(&out, app_version, &mut errors);
            errors.retain(|d| d.severity == crate::Severity::Error);
            if errors.is_empty() {
                return (out, diagnostics);
            }
            diagnostics.extend(errors.into_iter().map(|d| {
                SkinDiagnostic::warning(d.field, format!("{}; params ignored", d.message))
            }));
        }
        Err(e) => diagnostics.push(SkinDiagnostic::warning("params", e)),
    }
    (skin.clone(), diagnostics)
}

/// Load-time checks on a deserialized skin: defaults recorded, unused params
/// reported, and every value a param can take validated where it lands.
pub(crate) fn finish(
    skin: &mut Skin,
    sites: Arc<[ParamSite]>,
    app_version: &str,
    out: &mut Vec<SkinDiagnostic>,
) {
    skin.param_sites = sites;
    skin.param_values = Arc::new(skin.param_defaults());
    if skin.params.is_empty() {
        return;
    }
    let bound: Vec<&str> = skin.bound_params().collect();
    for id in skin.params.keys() {
        if !bound.contains(&id.as_str()) && !skin.param_sites.iter().any(|s| &s.param == id) {
            out.push(SkinDiagnostic::warning(
                format!("params.{id}"),
                "is not used by any field or shader binding",
            ));
        }
    }
    let defaults = skin.param_defaults();
    for (check, ratio) in skin.failed_checks(&defaults) {
        out.push(SkinDiagnostic::warning(
            "param_checks",
            format!(
                "{} on {} is {ratio:.1}:1 at the defaults, below {}",
                check.foreground, check.background, check.min
            ),
        ));
    }
    // Every extreme a param can take, one param at a time, then each preset.
    let mut cases: Vec<(String, BTreeMap<String, Resolved>)> = Vec::new();
    for (id, spec) in &skin.params {
        if !skin.param_sites.iter().any(|s| &s.param == id) {
            continue;
        }
        let candidates: Vec<ParamValue> = match spec.kind {
            ParamType::Number => {
                let (min, max) = spec.range();
                vec![ParamValue::Number(min), ParamValue::Number(max)]
            }
            ParamType::Bool => vec![ParamValue::Bool(false), ParamValue::Bool(true)],
            ParamType::Enum => spec
                .options
                .iter()
                .map(|o| ParamValue::Text(o.id.clone()))
                .collect(),
            ParamType::Text => {
                let mut v = vec![ParamValue::Text("龘".repeat(spec.max_len() as usize))];
                if spec.allow_empty == Some(true) {
                    v.push(ParamValue::Text(String::new()));
                }
                v
            }
            ParamType::Color => Vec::new(), // Every colour fits a colour field.
        };
        for value in candidates {
            if let Ok(r) = spec.coerce(&value) {
                let mut values = defaults.clone();
                values.insert(id.clone(), r);
                cases.push((format!("params.{id} = {}", display(&value)), values));
            }
        }
    }
    for id in skin.presets.keys() {
        if let Some(values) = skin.preset_values(id) {
            for (check, ratio) in skin.failed_checks(&values) {
                out.push(SkinDiagnostic::warning(
                    format!("presets.{id}"),
                    format!(
                        "{} on {} is {ratio:.1}:1, below {}",
                        check.foreground, check.background, check.min
                    ),
                ));
            }
            if !skin.param_sites.is_empty() {
                cases.push((format!("preset `{id}`"), values));
            }
        }
    }
    let files: Vec<String> = skin
        .referenced_files()
        .iter()
        .map(|(_, p, _)| (*p).to_owned())
        .collect();
    for (case, values) in cases {
        let variant = match apply(skin, &values) {
            Ok(v) => v,
            Err(e) => {
                out.push(SkinDiagnostic::error("params", format!("with {case}: {e}")));
                continue;
            }
        };
        let mut errors = Vec::new();
        validate_values(&variant, app_version, &mut errors);
        for d in errors
            .into_iter()
            .filter(|d| d.severity == crate::Severity::Error)
        {
            out.push(SkinDiagnostic::error(
                d.field,
                format!("with {case}: {}", d.message),
            ));
        }
        let variant_files: Vec<&str> = variant
            .referenced_files()
            .iter()
            .map(|(_, p, _)| *p)
            .collect();
        if variant_files != files.iter().map(String::as_str).collect::<Vec<_>>() {
            out.push(SkinDiagnostic::error(
                "params",
                format!("with {case}: params must not change the files a skin uses"),
            ));
        }
    }
}

fn display(value: &ParamValue) -> String {
    match value {
        ParamValue::Bool(b) => b.to_string(),
        ParamValue::Number(n) => n.to_string(),
        ParamValue::Text(t) => format!("\"{t}\""),
    }
}

/// Put the placeholders back into a serialized skin, for export.
pub(crate) fn restore_placeholders(table: &mut Table, sites: &[ParamSite]) {
    for site in sites {
        let mut placeholder = Table::new();
        placeholder.insert("param".into(), Value::String(site.param.clone()));
        assign(table, &site.path, Value::Table(placeholder));
    }
}

#[cfg(test)]
mod tests;
