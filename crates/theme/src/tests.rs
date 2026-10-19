use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use nanatype_model::Rgba8;

use crate::*;

const APP: &str = "1.0.0";

/// A 4×4 PNG.
fn png(w: u32, h: u32) -> Vec<u8> {
    // Minimal valid PNG header + IHDR is enough for the size probe.
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    out.extend_from_slice(&13u32.to_be_bytes());
    out.extend_from_slice(b"IHDR");
    out.extend_from_slice(&w.to_be_bytes());
    out.extend_from_slice(&h.to_be_bytes());
    out.extend_from_slice(&[8, 6, 0, 0, 0]);
    out.extend_from_slice(&[0, 0, 0, 0]);
    out
}

fn write_skin(dir: &Path, manifest: &str, files: &[(&str, Vec<u8>)]) {
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join(MANIFEST), manifest).unwrap();
    for (path, bytes) in files {
        let p = dir.join(path);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, bytes).unwrap();
    }
    if !files.iter().any(|(p, _)| *p == PACKAGE_MANIFEST)
        && let Ok(table) = manifest.parse::<TomlTable>()
        && let Some(value) = table.get("meta")
        && let Ok(meta) = value.clone().try_into::<Meta>()
    {
        let mut dependencies = std::collections::BTreeMap::new();
        if let Some(base) = &meta.base {
            let registry = SkinRegistry::new(None, APP);
            let version = registry
                .load(base)
                .ok()
                .map(|l| l.skin.meta.version.clone())
                .or_else(|| {
                    PackageManifest::read(&DirFiles::new(dir.parent()?.join(base)))
                        .ok()
                        .map(|p| p.meta.version)
                })
                .unwrap_or_else(|| "1.0.0".into());
            dependencies.insert(base.clone(), version);
        }
        let package = PackageManifest {
            format: PACKAGE_FORMAT,
            meta,
            capabilities: vec![PackageCapability::Skin],
            dependencies,
            catalogue_order: 0,
            lua: None,
            data: Default::default(),
            created_at: String::new(),
            licensed_to: String::new(),
        };
        fs::write(
            dir.join(PACKAGE_MANIFEST),
            toml::to_string_pretty(&package).unwrap(),
        )
        .unwrap();
    }
}

fn load_dir(dir: &Path) -> Result<LoadedSkin, Vec<SkinDiagnostic>> {
    let registry = SkinRegistry::new(None, APP);
    struct R<'a>(&'a SkinRegistry);
    impl BaseResolver for R<'_> {
        fn base(&self, id: &str) -> Option<(TomlTable, Arc<dyn SkinFiles>)> {
            let loaded = self.0.load(id).ok()?;
            Some((read_manifest(&*loaded.files).ok()?, loaded.files))
        }
    }
    load(
        Arc::new(DirFiles::new(dir)),
        SkinOrigin::Directory(dir.to_path_buf()),
        &R(&registry),
        APP,
    )
}

fn errors(result: Result<LoadedSkin, Vec<SkinDiagnostic>>) -> Vec<String> {
    match result {
        Ok(_) => Vec::new(),
        Err(d) => d
            .into_iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| d.field)
            .collect(),
    }
}

#[test]
fn builtin_catalogue_defers_effects_and_reuses_validated_models_across_scans() {
    let mut registry = SkinRegistry::new(None, APP);
    let mut loaded = Vec::new();
    for entry in registry.entries() {
        assert!(entry.skin.as_ref().unwrap().shader_programs.is_empty());
        let a = registry.load(&entry.id).unwrap();
        let b = registry.load(&entry.id).unwrap();
        assert!(Arc::ptr_eq(&a.skin, &b.skin), "{}", entry.id);
        assert!(Arc::ptr_eq(&a.files, &b.files), "{}", entry.id);
        assert_eq!(a.skin.shader_programs.len(), a.skin.shaders.len());
        loaded.push(a);
    }
    registry.rescan();
    for previous in loaded {
        let current = registry.load(&previous.skin.meta.id).unwrap();
        assert!(Arc::ptr_eq(&previous.skin, &current.skin));
    }
}

#[test]
fn directory_loads_and_inherited_edits_remain_fresh_without_rescan() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("user.base");
    let child = tmp.path().join("user.child");
    let manifest = "[meta]\nid='user.base'\n[dialogue.text]\ncolor='#123456'\n";
    write_skin(&base, manifest, &[]);
    write_skin(&child, "[meta]\nid='user.child'\nbase='user.base'\n", &[]);
    let mut registry = SkinRegistry::new(Some(tmp.path().to_owned()), APP);
    let old = registry.load("user.child").unwrap();
    let mut watched = old.files.watch_roots();
    watched.sort();
    let mut expected = vec![base.clone(), child.clone()];
    expected.sort();
    assert_eq!(watched, expected);
    let scanned = registry.load_scanned("user.child").unwrap();
    assert!(Arc::ptr_eq(
        &scanned.skin,
        &registry.load_scanned("user.child").unwrap().skin
    ));
    // Same byte length: neither an id cache nor a size-only resource cache
    // may hide an edit to an ancestor.
    write_skin(&base, &manifest.replace("#123456", "#654321"), &[]);
    let new = registry.load("user.child").unwrap();
    assert_ne!(old.skin.dialogue.text.color, new.skin.dialogue.text.color);
    registry.rescan();
    assert_ne!(
        scanned.skin.dialogue.text.color,
        registry
            .load_scanned("user.child")
            .unwrap()
            .skin
            .dialogue
            .text
            .color
    );
    assert_eq!(
        registry
            .entry("user.child")
            .unwrap()
            .skin
            .as_ref()
            .unwrap()
            .dialogue
            .text
            .color,
        new.skin.dialogue.text.color
    );
    write_skin(&base, "invalid TOML = [", &[]);
    assert!(registry.load("user.child").is_err());
    registry.rescan();
    assert!(registry.load_scanned("user.child").is_err());
    // Already loaded content stays usable after a failed authoring edit.
    assert_eq!(new.skin.meta.id, "user.child");
}

#[test]
fn unchanged_user_effects_share_validation_and_decoding_but_changed_sources_are_rechecked() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("user.effect");
    write_skin(
        &dir,
        "[meta]\nid='user.effect'\nbase='nanatype.tidal'\n[shaders.artwork]\nsource='effect.wgsl'\n",
        &[(
            "effect.wgsl",
            b"fn effect(i:EffectInput)->vec4<f32>{return vec4(0.);}".to_vec(),
        )],
    );
    let mut registry = SkinRegistry::new(Some(tmp.path().to_owned()), APP);
    let a = registry.load("user.effect").unwrap();
    for _ in 0..4 {
        registry.rescan();
        let b = registry.load_scanned("user.effect").unwrap();
        assert!(Arc::ptr_eq(
            &a.skin.shader_programs["artwork"],
            &b.skin.shader_programs["artwork"]
        ));
    }
    fs::write(
        dir.join("effect.wgsl"),
        "fn effect(i:EffectInput)->vec4<f32>{return vec4(1.);}",
    )
    .unwrap();
    let changed = registry.load("user.effect").unwrap();
    assert!(!Arc::ptr_eq(
        &a.skin.shader_programs["artwork"],
        &changed.skin.shader_programs["artwork"]
    ));
    fs::write(
        dir.join("effect.wgsl"),
        "fn effect(i:EffectInput)->vec4<f32>{loop{} return vec4(1.);}",
    )
    .unwrap();
    assert!(registry.load("user.effect").is_err());
}

#[test]
fn compact_standalone_manifests_preserve_all_builtin_designs() {
    let registry = SkinRegistry::new(None, APP);
    let tmp = tempfile::tempdir().unwrap();
    for entry in registry.entries() {
        let loaded = registry.load(&entry.id).unwrap();
        let table = standalone_manifest(&loaded.skin).unwrap();
        let compact = toml::to_string_pretty(&table).unwrap();
        assert!(compact.len() < toml::to_string_pretty(&*loaded.skin).unwrap().len());
        let mut files = Vec::new();
        for path in loaded.files.list().into_iter().filter(|p| p != MANIFEST) {
            files.push((path.clone(), loaded.files.read(&path).unwrap()));
        }
        let refs: Vec<_> = files.iter().map(|(p, b)| (p.as_str(), b.clone())).collect();
        let dir = tmp.path().join(&entry.id);
        write_skin(&dir, &compact, &refs);
        let restored = load(Arc::new(DirFiles::new(dir)), SkinOrigin::Builtin, &(), APP).unwrap();
        let mut expected = (*loaded.skin).clone();
        expected.meta.base = None;
        assert_eq!(*restored.skin, expected, "{}", entry.id);
    }
}

#[test]
fn every_builtin_loads_without_warnings() {
    let registry = SkinRegistry::new(None, APP);
    assert_eq!(registry.entries().len(), 28);
    let mut ids: Vec<&str> = registry.entries().iter().map(|e| e.id.as_str()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 28);
    assert!(registry.load(DEFAULT_SKIN).is_ok());
    for entry in registry.entries() {
        assert!(entry.loads(), "{}: {:?}", entry.id, entry.errors);
        assert!(
            entry.warnings.is_empty(),
            "{}: {:?}",
            entry.id,
            entry.warnings
        );
        let skin = registry.load(&entry.id).unwrap().skin;
        for locale in ["zh-CN", "en-US", "ja-JP"] {
            assert!(skin.meta.name.contains_key(locale), "{} {locale}", entry.id);
            assert!(
                skin.meta.description.contains_key(locale),
                "{} {locale}",
                entry.id
            );
        }
        assert!(!skin.meta.tags.is_empty(), "{}", entry.id);
        for tag in &skin.meta.tags {
            assert!(SKIN_TAGS.contains(&tag.as_str()), "{}: {tag}", entry.id);
        }
    }
    let night = registry.load("nanatype.night").unwrap();
    assert_eq!(night.skin.chrome.appearance, Appearance::Dark);
    assert_eq!(
        night.skin.dialogue.box_.border_color,
        Rgba8::rgb(0x5c, 0xb8, 0xff)
    );
    // Untouched fields keep the defaults.
    assert_eq!(night.skin.dialogue.region, [760.0, 150.0]);
    // A partial `[history.text]` keeps the history defaults, not the dialogue's.
    assert_eq!(night.skin.history.text.size, 18.0);
    let bubble = registry.load("nanatype.bubble").unwrap();
    assert!(bubble.skin.dialogue.tail.is_some());
    let ink = registry.load("nanatype.ink").unwrap();
    assert!(ink.skin.dialogue.decor.iter().any(|d| d.image.is_some()));
    let echo = registry.load("nanatype.echo_vanilla").unwrap();
    assert_eq!(echo.skin.dialogue.decor.len(), 8);
    assert!(echo.skin.dialogue.continue_.glyph.is_empty());
    assert_eq!(echo.skin.history.layout, HistoryLayout::Inline);
    assert!(echo.skin.history.time_seconds);
    assert_eq!(echo.skin.history.title.as_ref().unwrap().text, "HISTORY");
    assert_eq!(echo.skin.history.decor.len(), 2);
    assert!(echo.skin.history.hide_latest);
    // Skins that say nothing keep the bubble list.
    assert_eq!(night.skin.history.layout, HistoryLayout::Stack);
    assert!(night.skin.history.panel.is_none());
    assert!(!night.skin.history.hide_latest);
}

#[test]
fn sonnet_declares_packaged_calligraphy_and_finite_whole_text_dissolve() {
    let loaded = SkinRegistry::new(None, APP)
        .load("nanatype.sonnet")
        .unwrap();
    let skin = &loaded.skin;
    assert_eq!(skin.dialogue.box_.color.a, 0);
    assert_eq!(skin.dialogue.reveal.entrance.ms, 360);
    assert_eq!(skin.dialogue.reveal.entrance.scope, TransitionScope::Glyph);
    let exit = skin.dialogue.reveal.exit.as_ref().unwrap();
    assert_eq!(exit.ms, 700);
    assert_eq!(exit.scope, TransitionScope::Text);
    assert!(
        skin.material_programs
            .contains_key(exit.material.as_ref().unwrap())
    );
    assert_eq!(
        skin.fonts.latin.as_ref().unwrap().file.as_deref(),
        Some("builtin:fonts/Tangerine-Bold.ttf")
    );
    let standalone = standalone_manifest(skin).unwrap();
    let restored: Skin = toml::Value::Table(standalone).try_into().unwrap();
    assert_eq!(restored.fonts.latin, skin.fonts.latin);
    assert_eq!(restored.dialogue.reveal.light, skin.dialogue.reveal.light);
    let dir = tempfile::tempdir().unwrap();
    write_skin(
        dir.path(),
        "[meta]\nid='user.bad-light'\n[dialogue.reveal.light]\nintensity=3\nradius=-1\n",
        &[],
    );
    let errors = load_dir(dir.path()).unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.field == "dialogue.reveal.light.intensity")
    );
    assert!(
        errors
            .iter()
            .any(|e| e.field == "dialogue.reveal.light.radius")
    );
}

#[test]
fn machine_learning_scramble_loads_roundtrips_and_rejects_invalid_timing() {
    let registry = SkinRegistry::new(None, APP);
    let loaded = registry.load("nanatype.machine_learning").unwrap();
    assert!(loaded.warnings.is_empty());
    let spec = loaded.skin.dialogue.reveal.scramble.as_ref().unwrap();
    assert_eq!(spec, &ScrambleSpec::default());
    assert_eq!(loaded.skin.dialogue.region, [860., 200.]);
    assert_eq!(loaded.skin.dialogue.reveal.exit.as_ref().unwrap().ms, 400);
    assert!(loaded.skin.shader_programs.contains_key("network"));
    let manifest = standalone_manifest(&loaded.skin).unwrap();
    let restored: Skin = toml::Value::Table(manifest).try_into().unwrap();
    assert_eq!(restored.dialogue.reveal.scramble.as_ref(), Some(spec));
    for invalid in [
        "interval_ms=0",
        "entrance_ms=0",
        "exit_hold_ms=400",
        "alphabet=''",
        "alphabet='汉字'",
    ] {
        let dir = tempfile::tempdir().unwrap();
        write_skin(
            dir.path(),
            &format!("[meta]\nid='user.bad-scramble'\n[dialogue.reveal.scramble]\n{invalid}\n"),
            &[],
        );
        let errors = load_dir(dir.path()).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| e.field.starts_with("dialogue.reveal.scramble")),
            "{invalid}: {errors:?}"
        );
    }
}

#[test]
fn shader_packages_roundtrip_and_inherit_validated_program_bytes() {
    let tmp = tempfile::tempdir().unwrap();
    let mut registry = SkinRegistry::new(Some(tmp.path().join("skins")), APP);
    for name in [
        "gothic",
        "celestial",
        "forest",
        "tidal",
        "lantern",
        "citrus",
    ] {
        let id = format!("nanatype.{name}");
        let original = registry.load(&id).unwrap();
        assert_eq!(original.skin.meta.schema, SKIN_SCHEMA);
        assert_eq!(original.skin.shader_programs.len(), 3);
        assert!(original.skin.dialogue.decor.is_empty());
        assert_eq!(original.skin.dialogue.shader_effects.len(), 1);
        assert_eq!(
            original.skin.dialogue.shader_effects[0].target,
            ShaderTarget::Artwork
        );
        assert!(original.skin.shaders["artwork"].params.is_empty());
        let responsive = responsive_dialogue(&original.skin, [952., 252.]);
        assert_eq!(responsive.dialogue.shader_effects[0].shader, "artwork");
        assert!(responsive.shaders["artwork"].static_fallback);
        let portrait = responsive_dialogue(&original.skin, [300., 1200.]);
        assert_eq!(
            portrait.dialogue.shader_effects[0].shader,
            "responsive_artwork"
        );
        assert_eq!(portrait.shaders["responsive_artwork"].params, [1., 1.]);
        let compact = responsive_dialogue(&original.skin, [300., 240.]);
        assert_eq!(
            compact.dialogue.shader_effects[0].shader,
            "responsive_compact"
        );
        assert_eq!(original.skin.shaders["artwork"].textures.len(), 1);
        let file = tmp.path().join(format!("{name}.ntpkg"));
        registry.export(&id, &file).unwrap();
        let loaded = registry.inspect_archive(&file).unwrap();
        assert_eq!(loaded.skin.shader_programs, original.skin.shader_programs);
        let texture = &original.skin.shaders["artwork"].textures[0];
        assert_eq!(
            loaded.files.read(texture).unwrap(),
            original.files.read(texture).unwrap()
        );
        let fallback = &original.skin.dialogue.box_.frame.as_ref().unwrap().image;
        assert_eq!(
            loaded.files.read(fallback).unwrap(),
            original.files.read(fallback).unwrap()
        );
        let still = image::load_from_memory(&loaded.files.read(fallback).unwrap())
            .unwrap()
            .to_rgba8();
        assert_eq!(still.dimensions(), (920, 220));
        assert_eq!(
            still.get_pixel(0, 0).0[3],
            0,
            "{name}: transparent canvas edge"
        );
        assert_eq!(
            still.get_pixel(460, 110).0[3],
            255,
            "{name}: static reading plane missing"
        );
        registry
            .duplicate(
                &id,
                &format!("my.{name}"),
                &Localized::from([("en-US".into(), name.into())]),
                &TomlTable::new(),
            )
            .unwrap();
        assert_eq!(
            registry
                .load(&format!("my.{name}"))
                .unwrap()
                .skin
                .shader_programs,
            original.skin.shader_programs
        );
    }
    let artwork_sample =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/lunar-atlas.ntpkg");
    registry.install(&artwork_sample).unwrap();
    let sample = registry.load("example.lunar_atlas").unwrap();
    assert!(sample.skin.meta.base.is_none());
    assert_eq!(
        sample.skin.dialogue.shader_effects[0].target,
        ShaderTarget::Artwork
    );
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/aurora.ntpkg");
    registry.install(&file).unwrap();
    registry
        .duplicate(
            "example.aurora",
            "my.detached",
            &Localized::from([("en-US".into(), "Detached".into())]),
            &TomlTable::new(),
        )
        .unwrap();
    let detached = tmp.path().join("detached.ntpkg");
    registry.export("my.detached", &detached).unwrap();
    registry.remove("my.detached").unwrap();
    registry.remove("example.aurora").unwrap();
    registry.install(&detached).unwrap();
    let loaded = registry.load("my.detached").unwrap();
    assert!(loaded.skin.meta.base.is_none());
    // Halo has static responsive print variants; the example contributes the only active base effect.
    assert!(loaded.skin.shader_programs.contains_key("aurora"));
    assert_eq!(loaded.skin.shader_programs.len(), 4);
    assert_eq!(loaded.skin.dialogue.shader_effects.len(), 1);
    assert_eq!(loaded.skin.dialogue.shader_effects[0].shader, "aurora");
    registry.install(&file).unwrap();
    assert_eq!(
        registry
            .load("example.aurora")
            .unwrap()
            .skin
            .shader_programs
            .len(),
        4
    );
    // Exercise named-table merging using an actually animated base, rather
    // than requiring the default minimal skin to carry an unrelated effect.
    write_skin(
        &tmp.path().join("skins/my.merge"),
        "[meta]\nschema=3\nid='my.merge'\nbase='nanatype.night'\n[dialogue]\nshader_effects=[{shader='extra',target='background'}]\n[shaders.extra]\nsource='extra.wgsl'\n",
        &[(
            "extra.wgsl",
            b"fn effect(i:EffectInput)->vec4<f32>{return vec4(i.accent.rgb,.1);}".to_vec(),
        )],
    );
    registry.rescan();
    let merged = registry.load("my.merge").unwrap();
    assert_eq!(merged.skin.shader_programs.len(), 5);
    assert_eq!(merged.skin.dialogue.shader_effects.len(), 1);
    let archive = tmp.path().join("merged.ntpkg");
    registry.export("my.merge", &archive).unwrap();
    let exported = registry.inspect_archive(&archive).unwrap();
    assert_eq!(exported.skin.shader_programs, merged.skin.shader_programs);
    assert!(exported.skin.meta.base.is_none());
}

#[test]
fn classic_artwork_exports_its_complete_static_fallback_and_keeps_special_layouts() {
    let tmp = tempfile::tempdir().unwrap();
    let registry = SkinRegistry::new(Some(tmp.path().join("skins")), APP);
    for name in [
        "halo",
        "night",
        "parchment",
        "adv",
        "rpg",
        "vanilla",
        "bubble",
        "livechat",
        "comic",
        "terminal",
        "pixel",
        "sakura",
        "ink",
        "echo_vanilla",
        "echo_glass",
        "echo_bubble",
    ] {
        let id = format!("nanatype.{name}");
        let original = registry.load(&id).unwrap();
        let skin = &original.skin;
        assert_eq!(skin.meta.schema, SKIN_SCHEMA, "{name}");
        let animated = [
            "night", "rpg", "bubble", "livechat", "terminal", "pixel", "sakura",
        ]
        .contains(&name);
        assert_eq!(
            skin.shader_programs.len(),
            usize::from(animated) + if name == "echo_glass" { 0 } else { 3 },
            "{name}"
        );
        assert_eq!(
            skin.dialogue.shader_effects.len(),
            usize::from(animated),
            "{name}"
        );
        if animated {
            assert_eq!(
                skin.dialogue.shader_effects[0].target,
                ShaderTarget::Artwork
            );
            assert!(skin.shaders["artwork"].params.is_empty());
            assert!(skin.shaders["artwork"].textures.len() <= 1);
        } else {
            assert!(
                skin.shaders
                    .values()
                    .all(|s| s.static_fallback && !s.animated),
                "{name}: static print must not schedule animation"
            );
        }
        let archive = tmp.path().join(format!("{name}.ntpkg"));
        registry.export(&id, &archive).unwrap();
        let exported = registry.inspect_archive(&archive).unwrap();
        assert_eq!(exported.skin.shader_programs, skin.shader_programs);
        for (_, resource, _) in skin.referenced_files() {
            if !resource.starts_with("builtin:") {
                assert_eq!(
                    exported.files.read(resource).unwrap(),
                    original.files.read(resource).unwrap(),
                    "{name}: {resource}"
                );
            }
        }
        if name == "echo_glass" {
            assert!(skin.dialogue.box_.frame.is_none());
            assert!(skin.dialogue.box_.shape.gradient.is_some());
            assert_eq!(skin.dialogue.box_.color.a, 0);
            continue;
        }
        let frame = skin.dialogue.box_.frame.as_ref().unwrap();
        assert!(
            !frame.layout_inset,
            "{name}: plate must preserve native padding"
        );
        assert!(skin.has_dialogue_artwork());
        let image = &frame.image;
        let still = image::load_from_memory(&original.files.read(image).unwrap())
            .unwrap()
            .to_rgba8();
        assert!(
            still.width() > 600 && still.height() > 90,
            "{name}: placeholder plate"
        );
        assert!(still.pixels().any(|p| p.0[3] > 100), "{name}: empty plate");
    }
    for name in ["subtitle", "echo_void"] {
        let skin = registry.load(&format!("nanatype.{name}")).unwrap().skin;
        assert!(
            skin.shaders.is_empty(),
            "transparent typography should sleep"
        );
        assert_eq!(skin.dialogue.box_.color.a, 0);
        assert!(skin.dialogue.box_.frame.is_none());
    }
    for name in ["bubble", "comic", "sakura"] {
        assert!(
            registry
                .load(&format!("nanatype.{name}"))
                .unwrap()
                .skin
                .dialogue
                .tail
                .is_some()
        );
    }
}

#[test]
fn shader_manifests_fail_closed_on_schema_resources_and_limits() {
    let tmp = tempfile::tempdir().unwrap();
    let shader = b"fn effect(i:EffectInput)->vec4<f32>{return vec4(0.);}".to_vec();
    let good = "[meta]\nschema=3\nid='shader-test'\n[shaders.test]\nsource='effect.wgsl'\n[[dialogue.shader_effects]]\nshader='test'\n";
    for manifest in [
        good.replace("schema=3", "schema=1"),
        good.replace("effect.wgsl", "../effect.wgsl"),
        good.replace("effect.wgsl", "missing.wgsl"),
        good.replace(
            "source='effect.wgsl'",
            "source='effect.wgsl'\nparams=[1,2,3,4,5,6,7,8,9]",
        ),
        good.replace(
            "source='effect.wgsl'",
            "source='effect.wgsl'\nintensity=nan",
        ),
        good.replace("shader='test'", "shader='missing'"),
        format!(
            "{good}{}",
            "[[dialogue.shader_effects]]\nshader='test'\n".repeat(4)
        ),
        good.replace(
            "source='effect.wgsl'",
            "source='effect.wgsl'\ntextures=['big.png']",
        ),
    ] {
        write_skin(
            tmp.path(),
            &manifest,
            &[("effect.wgsl", shader.clone()), ("big.png", png(2049, 16))],
        );
        assert!(load_dir(tmp.path()).is_err(), "accepted {manifest}");
    }
    write_skin(tmp.path(), good, &[("effect.wgsl", shader)]);
    assert!(load_dir(tmp.path()).is_ok());
}

#[test]
fn decoration_images_validate_packaged_paths_and_bytes() {
    let tmp = tempfile::tempdir().unwrap();
    let manifest = "[meta]\nid='image-decor'\n[[dialogue.decor]]\nimage='stroke.png'\n[[history.decor]]\nimage='stroke.png'\n";
    write_skin(tmp.path(), manifest, &[("stroke.png", png(64, 16))]);
    let loaded = load_dir(tmp.path()).unwrap();
    assert!(loaded.warnings.is_empty());
    assert_eq!(
        loaded
            .skin
            .referenced_files()
            .iter()
            .filter(|(_, path, _)| *path == "stroke.png")
            .count(),
        2
    );
    fs::remove_file(tmp.path().join("stroke.png")).unwrap();
    let fields = errors(load_dir(tmp.path()));
    assert!(fields.contains(&"dialogue.decor.image".to_owned()));
    assert!(fields.contains(&"history.decor.image".to_owned()));
    fs::write(
        tmp.path().join(MANIFEST),
        "[meta]\nid='unsafe-decor'\n[[dialogue.decor]]\nimage='../escape.png'\n",
    )
    .unwrap();
    assert!(errors(load_dir(tmp.path())).contains(&"dialogue.decor.image".to_owned()));
}

#[test]
fn builtin_ink_artwork_and_provenance_survive_export_and_inheritance() {
    let tmp = tempfile::tempdir().unwrap();
    let mut registry = SkinRegistry::new(Some(tmp.path().join("skins")), APP);
    let original = registry.load("nanatype.ink").unwrap();
    let archive = tmp.path().join("ink.ntpkg");
    registry.export("nanatype.ink", &archive).unwrap();
    let exported = registry.inspect_archive(&archive).unwrap();
    let names = Localized::from([("en-US".to_owned(), "My Ink".to_owned())]);
    registry
        .duplicate("nanatype.ink", "my-ink", &names, &TomlTable::new())
        .unwrap();
    let inherited = registry.load("my-ink").unwrap();
    for path in ["bamboo.png", "ART-LICENSE.txt"] {
        let bytes = original.files.read(path).unwrap();
        assert_eq!(exported.files.read(path).unwrap(), bytes);
        assert_eq!(inherited.files.read(path).unwrap(), bytes);
    }
    assert!(exported.warnings.is_empty());
}

#[test]
fn new_shape_fields_parse_without_warnings() {
    let tmp = tempfile::tempdir().unwrap();
    write_skin(
        tmp.path(),
        r##"
[meta]
id = "x"
tags = ["cute"]
[dialogue.box]
gradient = { kind = "radial", circle = true, stops = [[0, "#fff"], [0.5, "box"], [1.0, "#00000080"]] }
radii = [4, 18, 18, 18]
border_widths = [0, 0, 0, 4]
border_style = "dashed"
shadows = [{ color = "#000", dx = 4, dy = 4 }, { color = "accent", blur = 8, inset = true }]
margin = [0, 0, 24, 0]
[dialogue.tail]
side = "left"
at = 0.5
[[dialogue.decor]]
anchor = "bottom_right"
size = [4, "56%"]
polygon = [[0, 0], [100, 0], [50, 100]]
text = "✦"
text_color = "name_text"
when = "speaker"
[[dialogue.decor]]
inset = [5, 5, 5, 5]
border_color = "border"
border_width = 1
[dialogue.nameplate]
anchor = "top_right"
rotate = -3
border_color = "#000"
border_width = 2
text_shadow = { color = "#000", blur = 4 }
[dialogue.continue]
anchor = "bottom"
offset = [0, -6]
motion = "pulse"
[dialogue.text]
align = "center"
[history]
bubble_radii = [0, 8, 8, 8]
name_plate = { color = "accent", radius = 6 }
avatar = { shape = "circle", ring_color = "#fff", ring_width = 2 }
"##,
        &[],
    );
    let loaded = load_dir(tmp.path()).unwrap();
    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    let d = &loaded.skin.dialogue;
    let g = d.box_.shape.gradient.as_ref().unwrap();
    assert_eq!(g.kind, GradientKind::Radial);
    assert_eq!(g.stops[1], (0.5, ColorRef::Role(ColorRole::Box)));
    assert_eq!(d.decor[0].size, [Len::Px(4.0), Len::Percent(56.0)]);
    assert_eq!(d.decor[0].text_color, ColorRef::Role(ColorRole::NameText));
    assert_eq!(d.tail.as_ref().unwrap().side, Edge::Left);
    assert_eq!(d.continue_.motion, ContinueMotion::Pulse);
    assert_eq!(d.text.align, TextAlign::Center);
    assert_eq!(loaded.skin.meta.tags, ["cute"]);
}

#[test]
fn default_dialogue_is_unstyled_and_has_no_continue_animation() {
    let skin = Skin::default();
    let d = &skin.dialogue;
    assert!(d.tail.is_none());
    assert!(d.decor.is_empty());
    assert!(d.box_.shape.gradient.is_none());
    assert_eq!(d.box_.margin, [0.0; 4]);
    assert_eq!(d.continue_.offset, [-14.0, -8.0]);
    assert_eq!(d.continue_.motion, ContinueMotion::None);
    assert_eq!(d.box_.color.a, 0);
    assert_eq!(d.box_.border_width, 0.0);
    assert!(d.continue_.glyph.is_empty());
    assert_eq!(d.nameplate.anchor, Anchor::TopLeft);
}

#[test]
fn colour_refs_and_lengths_parse() {
    #[derive(serde::Deserialize)]
    struct T {
        c: ColorRef,
        l: Len,
    }
    let t: T = toml::from_str(
        r##"c = "#ff000080"
l = 24"##,
    )
    .unwrap();
    assert_eq!(t.c, ColorRef::Rgba(Rgba8::rgba(255, 0, 0, 128)));
    assert_eq!(t.l, Len::Px(24.0));
    let t: T = toml::from_str(
        r#"c = "name_text"
l = "50%""#,
    )
    .unwrap();
    assert_eq!(t.c, ColorRef::Role(ColorRole::NameText));
    assert_eq!(t.l, Len::Percent(50.0));
    let t: T = toml::from_str(
        r#"c = "box"
l = 24.5"#,
    )
    .unwrap();
    assert_eq!(t.l, Len::Px(24.5));
    for bad in [
        r#"c = "boxy"
l = 1"#,
        r#"c = "box"
l = "50""#,
        r#"c = "box"
l = "abc%""#,
    ] {
        assert!(toml::from_str::<T>(bad).is_err(), "{bad}");
    }
}

#[test]
fn unknown_keys_inside_decor_warn() {
    let tmp = tempfile::tempdir().unwrap();
    write_skin(
        tmp.path(),
        r#"[meta]
id = "x"
[[dialogue.decor]]
text = "a"
[[dialogue.decor]]
sparkle = true
"#,
        &[],
    );
    let skin = load_dir(tmp.path()).unwrap();
    assert_eq!(skin.warnings.len(), 1, "{:?}", skin.warnings);
    assert_eq!(skin.warnings[0].field, "dialogue.decor.1.sparkle");
}

#[test]
fn decor_motion_loads_and_validates_nested_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let manifest = |motion: &str| {
        format!("[meta]\nid = \"motion-test\"\n[[dialogue.decor]]\nmotion = {{ {motion} }}\n")
    };
    write_skin(
        tmp.path(),
        &manifest("kind = \"breathe\", period_s = 6, amplitude = 0.12, phase = 0.25"),
        &[],
    );
    let loaded = load_dir(tmp.path()).unwrap();
    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    let motion = loaded.skin.dialogue.decor[0].motion.unwrap();
    assert_eq!(motion.kind, DecorMotionKind::Breathe);
    assert_eq!(motion.phase, 0.25);
    for (config, field) in [
        ("kind = \"float\", period_s = 0", "period_s"),
        ("kind = \"float\", amplitude = nan", "amplitude"),
        ("kind = \"float\", amplitude = 33", "amplitude"),
        ("kind = \"breathe\", amplitude = 1.1", "amplitude"),
        ("kind = \"float\", phase = -0.1", "phase"),
    ] {
        write_skin(tmp.path(), &manifest(config), &[]);
        assert!(
            errors(load_dir(tmp.path())).contains(&format!("dialogue.decor.0.motion.{field}")),
            "{config}"
        );
    }
    write_skin(tmp.path(), &manifest("kind = \"float\", typo = 1"), &[]);
    let loaded = load_dir(tmp.path()).unwrap();
    assert_eq!(loaded.warnings[0].field, "dialogue.decor.0.motion.typo");
    write_skin(
        tmp.path(),
        "[meta]\nid = \"old-skin\"\n[[dialogue.decor]]\nrotate = 15\n",
        &[],
    );
    assert!(
        load_dir(tmp.path()).unwrap().skin.dialogue.decor[0]
            .motion
            .is_none()
    );
}

#[test]
fn decoration_loops_are_periodic_bounded_and_rest_under_reduced_motion() {
    for (kind, amplitude) in [
        (DecorMotionKind::Float, 2.0),
        (DecorMotionKind::Breathe, 0.15),
    ] {
        let motion = DecorMotionSpec {
            kind,
            amplitude,
            period_s: 8.0,
            phase: 0.35,
        };
        let start = decor_motion(&motion, 0.0, false);
        for i in 0..=96 {
            let t = f64::from(i) / 12.0;
            let (dy, alpha) = decor_motion(&motion, t, false);
            assert!(dy.abs() <= 2.0 && (0.85..=1.0).contains(&alpha));
            assert_eq!(decor_motion(&motion, t, true), (0.0, 1.0));
        }
        let end = decor_motion(&motion, 8.0, false);
        assert!((start.0 - end.0).abs() < 1e-5 && (start.1 - end.1).abs() < 1e-5);
    }
}

#[test]
fn vector_presets_are_opt_in_and_unknown_presets_are_rejected() {
    let old: Skin = toml::from_str("[meta]\nid = 'old'").unwrap();
    assert!(old.dialogue.box_.shape.contour.is_none());
    assert!(old.history.bubble_contour.is_none());
    let tmp = tempfile::tempdir().unwrap();
    write_skin(
        tmp.path(),
        "[meta]\nid='vector'\n[dialogue.box]\ncontour='cloud'\n[[dialogue.decor]]\nmotif='blossom'\n[history]\nbubble_contour='chat'\nname_plate={contour='ribbon'}\n",
        &[],
    );
    let loaded = load_dir(tmp.path()).unwrap();
    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    assert_eq!(
        loaded.skin.dialogue.box_.shape.contour,
        Some(ContourPreset::Cloud)
    );
    assert_eq!(
        loaded.skin.dialogue.decor[0].motif,
        Some(MotifPreset::Blossom)
    );
    assert_eq!(
        loaded
            .skin
            .history
            .name_plate
            .as_ref()
            .unwrap()
            .shape
            .contour,
        Some(ContourPreset::Ribbon)
    );
    for invalid in [
        "[dialogue.box]\ncontour='missing'",
        "[[dialogue.decor]]\nmotif='script()'",
        "[history]\nbubble_contour='missing'",
    ] {
        write_skin(tmp.path(), &format!("[meta]\nid='bad'\n{invalid}"), &[]);
        assert!(load_dir(tmp.path()).is_err());
    }
}

#[test]
fn contours_are_finite_closed_and_each_tail_is_part_of_the_outline() {
    for preset in ContourPreset::ALL {
        assert!(
            contour_path(preset, [0.0, 0.0], 20.0, 2.0, None)
                .commands
                .is_empty()
        );
        for size in [[1.0, 1.0], [24.0, 18.0], [760.0, 150.0], [1280.0, 180.0]] {
            let path = contour_path(preset, size, 20.0, 1.5, None);
            let [x, y, w, h] = path.bounds().unwrap();
            assert!(
                [x, y, w, h].into_iter().all(f32::is_finite),
                "{preset:?} {size:?}"
            );
            assert!(
                x >= 0.0 && y >= 0.0 && x + w <= size[0] + 0.01 && y + h <= size[1] + 0.01,
                "{preset:?} {size:?}: {:?}",
                path.bounds()
            );
            assert!(matches!(path.commands.last(), Some(ShapeCommand::Close)));
        }
        for side in [Edge::Top, Edge::Right, Edge::Bottom, Edge::Left] {
            let tail = TailSpec {
                side,
                width: 32.0,
                length: 22.0,
                lean: -8.0,
                ..Default::default()
            };
            let path = contour_path(preset, [760.0, 150.0], 20.0, 0.0, Some(&tail));
            assert_eq!(
                path.commands
                    .iter()
                    .filter(|c| matches!(c, ShapeCommand::Move(_)))
                    .count(),
                1
            );
            assert_eq!(
                path.commands
                    .iter()
                    .filter(|c| matches!(c, ShapeCommand::Close))
                    .count(),
                1
            );
            let [x, y, w, h] = path.bounds().unwrap();
            match side {
                Edge::Top => assert!(y <= -22.0),
                Edge::Right => assert!(x + w >= 782.0),
                Edge::Bottom => assert!(y + h >= 172.0),
                Edge::Left => assert!(x <= -22.0),
            }
        }
    }
}

#[test]
fn motifs_are_deterministic_bounded_and_have_no_external_resources() {
    for motif in MotifPreset::ALL {
        for size in [[1.0, 1.0], [32.0, 24.0], [700.0, 9.0]] {
            let parts = motif_paths(motif, size);
            assert_eq!(parts, motif_paths(motif, size));
            assert!(!parts.is_empty());
            for part in parts {
                let [x, y, w, h] = part.path.bounds().unwrap();
                assert!([x, y, w, h].into_iter().all(f32::is_finite));
                assert!(
                    x >= -size[0] * 0.1
                        && y >= -size[1] * 0.1
                        && x + w <= size[0] * 1.2
                        && y + h <= size[1] * 1.2,
                    "{motif:?}: {:?}",
                    part.path.bounds()
                );
            }
        }
    }
}

#[test]
fn too_many_decorations_fail() {
    let tmp = tempfile::tempdir().unwrap();
    let mut manifest = String::from("[meta]\nid = \"x\"\n");
    for _ in 0..17 {
        manifest.push_str("[[dialogue.decor]]\nradius = 1\n");
    }
    write_skin(tmp.path(), &manifest, &[]);
    assert!(errors(load_dir(tmp.path())).contains(&"dialogue.decor".to_owned()));
}

#[test]
fn decor_lists_replace_on_inheritance() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("a");
    write_skin(
        &dir,
        r#"[meta]
id = "a"
base = "nanatype.ink"
[[dialogue.decor]]
text = "x"
"#,
        &[],
    );
    let skin = load_dir(&dir).unwrap();
    assert_eq!(skin.skin.dialogue.decor.len(), 1);
    // Package metadata belongs to the child, including its catalogue tags.
    assert!(skin.skin.meta.tags.is_empty());
    let dir = tmp.path().join("b");
    write_skin(
        &dir,
        r#"[meta]
id = "b"
base = "nanatype.ink"
[dialogue]
decor = []
"#,
        &[],
    );
    assert!(load_dir(&dir).unwrap().skin.dialogue.decor.is_empty());
}

#[test]
fn tails_open_into_the_box() {
    let tail = TailSpec::default();
    let g = tail_geometry(&tail, 4.0);
    // Bottom tail: the fill triangle's base sits above the outer one so it
    // covers the box border, and its tip stays inside the outline.
    assert!(g.inner[0][1] < g.outer[0][1]);
    assert!(g.inner[2][1] < g.outer[2][1]);
    assert!(g.inner[0][0] > g.outer[0][0] && g.inner[1][0] < g.outer[1][0]);
    let flat = tail_geometry(&tail, 0.0);
    assert_eq!(flat.inner, flat.outer);
    // Other sides mirror the same shape.
    let top = tail_geometry(
        &TailSpec {
            side: Edge::Top,
            ..TailSpec::default()
        },
        4.0,
    );
    assert_eq!(top.outer[2][1], 100.0 - g.outer[2][1]);
    assert!(top.rect[1] < 0.0);
    let right = tail_geometry(
        &TailSpec {
            side: Edge::Right,
            ..TailSpec::default()
        },
        4.0,
    );
    assert_eq!(right.outer[2], [g.outer[2][1], g.outer[2][0]]);
    assert_eq!(right.rect[2], g.rect[3]);
}

#[test]
fn placement_follows_the_anchor() {
    let p = place(Anchor::TopLeft, [5.0, 6.0], [Len::Px(10.0), Len::Px(10.0)]);
    assert_eq!(
        (p.left.percent, p.left.px, p.top.percent, p.top.px),
        (0.0, 5.0, 0.0, 6.0)
    );
    let p = place(
        Anchor::BottomRight,
        [-2.0, 0.0],
        [Len::Px(10.0), Len::Px(20.0)],
    );
    assert_eq!((p.left.percent, p.left.px), (100.0, -12.0));
    assert_eq!((p.top.percent, p.top.px), (100.0, -20.0));
    let p = place(
        Anchor::Center,
        [0.0, 0.0],
        [Len::Percent(50.0), Len::Px(4.0)],
    );
    assert_eq!((p.left.percent, p.left.px), (25.0, 0.0));
    assert_eq!((p.top.percent, p.top.px), (50.0, -2.0));
}

#[test]
fn continue_motion_and_sprites() {
    for i in 0..20 {
        let t = i as f32 * 0.07;
        let (dy, o, s) = continue_motion(ContinueMotion::Bob, 3.0, 0.8, t);
        assert!((-3.0..=0.0).contains(&dy) && o == 1.0 && s == 1.0);
        let (_, o, _) = continue_motion(ContinueMotion::Blink, 3.0, 0.8, t);
        assert!(o == 1.0 || o == 0.15);
    }
    assert_eq!(sprite_frame(4, 8.0, 0.0), 0);
    assert_eq!(sprite_frame(4, 8.0, 0.1), 0);
    assert_eq!(sprite_frame(4, 8.0, 0.25), 2);
    assert_eq!(sprite_frame(1, 8.0, 9.0), 0);
}

#[test]
fn localized_names_fall_back_by_language_then_english() {
    let mut names = Localized::new();
    names.insert("zh-CN".into(), "光环".into());
    names.insert("en-US".into(), "Halo".into());
    assert_eq!(localized(&names, "zh-TW", "x"), "光环");
    assert_eq!(localized(&names, "fr-FR", "x"), "Halo");
    assert_eq!(localized(&Localized::new(), "fr-FR", "x"), "x");
}

#[test]
fn a_user_skin_inherits_from_its_base_and_keeps_its_own_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("sakura");
    write_skin(
        &dir,
        r##"
[meta]
id = "sakura"
base = "nanatype.night"
name = { en-US = "Sakura" }
[dialogue.box]
color = "#ffe0ecf0"
"##,
        &[],
    );
    let skin = load_dir(&dir).unwrap();
    assert_eq!(skin.skin.meta.id, "sakura");
    assert_eq!(
        skin.skin.dialogue.box_.color,
        Rgba8::rgba(0xff, 0xe0, 0xec, 0xf0)
    );
    assert_eq!(
        skin.skin.dialogue.box_.border_color,
        Rgba8::rgb(0x5c, 0xb8, 0xff)
    );
    assert_eq!(skin.skin.display_name("en-US"), "Sakura");
    assert!(skin.warnings.is_empty(), "{:?}", skin.warnings);
}

#[test]
fn unknown_fields_warn_but_load() {
    let tmp = tempfile::tempdir().unwrap();
    write_skin(
        tmp.path(),
        "[meta]\nid = \"x\"\n[dialogue]\nsparkle = true\n",
        &[],
    );
    let skin = load_dir(tmp.path()).unwrap();
    assert_eq!(skin.warnings[0].field, "dialogue.sparkle");
}

#[test]
fn validation_fails_closed() {
    type Case<'a> = (&'a str, &'a str, &'a [(&'a str, Vec<u8>)]);
    let cases: &[Case<'_>] = &[
        ("meta.id", "[meta]\nid = \"Bad Id\"\n", &[]),
        ("meta.schema", "[meta]\nid = \"x\"\nschema = 9\n", &[]),
        (
            "meta.min_app",
            "[meta]\nid = \"x\"\nmin_app = \"9.0.0\"\n",
            &[],
        ),
        ("meta.base", "[meta]\nid = \"x\"\nbase = \"missing\"\n", &[]),
        (
            "dialogue.region",
            "[meta]\nid = \"x\"\n[dialogue]\nregion = [10, 10]\n",
            &[],
        ),
        (
            "dialogue.text.size",
            "[meta]\nid = \"x\"\n[dialogue.text]\nsize = 0\n",
            &[],
        ),
        (
            "dialogue.portrait.image",
            "[meta]\nid = \"x\"\n[dialogue.portrait]\nimage = \"../evil.png\"\n",
            &[],
        ),
        (
            "dialogue.portrait.image",
            "[meta]\nid = \"x\"\n[dialogue.portrait]\nimage = \"missing.png\"\n",
            &[],
        ),
        (
            "dialogue.box.frame",
            "[meta]\nid = \"x\"\n[dialogue.box]\nframe = { image = \"f.png\", slice = [3, 3, 3, 3] }\n",
            &[("f.png", png(4, 4))],
        ),
        (
            "fonts.body.file",
            "[meta]\nid = \"x\"\n[fonts.body]\nfile = \"a.ttf\"\n",
            &[("a.ttf", b"not a font".to_vec())],
        ),
        (
            "sounds.se",
            "[meta]\nid = \"x\"\n[sounds.se]\ncoin = \"coin.txt\"\n",
            &[("coin.txt", vec![0; 64])],
        ),
        (
            "dialogue.continue.image",
            "[meta]\nid = \"x\"\n[dialogue.continue]\nimage = \"c.png\"\n",
            &[("c.png", b"garbage".to_vec())],
        ),
        (
            "history.name_weight",
            "[meta]
id = \"x\"
[history]
name_weight = 50
",
            &[],
        ),
        (
            "history.name_plate.border_width",
            "[meta]\nid='x'\n[history.name_plate]\nborder_widths=[0,-1,0,0]\n",
            &[],
        ),
        (
            "history.name_plate.padding",
            "[meta]\nid='x'\n[history.name_plate]\npadding=[-1,2]\n",
            &[],
        ),
        (
            "history.name_plate.shadows",
            "[meta]\nid='x'\n[history.name_plate]\nshadows=[{color='#000',blur=-1}]\n",
            &[],
        ),
        (
            "history.panel.padding",
            "[meta]
id = \"x\"
[history.panel]
padding = [-1, 0, 0, 0]
",
            &[],
        ),
        (
            "history.decor.0.polygon",
            "[meta]
id = \"x\"
[[history.decor]]
polygon = [[0, 0], [1, 1]]
",
            &[],
        ),
        (
            "dialogue.box.gradient",
            r##"[meta]
id = "x"
[dialogue.box]
gradient = { stops = [[0, "#fff"]] }
"##,
            &[],
        ),
        (
            "dialogue.box.gradient",
            r##"[meta]
id = "x"
[dialogue.box]
gradient = { stops = [[0.6, "#fff"], [0.2, "#000"]] }
"##,
            &[],
        ),
        (
            "dialogue.box.radius",
            r##"[meta]
id = "x"
[dialogue.box]
radii = [1, 2, -3, 4]
"##,
            &[],
        ),
        (
            "dialogue.box.shadows",
            r##"[meta]
id = "x"
[dialogue.box]
glow = "#fff"
inner_border_color = "#fff"
shadow = { color = "#000" }
shadows = [{ color = "#000" }, { color = "#000" }]
"##,
            &[],
        ),
        (
            "dialogue.box.margin",
            r##"[meta]
id = "x"
[dialogue.box]
margin = [100, 0, 100, 0]
"##,
            &[],
        ),
        (
            "dialogue.decor.0.polygon",
            r##"[meta]
id = "x"
[[dialogue.decor]]
polygon = [[0, 0], [1, 1]]
"##,
            &[],
        ),
        (
            "dialogue.continue.frames",
            r##"[meta]
id = "x"
[dialogue.continue]
frames = 0
"##,
            &[],
        ),
        (
            "dialogue.continue.fps",
            r##"[meta]
id = "x"
[dialogue.continue]
fps = 0
"##,
            &[],
        ),
        (
            "dialogue.tail",
            r##"[meta]
id = "x"
[dialogue.tail]
at = 1.5
"##,
            &[],
        ),
    ];
    for (field, manifest, files) in cases {
        let tmp = tempfile::tempdir().unwrap();
        write_skin(tmp.path(), manifest, files);
        let errs = errors(load_dir(tmp.path()));
        assert!(
            errs.iter()
                .any(|f| f == field || (field.starts_with("meta.") && f == PACKAGE_MANIFEST)),
            "{field}: {errs:?}"
        );
    }
}

#[test]
fn malformed_toml_and_bad_colours_are_errors() {
    let tmp = tempfile::tempdir().unwrap();
    write_skin(tmp.path(), "[meta\nid=", &[]);
    assert!(!errors(load_dir(tmp.path())).is_empty());
    write_skin(
        tmp.path(),
        "[meta]\nid = \"x\"\n[dialogue.text]\ncolor = \"red\"\n",
        &[],
    );
    assert!(!errors(load_dir(tmp.path())).is_empty());
}

#[test]
fn a_valid_nine_slice_frame_loads() {
    let tmp = tempfile::tempdir().unwrap();
    write_skin(
        tmp.path(),
        "[meta]\nid = \"x\"\n[dialogue.box]\nframe = { image = \"images/f.png\", slice = [8, 8, 8, 8] }\n",
        &[("images/f.png", png(32, 32))],
    );
    let skin = load_dir(tmp.path()).unwrap();
    assert_eq!(
        skin.skin.dialogue.box_.frame.as_ref().unwrap().slice,
        [8, 8, 8, 8]
    );
}

#[test]
fn clean_path_rejects_escapes() {
    assert_eq!(clean_path("./a\\b.png").unwrap(), "a/b.png");
    for bad in [
        "CON",
        "NUL.png",
        "AUX.txt",
        "COM1.log",
        "folder./x",
        "folder /x",
        "x\0.png",
        "a?.png",
    ] {
        assert!(clean_path(bad).is_err(), "{bad:?}");
    }
    for bad in ["../a", "/abs", "C:/x", "a/../../b", ""] {
        assert!(clean_path(bad).is_err(), "{bad}");
    }
}

fn zip_dir(src: &Path, dest: &Path, wrap: Option<&str>) {
    let file = fs::File::create(dest).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default();
    for path in DirFiles::new(src).list() {
        let name = match wrap {
            Some(w) => format!("{w}/{path}"),
            None => path.clone(),
        };
        zip.start_file(name, opts).unwrap();
        zip.write_all(&fs::read(src.join(&path)).unwrap()).unwrap();
    }
    zip.finish().unwrap();
}

#[test]
fn install_export_duplicate_and_remove() {
    let tmp = tempfile::tempdir().unwrap();
    let user = tmp.path().join("skins");
    fs::create_dir_all(&user).unwrap();
    let src = tmp.path().join("src");
    write_skin(
        &src,
        "[meta]\nid = \"cats\"\nversion = \"1.2.0\"\n[dialogue.portrait]\nvisible = true\nimage = \"p.png\"\n",
        &[("p.png", png(16, 16))],
    );
    let archive = tmp.path().join("cats.ntpkg");
    zip_dir(&src, &archive, Some("cats"));

    let mut registry = SkinRegistry::new(Some(user.clone()), APP);
    assert_eq!(
        registry.install(&archive).unwrap(),
        InstallOutcome::Installed { id: "cats".into() }
    );
    assert!(user.join("cats/p.png").is_file());
    assert!(registry.entry("cats").unwrap().loads());
    assert_eq!(
        registry.install(&archive).unwrap(),
        InstallOutcome::Replaced {
            id: "cats".into(),
            previous: "1.2.0".into()
        }
    );

    let exported = tmp.path().join("out.ntpkg");
    registry.export("cats", &exported).unwrap();
    let back = registry.inspect_archive(&exported).unwrap();
    assert_eq!(back.skin.meta.version, "1.2.0");

    let mut over = TomlTable::new();
    let mut text = TomlTable::new();
    text.insert("size".into(), 34.into());
    let mut dialogue = TomlTable::new();
    dialogue.insert("text".into(), text.into());
    over.insert("dialogue".into(), dialogue.into());
    let mut name = Localized::new();
    name.insert("en-US".into(), "Big cats".into());
    registry
        .duplicate("cats", "cats-big", &name, &over)
        .unwrap();
    let big = registry.load("cats-big").unwrap();
    assert_eq!(big.skin.dialogue.text.size, 34.0);
    assert!(big.skin.dialogue.portrait.visible, "inherited from cats");
    assert!(big.files.exists("p.png"), "base files are layered in");

    registry.remove("cats-big").unwrap();
    assert!(registry.entry("cats-big").is_none());
    assert!(registry.remove("nanatype.halo").is_err());
}

#[test]
fn builtin_ids_cannot_be_installed_over() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    write_skin(&src, "[meta]\nid = \"nanatype.halo\"\n", &[]);
    let archive = tmp.path().join("x.ntpkg");
    zip_dir(&src, &archive, None);
    let user = tmp.path().join("skins");
    fs::create_dir_all(&user).unwrap();
    let mut registry = SkinRegistry::new(Some(user), APP);
    assert!(registry.install(&archive).is_err());
}

#[test]
fn pacing_and_sound_rule_come_from_the_skin() {
    let registry = SkinRegistry::new(None, APP);
    let rpg = registry.load("nanatype.rpg").unwrap();
    let pacing = rpg.skin.pacing(1.0, true, true);
    assert_eq!(pacing.base_ms, 40);
    let rule = rpg.skin.sounds.typing.rule(true);
    assert_eq!(rule.variants, 1);
}

#[test]
fn skins_choose_how_long_text_overflows() {
    use nanatype_model::Overflow;
    let reg = SkinRegistry::new(None, APP);
    let skin = |id: &str| reg.load(id).unwrap().skin;
    assert_eq!(skin("nanatype.halo").dialogue.overflow, Overflow::Scroll);
    for id in ["nanatype.adv", "nanatype.rpg", "nanatype.pixel"] {
        assert_eq!(skin(id).dialogue.overflow, Overflow::Page, "{id}");
    }
    let d: Dialogue = toml::from_str("overflow = \"clip\"\noverflow_auto_ms = 1500").unwrap();
    assert_eq!(
        (d.overflow, d.overflow_auto_ms),
        (Overflow::Clip, Some(1500))
    );
}

#[test]
fn pixel_font_skins_stay_on_the_12px_grid() {
    // Fusion Pixel 12px blurs between its pixels at any other size: every
    // size and line box drawn in it is a whole multiple of 12.
    let reg = SkinRegistry::new(None, APP);
    let on_grid = |px: f32| px > 0.0 && (px / 12.0 - (px / 12.0).round()).abs() < 1e-4;
    let mut n = 0;
    for e in reg.entries() {
        let skin = reg.load(&e.id).unwrap().skin;
        let pixel = |f: &FontSpec| f.file.as_deref().is_some_and(|p| p.contains("FusionPixel"));
        if !pixel(&skin.fonts.body) {
            continue;
        }
        n += 1;
        let (d, h) = (&skin.dialogue, &skin.history);
        let mut sizes = vec![
            ("dialogue.text.size", d.text.size),
            ("dialogue.text line", d.text.size * d.text.line_height),
            ("history.text.size", h.text.size),
            ("history.text line", h.text.size * h.text.line_height),
            ("history chat text", h.text.size * h.chat.text_scale),
            ("history mention text", h.text.size * skin.mention.scale),
            (
                "history.time_size",
                h.time_size
                    .expect("pixel metadata has an explicit grid size"),
            ),
            (
                "history.reply.size",
                h.reply
                    .size
                    .expect("pixel replies have an explicit grid size"),
            ),
        ];
        if pixel(&skin.fonts.name) {
            sizes.push(("dialogue.nameplate.size", d.nameplate.size));
            sizes.push(("history.name_size", h.name_size));
            assert_eq!(
                h.name_weight, skin.fonts.name.weight,
                "{}: pixel names must not synthesize a heavier face",
                e.id
            );
        }
        for (what, px) in sizes {
            assert!(on_grid(px), "{}: {what} = {px}", e.id);
        }
    }
    assert_eq!(n, 2, "pixel and rpg");
}

#[test]
fn echo_menu_rows_show_only_on_the_display() {
    let reg = SkinRegistry::new(None, APP);
    for id in [
        "nanatype.echo_vanilla",
        "nanatype.echo_bubble",
        "nanatype.echo_glass",
        "nanatype.echo_void",
    ] {
        let skin = reg.load(id).unwrap().skin;
        let menu: Vec<_> = skin
            .dialogue
            .decor
            .iter()
            .filter(|d| !d.text.is_empty())
            .collect();
        assert_eq!(menu.len(), 7, "{id}");
        assert!(menu.iter().all(|d| d.when == DecorWhen::Display), "{id}");
    }
    let d: DecorSpec = toml::from_str("when = \"display\"").unwrap();
    assert_eq!(d.when, DecorWhen::Display);
}

// ---- docs consistency ------------------------------------------------------

/// The manifests in `builtin/`, parsed.
fn builtin_tables() -> Vec<toml::Table> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("builtin");
    let mut paths: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    paths.sort();
    paths
        .iter()
        .map(|p| read_manifest(&DirFiles::new(p)).unwrap())
        .collect()
}

/// `(id, zh-CN name)` of every built-in skin, sorted by id.
fn builtin_names() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = builtin_tables()
        .iter()
        .map(|t| {
            let meta = t["meta"].as_table().unwrap();
            let id = meta["id"].as_str().unwrap().to_owned();
            let name = meta["name"]["zh-CN"].as_str().unwrap().to_owned();
            (id, name)
        })
        .collect();
    out.sort();
    out
}

fn repo_file(path: &str) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    fs::read_to_string(root.join(path)).unwrap()
}

#[test]
fn docs_list_exactly_the_builtin_skins() {
    let builtins = builtin_names();
    // The registry ships exactly the manifests in builtin/.
    let registry = SkinRegistry::new(None, APP);
    let mut shipped: Vec<String> = registry.entries().iter().map(|e| e.id.clone()).collect();
    shipped.sort();
    let ids: Vec<String> = builtins.iter().map(|(id, _)| id.clone()).collect();
    assert_eq!(shipped, ids, "registry vs builtin/*/skin.toml");

    // docs/skins.md: one table row per skin, "| `id` | name | …".
    let docs = repo_file("docs/skins.md");
    let mut rows: Vec<(String, String)> = docs
        .lines()
        .filter_map(|l| {
            let (id, rest) = l.strip_prefix("| `")?.split_once('`')?;
            let name = rest.split('|').nth(1)?.trim();
            id.starts_with("nanatype.")
                .then(|| (id.to_owned(), name.to_owned()))
        })
        .collect();
    rows.sort();
    assert_eq!(rows, builtins, "docs/skins.md's built-in table");

    // README: "内置 N 套（name、name、…；…）".
    let readme = repo_file("README.md");
    let line = readme
        .lines()
        .find(|l| l.contains("**皮肤**"))
        .expect("README's skin bullet");
    let count = format!("内置 {} 套", builtins.len());
    assert!(line.contains(&count), "README should say {count}: {line}");
    let list = line
        .split_once('（')
        .and_then(|(_, r)| r.split(['；', '）']).next())
        .expect("README's skin list");
    let mut listed: Vec<&str> = list.split('、').map(str::trim).collect();
    listed.sort_unstable();
    let mut names: Vec<&str> = builtins.iter().map(|(_, n)| n.as_str()).collect();
    names.sort_unstable();
    assert_eq!(listed, names, "README's skin list");
}

/// Every key path a skin can write: the default skin's fields and every key
/// a built-in manifest uses. Free-form maps (localised names, portrait variants,
/// sound effects) contribute their own key only.
fn manifest_keys() -> std::collections::BTreeSet<String> {
    const OPEN_MAPS: &[&str] = &[
        "meta.name",
        "meta.description",
        "dialogue.portrait.variants",
        "sounds.se",
        "shaders",
        "materials",
    ];
    fn walk(prefix: &str, v: &toml::Value, out: &mut std::collections::BTreeSet<String>) {
        match v {
            toml::Value::Table(t) => {
                for (k, v) in t {
                    let path = if prefix.is_empty() {
                        k.clone()
                    } else {
                        format!("{prefix}.{k}")
                    };
                    out.insert(path.clone());
                    if !OPEN_MAPS.contains(&path.as_str()) {
                        walk(&path, v, out);
                    }
                }
            }
            toml::Value::Array(items) => {
                for item in items {
                    walk(prefix, item, out);
                }
            }
            _ => {}
        }
    }
    let mut out = std::collections::BTreeSet::new();
    walk(
        "",
        &toml::Value::try_from(Skin::default()).unwrap(),
        &mut out,
    );
    for table in builtin_tables() {
        walk("", &toml::Value::Table(table), &mut out);
    }
    out
}

/// Whether `word` appears in `text` as a whole key, not inside a longer one.
fn mentions_key(text: &str, word: &str) -> bool {
    let is_key = |c: char| c.is_ascii_alphanumeric() || c == '_';
    text.match_indices(word).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + word.len()..].chars().next();
        !before.is_some_and(is_key) && !after.is_some_and(is_key)
    })
}

#[test]
fn docs_name_every_manifest_key() {
    let docs = repo_file("docs/skins.md");
    let missing: Vec<String> = manifest_keys()
        .into_iter()
        .filter(|path| !mentions_key(&docs, path.rsplit('.').next().unwrap()))
        .collect();
    assert!(missing.is_empty(), "docs/skins.md never names {missing:?}");
    for section in [
        "[mention]",
        "[dialogue.reply]",
        "[history.reply]",
        "[history.chat]",
    ] {
        assert!(docs.contains(section), "docs/skins.md lacks {section}");
    }
    for key in ["when", "anchor", "contrast_guard", "hide_latest"] {
        assert!(
            docs.contains(&format!("{key} =")),
            "docs/skins.md lacks an example of `{key} =`"
        );
    }
}

// ---- built-in fonts ---------------------------------------------------------

/// Noto Sans SC, the OFL face NanaUI already embeds, as a `builtin:` font.
static NOTO: &[BuiltinFont] = &[BuiltinFont {
    path: "fonts/NotoSansSC-Regular.ttf",
    bytes: nana_ui_core::fonts::UI_FONT_REGULAR,
    license: "SIL Open Font License 1.1",
}];
const NOTO_REF: &str = "builtin:fonts/NotoSansSC-Regular.ttf";

fn diagnose_builtin(reference: &str, fonts: &BuiltinFonts) -> Vec<SkinDiagnostic> {
    let mut out = Vec::new();
    crate::load::validate_builtin_font("fonts.body.file", reference, fonts, &mut out);
    out
}

#[test]
fn builtin_font_references_resolve_to_embedded_bytes() {
    let fonts = BuiltinFonts::new(NOTO, &[]);
    let font = fonts.resolve(NOTO_REF).expect("resolves");
    assert_eq!(font.bytes.len(), nana_ui_core::fonts::UI_FONT_REGULAR.len());
    assert_eq!(font_family(font.bytes).as_deref(), Some("Noto Sans SC"));
    // The path is cleaned like any skin path.
    assert!(
        fonts
            .resolve("builtin:./fonts\\NotoSansSC-Regular.ttf")
            .is_some()
    );
    // Without the prefix it is a skin file, never a built-in font.
    assert!(fonts.resolve("fonts/NotoSansSC-Regular.ttf").is_none());
    assert!(fonts.resolve("builtin:fonts/Other.ttf").is_none());
    // `builtin:` cannot reach outside crates/theme/.
    assert_eq!(builtin_path("builtin:../secret.ttf"), None);
    assert!(
        fonts
            .resolve("builtin:../fonts/NotoSansSC-Regular.ttf")
            .is_none()
    );
    assert!(diagnose_builtin(NOTO_REF, &fonts).is_empty());
    assert_eq!(fonts.total_bytes(), font.bytes.len());
}

#[test]
fn builtin_font_left_out_of_the_build_falls_back_quietly() {
    // Named in crates/theme/fonts/ but not embedded (the feature is off).
    let absent = BuiltinFonts::new(&[], &["fonts/NotoSansSC-Regular.ttf"]);
    assert!(absent.resolve(NOTO_REF).is_none());
    assert!(absent.knows(NOTO_REF));
    assert!(diagnose_builtin(NOTO_REF, &absent).is_empty());
    // Not there at all: loads, with a warning about the likely typo.
    let unknown = diagnose_builtin("builtin:fonts/Typo.ttf", &absent);
    assert_eq!(unknown.len(), 1);
    assert_eq!(unknown[0].severity, Severity::Warning);
    // An escaping path is refused outright.
    let unsafe_ref = diagnose_builtin("builtin:../x.ttf", &absent);
    assert_eq!(unsafe_ref[0].severity, Severity::Error);
    // Bytes that are not a font are refused like a skin's own file.
    static BAD: &[BuiltinFont] = &[BuiltinFont {
        path: "fonts/Bad.ttf",
        bytes: b"not a font",
        license: "",
    }];
    let bad = diagnose_builtin("builtin:fonts/Bad.ttf", &BuiltinFonts::new(BAD, &[]));
    assert_eq!(bad[0].severity, Severity::Error);
}

#[test]
fn skin_naming_a_missing_builtin_font_loads_and_keeps_its_families() {
    let dir = tempfile::tempdir().unwrap();
    let skin_dir = dir.path().join("x");
    write_skin(
        &skin_dir,
        "[meta]\nid = \"x\"\nbase = \"nanatype.pixel\"\n\
         [fonts.body]\nfile = \"builtin:fonts/NotInThisBuild.ttf\"\n",
        &[],
    );
    let loaded = load_dir(&skin_dir).expect("a missing built-in font is not an error");
    let body = &loaded.skin.fonts.body;
    assert_eq!(
        body.file.as_deref(),
        Some("builtin:fonts/NotInThisBuild.ttf")
    );
    assert_eq!(body.families[0], "Fusion Pixel 12px Proportional SC");
    assert!(!body.has_usable_file());
    assert!(
        loaded.warnings.iter().any(|w| w.field == "fonts.body.file"),
        "{:?}",
        loaded.warnings
    );
    // A skin file is usable once the skin loaded; no file is not.
    let mut spec = FontSpec::default();
    assert!(!spec.has_usable_file());
    spec.file = Some("fonts/own.ttf".into());
    assert!(spec.has_usable_file());
}

#[test]
fn this_builds_embedded_fonts_parse_and_carry_their_licence() {
    let fonts = BuiltinFonts::embedded();
    // With the feature every font in fonts/ is embedded; without it, none.
    let embedded: Vec<&str> = fonts.fonts().iter().map(|f| f.path).collect();
    if cfg!(feature = "builtin-fonts") {
        assert_eq!(embedded, fonts.known());
    } else {
        assert!(embedded.is_empty());
    }
    for f in fonts.fonts() {
        assert!(f.path.starts_with("fonts/"), "{}", f.path);
        assert!(font_family(f.bytes).is_some(), "{} has no family", f.path);
        assert!(!f.license.trim().is_empty(), "{} has no licence", f.path);
        assert!(fonts.knows(&format!("{BUILTIN_FONT_PREFIX}{}", f.path)));
    }
    // Every built-in skin's `builtin:` reference names a font in fonts/.
    let registry = SkinRegistry::new(None, APP);
    for entry in registry.entries() {
        let skin = entry.skin.as_ref().unwrap();
        for spec in [&skin.fonts.body, &skin.fonts.name, &skin.fonts.accent]
            .into_iter()
            .chain(skin.fonts.latin.as_ref())
        {
            if let Some(f) = spec.file.as_deref().filter(|f| is_builtin_font(f)) {
                assert!(fonts.knows(f), "{}: {f}", entry.id);
            }
        }
    }
}

/// The fonts in the repository: pixel and rpg share Fusion Pixel, ink uses
/// LXGW WenKai, each with its OFL text, and together within the budget.
#[cfg(feature = "builtin-fonts")]
#[test]
fn the_bundled_fonts_serve_the_skins_that_name_them() {
    const PIXEL: &str = "builtin:fonts/FusionPixel12pxProportionalSC.ttf";
    const WENKAI: &str = "builtin:fonts/LXGWWenKai-Regular.ttf";
    let fonts = BuiltinFonts::embedded();
    assert!(
        fonts.total_bytes() <= 12 * 1024 * 1024,
        "{} bytes",
        fonts.total_bytes()
    );
    let registry = SkinRegistry::new(None, APP);
    for (id, reference, roles) in [
        ("nanatype.pixel", PIXEL, 2),
        ("nanatype.rpg", PIXEL, 2),
        ("nanatype.ink", WENKAI, 3),
    ] {
        let entry = registry.entries().iter().find(|e| e.id == id).unwrap();
        let skin = entry.skin.as_ref().unwrap();
        let named = [&skin.fonts.body, &skin.fonts.name, &skin.fonts.accent]
            .into_iter()
            .filter(|s| s.file.as_deref() == Some(reference))
            .count();
        assert_eq!(named, roles, "{id}");
        assert!(skin.fonts.body.has_usable_file(), "{id}");
    }
    for (reference, family, author) in [
        (PIXEL, "Fusion Pixel 12px Prop zh-Hans", "TakWolf"),
        (WENKAI, "LXGW WenKai", "LXGW / Fontworks Inc."),
    ] {
        let font = fonts.resolve(reference).expect(reference);
        assert_eq!(font.family().as_deref(), Some(family));
        assert_eq!(font.author().as_deref(), Some(author));
        assert_eq!(font.license_name(), Some("SIL Open Font License 1.1"));
        assert!(font.license.contains("Copyright"), "{reference}");
        // The subset keeps Latin, the common hanzi, kana and CJK punctuation.
        let face = ttf_parser::Face::parse(font.bytes, 0).unwrap();
        for ch in (' '..='~').chain("你好世界说话掌机像素水墨，。！？「」…あいうアイウ".chars())
        {
            assert!(face.glyph_index(ch).is_some(), "{reference} lacks {ch:?}");
        }
    }
}

#[test]
fn directory_resources_reject_links_escaping_the_selected_skin_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("skin");
    let outside = tmp.path().join("outside");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(root.join("ok.txt"), b"inside").unwrap();
    fs::write(outside.join("secret.txt"), b"outside").unwrap();
    let link = root.join("escape");
    #[cfg(windows)]
    assert!(
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .output()
            .unwrap()
            .status
            .success()
    );
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    let files = DirFiles::new(&root);
    assert_eq!(files.read("ok.txt").unwrap(), b"inside");
    assert!(matches!(
        files.read("escape/secret.txt"),
        Err(FileError::UnsafePath(_))
    ));
    // An explicitly selected linked directory remains a valid root.
    assert_eq!(DirFiles::new(&link).read("secret.txt").unwrap(), b"outside");
}

#[test]
fn crayon_materials_are_lazy_validated_and_roundtrip_with_their_fallbacks() {
    let tmp = tempfile::tempdir().unwrap();
    let mut registry = SkinRegistry::new(Some(tmp.path().join("skins")), APP);
    let entry = registry.entry("nanatype.crayon").unwrap();
    assert!(entry.skin.as_ref().unwrap().material_programs.is_empty());
    let loaded = registry.load("nanatype.crayon").unwrap();
    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    assert_eq!(loaded.skin.material_programs.len(), 6);
    assert!(
        loaded
            .skin
            .shaders
            .values()
            .all(|s| s.static_fallback && !s.animated)
    );
    let other = registry.load("nanatype.crayon").unwrap();
    assert!(std::sync::Arc::ptr_eq(
        &loaded.skin.material_programs["paper"],
        &other.skin.material_programs["paper"]
    ));
    let archive = tmp.path().join("crayon.ntpkg");
    registry.export("nanatype.crayon", &archive).unwrap();
    let exported = registry.inspect_archive(&archive).unwrap();
    assert_eq!(
        loaded.skin.material_programs,
        exported.skin.material_programs
    );
    for (_, path, _) in loaded.skin.referenced_files() {
        if !path.starts_with("builtin:") {
            assert_eq!(
                loaded.files.read(path).unwrap(),
                exported.files.read(path).unwrap()
            );
        }
    }
    let names = Localized::from([("en-US".into(), "Pencil copy".into())]);
    registry
        .duplicate("nanatype.crayon", "my-crayon", &names, &toml::Table::new())
        .unwrap();
    let inherited = registry.load("my-crayon").unwrap();
    assert_eq!(
        inherited.skin.material_programs,
        loaded.skin.material_programs
    );
    assert_eq!(
        inherited.skin.history.text.material.as_deref(),
        Some("pencil")
    );
    registry.export("my-crayon", &archive).unwrap();
    let detached = registry.inspect_archive(&archive).unwrap();
    let manifest = String::from_utf8(detached.files.read(MANIFEST).unwrap()).unwrap();
    assert!(
        !manifest.contains("base ="),
        "material export retained an authoring base"
    );
    assert_eq!(
        detached.skin.material_programs,
        loaded.skin.material_programs
    );
}
#[test]
fn material_validation_rejects_untrusted_resources_loops_and_signatures() {
    let valid = "fn material(i:MaterialInput)->vec4<f32>{return i.color;}";
    assert!(crate::validate_material(valid).is_ok());
    for body in [
        "loop{} return vec4(1.);",
        "var a:array<vec4<f32>,65>; return a[0];",
        "var a:array<array<vec4<f32>,64>,64>; return a[0][0];",
        "return vec4<f32>(dpdx(1.));",
    ] {
        let effect = format!("fn effect(i:EffectInput)->vec4<f32>{{{body}}}");
        let material = format!("fn material(i:MaterialInput)->vec4<f32>{{{body}}}");
        assert!(
            crate::validate_shader(&effect).is_err(),
            "accepted {effect}"
        );
        assert!(
            crate::validate_material(&material).is_err(),
            "accepted {material}"
        );
    }
    for bad in [
        "fn material(i:MaterialInput)->vec4<f32>{loop{} return i.color;}",
        "fn material(i:MaterialInput)->vec4<f32>{return nm_parameters[0];}",
        "fn material(i:MaterialInput)->f32{return 1.;}",
        "fn sample0()->vec4<f32>{loop{} return vec4(0.);}
fn material(i:MaterialInput)->vec4<f32>{return sample0();}",
        "@group(0) @binding(0) var bad:texture_2d<f32>;
fn material(i:MaterialInput)->vec4<f32>{return i.color;}",
    ] {
        assert!(crate::validate_material(bad).is_err(), "accepted {bad}");
    }
    assert!(
        crate::validate_shader(
            "fn material_param()->vec4<f32>{loop{} return vec4(0.);}
fn effect(i:EffectInput)->vec4<f32>{return material_param();}"
        )
        .is_err()
    );
}

#[test]
fn responsive_inheritance_keeps_configuration_and_exports_all_variant_assets() {
    let dir = tempfile::tempdir().unwrap();
    let old = "[meta]\nid='user.legacy'\nbase='nanatype.gothic'\n";
    write_skin(dir.path(), old, &[]);
    assert!(
        load_dir(dir.path())
            .unwrap()
            .skin
            .dialogue
            .responsive
            .is_some()
    );
    write_skin(dir.path(), &format!("{old}\n[dialogue.responsive]\n"), &[]);
    let loaded = load_dir(dir.path()).unwrap();
    assert!(loaded.skin.dialogue.responsive.is_some());
    for path in ["rose.png", "tracery.png", "artwork.wgsl"] {
        assert!(
            loaded
                .skin
                .referenced_files()
                .iter()
                .any(|(_, p, _)| *p == path)
        );
        assert!(!loaded.files.read(path).unwrap().is_empty());
    }
    let manifest = toml::to_string(&standalone_manifest(&loaded.skin).unwrap()).unwrap();
    let standalone = tempfile::tempdir().unwrap();
    let paths: Vec<_> = loaded
        .files
        .list()
        .into_iter()
        .filter(|p| p != MANIFEST)
        .collect();
    let assets: Vec<_> = paths
        .iter()
        .map(|p| (p.as_str(), loaded.files.read(p).unwrap()))
        .collect();
    write_skin(standalone.path(), &manifest, &assets);
    assert_eq!(
        load_dir(standalone.path())
            .unwrap()
            .skin
            .dialogue
            .responsive,
        loaded.skin.dialogue.responsive
    );
    write_skin(
        dir.path(),
        &format!("{old}\n[dialogue.responsive.portrait]\ndecor=[{{aspect_ratio=0}}]\n"),
        &[],
    );
    assert!(load_dir(dir.path()).is_err());
}

#[test]
fn archive_entries_cannot_inflate_past_their_declared_size() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("bomb.ntpkg");
    let mut zip = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("a.txt", opts).unwrap();
    zip.write_all(&[b'a'; 64 * 1024]).unwrap();
    zip.finish().unwrap();
    // Declare 1 byte in the central directory (uncompressed size, offset 24).
    let mut bytes = fs::read(&archive).unwrap();
    let central = bytes
        .windows(4)
        .rposition(|w| w == [0x50, 0x4b, 0x01, 0x02])
        .unwrap();
    bytes[central + 24..central + 28].copy_from_slice(&1u32.to_le_bytes());
    fs::write(&archive, bytes).unwrap();

    let files = ZipFiles::open(&archive).unwrap();
    assert_eq!(files.file_len("a.txt").unwrap(), 1);
    assert!(files.read("a.txt").is_err());
}

#[test]
fn responsive_layouts_merge_onto_the_base_dialogue() {
    let dir = tempfile::tempdir().unwrap();
    let base = "[meta]\nid='user.partial'\n[dialogue.box]\nradius=6\ncolor='#102030'\n\
                glow='#ff0000'\nshadows=[{color='#000',dx=1},{color='#000',dx=2}]\n\
                [dialogue.box.shadow]\ncolor='#000'\ndx=3\nblur=4\n\
                [dialogue.nameplate]\nborder_color='#ffffff'\n";
    write_skin(
        dir.path(),
        &format!(
            "{base}[dialogue.responsive.portrait]\nunset=['nameplate.border_color']\n\
             nameplate={{}}\nbox={{radius=2, shadow={{dx=5}}, shadows=[{{color='#000',dx=9}}]}}\n\
             [[dialogue.responsive.rules]]\nmax_width=200\nlayout={{box={{}}}}\n"
        ),
        &[],
    );
    let loaded = load_dir(dir.path()).unwrap();
    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    let skin = &loaded.skin;
    let portrait = responsive_dialogue(skin, [300., 1200.]).dialogue;
    assert_eq!(portrait.box_.radius, 2.);
    assert_eq!(portrait.box_.color, skin.dialogue.box_.color);
    assert_eq!(portrait.box_.glow, skin.dialogue.box_.glow);
    let shadow = portrait.box_.shadow.as_ref().unwrap();
    assert_eq!(
        (shadow.dx, shadow.blur),
        (5., 4.),
        "tables merge field by field"
    );
    assert_eq!(portrait.box_.shape.shadows.len(), 1, "arrays replace");
    assert!(portrait.nameplate.border_color.is_none());
    // Naming a spec, even empty, resets it to the base over the portrait variant.
    let narrow = responsive_dialogue(skin, [200., 1200.]).dialogue;
    assert_eq!(narrow.box_, skin.dialogue.box_);
    assert!(narrow.nameplate.border_color.is_none());

    let manifest = toml::to_string(&standalone_manifest(skin).unwrap()).unwrap();
    assert!(
        manifest.contains("unset = [\"nameplate.border_color\"]"),
        "{manifest}"
    );
    write_skin(dir.path(), &manifest, &[]);
    assert_eq!(load_dir(dir.path()).unwrap().skin, loaded.skin);

    write_skin(
        dir.path(),
        &format!("{base}[dialogue.responsive.portrait]\nunset=['box.nope']\n"),
        &[],
    );
    assert!(
        load_dir(dir.path())
            .unwrap()
            .warnings
            .iter()
            .any(|w| w.field == "dialogue.responsive.unset")
    );
}
