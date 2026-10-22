use std::fs;

use nanatype_model::AfterLine;

use crate::*;

#[test]
fn old_enabled_ids_do_not_create_trust_and_revoked_approvals_are_removed() {
    let mut settings: Settings =
        serde_json::from_str(r#"{"enabled_plugins":["example.plugin"]}"#).unwrap();
    settings.sanitize();
    assert!(settings.plugin_approvals.is_empty());
    settings.plugin_approvals.insert(
        "example.plugin".into(),
        PluginApproval {
            digest: "a".repeat(64),
            trusted_source: false,
            policy: PLUGIN_CONSENT_POLICY,
        },
    );
    settings.sanitize();
    assert!(settings.plugin_approvals.is_empty());
    settings.plugin_approvals.insert(
        "example.plugin".into(),
        PluginApproval {
            digest: "a".repeat(64),
            trusted_source: true,
            policy: PLUGIN_CONSENT_POLICY - 1,
        },
    );
    settings.sanitize();
    assert!(
        settings.plugin_approvals.is_empty(),
        "consent under an older policy does not cover newer script capabilities"
    );
    settings.plugin_approvals.insert(
        "example.plugin".into(),
        PluginApproval {
            digest: "a".repeat(64),
            trusted_source: true,
            policy: PLUGIN_CONSENT_POLICY,
        },
    );
    settings.sanitize();
    assert_eq!(settings.plugin_approvals.len(), 1);
    settings.enabled_plugins.clear();
    settings.sanitize();
    assert!(settings.plugin_approvals.is_empty());
}

#[test]
fn telegram_download_preferences_are_backward_compatible_and_contain_no_login_secrets() {
    let mut settings: Settings =
        serde_json::from_str(r#"{"assets":{"sticker_dirs":["stickers/cats"]}}"#).unwrap();
    assert_eq!(settings.assets.telegram.api_id, 0);
    assert!(settings.assets.telegram.proxy.is_empty());
    settings.assets.telegram.api_id = 12345;
    settings.assets.telegram.proxy = " socks5://127.0.0.1:7890 ".into();
    settings.sanitize();
    let value = serde_json::to_value(&settings).unwrap();
    let preferences = value["assets"]["telegram"].as_object().unwrap();
    assert_eq!(preferences.len(), 2);
    assert_eq!(preferences["api_id"], 12345);
    assert_eq!(preferences["proxy"], "socks5://127.0.0.1:7890");
    let restored: Settings = serde_json::from_value(value).unwrap();
    assert_eq!(restored.assets, settings.assets);
}

#[test]
fn overlays_default_to_interactive_and_preserve_explicit_locks() {
    for s in [
        Settings::default(),
        serde_json::from_str::<Settings>(r#"{"schema":2,"overlays":{"display":{}}}"#).unwrap(),
    ] {
        assert_eq!(s.overlays.display.lock, Lock::Interactive);
        assert_eq!(s.overlays.peer_display.lock, Lock::Interactive);
        assert_eq!(s.overlays.history.lock, Lock::Interactive);
    }
    let s: Settings =
        serde_json::from_str(r#"{"schema":2,"overlays":{"display":{"lock":"locked"}}}"#).unwrap();
    assert_eq!(s.overlays.display.lock, Lock::Locked);
}

#[test]
fn retired_layout_mode_loads_as_interactive_and_saves_without_the_old_mode() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");
    for schema in [1, SETTINGS_SCHEMA] {
        let mut overlays = serde_json::Map::new();
        for key in ["input", "display", "peer_display", "history"] {
            overlays.insert(
                key.into(),
                serde_json::json!({
                    "lock": "edit_layout", "size": [900.0, 300.0], "visible": false
                }),
            );
        }
        fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({
                "schema": schema, "overlays": overlays, "general": {"speaker_name": "Nana"}
            }))
            .unwrap(),
        )
        .unwrap();
        let s = load_settings(&path, None).value;
        assert_eq!(s.general.speaker_name, "Nana");
        for o in [
            &s.overlays.input,
            &s.overlays.display,
            &s.overlays.peer_display,
            &s.overlays.history,
        ] {
            assert_eq!(o.lock, Lock::Interactive);
            assert_eq!(o.size, Some([900.0, 300.0]));
            assert!(!o.visible);
        }
        save_json(&path, &s).unwrap();
        let saved: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        for key in ["input", "display", "peer_display", "history"] {
            assert_eq!(saved["overlays"][key]["lock"], "interactive");
        }
    }
}

#[test]
fn legacy_layout_adjust_migrates_to_interactive() {
    let mut value = serde_json::json!({"layout_adjust": true});
    migrate(&mut value).unwrap();
    assert!(value.get("layout_adjust").is_none());
    let s: Settings = serde_json::from_value(value).unwrap();
    for o in [&s.overlays.input, &s.overlays.display, &s.overlays.history] {
        assert_eq!(o.lock, Lock::Interactive);
    }
}

#[test]
fn douyin_defaults_off_and_never_serializes_session_credentials() {
    let mut settings: Settings =
        serde_json::from_str(r#"{"schema":2,"integrations":{"bilibili":{"room":"42"}}}"#).unwrap();
    assert!(!settings.integrations.douyin.enabled);
    settings.sanitize();
    assert_eq!(settings.integrations.bilibili.room, "42");
    let value = serde_json::to_value(settings).unwrap();
    assert!(value["integrations"]["douyin"].get("cookie").is_none());
}

#[test]
fn retired_douyin_browser_settings_load_as_native_without_losing_room_or_other_settings() {
    for backend in ["auto", "browser", "native", "unknown"] {
        let value = serde_json::json!({"schema":2,"integrations":{
            "douyin":{"enabled":true,"room":" 694562812381 ","backend":backend,"browser":"C:/old/edge.exe"},
            "bilibili":{"room":"42"}
        }});
        let mut settings: Settings = serde_json::from_value(value).unwrap();
        settings.sanitize();
        assert!(settings.integrations.douyin.enabled);
        assert_eq!(settings.integrations.douyin.room, "694562812381");
        assert_eq!(settings.integrations.bilibili.room, "42");
        let saved = serde_json::to_value(settings).unwrap();
        assert_eq!(
            saved["integrations"]["douyin"],
            serde_json::json!({"enabled":true,"room":"694562812381"})
        );
    }
}

const V0: &str = r#"{
  "theme": "Night",
  "speaker_name": "奈奈",
  "after_line": "Fade",
  "fade_base_ms": 1500,
  "fade_per_char_ms": 30,
  "speed_ms": 25,
  "simulate_typing": true,
  "simulate_pop": false,
  "per_character": true,
  "typewriter_sound": true,
  "input_presence": "Hotkey",
  "summon_hotkey": { "ctrl": false, "shift": true, "alt": false, "win": false, "key": "F8" },
  "continue_hotkey": { "ctrl": false, "shift": false, "alt": false, "win": false, "key": "F10" },
  "layout_adjust": false,
  "show_input": true,
  "show_typewriter": true,
  "show_history": false,
  "spout_typewriter": true,
  "spout_history": false,
  "spout_name_typewriter": "My/Typewriter",
  "spout_name_history": "NanaType/History",
  "input_width": 760, "input_height": 304,
  "typewriter_width": 760, "typewriter_height": 200,
  "history_width": 400, "history_height": 600,
  "sticker_dir": "D:/stickers",
  "geometry": { "input": { "65537": { "x": 1, "y": 2, "width": 3, "height": 4 } } }
}"#;

#[test]
fn input_skin_preview_defaults_to_plain_text_and_survives_restart() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("settings.json");
    let old: Settings =
        serde_json::from_str(r#"{"schema":2,"general":{"speaker_name":"奈奈"}}"#).unwrap();
    assert!(!old.general.input_skin_preview);
    let mut settings = old;
    for preview in [true, false] {
        settings.general.input_skin_preview = preview;
        save_json(&path, &settings).unwrap();
        let restored = load_settings(&path, None);
        assert_eq!(restored.value, settings);
    }
}

#[test]
fn legacy_chat_preferences_migrate_without_service_or_auth_code() {
    for (kind, room, expected) in [("room_id", "123", "123"), ("auth_code", "secret-code", "")] {
        let mut value = serde_json::json!({"schema":1,"integrations":{"blivechat":{"enabled":true,"room_kind":kind,"room":room,"host":"127.0.0.1","port":12450,"panel_style":"history","panel_open":true,"panel":{"gift":false}}}});
        assert_eq!(migrate(&mut value).unwrap(), 1);
        let settings: Settings = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(settings.schema, 2);
        assert_eq!(settings.integrations.bilibili.room, expected);
        assert_eq!(
            settings.integrations.bilibili.panel_style,
            ChatPanelStyle::History
        );
        assert!(settings.integrations.bilibili.panel_open);
        assert!(!settings.integrations.bilibili.panel.gift);
        assert!(!settings.integrations.bilibili.sync_profile);
        assert!(value["integrations"].get("blivechat").is_none());
        assert!(value["integrations"]["bilibili"].get("host").is_none());
        assert!(
            !serde_json::to_string(&value)
                .unwrap()
                .contains("secret-code")
        );
    }
}

#[test]
fn chat_panel_style_defaults_and_round_trips_without_a_schema_change() {
    let old: Settings =
        serde_json::from_str(r#"{"schema":1,"integrations":{"bilibili":{"panel_open":true}}}"#)
            .unwrap();
    assert_eq!(
        old.integrations.bilibili.panel_style,
        ChatPanelStyle::Compact
    );
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");
    for (style, wire) in [
        (ChatPanelStyle::Compact, "compact"),
        (ChatPanelStyle::History, "history"),
    ] {
        let mut settings = old.clone();
        settings.integrations.bilibili.panel_style = style;
        save_json(&path, &settings).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(json["integrations"]["bilibili"]["panel_style"], wire);
        let loaded = load_settings(&path, None).value;
        assert_eq!(loaded.integrations.bilibili.panel_style, style);
        assert!(loaded.integrations.bilibili.panel_open);
        assert_eq!(loaded.schema, SETTINGS_SCHEMA);
    }
}

#[test]
fn v0_settings_migrate_into_the_new_schema() {
    let tmp = tempfile::tempdir().unwrap();
    let legacy = tmp.path().join("legacy.json");
    fs::write(&legacy, V0).unwrap();
    let path = tmp.path().join("new/settings.json");
    let loaded = load_settings(&path, Some(&legacy));
    assert!(matches!(loaded.note, LoadNote::Migrated { from: 0 }));
    let s = loaded.value;
    assert_eq!(s.schema, SETTINGS_SCHEMA);
    assert_eq!(s.skin.id, "nanatype.night");
    assert_eq!(s.general.speaker_name, "奈奈");
    assert_eq!(s.general.input_mode, InputMode::Mirror);
    assert_eq!(s.general.input_presence, InputPresence::Summon);
    assert_eq!(
        s.playback.after,
        AfterLine::Fade {
            delay_ms: 1500,
            per_unit_ms: 30
        }
    );
    assert_eq!(s.playback.speed_scale, 0.5);
    assert_eq!(
        s.playback.entrance,
        Some(nanatype_model::EntranceKind::None)
    );
    assert!(s.sound.typing);
    assert!(
        !s.sound.keystroke,
        "keystroke clicks stay off until enabled"
    );
    assert_eq!(s.sound.keystroke_volume, 0.6);
    assert_eq!(s.sound.keystroke_scheme, KeystrokeScheme::Mechanical);
    assert!(s.hotkeys.summon.as_ref().unwrap().shift);
    assert_eq!(
        s.hotkeys.continue_,
        HotkeySettings::default().continue_,
        "F10 moves to F9"
    );
    assert!(!s.overlays.history.visible);
    assert!(s.output.display.enabled);
    assert_eq!(s.output.display.name, "My/Typewriter");
    assert_eq!(s.overlays.history.size, Some([400.0, 600.0]));
    assert_eq!(s.assets.sticker_dirs, vec!["D:/stickers".to_string()]);
    assert!(s.unknown.is_empty(), "{:?}", s.unknown);
}

#[test]
fn unknown_fields_survive_a_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");
    fs::write(
        &path,
        r#"{ "schema": 1, "future_feature": { "x": 1 }, "general": { "speaker_name": "a", "later": true } }"#,
    )
    .unwrap();
    let loaded = load_settings(&path, None);
    assert!(matches!(loaded.note, LoadNote::Migrated { .. }));
    assert_eq!(loaded.value.general.speaker_name, "a");
    assert!(loaded.value.unknown.contains_key("future_feature"));
    save_json(&path, &loaded.value).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("future_feature"));
}

#[test]
fn missing_fields_take_defaults_instead_of_resetting_everything() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");
    fs::write(
        &path,
        r#"{ "schema": 1, "skin": { "id": "nanatype.rpg" } }"#,
    )
    .unwrap();
    let s = load_settings(&path, None).value;
    assert_eq!(s.skin.id, "nanatype.rpg");
    assert_eq!(s.history.max_items, 200);
}

#[test]
fn files_from_before_bilibili_get_its_defaults() {
    let json = r#"{"schema": 1, "integrations": {"osc": {"enabled": true}}}"#;
    let s: Settings = serde_json::from_str(json).unwrap();
    let b = &s.integrations.bilibili;
    assert!(b.enabled);
    assert!(!b.sync_profile);
    assert!(b.room.is_empty());
    assert!(b.quote_on_display && b.panel.text && b.panel.gift);
    assert!(
        !s.history.chat.enabled,
        "chat stays out of history until asked"
    );
    assert_eq!(
        s.history.chat.filter.min_gift_coin, 1,
        "no free gifts in history"
    );
    assert!(s.history.show_quote);
}

#[test]
fn bilibili_values_are_clamped() {
    let mut s = Settings::default();
    let b = &mut s.integrations.bilibili;

    b.room = " 123 ".into();
    assert!(s.sanitize());
    let b = &s.integrations.bilibili;
    assert!(!b.sync_profile);
    assert_eq!(b.room, "123");
}

#[test]
fn corrupt_files_are_quarantined_not_overwritten() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");
    fs::write(&path, "{ not json").unwrap();
    let loaded = load_settings(&path, None);
    match loaded.note {
        LoadNote::Quarantined { moved_to, .. } => {
            assert!(moved_to.exists());
            assert_eq!(fs::read_to_string(moved_to).unwrap(), "{ not json");
        }
        other => panic!("{other:?}"),
    }
    assert!(!path.exists());
}

#[test]
fn newer_schemas_load_read_only() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");
    fs::write(
        &path,
        r#"{ "schema": 7, "general": { "speaker_name": "z" } }"#,
    )
    .unwrap();
    let loaded = load_settings(&path, None);
    assert!(matches!(loaded.note, LoadNote::TooNew { schema: 7 }));
    assert_eq!(loaded.value.general.speaker_name, "z");
}

#[test]
fn atomic_writes_keep_a_backup() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");
    save_json(&path, &Settings::default()).unwrap();
    let mut s = Settings::default();
    s.general.speaker_name = "b".into();
    save_json(&path, &s).unwrap();
    assert!(path.with_extension("json.bak").exists());
    assert_eq!(load_settings(&path, None).value.general.speaker_name, "b");
}

#[test]
fn sanitize_clamps_hand_edited_values() {
    let mut s = Settings::default();
    s.playback.speed_scale = f32::NAN;
    s.sound.master = 7.0;
    s.history.visible_items = 99_999;
    assert!(s.sanitize());
    assert_eq!(s.playback.speed_scale, 1.0);
    assert_eq!(s.sound.master, 1.0);
    assert_eq!(s.history.visible_items, s.history.max_items);
    assert!(!s.sanitize());
}

#[test]
fn hotkey_labels_and_global_safety() {
    let h = Hotkey {
        ctrl: true,
        shift: true,
        ..Hotkey::bare("KeyN")
    };
    assert_eq!(h.label("Win"), "Ctrl+Shift+N");
    assert!(h.is_safe_global());
    assert!(Hotkey::bare("F9").is_safe_global());
    assert!(!Hotkey::bare("Space").is_safe_global());
    assert!(!Hotkey::bare("KeyA").is_safe_global());
}

fn monitor(key: &str, x: f64, primary: bool) -> MonitorInfo {
    MonitorInfo {
        key: MonitorKey(key.into()),
        work_area: [x, 0.0, 1920.0, 1040.0],
        primary,
    }
}

#[test]
fn sidebar_geometry_is_backward_compatible_and_survives_a_disk_roundtrip() {
    let old: GeometryStore = serde_json::from_str(r#"{"profiles":{}}"#).unwrap();
    assert!(old.input_dock.is_none());
    assert_eq!(
        input_sidebar_width(old.input_sidebar_width),
        INPUT_SIDEBAR_DEFAULT
    );
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("geometry.json");
    let mut geometry = old;
    let monitors = [monitor("A", 0.0, true)];
    let frame = Placement {
        x: 10.0,
        y: 20.0,
        w: 920.0,
        h: 480.0,
    };
    geometry.remember(&monitors, "input", frame);
    geometry.input_sidebar_width = Some(420.0);
    save_json(&path, &geometry).unwrap();
    let restored: GeometryStore = load_json(&path).value;
    assert_eq!(restored.restore(&monitors, "input"), Some(frame));
    assert_eq!(input_sidebar_width(restored.input_sidebar_width), 420.0);
    assert_eq!(input_sidebar_width(Some(f32::NAN)), INPUT_SIDEBAR_DEFAULT);
    assert_eq!(
        input_sidebar_width(Some(f32::INFINITY)),
        INPUT_SIDEBAR_DEFAULT
    );
    assert_eq!(input_sidebar_width(Some(-5.0)), INPUT_SIDEBAR_MIN);
    assert_eq!(input_sidebar_width(Some(5000.0)), INPUT_SIDEBAR_MAX);
}

#[test]
fn input_dock_regions_and_active_tab_survive_disk_roundtrip() {
    let layout = InputDockLayout {
        regions: vec![
            InputDockRegion {
                id: 9,
                kind: InputDockRegionKind::Stickers,
                number: 3,
            },
            InputDockRegion {
                id: 2,
                kind: InputDockRegionKind::Phrases,
                number: 1,
            },
            InputDockRegion {
                id: 0,
                kind: InputDockRegionKind::Commands,
                number: 1,
            },
        ],
        selected: 2,
    };
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("geometry.json");
    let monitors = [monitor("A", 0.0, true)];
    let frame = Placement {
        x: 10.0,
        y: 20.0,
        w: 920.0,
        h: 480.0,
    };
    let mut geometry = GeometryStore {
        input_sidebar_width: Some(185.0),
        input_dock: Some(layout.clone()),
        ..Default::default()
    };
    geometry.remember(&monitors, "input", frame);
    save_json(&path, &geometry).unwrap();
    let restored: GeometryStore = load_json(&path).value;
    assert_eq!(restored.input_sidebar_width, Some(185.0));
    assert_eq!(restored.restore(&monitors, "input"), Some(frame));
    assert_eq!(restored.input_dock.unwrap().normalized(), layout);
}

#[test]
fn input_dock_repairs_unsupported_regions_duplicates_and_missing_selection() {
    let layout: InputDockLayout = serde_json::from_str(
        r#"{
        "regions": [
            {"id":5,"kind":"phrases","number":0},
            {"id":5,"kind":"commands","number":1},
            {"id":9,"kind":"future_region","number":1},
            {"id":0,"kind":"commands","number":1}
        ],
        "selected":9
    }"#,
    )
    .unwrap();
    let restored = layout.normalized();
    assert_eq!(
        restored.regions.iter().map(|r| r.id).collect::<Vec<_>>(),
        vec![5, 0]
    );
    assert_eq!(restored.regions[0].kind, InputDockRegionKind::Phrases);
    assert_eq!(restored.regions[0].number, 1);
    assert_eq!(restored.selected, 5);
    let empty = InputDockLayout {
        regions: vec![],
        selected: 99,
    };
    assert_eq!(empty.normalized(), InputDockLayout::default());
}

#[test]
fn geometry_restores_per_layout_and_falls_back_to_a_connected_monitor() {
    let desk = [monitor("A", 0.0, true), monitor("B", 1920.0, false)];
    let mut store = GeometryStore::default();
    let rect = Placement {
        x: 1920.0 + 960.0,
        y: 520.0,
        w: 760.0,
        h: 200.0,
    };
    store.remember(&desk, "display", rect);
    assert_eq!(store.restore(&desk, "display"), Some(rect));

    // Laptop alone: B is gone, so there is nothing to restore.
    let laptop = [monitor("A", 0.0, true)];
    assert_eq!(store.restore(&laptop, "display"), None);

    // B reconnected on the left: same relative spot on B.
    let moved = [monitor("B", -1920.0, false), monitor("A", 0.0, true)];
    let p = store.restore(&moved, "display").unwrap();
    assert_eq!(p.x, -1920.0 + 960.0);

    // A frame partly off its monitor is pulled back inside.
    let mut store = GeometryStore::default();
    store.remember(
        &laptop,
        "history",
        Placement {
            x: 1800.0,
            y: 900.0,
            w: 400.0,
            h: 600.0,
        },
    );
    let p = store.restore(&laptop, "history").unwrap();
    assert!(p.x + p.w <= 1920.0 && p.y + p.h <= 1040.0);
}

#[test]
fn only_a_new_installation_enables_chat_history() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");
    assert!(load_settings(&path, None).value.history.chat.enabled);
    for json in [
        r#"{"schema":1}"#,
        r#"{"schema":1,"history":{"chat":{"enabled":false}}}"#,
        "broken json",
    ] {
        fs::write(&path, json).unwrap();
        assert!(!load_settings(&path, None).value.history.chat.enabled);
    }
    fs::write(&path, r#"{"schema":1,"history":{"chat":{"enabled":true}}}"#).unwrap();
    assert!(load_settings(&path, None).value.history.chat.enabled);
}

#[test]
fn old_settings_preserve_local_output_and_add_disabled_partner_output() {
    let s: Settings = serde_json::from_str(
        r#"{
        "schema":1,
        "output":{"display":{"enabled":true,"name":"My Display","size":[1280,720]}},
        "overlays":{"display":{"size":[900,300]}}
    }"#,
    )
    .unwrap();
    assert!(s.output.display.enabled);
    assert_eq!(s.output.display.name, "My Display");
    assert_eq!(s.output.display.size, Some([1280, 720]));
    assert_eq!(s.overlays.display.size, Some([900.0, 300.0]));
    assert!(!s.output.peer_display.enabled);
    assert_eq!(s.output.peer_display.name, "NanaType Partner");
    assert_eq!(s.overlays.peer_display.size, None);
}

#[test]
fn partner_settings_round_trip_and_sanitize() {
    let mut s = Settings::default();
    s.output.peer_display.enabled = true;
    s.output.peer_display.name = "  Partner  ".into();
    s.output.peer_display.size = Some([0, 99999]);
    s.overlays.peer_display.size = Some([0.0, 99999.0]);
    s.overlays.peer_display.lock = Lock::Interactive;
    assert!(s.sanitize());
    assert_eq!(s.output.peer_display.name, "Partner");
    assert_eq!(s.output.peer_display.size, Some([16, 8192]));
    assert_eq!(s.overlays.peer_display.size, Some([160.0, 8192.0]));
    let loaded: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(loaded, s);
    assert!(!s.output.display.enabled);
    assert_eq!(s.overlays.display.size, None);
}

#[test]
fn application_appearance_accepts_legacy_skin_and_round_trips() {
    assert_eq!(Settings::default().skin.chrome, ChromeMode::System);
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");
    for (old, mode, saved) in [
        ("skin", ChromeMode::System, "system"),
        ("system", ChromeMode::System, "system"),
        ("light", ChromeMode::Light, "light"),
        ("dark", ChromeMode::Dark, "dark"),
    ] {
        let value = serde_json::json!({"schema": SETTINGS_SCHEMA, "skin": {"id": "nanatype.night", "chrome": old}});
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        let loaded = load_settings(&path, None);
        assert!(matches!(loaded.note, LoadNote::Loaded));
        assert_eq!(loaded.value.skin.chrome, mode);
        assert_eq!(loaded.value.skin.id, "nanatype.night");
        save_json(&path, &loaded.value).unwrap();
        let saved_value: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved_value["skin"]["chrome"], saved);
        assert_eq!(load_settings(&path, None).value, loaded.value);
    }
}

#[test]
fn per_skin_personalizations_survive_restart_and_keep_legacy_settings() {
    let mut s = Settings::default();
    s.skin.overrides.text_size = Some(31.0);
    let mut p = SkinPersonalization::default();
    p.display.font_family = Some("Consolas".into());
    p.display.text_size = Some(42.0);
    p.display.size = Some([300.0, 120.0]);
    p.history.text_size = Some(18.0);
    p.history.line_height = Some(1.7);
    s.skin
        .personalizations
        .insert("test.skin".into(), p.clone());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    save_json(&path, &s).unwrap();
    let loaded: Settings = load_settings(&path, None).value;
    assert_eq!(loaded.skin.personalizations["test.skin"], p);
    assert_eq!(loaded.skin.overrides.text_size, Some(31.0));
    assert_eq!(loaded.skin.id, s.skin.id);
    let old: Settings = serde_json::from_str(r#"{"skin":{"overrides":{"text_size":35}}}"#).unwrap();
    assert!(old.skin.personalizations.is_empty());
    assert_eq!(old.skin.overrides.text_size, Some(35.0));
}

#[test]
fn personalization_rejects_invalid_inputs_and_sanitizes_hand_edited_files() {
    let p = SurfacePersonalization {
        text_size: Some(f32::NAN),
        line_height: Some(9.0),
        size: Some([1.0, f32::INFINITY]),
        font_family: Some("  ".into()),
        ..Default::default()
    };
    assert!(!p.valid());
    let mut s = Settings::default();
    s.skin.personalizations.insert(
        "test".into(),
        SkinPersonalization {
            display: p,
            ..Default::default()
        },
    );
    assert!(s.sanitize());
    let p = &s.skin.personalizations["test"].display;
    assert!(p.valid());
    assert_eq!(p.size, Some([160.0, 200.0]));
    assert_eq!(p.line_height, Some(3.0));
    assert_eq!(p.font_family, None);
}

#[test]
fn skin_params_round_trip_and_bad_values_never_fail_the_file() {
    use nanatype_model::ParamValue;
    let mut s = Settings::default();
    let mut p = SkinPersonalization::default();
    p.params
        .insert("ear_height".into(), ParamValue::Number(42.0));
    p.params
        .insert("fur".into(), ParamValue::Text("#fff7ec".into()));
    p.params.insert("tail".into(), ParamValue::Bool(false));
    s.skin
        .personalizations
        .insert("test.skin".into(), p.clone());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    save_json(&path, &s).unwrap();
    assert_eq!(
        load_settings(&path, None).value.skin.personalizations["test.skin"],
        p
    );
    // Without params the field is not written at all.
    let plain = serde_json::to_value(SkinPersonalization::default()).unwrap();
    assert!(plain.get("params").is_none());

    let loaded: Settings = serde_json::from_str(
        r#"{"skin":{"personalizations":{"a":{"display":{"text_size":30},"params":{"ok":1,"Bad":2,"n":null,"arr":[1],"obj":{}}}, "b":{"params":"nonsense"}}}}"#,
    )
    .unwrap();
    let a = &loaded.skin.personalizations["a"];
    assert_eq!(a.display.text_size, Some(30.0));
    assert_eq!(a.params.len(), 1);
    assert_eq!(a.params["ok"], ParamValue::Number(1.0));
    assert!(loaded.skin.personalizations["b"].params.is_empty());

    let mut big = SkinPersonalization::default();
    for i in 0..70 {
        big.params.insert(format!("p{i}"), ParamValue::Bool(true));
    }
    big.params
        .insert("long".into(), ParamValue::Text("x".repeat(300)));
    assert!(!big.valid());
    let mut s = Settings::default();
    s.skin.personalizations.insert("big".into(), big);
    s.sanitize();
    let big = &s.skin.personalizations["big"];
    assert!(big.valid());
    assert_eq!(big.params.len(), 64);
}

#[test]
fn third_party_chat_defaults_off_and_round_trips_at_schema_2() {
    let old: Settings = serde_json::from_str(r#"{"schema":2}"#).unwrap();
    let b = &old.integrations.blivechat;
    assert!(!b.enabled);
    assert_eq!(b.source, BlivechatSource::Server);
    assert_eq!((b.host.as_str(), b.port), ("127.0.0.1", BLIVECHAT_PORT));
    assert_eq!(b.comet_port, COMET_RELAY_PORT);
    assert!(b.comet_token.is_empty());

    let mut value = serde_json::json!({"schema":2,"integrations":{"blivechat":{"enabled":true,"source":"comet","room_kind":"room_id","room_id":"21452505","comet_token":"abc123"}}});
    assert_eq!(migrate(&mut value).unwrap(), 2);
    assert_eq!(
        value["integrations"]["blivechat"]["source"], "comet",
        "a v2 file keeps its blivechat settings"
    );
    let settings: Settings = serde_json::from_value(value).unwrap();
    let b = &settings.integrations.blivechat;
    assert!(b.enabled);
    assert_eq!(b.source, BlivechatSource::Comet);
    assert_eq!(b.room_kind, BliveRoomKind::RoomId);
    assert_eq!(b.room_id, "21452505");
    assert_eq!(b.comet_token, "abc123");
    assert!(value_has_no_auth_code(&settings));
}

fn value_has_no_auth_code(settings: &Settings) -> bool {
    serde_json::to_value(settings).unwrap()["integrations"]["blivechat"]
        .get("auth_code")
        .is_none()
}

#[test]
fn third_party_chat_values_are_repaired() {
    let mut s = Settings::default();
    let b = &mut s.integrations.blivechat;
    b.host = "  ".into();
    b.port = 0;
    b.comet_port = 0;
    b.room_id = " 42 ".into();
    b.comet_token = "ab\"<cd>".into();
    assert!(s.sanitize());
    let b = &s.integrations.blivechat;
    assert_eq!(b.host, "127.0.0.1");
    assert_eq!((b.port, b.comet_port), (BLIVECHAT_PORT, COMET_RELAY_PORT));
    assert_eq!(b.room_id, "42");
    assert_eq!(b.comet_token, "abcd");
}
