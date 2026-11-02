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
                "schema": schema, "overlays": overlays,
                "general": if schema < 5 {
                    serde_json::json!({"speaker_name": "Nana"})
                } else {
                    serde_json::json!({"characters": [{"name": "Nana"}]})
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let s = load_settings(&path, None).value;
        assert_eq!(s.general.active_character().name, "Nana");
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
fn retired_direct_chat_settings_are_dropped_but_panel_settings_survive() {
    let mut value = serde_json::json!({
        "schema": 3,
        "integrations": {
            "bilibili": {
                "enabled": true,
                "room": "42",
                "sync_profile": true,
                "panel_style": "history",
                "panel_open": true,
                "panel": {"gift": false}
            },
            "douyin": {"enabled": true, "room": "694562812381"},
            "youtube": {"enabled": true, "target": "@nana"}
        }
    });
    assert_eq!(migrate(&mut value).unwrap(), 3);
    let settings: Settings = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(
        settings.integrations.chat.panel_style,
        ChatPanelStyle::History
    );
    assert!(settings.integrations.chat.panel_open);
    assert!(!settings.integrations.chat.panel.gift);
    let saved = serde_json::to_value(settings).unwrap();
    assert!(saved["integrations"].get("bilibili").is_none());
    assert!(saved["integrations"].get("douyin").is_none());
    assert!(saved["integrations"].get("youtube").is_none());
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
        serde_json::from_str(r#"{"schema":5,"general":{"characters":[{"name":"奈奈"}]}}"#).unwrap();
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
        assert_eq!(settings.schema, SETTINGS_SCHEMA);
        assert_eq!(settings.integrations.blivechat.room_id, expected);
        assert_eq!(
            settings.integrations.chat.panel_style,
            ChatPanelStyle::History
        );
        assert!(settings.integrations.chat.panel_open);
        assert!(!settings.integrations.chat.panel.gift);
        assert!(settings.integrations.blivechat.enabled);
        assert_eq!(settings.integrations.blivechat.host, "127.0.0.1");
        assert!(value["integrations"].get("bilibili").is_none());
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
        serde_json::from_str(r#"{"schema":4,"integrations":{"chat":{"panel_open":true}}}"#)
            .unwrap();
    assert_eq!(old.integrations.chat.panel_style, ChatPanelStyle::Compact);
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");
    for (style, wire) in [
        (ChatPanelStyle::Compact, "compact"),
        (ChatPanelStyle::History, "history"),
    ] {
        let mut settings = old.clone();
        settings.integrations.chat.panel_style = style;
        save_json(&path, &settings).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(json["integrations"]["chat"]["panel_style"], wire);
        let loaded = load_settings(&path, None).value;
        assert_eq!(loaded.integrations.chat.panel_style, style);
        assert!(loaded.integrations.chat.panel_open);
        assert_eq!(loaded.schema, SETTINGS_SCHEMA);
    }
}

#[test]
fn send_key_choice_migrates_into_the_input_window_keys() {
    for (wire, send, newline) in [
        ("enter", Hotkey::bare("Enter"), Hotkey::shift("Enter")),
        ("ctrl_enter", Hotkey::ctrl("Enter"), Hotkey::shift("Enter")),
    ] {
        let mut value =
            serde_json::json!({"schema":2,"general":{"send_key":wire,"speaker_name":"奈奈"}});
        assert_eq!(migrate(&mut value).unwrap(), 2);
        assert!(value["general"].get("send_key").is_none());
        let settings: Settings = serde_json::from_value(value).unwrap();
        assert_eq!(settings.schema, SETTINGS_SCHEMA);
        assert_eq!(settings.general.active_character().name, "奈奈");
        let input = &settings.hotkeys.input;
        assert_eq!(input.send.as_ref(), Some(&send));
        assert_eq!(input.newline.as_ref(), Some(&newline));
        assert_eq!(input.bold, InputKeys::default().bold);
        assert!(settings.unknown.is_empty(), "{:?}", settings.unknown);
    }
}

#[test]
fn input_keys_default_round_trip_and_match_their_chords() {
    let keys = InputKeys::default();
    for key in InputKey::ALL {
        let Some(chord) = key.get(&keys) else {
            assert_eq!(key.default_binding(), None, "{key:?}");
            continue;
        };
        assert_eq!(keys.matching(chord), Some(key));
        assert_eq!(key.refusal(chord), None, "{key:?}");
    }
    let hotkeys = HotkeySettings::default();
    for chord in [&hotkeys.summon, &hotkeys.continue_].into_iter().flatten() {
        assert_eq!(chord.refusal_global(false), None, "{chord:?}");
    }
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");
    let mut settings = Settings::default();
    settings.hotkeys.input.send = Some(Hotkey {
        alt: true,
        ..Hotkey::bare("KeyS")
    });
    settings.hotkeys.input.italic = None;
    save_json(&path, &settings).unwrap();
    assert_eq!(load_settings(&path, None).value, settings);
}

#[test]
fn local_keys_may_be_bare_only_when_they_do_not_type() {
    for key in [
        "Enter", "Escape", "Tab", "ArrowUp", "PageDown", "F5", "Delete",
    ] {
        assert!(Hotkey::bare(key).is_safe_local(), "{key}");
    }
    for key in ["KeyA", "Digit1", "Space", "Backquote", ""] {
        assert!(!Hotkey::bare(key).is_safe_local(), "{key}");
    }
    assert!(Hotkey::ctrl("KeyA").is_safe_local());
    assert!(!Hotkey::bare("Enter").is_safe_global());
    assert_eq!(Hotkey::ctrl("ArrowUp").label("Win"), "Ctrl+↑");
    assert_eq!(Hotkey::bare("Escape").label("Win"), "Esc");
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
    assert_eq!(s.general.active_character().name, "奈奈");
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
    assert_eq!(loaded.value.general.active_character().name, "a");
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
fn files_from_before_chat_settings_get_their_defaults() {
    let json = r#"{"schema": 1, "integrations": {"osc": {"enabled": true}}}"#;
    let s: Settings = serde_json::from_str(json).unwrap();
    let b = &s.integrations.chat;
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
    let a = &s.history.archive;
    assert!(
        a.enabled && !a.chat,
        "own lines are saved, viewer chat is not"
    );
    assert_eq!(a.keep_days, 30);
}

#[test]
fn blivechat_values_are_clamped() {
    let mut s = Settings::default();
    s.integrations.blivechat.room_id = " 123 ".into();
    assert!(s.sanitize());
    assert_eq!(s.integrations.blivechat.room_id, "123");
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
        r#"{ "schema": 7, "general": { "characters": [{ "name": "z" }] } }"#,
    )
    .unwrap();
    let loaded = load_settings(&path, None);
    assert!(matches!(loaded.note, LoadNote::TooNew { schema: 7 }));
    assert_eq!(loaded.value.general.active_character().name, "z");
}

#[test]
fn atomic_writes_keep_a_backup() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");
    save_json(&path, &Settings::default()).unwrap();
    let mut s = Settings::default();
    s.general.characters[0].name = "b".into();
    save_json(&path, &s).unwrap();
    assert!(path.with_extension("json.bak").exists());
    assert_eq!(
        load_settings(&path, None).value.general.characters[0].name,
        "b"
    );
}

#[test]
fn sanitize_clamps_hand_edited_values() {
    let mut s = Settings::default();
    s.playback.speed_scale = f32::NAN;
    s.sound.master = 7.0;
    s.history.visible_items = 99_999;
    s.skin.frame_rate = 144;
    assert!(s.sanitize());
    assert_eq!(s.playback.speed_scale, 1.0);
    assert_eq!(s.sound.master, 1.0);
    assert_eq!(s.history.visible_items, s.history.max_items);
    assert_eq!(s.skin.frame_rate, 30);
    s.skin.frame_rate = 60;
    assert!(!s.sanitize());
}

#[test]
fn accessibility_defaults_load_into_older_files_and_stay_global() {
    // A file from before the setting: system contrast, the system's size.
    let s: Settings = serde_json::from_str(r#"{"schema":5}"#).unwrap();
    assert_eq!(s.accessibility, AccessibilitySettings::default());
    assert_eq!(s.accessibility.contrast, ContrastMode::System);
    assert_eq!(s.accessibility.scale(), 1.0);
    assert!(!s.accessibility.ui_scale_follows_system);
    // Following the system takes its text size where it reports one.
    let follows = AccessibilitySettings {
        ui_scale: 150,
        ui_scale_follows_system: true,
        ..Default::default()
    };
    assert_eq!(follows.zoom(Some(1.25)), 1.25);
    assert_eq!(follows.zoom(None), 1.5);
    assert_eq!(AccessibilitySettings::default().zoom(Some(2.0)), 1.0);
    for (mode, system, on) in [
        (ContrastMode::System, false, false),
        (ContrastMode::System, true, true),
        (ContrastMode::Off, true, false),
        (ContrastMode::On, false, true),
    ] {
        let a = AccessibilitySettings {
            contrast: mode,
            ..Default::default()
        };
        assert_eq!(a.high_contrast(system), on, "{mode:?} with system {system}");
    }
    // Round trips, and a hand-edited size lands on a step in range.
    let mut s = Settings {
        accessibility: AccessibilitySettings {
            contrast: ContrastMode::On,
            ui_scale: 150,
            ui_scale_follows_system: true,
        },
        ..Default::default()
    };
    let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(back.accessibility, s.accessibility);
    assert!(!s.sanitize());
    for (written, kept) in [(5, 80), (1000, 200), (144, 140), (146, 150)] {
        s.accessibility.ui_scale = written;
        s.sanitize();
        assert_eq!(s.accessibility.ui_scale, kept, "{written}%");
    }
    // The settings window's look follows the computer, not the profile.
    assert!(
        !crate::transfer::PROFILE_SCOPE
            .iter()
            .any(|p| p.starts_with("/accessibility"))
    );
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
fn input_dock_tab_and_chat_placement_survive_disk_roundtrip() {
    let layout = InputDockLayout {
        tab: InputDockTab::Phrases,
        chat_pinned: true,
        chat_height: 180.0,
        collapsed: true,
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
        input_sidebar_width: Some(285.0),
        input_dock: Some(layout),
        ..Default::default()
    };
    geometry.remember(&monitors, "input", frame);
    save_json(&path, &geometry).unwrap();
    let restored: GeometryStore = load_json(&path).value;
    assert_eq!(restored.input_sidebar_width, Some(285.0));
    assert_eq!(restored.restore(&monitors, "input"), Some(frame));
    assert_eq!(restored.input_dock.unwrap().normalized(), layout);
}

#[test]
fn input_dock_reads_the_old_region_list_and_repairs_unknown_values() {
    // The multi-region dock of earlier versions reads as the default layout.
    let old: InputDockLayout = serde_json::from_str(
        r#"{
        "regions": [{"id":5,"kind":"phrases","number":1}],
        "selected":5
    }"#,
    )
    .unwrap();
    assert_eq!(old.normalized(), InputDockLayout::default());
    let odd: InputDockLayout =
        serde_json::from_str(r#"{"tab":"future_panel","chat_pinned":true,"chat_height":-4}"#)
            .unwrap();
    let odd = odd.normalized();
    assert_eq!(odd.tab, InputDockTab::Controls);
    assert!(odd.chat_pinned);
    assert_eq!(odd.chat_height, INPUT_CHAT_MIN);
    // Pinned chat is never also the lower panel.
    let both = InputDockLayout {
        tab: InputDockTab::Chat,
        chat_pinned: true,
        ..Default::default()
    };
    assert_eq!(both.normalized().tab, InputDockTab::Controls);
}

#[test]
fn the_sidebar_rail_opens_panels_folds_and_moves_chat() {
    use InputDockTab::*;
    let open = InputDockLayout::default();
    // Another panel opens; the open one folds the sidebar; any icon unfolds.
    let stickers = open.rail(Stickers);
    assert!(stickers.showing(Stickers) && !stickers.showing(Controls));
    let folded = stickers.rail(Stickers);
    assert!(folded.collapsed && !folded.showing(Stickers));
    assert!(folded.rail(Phrases).showing(Phrases));
    // Unpinned chat is a panel like the others.
    let chat = open.rail(Chat);
    assert!(chat.chat_visible() && chat.showing(Chat) && !chat.showing(Controls));
    // Pinning keeps chat on screen above the last insert panel.
    let pinned = chat.toggle_pin();
    assert!(pinned.chat_pinned && pinned.showing(Controls) && pinned.chat_visible());
    // Unpinning gives chat the whole sidebar.
    let unpinned = pinned.toggle_pin();
    assert!(!unpinned.chat_pinned && unpinned.showing(Chat));
    // Pinned chat's rail icon puts it away and leaves the insert panel.
    let hidden = pinned.rail(Chat);
    assert!(!hidden.chat_visible() && hidden.showing(Controls));
    // A folded sidebar with pinned chat unfolds without losing the pin.
    let refolded = InputDockLayout {
        collapsed: true,
        ..pinned
    }
    .rail(Chat);
    assert!(refolded.chat_pinned && refolded.showing(Controls));
    // The tray shows chat by pinning it, and hides it either way.
    assert!(open.toggle_chat().chat_pinned);
    assert!(!pinned.toggle_chat().chat_visible());
    assert!(!chat.toggle_chat().chat_visible());
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
    assert_eq!(s.output.peer_display.size, Some([16, 1920]));
    assert_eq!(s.overlays.peer_display.size, Some([160.0, 8192.0]));
    let loaded: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(loaded, s);
    assert!(!s.output.display.enabled);
    assert_eq!(s.overlays.display.size, None);
}

#[test]
fn output_sizes_fit_1080p_either_way_round() {
    assert_eq!(fit_output_size(960.0, 320.0), Some([960, 320]));
    assert_eq!(fit_output_size(3840.0, 2160.0), Some([1920, 1080]));
    assert_eq!(fit_output_size(2560.0, 1440.0), Some([1920, 1080]));
    // Portrait keeps 1080 across and 1920 down.
    assert_eq!(fit_output_size(1200.0, 2400.0), Some([960, 1920]));
    // Wider than 16:9 is held by the long side, taller boxes by the short.
    assert_eq!(fit_output_size(3840.0, 960.0), Some([1920, 480]));
    assert_eq!(fit_output_size(1600.0, 1600.0), Some([1080, 1080]));
    assert_eq!(fit_output_size(959.6, 320.4), Some([960, 320]));
    assert_eq!(fit_output_size(4.0, 4.0), Some([16, 16]));
    for bad in [
        (0.0, 10.0),
        (10.0, -1.0),
        (f64::NAN, 10.0),
        (f64::INFINITY, 10.0),
    ] {
        assert_eq!(fit_output_size(bad.0, bad.1), None);
    }
}

#[test]
fn obs_link_round_trips_without_the_runtime_size() {
    let old: Settings = serde_json::from_str(r#"{"schema":3}"#).unwrap();
    assert_eq!(old.output.obs, ObsLink::default());
    assert_eq!(
        (old.output.obs.host.as_str(), old.output.obs.port),
        ("127.0.0.1", 4455)
    );
    assert!(old.output.history.obs_source.is_empty());
    let mut s = Settings::default();
    s.output.obs.enabled = true;
    s.output.history.obs_source = "NanaType History".into();
    s.output.history.size = Some([3840, 2160]);
    s.output.history.obs_size = Some([800, 600]);
    assert_eq!(s.output.history.extent(), Some([800, 600]));
    s.sanitize();
    assert_eq!(s.output.history.size, Some([1920, 1080]));
    let json = serde_json::to_string(&s).unwrap();
    assert!(!json.contains("obs_size"));
    let loaded: Settings = serde_json::from_str(&json).unwrap();
    assert_eq!(loaded.output.history.obs_size, None);
    assert_eq!(loaded.output.history.extent(), Some([1920, 1080]));
    assert_eq!(loaded.output.history.obs_source, "NanaType History");
    assert!(loaded.output.obs.enabled);
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

#[test]
fn diagnostics_default_off_round_trip_and_peek_without_touching_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    assert_eq!(peek_diagnostics(&path), DiagnosticsSettings::default());

    let mut settings = Settings::default();
    assert!(!settings.diagnostics.verbose);
    settings.diagnostics.verbose = true;
    settings.diagnostics.crash_seen = 42;
    save_json(&path, &settings).unwrap();
    let peeked = peek_diagnostics(&path);
    assert!(peeked.verbose);
    assert_eq!(peeked.crash_seen, 42);

    // Older files have no section; a broken file stays where it is.
    fs::write(&path, r#"{"schema":3,"general":{}}"#).unwrap();
    assert_eq!(peek_diagnostics(&path), DiagnosticsSettings::default());
    fs::write(&path, "{not json").unwrap();
    assert_eq!(peek_diagnostics(&path), DiagnosticsSettings::default());
    assert_eq!(fs::read_to_string(&path).unwrap(), "{not json");
}

#[test]
fn performance_defaults_off_and_round_trips() {
    // Older files have no section: every preference starts off.
    let settings: Settings = serde_json::from_str(r#"{"schema":3,"general":{}}"#).unwrap();
    assert_eq!(settings.performance, PerformanceSettings::default());
    assert!(!settings.performance.gpu_high);

    let settings = Settings {
        performance: PerformanceSettings {
            gpu_high: true,
            cpu_high: true,
            cpu_performance_cores: true,
            gpu_adapter: Some("GPU B".into()),
        },
        ..Settings::default()
    };
    let json = serde_json::to_string(&settings).unwrap();
    let back: Settings = serde_json::from_str(&json).unwrap();
    assert_eq!(back.performance, settings.performance);
    assert!(back.unknown.is_empty());
}

#[test]
fn performance_peeks_the_gpu_choice_before_startup() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    assert_eq!(peek_performance(&path), PerformanceSettings::default());

    let mut settings = Settings::default();
    settings.performance.gpu_adapter = Some("GPU B".into());
    save_json(&path, &settings).unwrap();
    assert_eq!(
        peek_performance(&path).gpu_adapter.as_deref(),
        Some("GPU B")
    );

    fs::write(&path, "{not json").unwrap();
    assert_eq!(peek_performance(&path), PerformanceSettings::default());
}

#[test]
fn speech_defaults_migrate_and_activation_is_not_persisted() {
    let mut settings: Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(settings.speech.mode, SpeechMode::Manual);
    assert_eq!(settings.speech.backend, SpeechBackend::Auto);
    assert_eq!(settings.speech.silence_seconds, 3);
    settings.speech.silence_seconds = 999;
    settings.speech.language = Some("invalid/path".into());
    settings.sanitize();
    assert_eq!(settings.speech.silence_seconds, 10);
    assert_eq!(settings.speech.language, None);
    settings.speech.silence_seconds = 0;
    settings.sanitize();
    assert_eq!(settings.speech.silence_seconds, 1);
    let value = serde_json::to_value(&settings).unwrap();
    assert!(value["speech"].get("active").is_none());
    settings.speech.mode = SpeechMode::Auto;
    settings.speech.language = Some("ja-JP".into());
    assert_eq!(
        serde_json::from_value::<Settings>(serde_json::to_value(&settings).unwrap()).unwrap(),
        settings
    );
}

#[test]
fn tts_is_off_by_default_and_older_files_load_without_it() {
    let mut settings: Settings =
        serde_json::from_str(r#"{"schema":5,"general":{"characters":[{"name":"Nana"}]}}"#).unwrap();
    assert_eq!(settings.tts, TtsSettings::default());
    assert!(!settings.tts.enabled && !settings.tts.chat && !settings.tts.partners);
    assert_eq!((settings.tts.rate, settings.tts.volume), (1.0, 1.0));
    assert_eq!(settings.general.characters[0].voice, None);
    assert!(!settings.sanitize());

    settings.tts.rate = 9.0;
    settings.tts.volume = f32::NAN;
    settings.tts.voice = Some(String::new());
    settings.general.characters[0].voice = Some("x".repeat(600));
    settings.sanitize();
    assert_eq!(settings.tts.rate, TTS_RATE.1);
    assert_eq!(settings.tts.volume, 1.0);
    assert_eq!(settings.tts.voice, None);
    assert_eq!(settings.general.characters[0].voice, None);

    settings.tts.enabled = true;
    settings.general.characters[0].voice = Some("com.apple.voice.compact.ja-JP.Kyoko".into());
    let mut value = serde_json::to_value(&settings).unwrap();
    assert_eq!(migrate(&mut value), Ok(SETTINGS_SCHEMA));
    assert_eq!(serde_json::from_value::<Settings>(value).unwrap(), settings);
}

#[test]
fn character_voices_switch_with_profiles_and_tts_stays_global() {
    let mut base = Settings::default();
    base.general.characters[0].voice = Some("narrator".into());
    let mut singing = base.clone();
    singing.general.characters[0].voice = Some("singer".into());
    singing.tts.rate = 1.5;
    let profile = Profile::capture("p1".into(), "歌回".into(), &singing);
    assert!(profile.settings.get("tts").is_none());

    let mut here = base.clone();
    here.tts.enabled = true;
    let switched = profile.apply(&here);
    assert_eq!(
        switched.general.characters[0].voice.as_deref(),
        Some("singer")
    );
    assert_eq!(switched.tts, here.tts);
    assert_eq!(changed_sections(&base, &here), vec!["tts"]);
}

#[test]
fn only_the_send_key_takes_enter_and_ctrl_enter() {
    let meta = |key: &str| Hotkey {
        meta: true,
        ..Hotkey::bare(key)
    };
    for chord in [
        Hotkey::bare("Enter"),
        Hotkey::ctrl("Enter"),
        meta("Enter"),
        Hotkey::bare("NumpadEnter"),
    ] {
        assert_eq!(InputKey::Send.refusal(&chord), None, "{chord:?}");
        for key in InputKey::ALL.into_iter().filter(|k| *k != InputKey::Send) {
            assert_eq!(
                key.refusal(&chord),
                Some(Refusal::SendOnly),
                "{key:?} {chord:?}"
            );
        }
        assert_eq!(chord.refusal_global(false), Some(Refusal::SendOnly));
    }
    // Shift+Enter and Alt+Enter are anyone's.
    let alt_enter = Hotkey {
        alt: true,
        ..Hotkey::bare("Enter")
    };
    for chord in [Hotkey::shift("Enter"), alt_enter] {
        assert_eq!(InputKey::Newline.refusal(&chord), None, "{chord:?}");
    }
}

#[test]
fn chords_the_system_or_editor_owns_are_refused() {
    let alt = |key: &str| Hotkey {
        alt: true,
        ..Hotkey::bare(key)
    };
    let ctrl_alt = |key: &str| Hotkey {
        ctrl: true,
        ..alt(key)
    };
    // Shift alone still types: system-wide it would swallow capitals.
    assert_eq!(
        Hotkey::shift("KeyA").refusal_global(false),
        Some(Refusal::Types)
    );
    assert_eq!(Hotkey::shift("F8").refusal_global(false), None);
    for chord in [
        Hotkey::ctrl("KeyC"),
        Hotkey::ctrl("KeyV"),
        Hotkey::ctrl("KeyZ"),
        Hotkey::ctrl("KeyA"),
        alt("F4"),
        alt("Tab"),
        Hotkey::ctrl("Space"),
        Hotkey::ctrl("ArrowLeft"),
        Hotkey::ctrl("Backspace"),
    ] {
        assert_eq!(
            chord.refusal_global(false),
            Some(Refusal::Reserved),
            "{chord:?}"
        );
        assert_eq!(
            InputKey::Reply.refusal(&chord),
            Some(Refusal::Reserved),
            "{chord:?}"
        );
    }
    // Caret keys and Tab stay the editor's inside the input window.
    for chord in [
        Hotkey::bare("ArrowLeft"),
        Hotkey::bare("Home"),
        Hotkey::bare("Delete"),
        Hotkey::bare("Tab"),
        Hotkey::shift("Tab"),
        ctrl_alt("ArrowLeft"),
    ] {
        assert_eq!(
            InputKey::Hide.refusal(&chord),
            Some(Refusal::Reserved),
            "{chord:?}"
        );
    }
    assert_eq!(ctrl_alt("ArrowLeft").refusal_global(false), None);
    assert_eq!(Hotkey::ctrl("KeyB").refusal_global(false), None);
    assert_eq!(InputKey::Hide.refusal(&Hotkey::bare("Escape")), None);
    assert_eq!(InputKey::ChatPrev.refusal(&alt("ArrowUp")), None);
}

#[test]
fn sanitize_hands_enter_back_to_the_send_key() {
    let mut s = Settings::default();
    s.hotkeys.input.send = Some(Hotkey::ctrl("Enter"));
    s.hotkeys.input.newline = Some(Hotkey::bare("Enter"));
    s.hotkeys.input.reply = Some(Hotkey::ctrl("Enter"));
    s.hotkeys.summon = Some(Hotkey::ctrl("Enter"));
    assert!(s.sanitize());
    let input = &s.hotkeys.input;
    assert_eq!(input.send, Some(Hotkey::ctrl("Enter")));
    assert_eq!(input.newline, Some(Hotkey::shift("Enter")));
    assert_eq!(input.reply, InputKey::Reply.default_binding());
    assert_eq!(s.hotkeys.summon, None);
    // A default another key now holds is not handed out twice.
    let mut s = Settings::default();
    s.hotkeys.input.hide = Some(Hotkey::shift("Enter"));
    s.hotkeys.input.newline = Some(Hotkey::bare("Enter"));
    s.sanitize();
    assert_eq!(s.hotkeys.input.newline, None);
    assert!(!s.sanitize());
}

#[test]
fn the_speaker_name_and_avatar_become_the_first_character() {
    let mut value = serde_json::json!({
        "schema": 4,
        "general": {"speaker_name": "奈奈", "avatar": "C:/me.png", "locale": "ja-JP"}
    });
    assert_eq!(migrate(&mut value).unwrap(), 4);
    assert!(value["general"].get("speaker_name").is_none());
    assert!(value["general"].get("avatar").is_none());
    let s: Settings = serde_json::from_value(value).unwrap();
    assert_eq!(s.schema, SETTINGS_SCHEMA);
    assert_eq!(s.general.locale.as_deref(), Some("ja-JP"));
    assert_eq!(
        s.general.characters,
        vec![Character {
            name: "奈奈".into(),
            avatar: Some("C:/me.png".into()),
            ..Character::default()
        }]
    );
    assert_eq!(s.general.character, 0);
    assert!(s.unknown.is_empty(), "{:?}", s.unknown);
    // Nothing to carry: the default single character.
    let mut value = serde_json::json!({"schema": 4, "general": {}});
    migrate(&mut value).unwrap();
    let s: Settings = serde_json::from_value(value).unwrap();
    assert_eq!(s.general.characters, vec![Character::default()]);
}

#[test]
fn characters_switch_wrap_and_keep_the_last_one() {
    let mut g = General::default();
    for name in ["A", "B", "C"] {
        g.characters.push(Character {
            name: name.into(),
            ..Character::default()
        });
    }
    g.characters.remove(0);
    g.select_character(2);
    assert_eq!(g.active_character().name, "C");
    g.cycle_character(1);
    assert_eq!(g.active_character().name, "A");
    g.cycle_character(-1);
    assert_eq!(g.active_character().name, "C");
    g.select_character(9);
    assert_eq!(g.character, 2);
    // The active character moves and survives removals before it.
    g.move_character(2, -1);
    assert_eq!((g.character, g.active_character().name.as_str()), (1, "C"));
    g.remove_character(0);
    assert_eq!(g.active_character().name, "C");
    g.remove_character(0);
    assert_eq!(g.active_character().name, "B");
    g.remove_character(0);
    assert_eq!(g.characters.len(), 1, "the last character stays");
    // A hand-edited file cannot leave the list empty or point past it.
    let mut s = Settings::default();
    s.general.characters.clear();
    s.general.character = 4;
    s.sanitize();
    assert_eq!(s.general.characters.len(), 1);
    assert_eq!(s.general.character, 0);
}

#[test]
fn the_active_character_survives_a_restart() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");
    let mut s = Settings::default();
    s.general.characters.push(Character {
        name: "Two".into(),
        color: Some(nanatype_model::Rgba8::rgb(255, 0, 0)),
        hotkey: Some(Hotkey::ctrl("Digit2")),
        ..Character::default()
    });
    s.general.select_character(1);
    save_json(&path, &s).unwrap();
    let loaded = load_settings(&path, None);
    assert!(matches!(loaded.note, LoadNote::Loaded));
    assert_eq!(loaded.value, s);
    assert_eq!(loaded.value.general.active_character().name, "Two");
}

/// Settings that touch every kind of section an export or profile handles.
fn busy_settings() -> Settings {
    let mut s = Settings::fresh();
    s.general.locale = Some("ja-JP".into());
    s.general.characters[0].name = "Nana".into();
    s.playback.speed_scale = 1.5;
    s.sound.master = 0.4;
    s.sound.device = Some("Speakers".into());
    s.skin.id = "nanatype.night".into();
    s.overlays.display.always_on_top = false;
    s.hotkeys.summon = Some(Hotkey::ctrl("KeyJ"));
    s.integrations.twitch.channel = "nana".into();
    s.integrations.vts.token = Some("vts-secret".into());
    s.integrations.blivechat.comet_token = "comet0secret".into();
    s.update.proxy = "http://user:pass@proxy.example:8080/".into();
    s.assets.phrases.push(QuickPhrase {
        label: "hi".into(),
        markup: "hello".into(),
        send: false,
        hotkey: None,
    });
    s.sanitize();
    s
}

#[test]
fn export_then_import_round_trips_without_credentials() {
    let s = busy_settings();
    let exported = export(
        &s,
        &GeometryStore::default(),
        ExportOptions::default(),
        "1.0",
        7,
    );
    let text = serde_json::to_string(&exported.envelope).unwrap();
    assert!(!text.contains("vts-secret") && !text.contains("comet0secret"));
    assert!(!text.contains("user:pass"), "proxy credentials stay behind");
    assert!(text.contains(r#""app":"NanaType""#));

    let incoming = parse_import(&text).unwrap();
    assert_eq!(incoming.from_schema, SETTINGS_SCHEMA);
    assert_eq!(incoming.exported_at, Some(7));
    // Onto a fresh install: everything travels but the credentials and
    // this computer's devices.
    let mut other = Settings::fresh();
    other.integrations.vts.token = Some("theirs".into());
    let merged = incoming.merge(&other);
    assert_eq!(merged.playback, s.playback);
    assert_eq!(merged.skin, s.skin);
    assert_eq!(merged.hotkeys, s.hotkeys);
    assert_eq!(merged.general.locale, s.general.locale);
    assert_eq!(merged.integrations.twitch.channel, "nana");
    assert_eq!(merged.integrations.vts.token.as_deref(), Some("theirs"));
    assert_eq!(merged.integrations.blivechat.comet_token, "");
    assert_eq!(merged.sound.device, None);
    assert_eq!(merged.update.proxy, "http://proxy.example:8080/");
    // Back onto itself, nothing changes.
    let mut same = s.clone();
    same.update.proxy = "http://proxy.example:8080/".into();
    assert_eq!(incoming.merge(&s), same);
    assert_eq!(changed_sections(&s, &same), vec!["update"]);
}

#[test]
fn integrations_and_window_positions_are_optional() {
    let s = busy_settings();
    let geometry = GeometryStore {
        input_dock: Some(InputDockLayout::default()),
        ..GeometryStore::default()
    };
    let options = ExportOptions {
        window_positions: true,
        integrations: false,
        avatars: false,
    };
    let envelope = export(&s, &geometry, options, "1.0", 1).envelope;
    assert!(envelope.settings.get("integrations").is_none());
    assert_eq!(envelope.geometry, Some(geometry));
    let text = serde_json::to_string(&envelope).unwrap();
    let incoming = parse_import(&text).unwrap();
    let mut here = Settings::fresh();
    here.integrations.twitch.channel = "mine".into();
    let merged = incoming.merge(&here);
    assert_eq!(
        merged.integrations, here.integrations,
        "kept when not carried"
    );
    assert_eq!(merged.playback, s.playback);
    let without = export(
        &s,
        &GeometryStore::default(),
        ExportOptions::default(),
        "",
        1,
    );
    assert!(
        without.envelope.geometry.is_none(),
        "positions stay by default"
    );
}

#[test]
fn avatars_travel_inside_the_export() {
    let tmp = tempfile::tempdir().unwrap();
    let avatar = tmp.path().join("face.png");
    fs::write(&avatar, b"not really a png").unwrap();
    let mut s = Settings::fresh();
    s.general.characters[0].avatar = Some(avatar.display().to_string());
    s.general.characters.push(Character {
        avatar: Some(tmp.path().join("gone.png").display().to_string()),
        ..Character::default()
    });
    let exported = export(
        &s,
        &GeometryStore::default(),
        ExportOptions::default(),
        "",
        1,
    );
    assert_eq!(exported.missing_avatars, 1);
    let text = serde_json::to_string(&exported.envelope).unwrap();
    let incoming = parse_import(&text).unwrap();
    let mut merged = incoming.merge(&Settings::fresh());
    let dir = tmp.path().join("avatars");
    assert_eq!(incoming.unpack_avatars(&dir, &mut merged, 9).unwrap(), 1);
    let placed = merged.general.characters[0].avatar.clone().unwrap();
    assert!(placed.starts_with(&dir.display().to_string()) && placed.ends_with(".png"));
    assert_eq!(fs::read(placed).unwrap(), b"not really a png");
}

#[test]
fn older_imports_migrate_and_newer_ones_are_refused() {
    let v4 = r#"{"app":"NanaType","format":1,"schema":4,"settings":
        {"schema":4,"general":{"speaker_name":"Old","onboarding_done":true}}}"#;
    let incoming = parse_import(v4).unwrap();
    assert_eq!(incoming.from_schema, 4);
    let merged = incoming.merge(&Settings::fresh());
    assert_eq!(merged.general.characters[0].name, "Old");
    assert_eq!(merged.schema, SETTINGS_SCHEMA);
    // A bare settings.json from the very first build.
    let v0 = parse_import(r#"{"theme":"night","speaker_name":"Zero"}"#);
    assert_eq!(
        v0.unwrap_err(),
        ImportError::NotSettings,
        "too bare to tell"
    );
    let v0 = parse_import(r#"{"general":{},"theme":"night"}"#).unwrap();
    assert_eq!(v0.from_schema, 0);

    let newer = format!(
        r#"{{"app":"NanaType","format":1,"schema":{n},"settings":{{"schema":{n}}}}}"#,
        n = SETTINGS_SCHEMA + 1
    );
    assert_eq!(
        parse_import(&newer).unwrap_err(),
        ImportError::TooNew(SETTINGS_SCHEMA + 1)
    );
    let format = r#"{"app":"NanaType","format":99,"schema":1,"settings":{}}"#;
    assert_eq!(
        parse_import(format).unwrap_err(),
        ImportError::FormatTooNew(99)
    );
    assert_eq!(
        parse_import(r#"{"app":"Other","settings":{}}"#).unwrap_err(),
        ImportError::NotSettings
    );
    assert!(matches!(
        parse_import("not json"),
        Err(ImportError::Unreadable(_))
    ));
}

#[test]
fn reset_keeps_credentials_and_language() {
    let mut s = busy_settings();
    s.general.onboarding_done = true;
    let reset = s.reset_from();
    assert_eq!(reset.integrations.vts.token.as_deref(), Some("vts-secret"));
    assert_eq!(reset.integrations.blivechat.comet_token, "comet0secret");
    assert_eq!(reset.general.locale.as_deref(), Some("ja-JP"));
    assert!(reset.general.onboarding_done);
    let mut expected = Settings::fresh();
    expected.integrations.vts.token = s.integrations.vts.token.clone();
    expected.integrations.blivechat.comet_token = s.integrations.blivechat.comet_token.clone();
    expected.general.locale = s.general.locale.clone();
    expected.general.onboarding_done = true;
    assert_eq!(reset, expected);
}

#[test]
fn switching_profiles_changes_only_profile_sections() {
    let base = busy_settings();
    let mut singing = base.clone();
    singing.skin.id = "nanatype.parchment".into();
    singing.playback.speed_scale = 0.5;
    singing.sound.master = 0.9;
    singing.general.characters[0].name = "Utaite".into();
    singing.assets.phrases.clear();
    let profile = Profile::capture("p1".into(), "歌回".into(), &singing);
    assert!(profile.settings.pointer("/sound/device").is_none());
    assert!(profile.settings.get("hotkeys").is_none());

    let mut here = base.clone();
    here.hotkeys.summon = Some(Hotkey::ctrl("KeyK"));
    here.general.locale = Some("en-US".into());
    here.sound.device = Some("Headset".into());
    here.history.archive.keep_days = 3;
    here.skin.chrome = ChromeMode::Dark;
    let switched = profile.apply(&here);
    assert_eq!(switched.skin.id, "nanatype.parchment");
    assert_eq!(switched.playback.speed_scale, 0.5);
    assert_eq!(switched.sound.master, 0.9);
    assert_eq!(switched.general.characters[0].name, "Utaite");
    assert!(switched.assets.phrases.is_empty());
    // Global sections are untouched.
    assert_eq!(switched.hotkeys, here.hotkeys);
    assert_eq!(switched.general.locale, here.general.locale);
    assert_eq!(switched.integrations, here.integrations);
    assert_eq!(switched.sound.device, here.sound.device);
    assert_eq!(switched.history.archive, here.history.archive);
    assert_eq!(switched.skin.chrome, ChromeMode::Dark);
    assert_eq!(
        changed_sections(&here, &switched),
        vec!["general", "playback", "sound", "skin", "assets"]
    );
}

#[test]
fn profiles_are_saved_and_listed_from_their_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join(PROFILE_DIR);
    let a = Profile::capture("pa".into(), "杂谈".into(), &busy_settings());
    let b = Profile::capture("pb".into(), "歌回".into(), &Settings::fresh());
    for p in [&a, &b] {
        save_json(&Profile::path(&dir, &p.id), &p.file(1)).unwrap();
    }
    fs::write(dir.join("broken.json"), "{").unwrap();
    fs::write(dir.join("Bad Name.json"), "{}").unwrap();
    let mut newer = b.file(1);
    newer.settings["schema"] = (SETTINGS_SCHEMA + 1).into();
    save_json(&Profile::path(&dir, "pnew"), &newer).unwrap();
    let (profiles, skipped) = load_profiles(&dir);
    let names: Vec<&str> = profiles.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["杂谈", "歌回"]);
    assert_eq!(profiles[0], a);
    assert_eq!(skipped.len(), 2, "{skipped:?}");
    let id = new_profile_id(0xa, |id| id == "pa");
    assert_eq!(id, "pb");
    assert!(valid_profile_id(&id) && !valid_profile_id("../x"));
}

#[test]
fn translation_is_off_by_default_and_older_files_load_without_it() {
    let mut settings: Settings = serde_json::from_str(r#"{"schema":5}"#).unwrap();
    let t = &settings.translate;
    assert_eq!(*t, TranslateSettings::default());
    assert!(!t.enabled && t.own && !t.partners && !t.chat);
    assert!(t.display && t.history);
    assert_eq!(t.provider, TranslateProvider::Openai);
    assert_eq!(t.target, "en");
    assert!(!settings.sanitize());

    settings.translate.target = " zh-Hant ".into();
    settings.sanitize();
    assert_eq!(settings.translate.target, "zh-Hant");
    for bad in ["", "e n", "en_US", "-en", &"x".repeat(40)] {
        settings.translate.target = bad.into();
        settings.sanitize();
        assert_eq!(settings.translate.target, "en", "{bad:?}");
    }
    settings.translate.model = "m".repeat(300);
    settings.sanitize();
    assert_eq!(settings.translate.model, "");

    settings.translate.enabled = true;
    settings.translate.provider = TranslateProvider::Deepl;
    let value = serde_json::to_value(&settings).unwrap();
    assert_eq!(value["translate"]["provider"], "deepl");
    let mut migrated = value.clone();
    assert_eq!(migrate(&mut migrated), Ok(SETTINGS_SCHEMA));
    assert_eq!(
        serde_json::from_value::<Settings>(migrated).unwrap(),
        settings
    );
}

#[test]
fn translation_stays_global_and_exports_carry_no_credentials() {
    let mut s = Settings::default();
    s.translate.enabled = true;
    s.translate.base_url = "http://localhost:11434/v1".into();
    s.translate.proxy = "http://user:secret@proxy.local:8080".into();
    let profile = Profile::capture("p1".into(), "歌回".into(), &s);
    assert!(profile.settings.get("translate").is_none());
    let here = Settings::default();
    assert_eq!(profile.apply(&here).translate, here.translate);
    assert_eq!(changed_sections(&here, &s), vec!["translate"]);

    let full = export(
        &s,
        &GeometryStore::default(),
        ExportOptions::default(),
        "1",
        1,
    );
    let text = serde_json::to_string(&full.envelope).unwrap();
    assert!(
        !text.contains("secret") && !text.contains("user:"),
        "{text}"
    );
    let t = &full.envelope.settings["translate"];
    assert_eq!(t["proxy"], "http://proxy.local:8080");
    assert_eq!(t["base_url"], "http://localhost:11434/v1");
    // No key of any kind is a setting: they live in the secret store.
    let keys: Vec<&String> = t.as_object().unwrap().keys().collect();
    assert!(
        keys.iter()
            .all(|k| !k.contains("key") && !k.contains("token")),
        "{keys:?}"
    );
    let bare = ExportOptions {
        integrations: false,
        ..ExportOptions::default()
    };
    let bare = export(&s, &GeometryStore::default(), bare, "1", 1);
    assert!(
        bare.envelope.settings["translate"]
            .get("base_url")
            .is_none()
    );
}
