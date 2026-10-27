//! Version steps. A file without `schema` is v0: the flat settings written by
//! the first NanaType build (`%APPDATA%/NanaType/settings.json`).

use serde_json::{Map, Value, json};

use crate::schema::SETTINGS_SCHEMA;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrateError {
    NotAnObject,
    /// Written by a newer build; loading it would drop its fields' meaning.
    TooNew(u32),
}

/// Bring a parsed settings value up to [`SETTINGS_SCHEMA`]. Returns the
/// version it started from.
pub fn migrate(value: &mut Value) -> Result<u32, MigrateError> {
    let obj = value.as_object_mut().ok_or(MigrateError::NotAnObject)?;
    let from = obj
        .get("schema")
        .and_then(Value::as_u64)
        .map(|v| v as u32)
        .unwrap_or(0);
    if from > SETTINGS_SCHEMA {
        return Err(MigrateError::TooNew(from));
    }
    if from == 0 {
        *obj = v0_to_v1(obj);
    }
    if from < 2 {
        if let Some(integrations) = obj.get_mut("integrations").and_then(Value::as_object_mut)
            && let Some(Value::Object(mut legacy)) = integrations.remove("blivechat")
        {
            let room_kind = legacy
                .remove("room_kind")
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_else(|| "auth_code".into());
            let room = legacy
                .remove("room")
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default();
            let mut chat = Map::new();
            for key in ["panel", "panel_style", "quote_on_display", "panel_open"] {
                if let Some(value) = legacy.remove(key) {
                    chat.insert(key.into(), value);
                }
            }
            integrations
                .entry("chat")
                .or_insert_with(|| Value::Object(chat));
            let mut blive = Map::new();
            blive.insert(
                "enabled".into(),
                legacy.remove("enabled").unwrap_or(json!(false)),
            );
            blive.insert("source".into(), json!("server"));
            blive.insert(
                "room_kind".into(),
                json!(if room_kind == "room_id" {
                    "room_id"
                } else {
                    "auth_code"
                }),
            );
            if room_kind == "room_id" {
                blive.insert("room_id".into(), room.into());
            }
            if let Some(host) = legacy.remove("host") {
                blive.insert("host".into(), host);
            }
            if let Some(port) = legacy.remove("port") {
                blive.insert("port".into(), port);
            }
            integrations
                .entry("blivechat")
                .or_insert(Value::Object(blive));
        }
        obj.insert("schema".into(), json!(2));
    }
    if from < 3 {
        // The Enter / Ctrl+Enter choice became the input window's own keys.
        let send_key = obj
            .get_mut("general")
            .and_then(Value::as_object_mut)
            .and_then(|g| g.remove("send_key"));
        if send_key.as_ref().and_then(Value::as_str) == Some("ctrl_enter") {
            let hotkeys = obj
                .entry("hotkeys")
                .or_insert_with(|| json!({}))
                .as_object_mut();
            if let Some(hotkeys) = hotkeys {
                let input = hotkeys.entry("input").or_insert_with(|| json!({}));
                if let Some(input) = input.as_object_mut() {
                    input.insert("send".into(), json!({ "ctrl": true, "key": "Enter" }));
                    input.insert("newline".into(), json!({ "key": "Enter" }));
                }
            }
        }
        obj.insert("schema".into(), json!(3));
    }
    if from < 4 {
        if let Some(integrations) = obj.get_mut("integrations").and_then(Value::as_object_mut) {
            let mut chat = integrations
                .remove("chat")
                .and_then(|v| v.as_object().cloned())
                .unwrap_or_default();
            if let Some(Value::Object(mut old)) = integrations.remove("bilibili") {
                for key in ["panel", "panel_style", "quote_on_display", "panel_open"] {
                    if let Some(value) = old.remove(key) {
                        chat.entry(key).or_insert(value);
                    }
                }
            }
            integrations.insert("chat".into(), Value::Object(chat));
        }
        obj.insert("schema".into(), json!(4));
    }
    Ok(from)
}

fn take(obj: &mut Map<String, Value>, key: &str) -> Option<Value> {
    obj.remove(key)
}

fn v0_to_v1(old: &mut Map<String, Value>) -> Map<String, Value> {
    let mut general = Map::new();
    let mut playback = Map::new();
    let mut sound = Map::new();
    let mut skin = Map::new();
    let mut hotkeys = Map::new();
    let mut output = json!({ "display": {}, "history": {} });
    let mut overlays = json!({ "input": {}, "display": {}, "history": {} });
    let mut assets = Map::new();

    if let Some(Value::String(theme)) = take(old, "theme") {
        let id = match theme.to_ascii_lowercase().as_str() {
            "night" => "nanatype.night",
            "parchment" => "nanatype.parchment",
            _ => "nanatype.halo",
        };
        skin.insert("id".into(), id.into());
    }
    if let Some(v) = take(old, "speaker_name") {
        general.insert("speaker_name".into(), v);
    }
    let fade_base = take(old, "fade_base_ms")
        .and_then(|v| v.as_u64())
        .unwrap_or(1200);
    let fade_per = take(old, "fade_per_char_ms")
        .and_then(|v| v.as_u64())
        .unwrap_or(40);
    if let Some(Value::String(after)) = take(old, "after_line") {
        let after = if after.eq_ignore_ascii_case("fade") {
            json!({ "kind": "fade", "delay_ms": fade_base, "per_unit_ms": fade_per })
        } else {
            json!({ "kind": "hold" })
        };
        playback.insert("after".into(), after);
    }
    if let Some(ms) = take(old, "speed_ms").and_then(|v| v.as_f64()) {
        playback.insert("speed_scale".into(), json!((ms / 50.0).clamp(0.1, 5.0)));
    }
    if let Some(v) = take(old, "simulate_typing") {
        playback.insert("typing".into(), v);
    }
    if let Some(Value::Bool(false)) = take(old, "simulate_pop") {
        playback.insert("entrance".into(), "none".into());
    }
    if let Some(Value::Bool(true)) = take(old, "per_character") {
        general.insert("input_mode".into(), "mirror".into());
    }
    if let Some(v) = take(old, "typewriter_sound") {
        sound.insert("typing".into(), v);
    }
    if let Some(Value::String(p)) = take(old, "input_presence") {
        let p = if p == "Hotkey" { "summon" } else { "resident" };
        general.insert("input_presence".into(), p.into());
    }
    let hotkey = |v: Value| -> Value {
        // v0 used `win`; the alias on `meta` accepts it.
        v
    };
    if let Some(v) = take(old, "summon_hotkey") {
        hotkeys.insert("summon".into(), hotkey(v));
    }
    if let Some(v) = take(old, "continue_hotkey") {
        // F10 globally blocks menu activation in other apps; keep it only when
        // the user chose something else.
        let is_f10 = v.get("key").and_then(Value::as_str) == Some("F10")
            && !["ctrl", "shift", "alt", "win"]
                .iter()
                .any(|m| v.get(m).and_then(Value::as_bool) == Some(true));
        if !is_f10 {
            hotkeys.insert("continue".into(), hotkey(v));
        }
    }
    take(old, "layout_adjust");
    for (from, to) in [
        ("show_input", "input"),
        ("show_typewriter", "display"),
        ("show_history", "history"),
    ] {
        if let Some(v) = take(old, from) {
            overlays[to]["visible"] = v;
        }
    }
    for (from, to, key) in [
        ("spout_typewriter", "display", "enabled"),
        ("spout_history", "history", "enabled"),
        ("spout_name_typewriter", "display", "name"),
        ("spout_name_history", "history", "name"),
    ] {
        if let Some(v) = take(old, from) {
            output[to][key] = v;
        }
    }
    let size = |old: &mut Map<String, Value>, w: &str, h: &str| -> Option<Value> {
        let w = take(old, w)?.as_f64()?;
        let h = take(old, h)?.as_f64()?;
        Some(json!([w, h]))
    };
    if let Some(s) = size(old, "history_width", "history_height") {
        overlays["history"]["size"] = s;
    }
    // Input and typewriter sizes followed the theme; the skin owns them now.
    for key in [
        "input_width",
        "input_height",
        "typewriter_width",
        "typewriter_height",
        "geometry",
    ] {
        old.remove(key);
    }
    if let Some(Value::String(dir)) = take(old, "sticker_dir")
        && !dir.is_empty()
    {
        assets.insert("sticker_dirs".into(), json!([dir]));
    }

    let mut out = Map::new();
    out.insert("schema".into(), SETTINGS_SCHEMA.into());
    out.insert("general".into(), general.into());
    out.insert("playback".into(), playback.into());
    out.insert("sound".into(), sound.into());
    out.insert("skin".into(), skin.into());
    out.insert("hotkeys".into(), hotkeys.into());
    out.insert("output".into(), output);
    out.insert("overlays".into(), overlays);
    out.insert("assets".into(), assets.into());
    // Anything left was unknown to v0 as well; keep it.
    for (k, v) in std::mem::take(old) {
        out.entry(k).or_insert(v);
    }
    out
}
