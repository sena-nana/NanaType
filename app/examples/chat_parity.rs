//! Real NanaUI interaction capture, driven through its buttons and editor.
//! cargo run -p nanatype --example chat_parity -- <out> [skin|all] [locale] [profile] [compact|history]
//! Profiles: capture (150 frames), capture-panel, stills, audit, verify, source, compose, queue, reduced.
//! capture / capture-panel also write `<surface>/capture.json` (NanaPVMaker capture contract).
use nana_ui::runtime::view::{entity_ref, with_refs};
use nana_ui::runtime::{
    DocumentId, Entity, RichEditCommand, RichTextEditor, RuntimeDocument, TextArea, Toast,
    ToastStack,
};
use nana_ui_devtools::agent::{AccessibilityDumpNode, AgentSession, RuntimeAgentSession};
use nanatype::{
    chat::{ChatKind, ChatMessage},
    msg::Msg,
    ui::{self, Ui, danmaku, dialogue, history, input},
};
use nanatype_core::{Cmd, Core, Effects, Wake};
use nanatype_i18n::Locale;
use nanatype_model::{AssetRef, ChatUser, Document, Inline, Mention};
use nanatype_settings::{ChatPanelStyle, HideLatest, InputMode, Settings};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

struct Rig {
    ui: Ui,
    core: Core,
    wakes: Vec<Wake>,
    panel: RuntimeAgentSession,
    display: RuntimeAgentSession,
    history: RuntimeAgentSession,
    input: RuntimeAgentSession,
    editor: Entity<RichTextEditor>,
    source: Entity<TextArea>,
    toast: Entity<ToastStack>,
    stills_only: bool,
    panel_only: bool,
    /// NanaPVMaker capture events: (frame, kind, label) where the script acts.
    events: Vec<(u64, &'static str, &'static str)>,
}
fn document(id: u64, theme: Arc<nana_ui::CompiledTheme>, locale: Locale) -> RuntimeDocument {
    let mut doc = RuntimeDocument::new(DocumentId::new(id).unwrap());
    doc.context_mut().set_theme_tokens(theme).unwrap();
    doc.context_mut()
        .set_default_language(nana_ui::runtime::LanguageTag::new(locale.tag()));
    doc
}
impl Rig {
    fn new(
        skin: Arc<nanatype_theme::Skin>,
        urls: Arc<std::collections::HashMap<String, String>>,
        locale: Locale,
        profile: &str,
        panel_style: ChatPanelStyle,
    ) -> Self {
        let mut settings = Settings::default();
        settings.general.speaker_name = "奈奈".into();
        settings.integrations.bilibili.panel_style = panel_style;
        settings.general.input_mode = if matches!(profile, "compose" | "queue") {
            InputMode::Compose
        } else {
            InputMode::Mirror
        };
        settings.history.chat.enabled = true;
        settings.history.hide_latest = HideLatest::Never;
        settings.integrations.bilibili.enabled = true;
        settings.integrations.bilibili.room = "21452505".into();
        if profile == "queue" {
            settings.playback.queue = nanatype_playback::QueuePolicy::Queue;
            settings.playback.speed_scale = 4.0;
        }
        let panel_width = if profile == "narrow" { 300 } else { 400 };
        let ui = Ui::new(Arc::new(settings.clone()), skin.clone(), locale);
        ui.asset_urls.set(urls);
        ui.chat_width.set(panel_width as f32);
        ui.source_mode.set(profile == "source");
        ui.reduced_motion.set(profile == "reduced");
        let mut status = (*ui.status.get_untracked()).clone();
        status.bilibili = "Bilibili · 21452505".into();
        status.bilibili_link = danmaku::Link::Connected;
        ui.status.set(Arc::new(status));
        let theme = nanatype::ui::kit::theme::compile(nanatype::skin_theme::dark_mode(
            settings.skin.chrome,
            false,
        ));
        let mut p = document(1, theme.clone(), locale);
        let (_, toast) = p
            .context_mut()
            .mount_view_root(DocumentId::new(1).unwrap(), move || {
                let stack = entity_ref::<ToastStack>();
                with_refs(
                    ui::feedback::with_toasts_limit(danmaku::chat_window(ui), stack, 1),
                    stack,
                )
            })
            .unwrap();
        let mut d = document(2, theme.clone(), locale);
        d.context_mut()
            .mount_view_root(DocumentId::new(2).unwrap(), move || {
                dialogue::dialogue_box(ui, dialogue::Source::Display, None)
            })
            .unwrap();
        let mut h = document(3, theme.clone(), locale);
        h.context_mut()
            .mount_view_root(DocumentId::new(3).unwrap(), move || {
                history::history_window(ui)
            })
            .unwrap();
        let mut i = document(4, theme, locale);
        let (_, (editor, source, _)) = i
            .context_mut()
            .mount_view_root(DocumentId::new(4).unwrap(), move || input::input_window(ui))
            .unwrap();
        let scale = if matches!(profile, "capture" | "capture-panel") {
            2.0
        } else {
            1.0
        };
        let [w, dh] = skin.dialogue.region;
        let w = (w + skin.dialogue.overscan * 2.0) as u32;
        let dh = (dh + skin.dialogue.overscan * 2.0) as u32;
        let hh = skin.history.height as u32;
        let hw = skin.history.width as u32;
        let mut rig = Self {
            ui,
            core: Core::new(settings, skin),
            wakes: Vec::new(),
            panel: RuntimeAgentSession::new_scaled(p, panel_width, 450, scale).unwrap(),
            display: RuntimeAgentSession::new_scaled(d, w, dh, scale).unwrap(),
            history: RuntimeAgentSession::new_scaled(h, hw, hh, scale).unwrap(),
            input: RuntimeAgentSession::new(
                i,
                nanatype::windows::INPUT_SIZE.0 as u32,
                nanatype::windows::INPUT_SIZE.1 as u32,
            )
            .unwrap(),
            editor,
            source,
            toast,
            stills_only: matches!(profile, "stills" | "audit"),
            panel_only: profile == "capture-panel",
            events: Vec::new(),
        };
        rig.display.set_clear(Some([0.0; 4]));
        rig.history.set_clear(Some([0.0; 4]));
        for session in [
            &mut rig.panel,
            &mut rig.display,
            &mut rig.history,
            &mut rig.input,
        ] {
            session.set_gpu_renderers(ui::sweep::renderers());
        }
        if profile == "reduced" {
            for session in [
                &mut rig.panel,
                &mut rig.display,
                &mut rig.history,
                &mut rig.input,
            ] {
                session
                    .document_mut()
                    .context_mut()
                    .set_reduced_motion(true);
            }
        }
        rig
    }
    fn apply(&mut self, fx: Effects) {
        self.wakes.extend(fx.wakes);
        if let Some(doc) = fx.replace_draft {
            self.ui.draft.set(Arc::new(doc.clone()));
            let mut src = self.ui.source.get_untracked();
            src.text = nanatype_markup::serialize(&doc);
            src.selection = (src.text.len(), src.text.len());
            src.revision += 1;
            self.ui.source.set(src);
        }
        if fx.reply_changed {
            self.ui.reply.set(self.core.pending_reply().cloned());
        }
        if fx.display_changed {
            self.ui.display.set(Arc::new(self.core.display().clone()));
        }
        if fx.history_changed {
            self.ui
                .history
                .set(Arc::new(self.core.history().iter().cloned().collect()));
        }
    }
    fn cmd(&mut self, cmd: Cmd, now: u64) {
        let fx = self.core.handle(cmd, now);
        self.apply(fx);
    }
    fn feedback(&mut self, title: String) {
        let cx = self.panel.document_mut().context_mut();
        let toast = cx
            .push_toast(
                self.toast,
                Toast::new(title, nana_ui::ToastTone::Success).timeout(Duration::from_millis(1200)),
            )
            .unwrap();
        cx.on(toast, move |_, _: &nana_ui::runtime::ToastDismissed, cx| {
            cx.dispatch_program_all(Msg::ToastDismissed(
                nanatype::windows::WindowKind::Chat,
                toast,
            ));
        })
        .unwrap();
    }
    fn route(&mut self, message: Msg, now: u64) {
        input::capture_source_selection(self.ui, self.input.document().context(), self.source);
        match message {
            Msg::ChatMention { seq, reply } => {
                let m = self
                    .ui
                    .chat
                    .with_untracked(|rows| rows.iter().find(|m| m.seq == seq).unwrap().clone());
                if reply {
                    self.cmd(
                        Cmd::SetReply(Some(m.quote(self.ui.locale.get_untracked()))),
                        now,
                    );
                } else if self.ui.source_mode.get_untracked() {
                    let inline = Inline::Mention(Mention::new(m.user.clone()));
                    let mut src = self.ui.source.get_untracked();
                    let at = src.selection.0;
                    let text =
                        nanatype_markup::serialize(&Document::from_inlines(vec![inline])) + " ";
                    src.text.insert_str(at, &text);
                    src.selection = (at + text.len(), at + text.len());
                    src.revision += 1;
                    let doc = nanatype_markup::parse(&src.text).doc;
                    self.ui.source.set(src);
                    self.ui.draft.set(Arc::new(doc.clone()));
                    self.cmd(Cmd::DraftChanged(doc), now);
                } else {
                    let inline = Inline::Mention(Mention::new(m.user.clone()));
                    let rich = ui::rich::to_rich(
                        &Document::from_inlines(vec![inline]),
                        &ui::rich::Env {
                            look: &dialogue::Base::editor(&self.ui.skin.get_untracked()).look(),
                            urls: &self.ui.asset_urls.get_untracked(),
                            editor: true,
                            locale: self.ui.locale.get_untracked(),
                        },
                    );
                    let cx = self.input.document_mut().context_mut();
                    cx.rich_edit(
                        self.editor,
                        RichEditCommand::InsertObject(rich.objects()[0].1.clone()),
                    )
                    .unwrap();
                    cx.rich_edit(self.editor, RichEditCommand::InsertText(" ".into()))
                        .unwrap();
                }
                self.ui.chat_target.set(Some(seq));
                let target = if self.ui.source_mode.get_untracked() {
                    self.source.stable_id()
                } else {
                    self.editor.stable_id()
                };
                self.input.flush().unwrap();
                self.input
                    .document_mut()
                    .context_mut()
                    .focus_node(DocumentId::new(4).unwrap(), target)
                    .unwrap();
                self.feedback(self.ui.tr(
                    if reply {
                        "chat-replying"
                    } else {
                        "chat-inserted"
                    },
                    &[("name", m.user.name.as_str().into())],
                ));
            }
            Msg::EditorChanged(rich) => {
                let doc = ui::rich::from_rich(
                    &rich,
                    &dialogue::Base::editor(&self.ui.skin.get_untracked()).look(),
                );
                self.ui.editor_value.set(rich);
                self.ui.draft.set(Arc::new(doc.clone()));
                self.cmd(Cmd::DraftChanged(doc), now);
            }
            Msg::Markup(nanatype::msg::MarkupEdit::Typed { text, selection }) => {
                let mut src = self.ui.source.get_untracked();
                src.text = text;
                src.selection = selection;
                let doc = nanatype_markup::parse(&src.text).doc;
                self.ui.source.set(src);
                self.ui.draft.set(Arc::new(doc.clone()));
                self.cmd(Cmd::DraftChanged(doc), now);
            }
            Msg::Core(cmd) => {
                let send = matches!(cmd, Cmd::Send);
                self.cmd(cmd, now);
                if send {
                    self.ui.chat_target.set(None);
                    self.feedback(self.ui.now_t("status-sent"));
                }
            }
            Msg::Edit(edit) => {
                let mut settings = (*self.ui.settings.get_untracked()).clone();
                edit(&mut settings);
                self.ui.settings.set(Arc::new(settings.clone()));
                self.cmd(Cmd::SetSettings(Box::new(settings)), now);
            }
            Msg::ClearReply => {
                self.cmd(Cmd::SetReply(None), now);
                self.ui.chat_target.set(None);
            }
            Msg::ToastDismissed(_, toast) => {
                self.panel
                    .document_mut()
                    .context_mut()
                    .remove_toast(self.toast, toast)
                    .unwrap();
            }
            _ => panic!("unexpected capture action"),
        }
    }
    fn pump(&mut self, now: u64) {
        for _ in 0..20 {
            self.input.flush().unwrap();
            self.panel.flush().unwrap();
            let mut messages = self
                .input
                .document_mut()
                .context_mut()
                .take_program_messages();
            messages.extend(
                self.panel
                    .document_mut()
                    .context_mut()
                    .take_program_messages(),
            );
            if messages.is_empty() {
                return;
            }
            for msg in messages {
                if let Ok(msg) = msg.downcast::<Msg>() {
                    self.route(*msg, now);
                }
            }
        }
        panic!("interaction did not settle");
    }
    fn click(&mut self, panel: bool, label: &str, index: usize, now: u64) {
        let session = if panel {
            &mut self.panel
        } else {
            &mut self.input
        };
        session.flush().unwrap();
        let nodes: Vec<_> = session
            .accessibility_dump()
            .into_iter()
            .filter(|n| n.role == "button" && n.label.as_deref() == Some(label))
            .collect();
        let target = nodes
            .get(index)
            .unwrap_or_else(|| panic!("missing button {label}, index {index}"));
        assert!(session.click_node(target.id).unwrap());
        self.pump(now);
    }
    fn step(&mut self, f: u64, out: &Path, capture: bool) {
        let now = f * 1000 / 30;
        self.ui.now.set(now);
        while let Some(i) = self.wakes.iter().position(|w| w.at_ms <= now) {
            let w = self.wakes.remove(i);
            self.cmd(
                Cmd::Wake {
                    epoch: w.epoch,
                    reason: w.reason,
                },
                now,
            );
        }
        if [8, 20, 32].contains(&f) {
            let n = if f == 8 {
                0
            } else if f == 20 {
                1
            } else {
                2
            };
            self.events.push((f, "show", ["c1", "c2", "c3"][n]));
            let m = Arc::new(sample(n));
            let mut rows = (*self.ui.chat.get_untracked()).clone();
            rows.push(m.clone());
            self.ui.chat.set(Arc::new(rows));
            self.cmd(
                Cmd::External(m.history_item(self.ui.locale.get_untracked())),
                now,
            );
        }
        if f == 44 {
            let buttons: Vec<_> = self
                .panel
                .accessibility_dump()
                .into_iter()
                .filter(|n| n.label.as_deref() == Some(self.ui.now_t("chat-reply").as_str()))
                .collect();
            if let Some(button) = buttons.get(1) {
                assert!(
                    self.ui
                        .settings
                        .with(|s| s.integrations.bilibili.panel_style)
                        != ChatPanelStyle::Compact
                        || button.bounds.y - buttons[0].bounds.y <= 72.0,
                    "short messages stay compact in the operator panel"
                );
                self.panel
                    .hover_xy(button.bounds.x + 4.0, button.bounds.y + 4.0)
                    .unwrap();
            }
        }
        if f == 56 {
            let draft = self.ui.draft.get_untracked();
            let source = self.ui.source.get_untracked();
            let label = self.ui.now_t("chat-reply");
            self.events.push((f, "click", "reply"));
            self.click(true, &label, 1, now);
            assert_eq!(self.ui.draft.get_untracked(), draft);
            assert_eq!(self.ui.source.get_untracked().text, source.text);
            assert!(self.core.pending_reply().is_some());
        }
        if (64..=90).contains(&f)
            && f.is_multiple_of(2)
            && let Some(ch) = "今天打新游戏哦～".chars().nth(((f - 64) / 2) as usize)
        {
            if f == 64 {
                self.events.push((f, "type", "replyText"));
            }
            self.input.type_text(&ch.to_string()).unwrap();
            self.pump(now);
        }
        if f == 96 {
            self.events.push((f, "show", "images"));
            // Images arrive while the draft is focused. Message keys and selection
            // must survive the update in all four views.
            let before: Vec<_> = self
                .panel
                .accessibility_dump()
                .into_iter()
                .filter(|n| n.role == "button")
                .map(|n| n.id)
                .collect();
            let selection = self
                .input
                .accessibility_dump()
                .into_iter()
                .find(|n| n.focused)
                .and_then(|n| n.selection);
            let blobs = out.join("cache/images");
            std::fs::create_dir_all(&blobs).unwrap();
            let library = nanatype::assets::AssetLibrary::new(blobs.clone());
            let mut urls = (*self.ui.asset_urls.get_untracked()).clone();
            for n in 0..3 {
                let color = [
                    [240, 145, 174, 255],
                    [94, 174, 232, 255],
                    [230, 190, 70, 255],
                ][n];
                image::RgbaImage::from_pixel(64, 64, image::Rgba(color))
                    .save(blobs.join(format!("demo-{n}.png")))
                    .unwrap();
                let asset = AssetRef::Blob(format!("demo-{n}"));
                urls.insert(asset.to_string(), library.url(&asset, None).unwrap());
            }
            self.ui.asset_urls.set(Arc::new(urls));
            self.pump(now);
            assert_eq!(
                selection,
                self.input
                    .accessibility_dump()
                    .into_iter()
                    .find(|n| n.focused)
                    .and_then(|n| n.selection)
            );
            assert_eq!(
                before,
                self.panel
                    .accessibility_dump()
                    .into_iter()
                    .filter(|n| n.role == "button")
                    .map(|n| n.id)
                    .collect::<Vec<_>>()
            );
        }
        if f == 100 {
            let draft = self.ui.draft.get_untracked().plain_text();
            let mods = nana_ui_platform::InputModifiers {
                control: true,
                ..Default::default()
            };
            self.input.key_press("z", "KeyZ", mods).unwrap();
            self.pump(now);
            assert_ne!(
                draft,
                self.ui.draft.get_untracked().plain_text(),
                "undo history survived image arrival"
            );
            self.input
                .key_press(
                    "z",
                    "KeyZ",
                    nana_ui_platform::InputModifiers {
                        shift: true,
                        ..mods
                    },
                )
                .unwrap();
            self.pump(now);
            assert_eq!(draft, self.ui.draft.get_untracked().plain_text());
        }
        if f == 112 {
            assert_eq!(
                self.ui.draft.get_untracked().plain_text(),
                "今天打新游戏哦～",
                "reply attaches a quote without inserting an author label or space"
            );
            let label = self.ui.now_t("tb-send");
            self.events.push((f, "click", "send1"));
            self.click(false, &label, 0, now);
            assert!(
                self.core.history().iter().any(|r| r
                    .item
                    .reply
                    .as_ref()
                    .is_some_and(|r| r.user.name == "阿澈"))
            );
        }
        if f == 122 {
            let label = self.ui.now_t("chat-at-action");
            self.events.push((f, "click", "mention"));
            self.click(true, &label, 2, now);
        }
        if (124..=140).contains(&f)
            && let Some(ch) = "自我介绍马上来～".chars().nth((f - 124) as usize)
        {
            if f == 124 {
                self.events.push((f, "type", "mentionText"));
            }
            self.input.type_text(&ch.to_string()).unwrap();
            self.pump(now);
        }
        if f == 120 && self.ui.chat_width.get_untracked() == 300.0 {
            let mut rows = (*self.ui.chat.get_untracked()).clone();
            let mut last = (*rows[2]).clone();
            last.user.name = "这是一位名字特别长特别长的观众 A very long viewer name".into();
            // Existing rows normally keep an immutable user identity; a new
            // message exercises truncation without changing the selected row.
            last.seq = 99;
            last.doc = Document::plain(
                "这是一段会在窄窗口里换成好几行的弹幕。This should wrap without overlapping the actions.",
            );
            rows.push(Arc::new(last));
            self.ui.chat.set(Arc::new(rows));
        }
        if f == 145 {
            let label = self.ui.now_t("tb-send");
            self.events.push((f, "click", "send2"));
            self.click(false, &label, 0, now);
        }
        self.pump(now);
        for session in [
            &mut self.panel,
            &mut self.display,
            &mut self.history,
            &mut self.input,
        ] {
            session
                .document_mut()
                .context_mut()
                .advance_animations(Duration::from_millis(now));
            session.flush().unwrap();
        }
        if capture || f == 149 || (!self.stills_only && [0, 40, 62, 90, 116].contains(&f)) {
            for (name, session) in [
                ("panel", &mut self.panel),
                ("display", &mut self.display),
                ("history", &mut self.history),
            ] {
                if self.panel_only && name != "panel" {
                    continue;
                }
                let dir = out.join(name);
                std::fs::create_dir_all(&dir).unwrap();
                session
                    .screenshot_png(dir.join(format!("{f:04}.png")))
                    .unwrap();
            }
        }
        if f == 149 || (!self.stills_only && [62, 90, 116].contains(&f)) {
            self.input
                .screenshot_png(out.join(format!("input-{f}.png")))
                .unwrap();
        }
    }

    /// Preserve each skin's actual history layout while exercising difficult content.
    fn audit(&mut self, out: &Path) {
        let skin = self.ui.skin.get_untracked();
        // This fixture checks short-log top alignment. Pixel-sized headers can
        // legitimately make the interaction log overflow a 560px window; use
        // a tall viewport here, then restore real sizes for both stress shots.
        self.history
            .set_viewport(skin.history.width as u32, 2000, 1.0)
            .unwrap();
        self.history.flush().unwrap();
        std::fs::write(
            out.join("spacing-baseline.json"),
            serde_json::to_string_pretty(&history_spacing(
                &self.history.accessibility_dump(),
                &skin,
                true,
            ))
            .unwrap(),
        )
        .unwrap();
        let mut message = sample(1);
        message.seq = 201;
        message.user.name = "这是一位名字特别长的观众 A very long viewer name".into();
        message.doc = Document::plain(
            "长弹幕第一行：检查昵称截断、文字换行与回复引用。\n第二行包含 English words，正文应在窗口内完整换行，不与操作按钮重叠。",
        );
        message.guard = nanatype_bilibili::Guard::Captain;
        message.medal_name = "粉丝牌".into();
        message.medal_level = 12;
        let mut rows = (*self.ui.chat.get_untracked()).clone();
        rows.push(Arc::new(message.clone()));
        self.ui.chat.set(Arc::new(rows));
        self.cmd(
            Cmd::External(message.history_item(self.ui.locale.get_untracked())),
            6000,
        );
        self.cmd(
            Cmd::SetReply(Some(message.quote(self.ui.locale.get_untracked()))),
            6010,
        );
        let doc = nanatype_markup::parse("[b]多行台词与引用[/b]：第一行保持皮肤的字体与颜色。\n第二行 [at=demo-1 name=\"阿澈\" avatar=blob:demo-1] 谢谢你的留言～").doc;
        self.cmd(Cmd::DraftChanged(doc), 6020);
        self.cmd(Cmd::Send, 6030);
        self.cmd(
            Cmd::DraftChanged(Document::plain("连续台词：按这套皮肤的分组规则显示。")),
            6040,
        );
        self.cmd(Cmd::Send, 6050);

        for (width, name) in [(skin.history.width as u32, "default"), (300, "narrow")] {
            let mut settings = (*self.ui.settings.get_untracked()).clone();
            settings.history.show_time = true;
            settings.overlays.history.size = Some([width as f32, skin.history.height]);
            self.ui.settings.set(Arc::new(settings));
            let panel_width = if name == "narrow" { 300 } else { 400 };
            self.ui.chat_width.set(panel_width as f32);
            self.panel.set_viewport(panel_width, 450, 1.0).unwrap();
            self.history
                .set_viewport(width, skin.history.height as u32, 1.0)
                .unwrap();
            self.ui.now.set(6500);
            self.pump(6500);
            for session in [&mut self.panel, &mut self.history] {
                session
                    .document_mut()
                    .context_mut()
                    .advance_animations(Duration::from_millis(6500));
                session.flush().unwrap();
            }
            for (surface, session, viewport) in [
                ("panel", &mut self.panel, panel_width),
                ("history", &mut self.history, width),
            ] {
                let nodes = session.accessibility_dump();
                if surface == "history" {
                    std::fs::write(
                        out.join(format!("spacing-{name}.json")),
                        serde_json::to_string_pretty(&history_spacing(&nodes, &skin, false))
                            .unwrap(),
                    )
                    .unwrap();
                }
                if surface == "history"
                    && skin.history.layout == nanatype_theme::HistoryLayout::Inline
                {
                    let author = nodes
                        .iter()
                        .find(|n| n.label.as_deref() == Some(message.user.name.as_str()))
                        .unwrap();
                    let timestamp = history::local_time(message.at, skin.history.time_seconds);
                    let time = nodes
                        .iter()
                        .find(|n| {
                            n.label.as_deref() == Some(timestamp.as_str())
                                && (n.bounds.y - author.bounds.y).abs() < 4.0
                        })
                        .unwrap();
                    assert!(
                        time.bounds.x - (author.bounds.x + author.bounds.width) - 20.0
                            >= skin.history.text.size * 5.0,
                        "{} {name}: inline body needs room for readable phrases and @ labels",
                        skin.meta.id
                    );
                }
                for node in &nodes {
                    if node.label.as_ref().is_some_and(|l| !l.is_empty()) {
                        assert!(
                            node.bounds.x >= -1.0
                                && node.bounds.x + node.bounds.width <= viewport as f32 + 1.0,
                            "{} {surface} {name}: text/control overflows: {:?} {:?}",
                            skin.meta.id,
                            node.label,
                            node.bounds
                        );
                    }
                    assert!(
                        surface != "history" || node.role != "button",
                        "history must not contain streamer controls"
                    );
                }
                std::fs::write(
                    out.join(format!("{surface}-{name}.json")),
                    serde_json::to_string_pretty(&nodes).unwrap(),
                )
                .unwrap();
                session
                    .screenshot_png(out.join(format!("{surface}-{name}.png")))
                    .unwrap();
                if surface == "panel" {
                    // Keep both the long-message tail and the first messages:
                    // tall skins may have no viewer action inside the tail viewport.
                    session.scroll_by(150.0, 200.0, 0.0, 10_000.0).unwrap();
                    session.flush().unwrap();
                    session
                        .screenshot_png(out.join(format!("panel-{name}-top.png")))
                        .unwrap();
                    let nodes = session.accessibility_dump();
                    let reply = self.ui.now_t("chat-reply");
                    if let Some(b) = nodes.iter().find(|n| {
                        n.role == "button"
                            && n.label.as_deref() == Some(reply.as_str())
                            && n.bounds.y >= 76.0
                            && n.bounds.y < 410.0
                    }) {
                        session
                            .hover_xy(b.bounds.x + 2.0, b.bounds.y + 2.0)
                            .unwrap();
                        session
                            .screenshot_png(out.join(format!("panel-{name}-top-hover.png")))
                            .unwrap();
                    }
                }
            }
        }
    }
}

/// Check the real entry rectangles, including grouped and multiline rows.
fn history_spacing(
    nodes: &[AccessibilityDumpNode],
    skin: &nanatype_theme::Skin,
    at_top: bool,
) -> serde_json::Value {
    let ancestors = |name: &str| {
        let mut path = Vec::new();
        let mut current = Some(
            nodes
                .iter()
                .find(|n| n.label.as_deref() == Some(name))
                .unwrap()
                .id,
        );
        while let Some(id) = current {
            path.push(id);
            current = nodes.iter().find(|n| n.id == id).unwrap().parent;
        }
        path
    };
    let paths = [ancestors("小麦"), ancestors("阿澈"), ancestors("路过的猫")];
    let common = paths[0]
        .iter()
        .find(|id| paths[1].contains(id) && paths[2].contains(id))
        .unwrap();
    let list = nodes.iter().find(|n| n.id == *common).unwrap();
    let rows: Vec<_> = list
        .children
        .iter()
        .map(|id| {
            let mut row = nodes.iter().find(|n| n.id == *id).unwrap();
            // Native dynamic/transition containers include the entry's margin.
            // Descend to its content box, whose bottom/next top delimit the gap.
            while row.children.len() == 1 {
                row = nodes.iter().find(|n| n.id == row.children[0]).unwrap();
            }
            row
        })
        .filter(|n| n.bounds.height > 0.0)
        .collect();
    assert!(
        rows.len() >= 5,
        "{}: all history entry rectangles are present",
        skin.meta.id
    );
    let gaps: Vec<_> = rows
        .windows(2)
        .map(|r| r[1].bounds.y - (r[0].bounds.y + r[0].bounds.height))
        .collect();
    for gap in &gaps {
        assert!(
            (7.9..=20.1).contains(gap),
            "{}: history gap must stay within 8–20px, got {gap}",
            skin.meta.id
        );
    }
    if at_top {
        let inset = skin.history.panel.as_ref().map_or(16.0, |p| {
            p.margin[0] + p.shape.border_widths(p.border_width)[0] + p.padding[1]
        });
        let expected = inset + skin.history.title.as_ref().map_or(0.0, |t| t.height);
        assert!(
            (rows[0].bounds.y - expected).abs() <= 1.0,
            "{}: history starts at content top {expected}, got {}",
            skin.meta.id,
            rows[0].bounds.y
        );
    }
    serde_json::json!({"first_y": rows[0].bounds.y, "gaps": gaps})
}

fn sample(n: usize) -> ChatMessage {
    let (name, text) = [
        ("小麦", "主播晚上好！"),
        ("阿澈", "今天打什么游戏呀"),
        ("路过的猫", "求个自我介绍～"),
    ][n];
    ChatMessage {
        platform: nanatype::chat::Platform::Bilibili,
        epoch: 0,
        room: String::new(),
        seq: n as u64 + 1,
        id: format!("demo-{n}"),
        kind: ChatKind::Text,
        user: ChatUser {
            uid: format!("demo-{n}"),
            name: name.into(),
            avatar: Some(AssetRef::Blob(format!("demo-{n}"))),
        },
        guard: Default::default(),
        owner: false,
        admin: false,
        medal_level: 0,
        medal_name: String::new(),
        doc: Document::plain(text),
        at: 1_760_000_000_000 + n as u64,
    }
}
/// NanaPVMaker capture contract (schema 1, its docs/capture-contract.md): one
/// `capture.json` per surface folder, so `pv capture` reads frames and events
/// directly instead of a hand-copied schedule.
fn write_capture_manifests(out: &Path, events: &[(u64, &str, &str)], panel_only: bool) {
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
    };
    let rev = git(&["rev-parse", "--short=9", "HEAD"]).unwrap_or_else(|| "unknown".into());
    let dirty =
        git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
    let events: Vec<_> = events
        .iter()
        .map(|&(f, kind, label)| {
            serde_json::json!({"t_ms": (f as f64 * 1000.0 / 30.0).round() as u64, "kind": kind, "label": label})
        })
        .collect();
    for name in ["panel", "display", "history"] {
        if panel_only && name != "panel" {
            continue;
        }
        let dir = out.join(name);
        let (w, h) = image::image_dimensions(dir.join("0000.png")).unwrap();
        let manifest = serde_json::json!({
            "schema": 1,
            "name": name,
            "source": {"repo": "NanaType", "rev": rev, "dirty": dirty},
            "fps": 30,
            "size": [w, h],
            "scale": 2,
            "theme": "light",
            "count": 150,
            "alpha": "straight",
            "dir": ".",
            "pattern": "%04d.png",
            "events": events,
            "capturedAt": utc_now(),
        });
        std::fs::write(
            dir.join("capture.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }
}

/// ISO-8601 UTC wall time (metadata only; frames use the capture clock).
fn utc_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn main() {
    std::thread::Builder::new()
        .name("parity-capture".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(run)
        .unwrap()
        .join()
        .unwrap();
}
fn run() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let out = PathBuf::from(
        args.first()
            .map(String::as_str)
            .unwrap_or("target/chat-parity"),
    );
    std::fs::create_dir_all(&out).unwrap();
    nanatype::services::media::init(&out.join("data")).unwrap();
    let id = args.get(1).map(String::as_str).unwrap_or("nanatype.halo");
    let locale = args
        .get(2)
        .and_then(|s| Locale::from_tag(s))
        .unwrap_or(Locale::ZhCn);
    let profile = args.get(3).map(String::as_str).unwrap_or("capture");
    let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
    let ids = if id == "all" {
        registry.entries().iter().map(|e| e.id.clone()).collect()
    } else {
        vec![id.to_owned()]
    };
    for id in ids {
        let loaded = registry.load(&id).unwrap();
        let mut fonts = nanatype::skin_fonts::BuiltinFontSet::default();
        let roles = [
            &loaded.skin.fonts.body,
            &loaded.skin.fonts.name,
            &loaded.skin.fonts.accent,
        ];
        let refs: Vec<_> = roles
            .iter()
            .copied()
            .chain(loaded.skin.fonts.latin.as_ref())
            .filter_map(|f| {
                f.file
                    .clone()
                    .filter(|f| nanatype_theme::is_builtin_font(f))
            })
            .collect();
        for e in fonts.sync(
            refs.iter().map(String::as_str),
            nanatype_theme::builtin_font,
            nanatype::skin_fonts::register_with_nana_ui,
        ) {
            eprintln!("font: {e}");
        }
        let mut packaged_fonts = nana_ui::HostFontScope::new();
        let families = roles.map(|f| {
            let path = f.file.as_ref()?;
            if nanatype_theme::is_builtin_font(path) {
                return fonts.family(path);
            }
            packaged_fonts
                .add_bytes(loaded.files.read(path).ok()?)
                .ok()?
                .families()
                .first()
                .cloned()
        });
        let skin = Arc::new(nanatype::skin_theme::prepare(
            &loaded.skin,
            &Default::default(),
            &families,
            locale.tag(),
        ));
        let urls = Arc::new(nanatype::assets::skin_preview_urls(
            &loaded.skin,
            &*loaded.files,
        ));
        let dir = if args.get(1).is_some_and(|s| s == "all") {
            out.join(&id)
        } else {
            out.clone()
        };
        std::fs::create_dir_all(&dir).unwrap();
        nanatype::clock::capture_at(0, || {
            let panel_style = if args.get(4).is_some_and(|s| s == "history") {
                ChatPanelStyle::History
            } else {
                ChatPanelStyle::Compact
            };
            let mut rig = Rig::new(skin, urls, locale, profile, panel_style);
            for f in 0..150 {
                nanatype::clock::capture_at(f * 1000 / 30, || {
                    rig.step(f, &dir, matches!(profile, "capture" | "capture-panel"))
                });
            }
            if matches!(profile, "capture" | "capture-panel") {
                write_capture_manifests(&dir, &rig.events, rig.panel_only);
            }
            for stream in [&rig.history, &rig.display] {
                assert!(
                    !stream
                        .accessibility_dump()
                        .iter()
                        .any(|n| n.role == "button"),
                    "captured surfaces must never contain streamer controls"
                );
            }
            if profile == "queue" {
                assert!(rig.core.queued() > 0, "the second reply is queued");
            }
            assert!(rig.core.pending_reply().is_none());
            assert!(rig.ui.draft.get_untracked().is_blank());
            if matches!(
                profile,
                "verify" | "source" | "compose" | "queue" | "reduced" | "narrow"
            ) {
                nanatype::clock::capture_at(5100, || {
                    // An external click must insert at the saved caret, not the end.
                    rig.input.type_text("前后").unwrap();
                    rig.pump(5100);
                    rig.input
                        .key_press("ArrowLeft", "ArrowLeft", Default::default())
                        .unwrap();
                    rig.pump(5100);
                    let label = rig.ui.now_t("chat-at-action");
                    rig.click(true, &label, 0, 5100);
                    assert_eq!(rig.ui.draft.get_untracked().plain_text(), "前@小麦 后");
                    let reply = rig.ui.now_t("chat-reply");
                    let draft = rig.ui.draft.get_untracked();
                    let source = rig.ui.source.get_untracked();
                    let selection = rig
                        .input
                        .accessibility_dump()
                        .into_iter()
                        .find(|n| n.focused)
                        .and_then(|n| n.selection);
                    rig.click(true, &reply, 0, 5100);
                    assert_eq!(
                        rig.ui.draft.get_untracked(),
                        draft,
                        "reply preserves existing text and explicitly inserted mentions"
                    );
                    assert_eq!(rig.ui.source.get_untracked().text, source.text);
                    assert_eq!(
                        selection,
                        rig.input
                            .accessibility_dump()
                            .into_iter()
                            .find(|n| n.focused)
                            .and_then(|n| n.selection),
                        "reply preserves the editor caret"
                    );
                    assert!(rig.core.pending_reply().is_some());
                    let cancel = rig.ui.now_t("chat-reply-cancel");
                    rig.click(false, &cancel, 0, 5100);
                    if profile == "queue" {
                        assert!(rig.core.queued() > 0, "the second reply is queued");
                    }
                    assert!(rig.core.pending_reply().is_none());
                    assert_eq!(
                        rig.ui.draft.get_untracked(),
                        draft,
                        "cancelling a quote preserves the draft"
                    );
                });
            }
            if profile == "audit" {
                nanatype::clock::capture_at(6500, || rig.audit(&dir));
            }
            if profile == "verify" && id == "nanatype.halo" {
                let history_nodes = rig.history.accessibility_dump();
                let names: Vec<_> = history_nodes
                    .iter()
                    .filter(|n| n.label.as_deref() == Some("奈奈"))
                    .collect();
                assert_eq!(
                    names.len(),
                    2,
                    "both consecutive host lines keep their name and avatar header"
                );
                let first = history_nodes
                    .iter()
                    .find(|n| n.label.as_deref() == Some("小麦"))
                    .unwrap();
                assert!(
                    first.bounds.y < 30.0,
                    "history starts at the top like the approved presentation"
                );
                // Compact rows fit more messages; add enough to exercise scrolling.
                let mut rows = (*rig.ui.chat.get_untracked()).clone();
                for n in 0..12 {
                    let mut m = sample(n % 3);
                    m.seq = 100 + n as u64;
                    rows.push(Arc::new(m));
                }
                rig.ui.chat.set(Arc::new(rows));
                rig.pump(5200);
                rig.panel.scroll_by(180.0, 180.0, 0.0, 120.0).unwrap();
                rig.pump(5200);
                let label = rig.ui.now_t("chat-latest");
                assert!(
                    rig.panel
                        .accessibility_dump()
                        .iter()
                        .any(|n| n.role == "button" && n.label.as_deref() == Some(label.as_str())),
                    "scrolling up exposes return-to-latest"
                );
                rig.panel
                    .screenshot_png(dir.join("panel-reading.png"))
                    .unwrap();
                rig.click(true, &label, 0, 5200);
                assert!(
                    !rig.panel
                        .accessibility_dump()
                        .iter()
                        .any(|n| n.role == "button" && n.label.as_deref() == Some(label.as_str())),
                    "return-to-latest resumes following"
                );
            }
            println!("{id} {profile}: native reply/@/typing/send and history assertions passed");
        });
    }
}
