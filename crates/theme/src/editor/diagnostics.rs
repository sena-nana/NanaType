//! Source diagnostics, key-span placement, and text search.

use super::inventory::extension;
use crate::{
    DataFormat, DiagnosticTarget, PACKAGE_MANIFEST, PackageManifest, PackageProject,
    SkinDiagnostic, load::MANIFEST,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Error,
    Warning,
}

/// A problem located in a file: byte range plus 1-based line and column.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceDiagnostic {
    pub path: String,
    pub offset: usize,
    pub len: usize,
    pub line: usize,
    pub column: usize,
    pub level: Level,
    pub message: String,
}

/// 1-based line and character column of a byte offset.
pub fn line_column(text: &str, offset: usize) -> (usize, usize) {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    let before = &text[..offset];
    let line = before.matches('\n').count() + 1;
    let column = before
        .rsplit_once('\n')
        .map_or(before, |(_, l)| l)
        .chars()
        .count()
        + 1;
    (line, column)
}

fn located(
    path: &str,
    text: &str,
    offset: usize,
    len: usize,
    level: Level,
    message: String,
) -> SourceDiagnostic {
    let (line, column) = line_column(text, offset);
    SourceDiagnostic {
        path: path.into(),
        offset,
        len,
        line,
        column,
        level,
        message,
    }
}

/// Fast per-keystroke checks for one file: TOML and JSON syntax, and the
/// strict `package.toml` schema. Declared data overrides the filename.
pub fn check_source(path: &str, text: &str, data: Option<DataFormat>) -> Vec<SourceDiagnostic> {
    let ext = data.map_or_else(|| extension(path), |f| f.name().to_owned());
    let mut out = Vec::new();
    if ext == "toml" {
        if let Err(e) = text.parse::<toml_edit::Document<String>>() {
            let span = e.span().unwrap_or(0..0);
            out.push(located(
                path,
                text,
                span.start,
                span.len().max(1),
                Level::Error,
                e.message().trim().to_owned(),
            ));
        } else if path == PACKAGE_MANIFEST
            && let Err(e) = PackageManifest::parse(text)
        {
            let message = e.to_string();
            let offset = message
                .split('`')
                .nth(1)
                .and_then(|key| key_span(text, &[key]))
                .map_or(0, |s| s.start);
            out.push(located(path, text, offset, 1, Level::Error, message));
        }
    } else if ext == "json"
        && let Err(e) = serde_json::from_str::<serde::de::IgnoredAny>(text)
    {
        let offset = text
            .split_inclusive('\n')
            .take(e.line().saturating_sub(1))
            .map(str::len)
            .sum::<usize>()
            + e.column().saturating_sub(1);
        out.push(located(path, text, offset, 1, Level::Error, e.to_string()));
    }
    out
}

/// Byte span of the deepest existing key along `keys` in a TOML document.
fn key_span(text: &str, keys: &[&str]) -> Option<std::ops::Range<usize>> {
    let doc = text.parse::<toml_edit::Document<String>>().ok()?;
    let mut item = doc.as_item();
    let mut span = None;
    for key in keys {
        let Some((k, next)) = item.as_table_like().and_then(|t| t.get_key_value(key)) else {
            break;
        };
        span = k.span().or(next.span()).or(span);
        item = next;
    }
    span
}

fn line_col_offset(text: &str, line: usize, column: usize) -> usize {
    if line == 0 || line > text.lines().count().max(1) {
        return 0;
    }
    text.split_inclusive('\n')
        .take(line - 1)
        .map(str::len)
        .sum::<usize>()
        + column.saturating_sub(1)
}

fn project_path(path: &str) -> String {
    crate::registry::bundled_path(path).unwrap_or_else(|_| path.to_owned())
}

/// Place a validation result in the file and key it is about using structured
/// [`DiagnosticTarget`] metadata when present.
pub fn locate(project: &PackageProject, diagnostic: &SkinDiagnostic) -> SourceDiagnostic {
    let level = match diagnostic.severity {
        crate::Severity::Error => Level::Error,
        crate::Severity::Warning => Level::Warning,
    };
    let field = diagnostic.field.as_str();
    let message = |path: &str| {
        if field.is_empty() || field == path {
            diagnostic.message.clone()
        } else {
            format!("{field}: {}", diagnostic.message)
        }
    };
    if let Some(target) = &diagnostic.target {
        match target {
            DiagnosticTarget::Source { file, line, column } => {
                let path = project_path(file);
                let text = project.text(&path).unwrap_or("");
                let offset = line_col_offset(text, *line, *column);
                return located(&path, text, offset, 1, level, message(&path));
            }
            DiagnosticTarget::Manifest { file, keys } => {
                let path = project_path(file);
                let text = project.text(&path).unwrap_or("");
                if path == PACKAGE_MANIFEST
                    && field == PACKAGE_MANIFEST
                    && let [k0, data_path] = keys.as_slice()
                    && k0 == "data"
                    && let Ok(manifest) = project.manifest()
                    && let Some((id, _)) = manifest
                        .data
                        .into_iter()
                        .find(|(_, d)| d.path == *data_path)
                {
                    let key_refs = ["data", id.as_str()];
                    let span = key_span(text, &key_refs).unwrap_or(0..0);
                    return located(
                        &path,
                        text,
                        span.start,
                        span.len().max(1),
                        level,
                        format!("data.{id}: {}", diagnostic.message),
                    );
                }
                let key_refs: Vec<&str> = keys.iter().map(String::as_str).collect();
                let span = key_span(text, &key_refs).unwrap_or(0..0);
                return located(
                    &path,
                    text,
                    span.start,
                    span.len().max(1),
                    level,
                    message(&path),
                );
            }
        }
    }
    // Fallback for synthetically constructed diagnostics without a target.
    let (path, keys): (&str, Vec<&str>) = if let Some(rest) = field.strip_prefix("data.") {
        (
            PACKAGE_MANIFEST,
            vec!["data", rest.rsplit('/').next().unwrap_or(rest)],
        )
    } else if let Some(rest) = field.strip_prefix("lua.") {
        (PACKAGE_MANIFEST, vec!["lua", rest])
    } else if let Some(rest) = field.strip_prefix("meta.") {
        (PACKAGE_MANIFEST, rest.split('.').collect())
    } else if project.files.contains_key(field) {
        (field, Vec::new())
    } else {
        (
            MANIFEST,
            field
                .split(['.', '['])
                .map(|k| k.trim_end_matches(']'))
                .filter(|k| !k.is_empty())
                .collect(),
        )
    };
    let text = project.text(path).unwrap_or("");
    let span = key_span(text, &keys).unwrap_or(0..0);
    located(
        path,
        text,
        span.start,
        span.len().max(1),
        level,
        message(path),
    )
}

pub fn find_matches(text: &str, needle: &str) -> Vec<(usize, usize)> {
    if needle.is_empty() {
        return Vec::new();
    }
    let mut lower = String::new();
    let mut offsets = Vec::new();
    for (start, c) in text.char_indices() {
        let end = start + c.len_utf8();
        for c in c.to_lowercase() {
            let before = lower.len();
            lower.push(c);
            offsets.extend(std::iter::repeat_n((start, end), lower.len() - before));
        }
    }
    let needle = needle.to_lowercase();
    let mut result = Vec::new();
    for (i, m) in lower.match_indices(&needle).take(10_000) {
        let (start, _) = offsets[i];
        let (_, end) = offsets[i + m.len() - 1];
        if result.last() != Some(&(start, end - start)) {
            result.push((start, end - start));
        }
    }
    result
}
