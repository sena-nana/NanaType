//! Local declaration patches preserve comments and sparse inheritance.
use super::*;
use crate::{DataSpec, PipelineSpec, clean_path};
use std::collections::BTreeSet;
pub use toml::Value as PropertyValue;
use toml_edit::{Item, Value};
pub fn parse_document(text: &str) -> Result<toml::Table, FileError> {
    toml::from_str(text).map_err(error)
}
pub fn skin_properties(skin: &crate::Skin) -> Result<toml::Table, FileError> {
    toml::Value::try_from(skin)
        .map_err(error)?
        .try_into()
        .map_err(error)
}
pub fn value_at<'a>(root: &'a toml::Table, path: &[String]) -> Option<&'a toml::Value> {
    let (first, rest) = path.split_first()?;
    let mut value = root.get(first)?;
    for key in rest {
        value = match value {
            toml::Value::Table(t) => t.get(key)?,
            toml::Value::Array(a) => a.get(key.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(value)
}
fn bindings(value: &PropertyValue, out: &mut BTreeMap<String, usize>) {
    match value {
        PropertyValue::Table(t)
            if t.len() == 1 && t.get("param").and_then(PropertyValue::as_str).is_some() =>
        {
            *out.entry(t["param"].as_str().unwrap().into()).or_default() += 1;
        }
        PropertyValue::Table(t) => {
            for v in t.values() {
                bindings(v, out)
            }
        }
        PropertyValue::Array(a) => {
            for v in a {
                bindings(v, out)
            }
        }
        _ => {}
    }
}
pub fn bound_at(root: &toml::Table, path: &[String]) -> bool {
    (1..=path.len()).any(|n| {
        value_at(root, &path[..n])
            .and_then(PropertyValue::as_table)
            .is_some_and(|t| {
                t.len() == 1 && t.get("param").and_then(PropertyValue::as_str).is_some()
            })
    }) || value_at(root, path).is_some_and(|v| {
        let mut found = BTreeMap::new();
        bindings(v, &mut found);
        !found.is_empty()
    })
}
pub fn literal(value: &toml::Value) -> String {
    edit_value(value).to_string()
}
fn edit_value(v: &toml::Value) -> Value {
    match v {
        toml::Value::String(v) => v.as_str().into(),
        toml::Value::Integer(v) => (*v).into(),
        toml::Value::Float(v) => (*v).into(),
        toml::Value::Boolean(v) => (*v).into(),
        toml::Value::Datetime(v) => v.to_string().parse().expect("valid datetime"),
        toml::Value::Array(v) => {
            let mut a = toml_edit::Array::new();
            for v in v {
                a.push(edit_value(v));
            }
            Value::Array(a)
        }
        toml::Value::Table(v) => {
            let mut t = toml_edit::InlineTable::new();
            for (k, v) in v {
                t.insert(k, edit_value(v));
            }
            Value::InlineTable(t)
        }
    }
}
pub fn set_declaration(
    project: &mut PackageProject,
    file: &str,
    path: &[String],
    raw: Option<&str>,
    inherited: Option<&toml::Table>,
    unbind: bool,
) -> Result<(), FileError> {
    if path.is_empty() || path.len() > 32 {
        return Err(error("select a declaration field"));
    }
    if file == PACKAGE_MANIFEST
        && matches!(
            path[0].as_str(),
            "id" | "format" | "schema" | "derived_from"
        )
    {
        return Err(error("package identity must be changed by deriving a copy"));
    }
    let text = project.text(file)?;
    let local = parse_document(text)?;
    if let Some(raw) = raw
        && !unbind
        && (bound_at(&local, path) || inherited.is_some_and(|t| bound_at(t, path)))
    {
        let before = value_at(&local, path).or_else(|| inherited.and_then(|t| value_at(t, path)));
        let parsed = parse_document(&format!("value={raw}"))?;
        let mut old = BTreeMap::new();
        let mut new = BTreeMap::new();
        if let Some(v) = before {
            bindings(v, &mut old);
        }
        if let Some(v) = parsed.get("value") {
            bindings(v, &mut new);
        }
        if old.is_empty() || old != new {
            return Err(error(
                "explicitly unlink the parameter before replacing a bound value",
            ));
        }
    }
    let value = raw
        .map(|raw| {
            format!("value = {raw}\n")
                .parse::<toml_edit::DocumentMut>()
                .map_err(error)
                .and_then(|d| {
                    if d.as_table().len() != 1 || !d["value"].is_value() {
                        return Err(error("enter exactly one TOML value"));
                    }
                    Ok(d["value"].clone())
                })
        })
        .transpose()?;
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(error)?;
    patch(doc.as_item_mut(), path, value, inherited, &mut Vec::new())?;
    let output = doc.to_string();
    if file == PACKAGE_MANIFEST {
        PackageManifest::parse(&output)?;
    }
    project.set(file, output.into_bytes())
}
fn seed(inherited: Option<&toml::Table>, path: &[String]) -> Option<Item> {
    Some(Item::Value(edit_value(value_at(inherited?, path)?)))
}
fn patch(
    item: &mut Item,
    keys: &[String],
    replacement: Option<Item>,
    inherited: Option<&toml::Table>,
    prefix: &mut Vec<String>,
) -> Result<(), FileError> {
    if keys.is_empty() {
        if let Some(mut next) = replacement {
            if let (Some(a), Some(b)) = (item.as_value(), next.as_value_mut()) {
                *b.decor_mut() = a.decor().clone();
            }
            *item = next;
        } else {
            *item = Item::None;
        }
        return Ok(());
    }
    if let Some(value) = item.as_value_mut() {
        return patch_value(value, keys, replacement, inherited, prefix);
    }
    if let Some(array) = item.as_array_of_tables_mut() {
        let i = keys[0].parse::<usize>().map_err(error)?;
        if keys.len() == 1 {
            if replacement.is_none() {
                if i >= array.len() {
                    return Err(error("array index is out of bounds"));
                }
                array.remove(i);
                return Ok(());
            }
            return Err(error("edit table array entries through their fields"));
        }
        let table = array
            .get_mut(i)
            .ok_or_else(|| error("array index is out of bounds"))?;
        prefix.push(keys[0].clone());
        let result = patch_table(table, &keys[1..], replacement, inherited, prefix);
        prefix.pop();
        return result;
    }
    if item.is_none() {
        *item = Item::Table(toml_edit::Table::new());
    }
    let table = item
        .as_table_mut()
        .ok_or_else(|| error("field parent is not a table"))?;
    patch_table(table, keys, replacement, inherited, prefix)
}
fn patch_table(
    table: &mut toml_edit::Table,
    keys: &[String],
    replacement: Option<Item>,
    inherited: Option<&toml::Table>,
    prefix: &mut Vec<String>,
) -> Result<(), FileError> {
    let key = &keys[0];
    prefix.push(key.clone());
    if keys.len() == 1 && replacement.is_none() {
        table.remove(key);
        prefix.pop();
        return Ok(());
    }
    if !table.contains_key(key) && keys.len() > 1 {
        let value = seed(inherited, prefix)
            .filter(|i| i.is_array() || i.is_array_of_tables())
            .unwrap_or_else(|| Item::Table(toml_edit::Table::new()));
        table.insert(key, value);
    }
    let result = patch(
        table.entry(key).or_insert(Item::None),
        &keys[1..],
        replacement,
        inherited,
        prefix,
    );
    prefix.pop();
    result
}
fn patch_value(
    value: &mut Value,
    keys: &[String],
    replacement: Option<Item>,
    inherited: Option<&toml::Table>,
    prefix: &mut Vec<String>,
) -> Result<(), FileError> {
    if keys.is_empty() {
        let mut next = replacement
            .ok_or_else(|| error("remove the containing field instead"))?
            .into_value()
            .map_err(|_| error("expected value"))?;
        *next.decor_mut() = value.decor().clone();
        *value = next;
        return Ok(());
    }
    let key = &keys[0];
    prefix.push(key.clone());
    let result = match value {
        Value::Array(array) => {
            let i = key.parse::<usize>().map_err(error)?;
            if i == array.len() && keys.len() == 1 {
                if let Some(next) = replacement {
                    array.push(
                        next.into_value()
                            .map_err(|_| error("expected array value"))?,
                    );
                }
                Ok(())
            } else if keys.len() == 1 && replacement.is_none() {
                if i >= array.len() {
                    return Err(error("array index is out of bounds"));
                }
                array.remove(i);
                Ok(())
            } else {
                patch_value(
                    array
                        .get_mut(i)
                        .ok_or_else(|| error("array index is out of bounds"))?,
                    &keys[1..],
                    replacement,
                    inherited,
                    prefix,
                )
            }
        }
        Value::InlineTable(table) => {
            if keys.len() == 1 && replacement.is_none() {
                table.remove(key);
                Ok(())
            } else {
                if !table.contains_key(key) {
                    table.insert(
                        key,
                        seed(inherited, prefix)
                            .and_then(|i| i.into_value().ok())
                            .unwrap_or_else(|| Value::InlineTable(toml_edit::InlineTable::new())),
                    );
                }
                patch_value(
                    table.get_mut(key).unwrap(),
                    &keys[1..],
                    replacement,
                    inherited,
                    prefix,
                )
            }
        }
        _ => Err(error("field parent is not a container")),
    };
    prefix.pop();
    result
}

/// Byte range of the source block that declares `path`: its `[a.b]` header
/// (or the n-th `[[a.b]]`) through its sub-tables, up to the next table.
pub fn table_range(text: &str, path: &[String]) -> Option<(usize, usize)> {
    enum Target<'a> {
        Table(&'a toml_edit::Table),
        Item(&'a toml_edit::Item),
    }

    fn lookup<'a>(root: &'a toml_edit::Table, path: &[String]) -> Option<&'a toml_edit::Table> {
        let mut cur = Target::Item(root.get(path.first()?)?);
        for key in &path[1..] {
            cur = match cur {
                Target::Item(toml_edit::Item::Table(t)) | Target::Table(t) => {
                    Target::Item(t.get(key)?)
                }
                Target::Item(toml_edit::Item::ArrayOfTables(a)) => {
                    let i = key.parse::<usize>().ok()?;
                    Target::Table(a.get(i)?)
                }
                Target::Item(_) => return None,
            };
        }
        match cur {
            Target::Table(t) => Some(t),
            Target::Item(toml_edit::Item::Table(t)) => Some(t),
            Target::Item(_) => None,
        }
    }

    fn collect_starts(item: &toml_edit::Item, skip: *const toml_edit::Table, out: &mut Vec<usize>) {
        match item {
            toml_edit::Item::Table(t) => {
                if std::ptr::eq(t, skip) {
                    return;
                }
                if !t.is_implicit()
                    && !t.is_dotted()
                    && let Some(span) = t.span()
                {
                    out.push(span.start);
                }
                for (_, child) in t.iter() {
                    collect_starts(child, skip, out);
                }
            }
            toml_edit::Item::ArrayOfTables(a) => {
                for t in a.iter() {
                    if std::ptr::eq(t, skip) {
                        continue;
                    }
                    if let Some(span) = t.span() {
                        out.push(span.start);
                    }
                    for (_, child) in t.iter() {
                        collect_starts(child, skip, out);
                    }
                }
            }
            _ => {}
        }
    }

    if path.is_empty() {
        return None;
    }
    let doc = toml_edit::Document::parse(text).ok()?;
    let target = lookup(doc.as_table(), path)?;
    if target.is_implicit() || target.is_dotted() {
        return None;
    }
    let begin = target.span()?.start;
    let mut others = Vec::new();
    for (_, child) in doc.as_table().iter() {
        collect_starts(child, target as *const toml_edit::Table, &mut others);
    }
    let end = others
        .into_iter()
        .filter(|&start| start > begin)
        .min()
        .unwrap_or(text.len());
    Some((begin, end))
}

/// Structural operation on a `[...]` dialogue/layout `decor` list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecorListOp {
    Add,
    Duplicate(usize),
    Remove(usize),
    MoveUp(usize),
    MoveDown(usize),
}

/// Apply `op` to `<target>.decor` in `skin.toml`, returning `Ok(true)` if the
/// document changed or `Ok(false)` when an index was out of range.
pub fn apply_decor_list(
    project: &mut PackageProject,
    target: &[String],
    inherited: Option<&toml::Table>,
    fallback_size: [f32; 2],
    op: DecorListOp,
) -> Result<bool, FileError> {
    let mut path = target.to_vec();
    path.push("decor".into());
    let local = project
        .text(MANIFEST)
        .ok()
        .and_then(|t| parse_document(t).ok());
    let mut values = local
        .as_ref()
        .and_then(|t| value_at(t, &path))
        .or_else(|| inherited.and_then(|t| value_at(t, &path)))
        .and_then(PropertyValue::as_array)
        .cloned()
        .unwrap_or_else(|| {
            let props = inherited
                .map(|p| super::authored_layout(p, fallback_size))
                .or_else(|| {
                    local
                        .as_ref()
                        .map(|p| super::authored_layout(p, fallback_size))
                });
            props
                .as_ref()
                .and_then(|p| value_at(p, &["dialogue".into(), "decor".into()]))
                .and_then(PropertyValue::as_array)
                .cloned()
                .unwrap_or_default()
        });
    match op {
        DecorListOp::Add => {
            let mut entry = toml::Table::new();
            entry.insert(
                "size".into(),
                PropertyValue::Array(vec![PropertyValue::Integer(48), PropertyValue::Integer(48)]),
            );
            entry.insert("color".into(), PropertyValue::String("#85a8ff".into()));
            entry.insert("radius".into(), PropertyValue::Integer(8));
            values.push(PropertyValue::Table(entry));
        }
        DecorListOp::Duplicate(i) if i < values.len() => values.insert(i + 1, values[i].clone()),
        DecorListOp::Remove(i) if i < values.len() => {
            values.remove(i);
        }
        DecorListOp::MoveUp(i) if i + 1 < values.len() => values.swap(i, i + 1),
        DecorListOp::MoveDown(i) if i > 0 && i < values.len() => values.swap(i, i - 1),
        _ => return Ok(false),
    }
    set_declaration(
        project,
        MANIFEST,
        &path,
        Some(&literal(&PropertyValue::Array(values))),
        inherited,
        true,
    )?;
    Ok(true)
}

/// Every declared pipeline in the skin, parsed one by one so a malformed
/// declaration shows its error instead of hiding the others.
pub fn pipelines(project: &PackageProject) -> Vec<(String, Result<PipelineSpec, String>)> {
    let Some(table) = project
        .text(MANIFEST)
        .ok()
        .and_then(|t| toml::from_str::<toml::Table>(t).ok())
    else {
        return Vec::new();
    };
    table
        .get("pipelines")
        .and_then(|p| p.as_table())
        .into_iter()
        .flatten()
        .map(|(name, spec)| {
            let spec = spec
                .clone()
                .try_into::<PipelineSpec>()
                .map_err(|e| e.message().trim().to_owned());
            (name.clone(), spec)
        })
        .collect()
}

/// Set `key` to `value`, keeping the comment and spacing the old value had.
fn set_kept(table: &mut toml_edit::Table, key: &str, mut value: toml_edit::Value) {
    if let Some(old) = table.get(key).and_then(toml_edit::Item::as_value) {
        *value.decor_mut() = old.decor().clone();
    }
    table[key] = toml_edit::Item::Value(value);
}

/// The `[<parent>.<name>]` tables: `previous` is renamed to (or replaced by)
/// `name`, keeping the comments inside it, or removed when `name` is `None`.
/// Returns the table to fill in.
fn entry_table<'a>(
    doc: &'a mut toml_edit::DocumentMut,
    parent: &str,
    previous: Option<&str>,
    name: Option<&str>,
) -> Result<Option<&'a mut toml_edit::Table>, FileError> {
    if !doc.contains_key(parent) {
        let mut table = toml_edit::Table::new();
        table.set_implicit(true);
        doc[parent] = toml_edit::Item::Table(table);
    }
    let parent_table = doc[parent]
        .as_table_mut()
        .ok_or_else(|| error(format!("`{parent}` must be a table")))?;
    let mut kept = previous.and_then(|p| parent_table.remove(p));
    let Some(name) = name else {
        if kept.is_none() {
            return Err(error(format!(
                "{parent}.{} does not exist",
                previous.unwrap_or("")
            )));
        }
        if parent_table.is_empty() {
            doc.remove(parent);
        }
        return Ok(None);
    };
    let parent_table = doc[parent]
        .as_table_mut()
        .ok_or_else(|| error(format!("`{parent}` must be a table")))?;
    if previous != Some(name) && parent_table.contains_key(name) {
        return Err(error(format!("{parent}.{name} already exists")));
    }
    let item = match kept.take() {
        Some(toml_edit::Item::Table(t)) => toml_edit::Item::Table(t),
        // An inline declaration becomes a table of its own.
        Some(toml_edit::Item::Value(toml_edit::Value::InlineTable(t))) => {
            toml_edit::Item::Table(t.into_table())
        }
        _ => toml_edit::Item::Table(toml_edit::Table::new()),
    };
    parent_table.insert(name, item);
    Ok(parent_table
        .get_mut(name)
        .and_then(toml_edit::Item::as_table_mut))
}

fn json_to_toml(value: &serde_json::Value) -> Option<toml_edit::Value> {
    Some(match value {
        serde_json::Value::Bool(b) => (*b).into(),
        serde_json::Value::Number(n) => match n.as_i64() {
            Some(i) => i.into(),
            None => n.as_f64()?.into(),
        },
        serde_json::Value::String(s) => s.as_str().into(),
        serde_json::Value::Array(items) => {
            toml_edit::Value::Array(items.iter().map(json_to_toml).collect::<Option<_>>()?)
        }
        serde_json::Value::Object(map) => {
            let mut table = toml_edit::InlineTable::new();
            for (k, v) in map {
                table.insert(k, json_to_toml(v)?);
            }
            toml_edit::Value::InlineTable(table)
        }
        serde_json::Value::Null => return None,
    })
}

/// Add, change, rename or (with `entry` = `None`) remove a `[data.<id>]`
/// declaration in `package.toml`, keeping comments and every other key. A
/// rename follows into `[lua] reads`. The project is unchanged unless the
/// result parses strictly and the declared file is part of the package.
pub fn apply_data(
    project: &mut PackageProject,
    previous: Option<&str>,
    entry: Option<(&str, &DataSpec)>,
) -> Result<(), FileError> {
    let mut doc = manifest_doc(project)?;
    if let Some((id, spec)) = entry {
        spec.validate(id).map_err(error)?;
        if spec.path.starts_with("dependencies/")
            || [PACKAGE_MANIFEST, INTEGRITY_FILE].contains(&spec.path.as_str())
            || !project.files.contains_key(&clean_path(&spec.path)?)
        {
            return Err(error(format!(
                "data.{id}: {} is not a file of this package",
                spec.path
            )));
        }
    }
    let name = entry.map(|(id, _)| id);
    if let Some(table) = entry_table(&mut doc, "data", previous, name)? {
        let (_, spec) = entry.expect("a table is only returned for an entry");
        set_kept(table, "path", spec.path.as_str().into());
        set_kept(table, "format", spec.format.name().into());
        set_kept(
            table,
            "format_version",
            i64::from(spec.format_version).into(),
        );
        set_kept(table, "source", spec.source.as_str().into());
        set_kept(table, "license", spec.license.as_str().into());
        match &spec.constraints {
            Some(c) => {
                let value = serde_json::to_value(c)
                    .ok()
                    .and_then(|v| json_to_toml(&v))
                    .ok_or_else(|| error("invalid constraints"))?;
                set_kept(table, "constraints", value);
            }
            None => {
                table.remove("constraints");
            }
        }
    }
    if let (Some(previous), Some(name)) = (previous, name)
        && previous != name
        && let Some(reads) = doc
            .get_mut("lua")
            .and_then(|l| l.get_mut("reads"))
            .and_then(toml_edit::Item::as_array_mut)
    {
        for read in reads.iter_mut() {
            if read.as_str() == Some(previous) {
                let decor = read.decor().clone();
                *read = name.into();
                *read.decor_mut() = decor;
            }
        }
    }
    write_manifest(project, &doc)
}

/// Pipeline errors of a `skin.toml` text, checked against the registered
/// processors with the skin's own materials (inherited ones are not seen).
fn pipeline_errors(text: &str) -> Result<BTreeSet<(String, String)>, FileError> {
    let table: toml::Table = toml::from_str(text).map_err(|e| error(e.message().trim()))?;
    let pipelines = match table.get("pipelines") {
        Some(p) => p
            .clone()
            .try_into()
            .map_err(|e: toml::de::Error| error(format!("pipelines: {}", e.message().trim())))?,
        None => BTreeMap::new(),
    };
    let skin = crate::Skin {
        pipelines,
        materials: table
            .get("materials")
            .cloned()
            .and_then(|m| m.try_into().ok())
            .unwrap_or_default(),
        ..Default::default()
    };
    Ok(crate::pipeline::validate(&skin, crate::PROCESSORS)
        .into_iter()
        .filter(|d| d.severity == crate::Severity::Error)
        .map(|d| (d.field, d.message))
        .collect())
}

/// Add, change, rename or (with `entry` = `None`) remove a
/// `[pipelines.<name>]` declaration in `skin.toml`, keeping comments and
/// every other key. The project is unchanged when the edit adds a pipeline
/// error (unknown processor, unbound or mistyped input, cycle, a material
/// losing its source) or names data this package does not declare.
pub fn apply_pipeline(
    project: &mut PackageProject,
    previous: Option<&str>,
    entry: Option<(&str, &PipelineSpec)>,
) -> Result<(), FileError> {
    let before = project.text(MANIFEST)?.to_owned();
    let mut doc = before.parse::<toml_edit::DocumentMut>().map_err(error)?;
    if let Some((name, spec)) = entry {
        let declared = project.manifest().map(|m| m.data).unwrap_or_default();
        let signature = crate::processor(&spec.processor, spec.version);
        for (input, source) in &spec.inputs {
            let Some(reference) = &source.data else {
                continue;
            };
            let (package, id) = crate::data::split_reference(reference).map_err(error)?;
            let expected = signature
                .and_then(|s| s.inputs.iter().find(|(n, _)| n == input))
                .map(|(_, t)| *t);
            match (package, declared.get(id), expected) {
                (Some(_), _, _) => {}
                (None, None, _) => {
                    return Err(error(format!(
                        "pipelines.{name}.inputs.{input}: undeclared data `{id}`"
                    )));
                }
                (None, Some(data), Some(crate::PortType::Data { format, version }))
                    if (data.format, data.format_version) != (format, version) =>
                {
                    return Err(error(format!(
                        "pipelines.{name}.inputs.{input}: `{id}` is {} v{}, expected {} v{version}",
                        data.format.name(),
                        data.format_version,
                        format.name()
                    )));
                }
                _ => {}
            }
        }
    }
    if let Some(table) = entry_table(&mut doc, "pipelines", previous, entry.map(|(n, _)| n))? {
        let (_, spec) = entry.expect("a table is only returned for an entry");
        set_kept(table, "processor", spec.processor.as_str().into());
        set_kept(table, "version", i64::from(spec.version).into());
        let mut inputs = toml_edit::InlineTable::new();
        for (input, source) in &spec.inputs {
            let mut port = toml_edit::InlineTable::new();
            for (key, value) in [
                ("data", &source.data),
                ("pipeline", &source.pipeline),
                ("output", &source.output),
            ] {
                if let Some(value) = value {
                    port.insert(key, value.as_str().into());
                }
            }
            inputs.insert(input, toml_edit::Value::InlineTable(port));
        }
        let mut params = toml_edit::InlineTable::new();
        for (param, value) in &spec.params {
            params.insert(param, f64::from(*value).into());
        }
        for (key, value) in [("inputs", inputs), ("params", params)] {
            if value.is_empty() {
                table.remove(key);
            } else {
                set_kept(table, key, toml_edit::Value::InlineTable(value));
            }
        }
    }
    let text = doc.to_string();
    let old = pipeline_errors(&before).unwrap_or_default();
    let added: Vec<String> = pipeline_errors(&text)?
        .difference(&old)
        .map(|(field, message)| format!("{field}: {message}"))
        .collect();
    if !added.is_empty() {
        return Err(error(added.join("; ")));
    }
    let mut next = project.clone();
    next.set(MANIFEST, text.into_bytes())?;
    *project = next;
    Ok(())
}
