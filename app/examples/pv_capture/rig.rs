//! The capture rig: real windows mounted offscreen, driven frame by frame on
//! the capture clock, with every audible moment recorded as an event.

use nana_ui::runtime::view::IntoView;
use nana_ui::runtime::{DocumentId, Entity, RichTextEditor, RuntimeDocument, TextArea};
use nana_ui_devtools::agent::{AgentSession, RuntimeAgentSession};
use nanatype::msg::Msg;
use nanatype::ui::{self, Ui, dialogue};
use nanatype_core::{Cmd, Core, Effects, Event, Wake};
use nanatype_playback::CueKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub const FPS: u64 = 30;

/// Milliseconds at frame `f`.
pub fn ms(f: u64) -> u64 {
    f * 1000 / FPS
}

pub struct Surface {
    pub name: &'static str,
    pub session: RuntimeAgentSession,
    /// Driven but not written (the input window behind a chat click).
    pub shoot: bool,
    scale: f32,
}

pub struct Rig {
    pub ui: Ui,
    pub core: Core,
    wakes: Vec<Wake>,
    pub surfaces: Vec<Surface>,
    input: Option<(usize, DocumentId, Entity<RichTextEditor>, Entity<TextArea>)>,
    theme: Arc<nana_ui::CompiledTheme>,
    scenario: &'static str,
    out: PathBuf,
    /// (t_ms, kind, label): what NanaPVMaker hangs sounds on.
    pub events: Vec<(u64, &'static str, String)>,
    lines: usize,
    next_doc: u64,
    warmed: bool,
    focused: bool,
}

/// The frame after the last character `typing` types.
pub fn typed_end(start: u64, every: u64, text: &str) -> u64 {
    start + every * text.chars().count() as u64
}

impl Rig {
    pub fn new(
        scenario: &'static str,
        out: &Path,
        ui: Ui,
        core: Core,
        theme: Arc<nana_ui::CompiledTheme>,
    ) -> Self {
        Self {
            ui,
            core,
            wakes: Vec::new(),
            surfaces: Vec::new(),
            input: None,
            theme,
            scenario,
            out: out.to_owned(),
            events: Vec::new(),
            lines: 0,
            next_doc: 1,
            warmed: false,
            focused: false,
        }
    }

    fn document(&mut self) -> (RuntimeDocument, DocumentId) {
        let id = DocumentId::new(self.next_doc).unwrap();
        self.next_doc += 1;
        let mut doc = RuntimeDocument::new(id);
        let locale = self.ui.locale.get_untracked();
        nanatype_frame::apply_locale(doc.context_mut(), locale);
        let _ = doc.context_mut().set_theme_tokens(self.theme.clone());
        (doc, id)
    }

    fn add(
        &mut self,
        name: &'static str,
        doc: RuntimeDocument,
        (w, h): (u32, u32),
        scale: f32,
        transparent: bool,
    ) -> usize {
        let mut session = RuntimeAgentSession::new_scaled(doc, w, h, scale).expect("session");
        session.set_gpu_renderers(ui::sweep::renderers());
        if transparent {
            session.set_clear(Some([0.0; 4]));
        }
        self.surfaces.push(Surface {
            name,
            session,
            shoot: true,
            scale,
        });
        self.surfaces.len() - 1
    }

    /// Any view, as its own window.
    pub fn mount<V: IntoView + 'static>(
        &mut self,
        name: &'static str,
        size: (u32, u32),
        scale: f32,
        transparent: bool,
        view: impl FnOnce() -> V + 'static,
    ) -> usize {
        let (mut doc, id) = self.document();
        doc.context_mut().mount_view_root(id, view).unwrap();
        self.add(name, doc, size, scale, transparent)
    }

    /// The display box over a transparent stream background.
    pub fn display(&mut self, name: &'static str, scale: f32) -> usize {
        let skin = self.ui.skin.get_untracked();
        let d = &skin.dialogue;
        let size = (
            (d.region[0] + d.overscan * 2.0) as u32,
            (d.region[1] + d.overscan * 2.0) as u32,
        );
        let ui = self.ui;
        self.mount(name, size, scale, true, move || {
            dialogue::display_window(ui)
        })
    }

    /// A partner's box.
    pub fn partner_display(&mut self, name: &'static str, scale: f32) -> usize {
        let skin = self.ui.skin.get_untracked();
        let d = &skin.dialogue;
        let size = (
            (d.region[0] + d.overscan * 2.0) as u32,
            (d.region[1] + d.overscan * 2.0) as u32,
        );
        let ui = self.ui;
        self.mount(name, size, scale, true, move || dialogue::peer_window(ui))
    }

    pub fn history(&mut self, name: &'static str, scale: f32) -> usize {
        let skin = self.ui.skin.get_untracked();
        let size = (skin.history.width as u32, skin.history.height as u32);
        let ui = self.ui;
        self.mount(name, size, scale, true, move || {
            ui::history::history_window(ui)
        })
    }

    /// The streamer's input window; typing and clicks go through it.
    pub fn input(&mut self, name: &'static str, scale: f32) -> usize {
        let (mut doc, id) = self.document();
        let ui = self.ui;
        let (_, (editor, source, _)) = doc
            .context_mut()
            .mount_view_root(id, move || ui::input::input_window(ui))
            .unwrap();
        let size = (
            nanatype::windows::INPUT_SIZE.0 as u32,
            nanatype::windows::INPUT_SIZE.1 as u32,
        );
        let index = self.add(name, doc, size, scale, false);
        self.input = Some((index, id, editor, source));
        index
    }

    /// The input's `/` completion list for `draft` (the text before the caret),
    /// placed under the caret as the host places it; `None` closes it.
    pub fn completion(&mut self, draft: Option<&str>) {
        use nanatype::completion::{self, CompletionView};
        let Some(draft) = draft else {
            self.ui.completion.set(None);
            self.ui.completion_at.set(None);
            return;
        };
        let Some(token) = completion::token(draft, draft.len()) else {
            return;
        };
        let items = completion::commands(
            &self.ui.skin.get_untracked(),
            Default::default(),
            &token.needle,
            self.ui.locale.get_untracked(),
        );
        self.ui.completion.set(Some(Arc::new(CompletionView {
            trigger: token.trigger,
            start: token.range.start,
            first: 0,
            items: items.into_iter().take(completion::MAX_ITEMS).collect(),
            selected: 0,
        })));
        let (i, _, editor, _) = self.input.as_ref().expect("an input window");
        let (i, editor) = (*i, editor.stable_id());
        // a second pass once the list knows its own size
        for _ in 0..2 {
            self.surfaces[i].session.flush().unwrap();
            let at = ui::input::completion::anchor(
                self.ui,
                self.surfaces[i].session.document().context().world(),
                editor,
            );
            self.ui.completion_at.set(at.map(|a| a.at));
        }
    }

    /// Click the text field whose agent path ends with `key` and type into it.
    pub fn type_into(&mut self, at: usize, key: &str, text: &str, now: u64) {
        let session = &mut self.surfaces[at].session;
        session.flush().unwrap();
        let node = session
            .accessibility_dump()
            .into_iter()
            .find(|n| n.agent_path.as_deref().is_some_and(|p| p.ends_with(key)))
            .unwrap_or_else(|| panic!("{}: no field {key}", self.scenario));
        session.click_node(node.id).unwrap();
        session.type_text(text).unwrap();
        self.pump(now);
    }

    /// The display box set vertically, in a window of its own shape.
    pub fn vertical_display(&mut self, name: &'static str, size: (u32, u32), scale: f32) -> usize {
        let ui = self.ui;
        self.mount(name, size, scale, true, move || {
            dialogue::display_window(ui)
        })
    }

    pub fn event(&mut self, f: u64, kind: &'static str, label: impl Into<String>) {
        self.events.push((ms(f), kind, label.into()));
    }

    fn apply(&mut self, fx: Effects, now: u64) {
        self.wakes.extend(fx.wakes);
        // Sounds, shakes and expressions at the time the app would run them.
        for batch in &fx.cues {
            for cue in batch.cues.iter() {
                let at = batch.origin_ms + u64::from(cue.at_ms);
                let label = match &cue.kind {
                    CueKind::TypeSound { .. } => {
                        let label = format!("{}-reveal", self.scenario);
                        self.events.push((at, "key", label));
                        continue;
                    }
                    CueKind::Sound { sound, .. } => format!("se-{sound}"),
                    CueKind::Shake { .. } => "shake".into(),
                    CueKind::Expression { name, .. } => format!("expr-{name}"),
                    _ => continue,
                };
                self.events
                    .push((at, "mark", format!("{}-{label}", self.scenario)));
            }
        }
        for event in &fx.events {
            if let Event::LineStarted(_) = event {
                self.lines += 1;
                self.events
                    .push((now, "show", format!("{}-line{}", self.scenario, self.lines)));
            }
        }
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

    pub fn cmd(&mut self, cmd: Cmd, now: u64) {
        let fx = self.core.handle(cmd, now);
        self.apply(fx, now);
    }

    fn route(&mut self, message: Msg, now: u64) {
        if let Some((i, _, _, source)) = self.input {
            ui::input::capture_source_selection(
                self.ui,
                self.surfaces[i].session.document().context(),
                source,
            );
        }
        match message {
            Msg::EditorChanged(rich) => {
                let look = dialogue::Base::editor(&self.ui.skin.get_untracked()).look();
                let doc = ui::rich::from_rich(&rich, &look);
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
            Msg::ChatMention { seq, reply: true } => {
                let m = self
                    .ui
                    .chat
                    .with_untracked(|rows| rows.iter().find(|m| m.seq == seq).cloned());
                if let Some(m) = m {
                    let quote = m.quote(self.ui.locale.get_untracked());
                    self.cmd(Cmd::SetReply(Some(quote)), now);
                    self.ui.chat_target.set(Some(seq));
                    self.focus_editor();
                }
            }
            Msg::Core(cmd) => {
                let send = matches!(cmd, Cmd::Send);
                self.cmd(cmd, now);
                if send {
                    self.ui.chat_target.set(None);
                }
            }
            Msg::ClearReply => {
                self.cmd(Cmd::SetReply(None), now);
                self.ui.chat_target.set(None);
            }
            Msg::LayoutMaterialsReady => self.ui.layout_epoch.update(|e| *e += 1),
            // Window chrome and side effects the footage does not need.
            _ => {}
        }
    }

    fn focus_editor(&mut self) {
        if let Some((i, root, editor, _)) = self.input {
            let session = &mut self.surfaces[i].session;
            session.flush().unwrap();
            session
                .document_mut()
                .context_mut()
                .focus_node(root, editor.stable_id())
                .unwrap();
        }
    }

    /// Deliver what the views asked for until they settle.
    pub fn pump(&mut self, now: u64) {
        for _ in 0..20 {
            let mut messages = Vec::new();
            for s in &mut self.surfaces {
                s.session.flush().unwrap();
                messages.extend(
                    s.session
                        .document_mut()
                        .context_mut()
                        .take_program_messages(),
                );
            }
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

    /// Click the `index`th button labelled `label` on surface `at`.
    pub fn click(&mut self, at: usize, label: &str, index: usize, now: u64) {
        let session = &mut self.surfaces[at].session;
        session.flush().unwrap();
        let nodes: Vec<_> = session
            .accessibility_dump()
            .into_iter()
            .filter(|n| n.role == "button" && n.label.as_deref() == Some(label))
            .collect();
        let target = nodes
            .get(index)
            .unwrap_or_else(|| panic!("{}: missing button {label} #{index}", self.scenario));
        assert!(session.click_node(target.id).unwrap());
        self.pump(now);
    }

    pub fn send(&mut self, now: u64) {
        let i = self.input.expect("an input window").0;
        let label = self.ui.now_t("tb-send");
        self.click(i, &label, 0, now);
    }

    pub fn type_text(&mut self, text: &str, now: u64) {
        let i = self.input.expect("an input window").0;
        if !self.focused {
            self.focus_editor();
            self.focused = true;
        }
        self.surfaces[i].session.type_text(text).unwrap();
        self.pump(now);
    }

    pub fn key(&mut self, key: &str, code: &str, now: u64) {
        let i = self.input.expect("an input window").0;
        self.surfaces[i]
            .session
            .key_press(key, code, Default::default())
            .unwrap();
        self.pump(now);
    }

    /// Type one character every `every` frames from `start`.
    pub fn typing(&mut self, f: u64, start: u64, every: u64, text: &str) {
        if f >= start
            && (f - start).is_multiple_of(every)
            && let Some(ch) = text.chars().nth(((f - start) / every) as usize)
        {
            let label = format!("{}-key", self.scenario);
            self.event(f, "key", label);
            self.type_text(&ch.to_string(), ms(f));
        }
    }

    /// Frames after the scripted `step`: clock, wakes, animation, pixels.
    pub fn run(&mut self, frames: u64, mut script: impl FnMut(&mut Self, u64)) {
        nanatype::clock::capture_at(0, || {
            for f in 0..frames {
                nanatype::clock::capture_at(ms(f), || {
                    let now = ms(f);
                    self.ui.now.set(now);
                    self.ui.peer_now.set(now);
                    self.ui.history_now.set(now);
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
                    script(self, f);
                    self.pump(now);
                    self.shoot(f, now);
                });
            }
        });
        self.write_manifests(frames);
    }

    fn shoot(&mut self, f: u64, now: u64) {
        for s in &mut self.surfaces {
            let cx = s.session.document_mut();
            cx.context_mut()
                .advance_animations(Duration::from_millis(now));
            cx.sync_presentation_clock(Duration::from_millis(now));
            s.session.flush().unwrap();
        }
        // Shaders compile off-thread: wait for them on the first frame and
        // whenever a new effect starts, without moving the capture clock.
        let warm = !self.warmed;
        self.warmed = true;
        for s in &mut self.surfaces {
            let start = std::time::Instant::now();
            loop {
                let refused = ui::shader_effects::cache_stats().refused;
                if warm || ui::shader_effects::cache_stats().pending > 0 {
                    let _ = s.session.screenshot_rgba().expect("shader warmup");
                }
                while ui::shader_effects::cache_stats().pending > 0 {
                    assert!(start.elapsed().as_secs() < 30, "shader warmup timed out");
                    std::thread::sleep(Duration::from_millis(5));
                }
                if ui::shader_effects::cache_stats().refused == refused {
                    break;
                }
            }
            if s.shoot {
                let dir = self.out.join(format!("{}-{}", self.scenario, s.name));
                std::fs::create_dir_all(&dir).unwrap();
                s.session
                    .screenshot_png(dir.join(format!("frame-{f:04}.png")))
                    .unwrap();
            }
        }
    }

    /// The NanaPVMaker capture contract, one manifest per written surface.
    fn write_manifests(&self, frames: u64) {
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
        let mut events = self.events.clone();
        events.sort_by_key(|e| e.0);
        let events: Vec<_> = events
            .iter()
            .map(|(t, kind, label)| serde_json::json!({"t_ms": t, "kind": kind, "label": label}))
            .collect();
        for s in self.surfaces.iter().filter(|s| s.shoot) {
            let name = format!("{}-{}", self.scenario, s.name);
            let dir = self.out.join(&name);
            let (w, h) = image::image_dimensions(dir.join("frame-0000.png")).unwrap();
            let manifest = serde_json::json!({
                "schema": 1,
                "name": name,
                "source": {"repo": "NanaType", "rev": rev, "dirty": dirty},
                "fps": FPS,
                "size": [w, h],
                "scale": s.scale,
                "theme": "light",
                "count": frames,
                "alpha": "straight",
                "dir": ".",
                "pattern": "frame-%04d.png",
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
