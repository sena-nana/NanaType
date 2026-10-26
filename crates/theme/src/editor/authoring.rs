//! Local declaration patches preserve comments and sparse inheritance.
use super::*;
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
    let text = String::from_utf8(project.read(file)?).map_err(error)?;
    let local = parse_document(&text)?;
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
