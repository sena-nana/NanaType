//! Formatting helpers for JSON and TOML package documents.

/// Re-indent JSON with two spaces, keeping key order and every token as written.
pub fn format_json(text: &str) -> Result<String, String> {
    serde_json::from_str::<serde::de::IgnoredAny>(text).map_err(|e| e.to_string())?;
    let mut out = String::with_capacity(text.len());
    let mut depth = 0usize;
    let mut chars = text.chars().peekable();
    let newline = |out: &mut String, depth: usize| {
        out.push('\n');
        out.extend(std::iter::repeat_n("  ", depth));
    };
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                out.push('"');
                let mut escaped = false;
                for c in chars.by_ref() {
                    out.push(c);
                    if escaped {
                        escaped = false;
                    } else if c == '\\' {
                        escaped = true;
                    } else if c == '"' {
                        break;
                    }
                }
            }
            '{' | '[' => {
                out.push(c);
                while chars.peek().is_some_and(|c| c.is_whitespace()) {
                    chars.next();
                }
                if matches!(chars.peek(), Some('}' | ']')) {
                    out.push(chars.next().unwrap_or_default());
                } else {
                    depth += 1;
                    newline(&mut out, depth);
                }
            }
            '}' | ']' => {
                depth = depth.saturating_sub(1);
                newline(&mut out, depth);
                out.push(c);
            }
            ',' => {
                out.push(',');
                newline(&mut out, depth);
            }
            ':' => out.push_str(": "),
            c if c.is_whitespace() => {}
            c => out.push(c),
        }
    }
    out.push('\n');
    Ok(out)
}

/// Normalise spacing around `=` and trailing whitespace while keeping every
/// comment, key order and value representation.
pub fn format_toml(text: &str) -> Result<String, String> {
    let before: toml::Table = toml::from_str(text).map_err(|e| e.message().to_owned())?;
    let mut doc = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| e.message().to_owned())?;
    fn tidy(table: &mut dyn toml_edit::TableLike) {
        for (mut key, item) in table.iter_mut() {
            match item {
                toml_edit::Item::Value(value) => {
                    // Indentation right before a key; comments and blank lines stay.
                    let prefix = key
                        .leaf_decor()
                        .prefix()
                        .and_then(|p| p.as_str())
                        .map(|p| p.trim_end_matches([' ', '\t']).to_owned());
                    key.leaf_decor_mut().set_suffix(" ");
                    if let Some(prefix) = prefix {
                        key.leaf_decor_mut().set_prefix(prefix);
                    }
                    let suffix = value.decor().suffix().and_then(|s| s.as_str()).map(|s| {
                        let s = s.trim_end();
                        if s.trim_start().starts_with('#') {
                            format!(" {}", s.trim_start())
                        } else {
                            String::new()
                        }
                    });
                    value.decor_mut().set_prefix(" ");
                    if let Some(suffix) = suffix {
                        value.decor_mut().set_suffix(suffix);
                    }
                }
                toml_edit::Item::Table(t) => tidy(t),
                toml_edit::Item::ArrayOfTables(a) => a.iter_mut().for_each(|t| tidy(t)),
                toml_edit::Item::None => {}
            }
        }
    }
    tidy(doc.as_table_mut());
    let mut out: String = doc
        .to_string()
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    out = out.trim_end().to_owned();
    out.push('\n');
    let after: toml::Table = toml::from_str(&out).map_err(|e| e.message().to_owned())?;
    if after != before {
        return Err("formatting would change the document".into());
    }
    Ok(out)
}
