//! Package validation and unified Lua error location mapping.
use nanatype_theme::editor::{self, FileInfo, Level, SourceDiagnostic};
use nanatype_theme::{PackageProject, PipelineSpec, Skin, SkinFiles, SkinRegistry};
use std::{collections::HashMap, path::Path, sync::Arc};

/// The result of one background validation: located diagnostics and, for a
/// valid skin, what the preview renders.
#[derive(Clone, Default)]
pub struct Report {
    pub pipelines: Arc<Vec<(String, Result<PipelineSpec, String>)>>,
    pub properties: Arc<nanatype_theme::TomlTable>,
    pub references: Arc<Vec<editor::Reference>>,
    pub inventory: Arc<Vec<FileInfo>>,
    pub diagnostics: Vec<SourceDiagnostic>,
    pub skin: Option<Arc<Skin>>,
    pub files: Option<Arc<dyn SkinFiles>>,
    pub urls: Arc<HashMap<String, String>>,
}

impl Report {
    pub fn errors(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.level == Level::Error)
            .count()
    }
}

/// Map a Lua worker error (`[string "plugin"]:N: ...`) onto `entry`.
/// When `wrapped` is true (live debugger), line 1 is the wrapper header
/// `local plugin=(function()`.
pub(crate) fn lua_error_location(
    entry: &str,
    source: &str,
    message: &str,
    wrapped: bool,
) -> Option<SourceDiagnostic> {
    let (_, rest) = message.split_once("\"plugin\"]:")?;
    let raw_line = rest.split(':').next()?.trim().parse::<usize>().ok()?;
    let line = if wrapped {
        raw_line.checked_sub(1).filter(|l| *l > 0)?
    } else {
        raw_line.max(1)
    };
    let line = line.min(source.split('\n').count().max(1));
    let offset: usize = source
        .split_inclusive('\n')
        .take(line.saturating_sub(1))
        .map(str::len)
        .sum();
    let len = source[offset.min(source.len())..]
        .find('\n')
        .unwrap_or(source.len().saturating_sub(offset.min(source.len())))
        .max(1);
    Some(SourceDiagnostic {
        path: entry.to_owned(),
        offset,
        len,
        line,
        column: 1,
        level: Level::Error,
        message: message.to_owned(),
    })
}

/// Validate exactly like an import: syntax of every local text file, then
/// the dependency closure, signatures, skin and data. Nothing executes.
pub fn validate(registry: &SkinRegistry, project: &PackageProject) -> Report {
    let mut report = Report {
        pipelines: Arc::new(editor::pipelines(project)),
        references: Arc::new(editor::references(project)),
        inventory: Arc::new(editor::inventory(project)),
        ..Report::default()
    };
    let manifest = project.manifest().ok();
    for (path, bytes) in &project.files {
        if path.starts_with("dependencies/") {
            continue;
        }
        if let Ok(text) = std::str::from_utf8(bytes) {
            let format = manifest
                .as_ref()
                .and_then(|m| m.data.values().find(|d| d.path == *path))
                .map(|d| d.format);
            report
                .diagnostics
                .extend(editor::check_source(path, text, format));
        }
    }
    match registry.inspect_project(project) {
        Ok((files, skin)) => {
            if let Some(loaded) = skin {
                report
                    .diagnostics
                    .extend(loaded.warnings.iter().map(|w| editor::locate(project, w)));
                report.urls = Arc::new(nanatype_frame::assets::skin_preview_urls(
                    &loaded.skin,
                    &*files,
                ));
                report.properties = loaded.authored.clone();
                report.skin = Some(loaded.skin);
            }
            report.files = Some(files);
        }
        Err(errors) => {
            if report.diagnostics.iter().all(|d| d.level != Level::Error) {
                report
                    .diagnostics
                    .extend(errors.iter().map(|e| editor::locate(project, e)));
            }
        }
    }
    report
}

/// Import checks plus compiler diagnostics. User code is never evaluated here.
pub fn validate_with_worker(
    registry: &SkinRegistry,
    project: &PackageProject,
    executable: &Path,
) -> Report {
    let mut report = validate(registry, project);
    if let Some(entry) = project.manifest().ok().and_then(|m| m.lua.map(|l| l.entry))
        && let Some(source) = project
            .files
            .get(&entry)
            .and_then(|b| std::str::from_utf8(b).ok())
        && let Err(message) =
            nanatype_plugins::check_source_in_worker(executable, source.to_owned())
    {
        let diagnostic =
            lua_error_location(&entry, source, &message, false).unwrap_or(SourceDiagnostic {
                path: entry,
                offset: 0,
                len: 1,
                line: 1,
                column: 1,
                level: Level::Error,
                message,
            });
        report.diagnostics.push(diagnostic);
    }
    report
}
