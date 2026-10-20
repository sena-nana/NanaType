//! The shared starting state: the skin, sample lines, chat, draft and status.

use super::*;

/// The sample content, settings and fonts every mode captures.
pub struct Scene {
    pub ui: Ui,
    pub skin: Arc<nanatype_theme::Skin>,
    pub settings: Settings,
    pub theme: Arc<nana_ui::CompiledTheme>,
    pub locale: Locale,
    /// The display window: the dialogue region plus overscan.
    pub w: u32,
    pub h: u32,
    /// The skin's fonts stay registered until every shot is taken.
    _builtin: nanatype::skin_fonts::BuiltinFontSet<nana_ui::HostFontScope>,
    _packaged: nana_ui::HostFontScope,
}

impl Scene {
    pub fn new(
        registry: &SkinRegistry,
        out: &std::path::Path,
        skin_id: &str,
        locale: Locale,
    ) -> Self {
        // A saved manifest set makes before/after comparisons use identical
        // fonts, sample content and rendering rather than stale screenshots.
        let loaded = match std::env::var_os("NANATYPE_SNAPSHOT_SKINS_DIR") {
            Some(dir) if skin_id.starts_with("nanatype.") => {
                let folder = PathBuf::from(dir).join(skin_id);
                nanatype_theme::load(
                    Arc::new(nanatype_theme::DirFiles::new(folder.clone())),
                    nanatype_theme::SkinOrigin::Directory(folder),
                    &(),
                    env!("CARGO_PKG_VERSION"),
                )
                .expect("snapshot manifest")
            }
            _ => registry.load(skin_id).expect("skin"),
        };
        // The skin's built-in fonts, registered as the app does; kept alive
        // until every shot is taken. None without the `builtin-fonts` feature.
        let mut builtin = nanatype::skin_fonts::BuiltinFontSet::default();
        let f = &loaded.skin.fonts;
        let roles = [&f.body, &f.name, &f.accent];
        let builtin_ref = |s: &&nanatype_theme::FontSpec| {
            s.file
                .clone()
                .filter(|p| nanatype_theme::is_builtin_font(p))
        };
        let refs: Vec<String> = roles
            .iter()
            .copied()
            .chain(f.latin.as_ref())
            .filter_map(|s| builtin_ref(&s))
            .collect();
        for e in builtin.sync(
            refs.iter().map(String::as_str),
            nanatype_theme::builtin_font,
            nanatype::skin_fonts::register_with_nana_ui,
        ) {
            println!("font: {e}");
        }
        let mut packaged_fonts = nana_ui::HostFontScope::new();
        let families = roles.map(|s| {
            let path = s.file.as_ref()?;
            if nanatype_theme::is_builtin_font(path) {
                return builtin.family(path);
            }
            packaged_fonts
                .add_bytes(loaded.files.read(path).ok()?)
                .ok()?
                .families()
                .first()
                .cloned()
        });
        let mut skin = Arc::new(nanatype::skin_theme::prepare(
            &loaded.skin,
            &Default::default(),
            &families,
            locale.tag(),
        ));
        if std::env::var_os("NANATYPE_SNAPSHOT_SWEEP").is_some() {
            Arc::make_mut(&mut skin).dialogue.sweep.enabled = true;
        }
        let mut settings = Settings::default();
        settings.general.speaker_name = "奈奈".into();
        settings.skin.id = skin_id.to_owned();
        settings.history.show_time = true;
        settings.history.chat.enabled = true;
        settings.integrations.bilibili.enabled = true;
        settings.integrations.bilibili.room = "21452505".into();
        let dark = std::env::var("NANATYPE_SNAPSHOT_APPEARANCE").is_ok_and(|v| v == "dark");
        settings.skin.chrome = if dark {
            nanatype_settings::ChromeMode::Dark
        } else {
            nanatype_settings::ChromeMode::Light
        };
        if std::env::var_os("NANATYPE_SNAPSHOT_MATERIALS_OFF").is_some() {
            settings.skin.advanced_effects = false;
        }
        let theme = nanatype::ui::kit::theme::compile(dark);
        let ui = Ui::new(Arc::new(settings.clone()), skin.clone(), locale);
        if std::env::var_os("NANATYPE_SNAPSHOT_STATIC").is_some() {
            ui.reduced_motion.set(true);
        }
        ui.skins
            .set(Arc::new(nanatype::ui::skin_rows(registry, locale)));
        let mut urls = nanatype::assets::skin_preview_urls(&loaded.skin, &*loaded.files);
        let chat = sample_chat(out, &mut urls);
        let stickers = sample_stickers(out, &mut urls);
        assert_eq!(stickers.len(), 4, "every sample sticker resolves");
        ui.stickers.set(Arc::new(stickers));
        ui.asset_urls.set(Arc::new(urls));
        // The portrait the way `App::refresh_assets` finds it.
        if let (Some(root), Some(img)) = (loaded.files.root(), &skin.dialogue.portrait.image)
            && let Some(url) = nanatype::assets::skin_file_url(root, img)
        {
            ui.portraits.set(Arc::new(
                [("default".to_owned(), url)].into_iter().collect(),
            ));
        }

        let mut core = Core::new(settings.clone(), skin.clone());
        for line in [
            "大家好！今天也来[b]打字[/b]啦～",
            "[color=#e8505b]红色[/color]、[wave]波浪[/wave]和[ruby=かんじ]漢字[/ruby]注音。[stroke=#e86a92,3][color=#ffffff]描边文字[/color][/stroke][shadow=#0876d0,0,0,8]发光[/shadow]",
        ] {
            core.handle(Cmd::SendMarkup(line.into()), 0);
            core.handle(Cmd::Skip, 1);
        }
        // Live chat joins the history; the last line answers a viewer.
        for m in &chat {
            core.handle(Cmd::External(m.history_item(locale)), 0);
        }
        core.handle(Cmd::SetReply(Some(chat[0].quote(locale))), 0);
        core.handle(
            Cmd::DraftChanged(
                parse(
                    "[at=1001 name=星野ほたる avatar=blob:bili-face-1001] 晚上好！今天也谢谢你来～",
                )
                .doc,
            ),
            0,
        );
        core.handle(Cmd::Send, 0);
        core.handle(Cmd::Skip, 1);
        if let Ok(line) = std::env::var("NANATYPE_SNAPSHOT_MARKUP") {
            core.handle(Cmd::SendMarkup(line), 0);
            core.handle(Cmd::Skip, 1);
        }
        ui.chat
            .set(Arc::new(chat.iter().cloned().map(Arc::new).collect()));
        ui.reply.set(Some(chat[1].quote(locale)));
        let mut status = (*ui.status.get_untracked()).clone();
        (status.bilibili, status.bilibili_link) = link_text::bilibili(
            locale,
            &ConnectionState::Connected(nanatype_bilibili::Room {
                id: 21452505,
                owner: 1,
                title: String::new(),
                max_chars: 20,
            }),
            "21452505",
        );
        assert_consistent(&settings, &status);
        ui.status.set(Arc::new(status));

        let draft = "[at=1002 name=路过的猫 avatar=blob:bili-face-1002] [b]粗体[/b]和[shake]抖动[/shake]，[wait=500]然后[pause]继续。[sticker=nana/heart][se=coin]";
        let parsed = parse(draft);
        ui.draft.set(Arc::new(parsed.doc.clone()));
        let look = dialogue::Base::editor(&skin).look();
        ui.editor_value.set(nanatype::ui::rich::to_rich(
            &parsed.doc,
            &nanatype::ui::rich::Env {
                look: &look,
                urls: &ui.asset_urls.get_untracked(),
                editor: true,
                locale,
            },
        ));
        let mut src = ui.source.get_untracked();
        src.text = draft.into();
        ui.source.set(src);
        ui.display.set(Arc::new(core.display().clone()));
        ui.history.set(Arc::new(
            core.history()
                .iter()
                .cloned()
                .map(|mut row| {
                    // Wall-clock timestamps also seed some effects. Keep pixel audits
                    // reproducible between independent before/after processes.
                    if row.item.kind == nanatype_model::SourceKind::Typewriter {
                        row.item.at = 1_760_000_100_000;
                    }
                    row
                })
                .collect(),
        ));
        ui.now.set(nanatype::clock::now_ms());
        ui.peer_now.set(nanatype::clock::now_ms());
        ui.history_now.set(nanatype::clock::now_ms());

        let d = &skin.dialogue;
        let (w, h) = (
            (d.region[0] + d.overscan * 2.0) as u32,
            (d.region[1] + d.overscan * 2.0) as u32,
        );
        Self {
            ui,
            skin,
            settings,
            theme,
            locale,
            w,
            h,
            _builtin: builtin,
            _packaged: packaged_fonts,
        }
    }
}
