//! Known manifest references are rewritten structurally; program text is only disclosed.
use super::*;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reference {
    pub file: String,
    pub path: Vec<String>,
    pub target: String,
    pub automatic: bool,
}
pub fn references(project: &PackageProject) -> Vec<Reference> {
    fn walk(file: &str, value: &PropertyValue, path: &mut Vec<String>, out: &mut Vec<Reference>) {
        match value {
            PropertyValue::Table(t) => {
                for (key, value) in t {
                    path.push(key.clone());
                    walk(file, value, path, out);
                    path.pop();
                }
            }
            PropertyValue::Array(a) => {
                for (i, value) in a.iter().enumerate() {
                    path.push(i.to_string());
                    walk(file, value, path, out);
                    path.pop();
                }
            }
            PropertyValue::String(target) => {
                let field = path
                    .iter()
                    .rev()
                    .find(|s| s.parse::<usize>().is_err())
                    .map(String::as_str)
                    .unwrap_or("");
                let resource = matches!(
                    field,
                    "file" | "image" | "preview" | "path" | "entry" | "textures"
                ) || field == "source"
                    && path
                        .first()
                        .is_some_and(|s| matches!(s.as_str(), "shaders" | "materials"))
                    || path.first().is_some_and(|s| s == "sounds")
                        && (path
                            .get(1)
                            .is_some_and(|s| matches!(s.as_str(), "se" | "open" | "advance"))
                            || field == "files");
                if resource {
                    out.push(Reference {
                        file: file.into(),
                        path: path.clone(),
                        target: target.clone(),
                        automatic: true,
                    });
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for file in [PACKAGE_MANIFEST, MANIFEST, crate::SOUNDS_MANIFEST] {
        if let Some(table) = project
            .files
            .get(file)
            .and_then(|b| std::str::from_utf8(b).ok())
            .and_then(|s| parse_document(s).ok())
        {
            walk(
                file,
                &PropertyValue::Table(table),
                &mut Vec::new(),
                &mut out,
            );
        }
    }
    for (file, bytes) in &project.files {
        if file.starts_with("dependencies/") || !matches!(extension(file).as_str(), "lua" | "wgsl")
        {
            continue;
        }
        if let Ok(source) = std::str::from_utf8(bytes) {
            for target in project
                .files
                .keys()
                .filter(|p| !p.starts_with("dependencies/") && source.contains(p.as_str()))
            {
                out.push(Reference {
                    file: file.clone(),
                    path: Vec::new(),
                    target: target.clone(),
                    automatic: false,
                });
            }
        }
    }
    out
}
pub fn rename_asset(project: &mut PackageProject, from: &str, to: &str) -> Result<(), FileError> {
    let to = clean_path(to)?;
    if [
        PACKAGE_MANIFEST,
        MANIFEST,
        crate::SOUNDS_MANIFEST,
        INTEGRITY_FILE,
    ]
    .contains(&from)
        || from.starts_with("dependencies/")
        || to.starts_with("dependencies/")
        || verify_integrity(project)?.is_some()
    {
        return Err(error(
            "file inventory is protected; derive an unsigned copy first",
        ));
    }
    if project.files.contains_key(&to) {
        return Err(error("destination already exists"));
    }
    let refs = references(project)
        .into_iter()
        .filter(|r| r.target == from)
        .collect::<Vec<_>>();
    let mut next = project.clone();
    for r in refs.iter().filter(|r| r.automatic) {
        if !next.can_edit(&r.file) {
            return Err(error(format!("reference is protected: {}", r.file)));
        }
        set_declaration(
            &mut next,
            &r.file,
            &r.path,
            Some(&literal(&PropertyValue::String(to.clone()))),
            None,
            false,
        )?;
    }
    let bytes = next
        .files
        .remove(from)
        .ok_or_else(|| error("missing source file"))?;
    let (folder, name) = to.rsplit_once('/').unwrap_or(("", to.as_str()));
    let mut probe = PackageProject::default();
    import_asset(&mut probe, folder, name, bytes.clone())?;
    next.set(&to, bytes)?;
    *project = next;
    Ok(())
}
pub fn replace_asset(
    project: &mut PackageProject,
    path: &str,
    bytes: Vec<u8>,
) -> Result<(), FileError> {
    if !project.files.contains_key(path) {
        return Err(error("missing target file"));
    }
    let (folder, name) = path.rsplit_once('/').unwrap_or(("", path));
    let mut probe = PackageProject::default();
    import_asset(&mut probe, folder, name, bytes.clone())?;
    project.set(path, bytes)
}
pub fn create_file(project: &mut PackageProject, path: &str) -> Result<(), FileError> {
    let path = clean_path(path)?;
    if project.files.contains_key(&path) {
        return Err(error("file already exists"));
    }
    let source = match extension(&path).as_str() {
        "lua" => LUA_TEMPLATE,
        "json" => "{}\n",
        "toml" | "txt" | "md" | "csv" => "",
        "wgsl" => "// WGSL source\n",
        _ => {
            return Err(error(
                "new files must be Lua, WGSL, TOML, JSON, text, Markdown or CSV",
            ));
        }
    };
    if path.starts_with("dependencies/")
        || [PACKAGE_MANIFEST, INTEGRITY_FILE].contains(&path.as_str())
    {
        return Err(error("reserved package path"));
    }
    project.set(&path, source.as_bytes().to_vec())
}
