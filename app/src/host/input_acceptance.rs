//! Windowless acceptance of the production view, routing and host handoff.
use super::*;
use nana_ui::runtime::{HeadlessInput, RichEditCommand, TextArea, TextEditAction};
use nana_ui::{NanaTextShaper, runtime::LayoutBox};
use nana_ui_devtools::agent::RuntimeAgentSession;
use nana_ui_platform::{
    CompositionInput, InputModifiers, KeyInput, PointerInput, PointerPhase, WheelInput, WheelUnit,
};
use std::sync::Mutex;
use std::time::Duration;

const CTRL: InputModifiers = InputModifiers {
    control: true,
    shift: false,
    alt: false,
    meta: false,
};

// Rich clipboard payloads are process-wide, like the OS pasteboard. Fixtures
// each have a private HeadlessHostServices clipboard, so they must not race
// another fixture's copy/cut/paste sequence against that shared payload.
static EDITOR_CLIPBOARD: Mutex<()> = Mutex::new(());

struct Fixture {
    _clipboard: std::sync::MutexGuard<'static, ()>,
    ui: crate::ui::Ui,
    session: RuntimeAgentSession,
    editor: Entity<RichTextEditor>,
    source: Entity<TextArea>,
    input: HeadlessInput,
    shaper: NanaTextShaper,
    messages: Arc<Mutex<Vec<Msg>>>,
    pending: Vec<Msg>,
    edits: usize,
}

impl Fixture {
    fn new(source_mode: bool, ctrl_enter: bool, text: &str) -> Self {
        let clipboard = EDITOR_CLIPBOARD
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let skin = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"))
            .load("nanatype.halo")
            .unwrap()
            .skin;
        let ui = crate::ui::Ui::new(
            Arc::new(nanatype_settings::Settings::default()),
            skin,
            nanatype_i18n::Locale::ZhCn,
        );
        ui.source_mode.set(source_mode);
        ui.draft
            .set(Arc::new(nanatype_model::Document::plain(text)));
        ui.source.set(crate::ui::Source {
            text: text.into(),
            selection: (0, 0),
            revision: 0,
        });
        let id = DocumentId::new(1).unwrap();
        let mut doc = RuntimeDocument::new(id);
        doc.context_mut()
            .set_theme_tokens(crate::skin_theme::compile(false))
            .unwrap();
        let (_, (editor, source, _)) = doc
            .context_mut()
            .mount_view_root(id, move || input::input_window(ui))
            .unwrap();
        let messages = Arc::new(Mutex::new(Vec::new()));
        for source_mode in [false, true] {
            let observed = messages.clone();
            // Same classifier and pre-editor callback phase as Host::mount.
            let policy = move |key: &KeyInput| {
                if let Some(msg) = input_shortcut(key, source_mode, ctrl_enter) {
                    observed.lock().unwrap().push(msg);
                    true
                } else {
                    false
                }
            };
            if source_mode {
                doc.context_mut().on_key(source, policy).unwrap();
            } else {
                doc.context_mut().on_key(editor, policy).unwrap();
            }
        }
        let mut session = RuntimeAgentSession::new(doc, 796, 480).unwrap();
        let input = HeadlessInput::bind(session.document_mut().context_mut(), id);
        Self {
            _clipboard: clipboard,
            ui,
            session,
            editor,
            source,
            input,
            shaper: NanaTextShaper::default(),
            messages,
            pending: Vec::new(),
            edits: 0,
        }
    }
    fn id(&self) -> DocumentId {
        self.session.document().document()
    }
    fn node(&self) -> nana_ui::runtime::StableNodeId {
        if self.ui.source_mode.get_untracked() {
            self.source.stable_id()
        } else {
            self.editor.stable_id()
        }
    }
    fn bounds(&self) -> LayoutBox {
        self.session
            .document()
            .context()
            .world()
            .layout_box(self.node())
            .unwrap()
    }
    fn offset(&self) -> f32 {
        self.session
            .document()
            .context()
            .world()
            .scroll_offset(self.node())
            .unwrap()
            .y
    }
    fn visible_bounds(&self) -> LayoutBox {
        let b = self.bounds();
        let world = self.session.document().context().world();
        let (x, y) = world
            .layout_pointer_position(self.node(), b.x, b.y)
            .unwrap();
        let (right, bottom) = world
            .layout_pointer_position(self.node(), b.x + b.width, b.y + b.height)
            .unwrap();
        LayoutBox {
            x,
            y,
            width: right - x,
            height: bottom - y,
        }
    }
    fn selection(&self) -> (usize, usize) {
        let cx = self.session.document().context();
        if self.ui.source_mode.get_untracked() {
            let sel = cx.world().text_input(self.node()).unwrap().selection;
            (sel.anchor, sel.focus)
        } else {
            cx.read(self.editor, |e| e.selection()).unwrap()
        }
    }
    fn text(&self) -> String {
        let cx = self.session.document().context();
        if self.ui.source_mode.get_untracked() {
            cx.read(self.source, |e| e.state.value.to_string()).unwrap()
        } else {
            cx.read(self.editor, |e| e.value.text().to_owned()).unwrap()
        }
    }
    fn event(&mut self, payload: InputPayload) {
        self.input.advance(Duration::from_millis(16));
        self.input
            .route_shaped(
                self.session.document_mut().context_mut(),
                payload,
                Some(&mut self.shaper),
            )
            .unwrap();
        if let Some(width) =
            input::changed_sidebar_width(self.ui, self.session.document().context())
        {
            self.ui.input_sidebar_width.set(width);
        }
        self.pump();
        self.session.flush().unwrap();
    }
    fn pump(&mut self) {
        for message in self
            .session
            .document_mut()
            .context_mut()
            .take_program_messages()
        {
            if let Ok(message) = message.downcast::<Msg>() {
                match *message {
                    Msg::EditorChanged(rich) => {
                        crate::app::sync_rich_draft(self.ui, rich);
                        self.edits += 1;
                    }
                    Msg::Markup(edit) => {
                        let mut src = self.ui.source.get_untracked();
                        crate::app::apply_source_markup(&mut src, edit);
                        self.ui
                            .draft
                            .set(Arc::new(nanatype_markup::parse(&src.text).doc));
                        self.ui.source.set(src);
                        self.edits += 1;
                    }
                    message => self.pending.push(message),
                }
            }
        }
    }
    fn key(&mut self, name: &'static str, modifiers: InputModifiers) {
        self.event(InputPayload::Key(KeyInput::named(
            "Unidentified",
            name,
            KeyState::Pressed,
            modifiers,
        )));
    }
    fn pointer(&mut self, phase: PointerPhase, x: f32, y: f32) {
        self.event(InputPayload::Pointer(PointerInput::mouse(phase, x, y)));
    }
    fn click(&mut self, x: f32, y: f32) {
        self.pointer(PointerPhase::Down, x, y);
        self.pointer(PointerPhase::Up, x, y);
    }
    fn wheel(&mut self, delta: f32) {
        let b = self.visible_bounds();
        self.event(InputPayload::Wheel(WheelInput {
            pointer_id: nana_ui_platform::PointerId(0),
            x: b.x + b.width / 2.0,
            y: b.y + b.height / 2.0,
            delta_x: 0.0,
            delta_y: delta,
            unit: WheelUnit::Pixels,
            modifiers: Default::default(),
        }));
    }
    fn sync(&mut self, focus: bool, source_undoable: bool, commands: Vec<RichEditCommand>) {
        sync_editor(
            self.session.document_mut(),
            self.ui,
            self.editor,
            Some(self.source),
            commands.into_iter().map(Into::into).collect(),
            focus,
            source_undoable,
        );
        self.pump();
        self.session.flush().unwrap();
    }
    fn focus(&mut self) {
        self.sync(true, false, Vec::new());
    }
    fn insert_phrase(&mut self, markup: &str) {
        let look = crate::ui::dialogue::Base::editor(&self.ui.skin.get_untracked()).look();
        let fragment = crate::ui::rich::to_rich(
            &nanatype_markup::parse(markup).doc,
            &crate::ui::rich::Env {
                look: &look,
                urls: &self.ui.asset_urls.get_untracked(),
                editor: true,
                locale: self.ui.locale.get_untracked(),
            },
        );
        sync_editor(
            self.session.document_mut(),
            self.ui,
            self.editor,
            Some(self.source),
            vec![crate::app::EditorCommand::Insert(fragment)],
            true,
            false,
        );
        self.pump();
        self.session.flush().unwrap();
    }
    fn action(&mut self, action: TextEditAction) {
        let id = self.id();
        let node = self.node();
        assert!(
            self.session
                .document_mut()
                .context_mut()
                .edit_text_action(id, node, action, self.input.services_mut())
                .unwrap(),
            "{action:?}"
        );
        self.pump();
        self.session.flush().unwrap();
    }
}

#[test]
fn rich_phrases_insert_at_the_caret_or_selection_as_one_undo_step() {
    let phrase = "[b]加粗[/b][ruby=かな]字[/ruby][pause][sticker=cats/hello]";
    for selection in [3..3, 3..6] {
        let mut f = Fixture::new(false, false, "甲乙丙");
        f.focus();
        f.sync(
            true,
            false,
            vec![RichEditCommand::Select(selection.clone())],
        );
        let edits = f.edits;
        f.insert_phrase(phrase);
        let suffix = if selection.is_empty() {
            "乙丙"
        } else {
            "丙"
        };
        let expected = nanatype_markup::parse(&format!("甲{phrase}{suffix}")).doc;
        assert_eq!(*f.ui.draft.get_untracked(), expected);
        assert_eq!(
            f.edits,
            edits + 1,
            "no unstyled intermediate reaches the core"
        );
        let caret = f.selection();
        assert_eq!(caret.0, caret.1);
        assert!(caret.0 < f.text().len(), "caret stays before the suffix");
        f.action(TextEditAction::Undo);
        assert_eq!(f.text(), "甲乙丙");
        assert_eq!(f.selection(), (selection.start, selection.end));
        f.action(TextEditAction::Redo);
        assert_eq!(*f.ui.draft.get_untracked(), expected);
        assert_eq!(f.selection(), caret);
    }
}

#[test]
fn same_text_phrases_replace_formatting_and_rich_edits_keep_line_headers() {
    let mut f = Fixture::new(false, false, "word");
    f.focus();
    let mut source = f.ui.source.get_untracked();
    source.text = "[@auto=200 after=clear delay=100]word".into();
    f.ui.source.set(source);
    f.sync(true, false, vec![RichEditCommand::SelectAll]);
    f.insert_phrase("[b]word[/b]");
    let parsed = nanatype_markup::parse(&f.ui.source.get_untracked().text);
    assert_eq!(parsed.doc, nanatype_markup::parse("[b]word[/b]").doc);
    assert_eq!(parsed.meta.auto_advance_ms, Some(200));
    assert_eq!(
        parsed.meta.after,
        Some(nanatype_model::AfterLine::Clear { delay_ms: 100 })
    );
    f.action(TextEditAction::Undo);
    assert_eq!(
        *f.ui.draft.get_untracked(),
        nanatype_model::Document::plain("word")
    );
    f.action(TextEditAction::Redo);
    assert_eq!(*f.ui.draft.get_untracked(), parsed.doc);
}

#[test]
fn recheck_phrase_then_typing_undo_redo_and_source_switch_keep_the_document() {
    let mut f = Fixture::new(false, false, "甲👩‍💻乙");
    f.focus();
    f.sync(true, false, vec![RichEditCommand::Select(3..3)]);
    f.insert_phrase("[b]你好[/b]\n[ruby=かな]字[/ruby][pause][sticker=cats/hello]");
    let inserted = f.ui.draft.get_untracked();
    f.event(InputPayload::Text(nana_ui_platform::CommittedText::new(
        "继续",
    )));
    let typed = f.ui.draft.get_untracked();
    assert_ne!(typed, inserted);
    f.action(TextEditAction::Undo);
    assert_eq!(f.ui.draft.get_untracked(), inserted);
    f.action(TextEditAction::Undo);
    assert_eq!(f.text(), "甲👩‍💻乙");
    f.action(TextEditAction::Redo);
    assert_eq!(f.ui.draft.get_untracked(), inserted);
    f.action(TextEditAction::Redo);
    assert_eq!(f.ui.draft.get_untracked(), typed);
    let caret = f.selection();
    f.ui.source_mode.set(true);
    f.focus();
    assert_eq!(nanatype_markup::parse(&f.text()).doc, *typed);
    f.ui.source_mode.set(false);
    f.focus();
    assert_eq!(f.ui.draft.get_untracked(), typed);
    assert_eq!(f.selection(), caret);
    assert!(!f.ui.editor_transaction.get_untracked());
}

#[test]
fn recheck_repeated_phrases_replace_selected_objects_without_duplicate_edits() {
    let mut f = Fixture::new(false, false, "左右");
    f.focus();
    f.sync(true, false, vec![RichEditCommand::Select(3..3)]);
    f.insert_phrase("[sticker=cats/hello][pause]");
    let first = f.ui.draft.get_untracked();
    f.insert_phrase("[sticker=cats/hello][pause]");
    let second = f.ui.draft.get_untracked();
    let objects = f
        .session
        .document()
        .context()
        .read(f.editor, |e| e.value.objects().len())
        .unwrap();
    assert_eq!(objects, 4);
    f.action(TextEditAction::Undo);
    assert_eq!(f.ui.draft.get_untracked(), first);
    f.action(TextEditAction::Redo);
    assert_eq!(f.ui.draft.get_untracked(), second);
    f.sync(
        true,
        false,
        vec![RichEditCommand::Select(3..3 + 2 * '\u{fffc}'.len_utf8())],
    );
    let edits = f.edits;
    f.insert_phrase("[i]替换[/i]");
    assert_eq!(f.edits, edits + 1);
    assert_eq!(
        *f.ui.draft.get_untracked(),
        nanatype_markup::parse("左[i]替换[/i][sticker=cats/hello][pause]右").doc
    );
    f.action(TextEditAction::Undo);
    assert_eq!(f.ui.draft.get_untracked(), second);
}

#[test]
fn empty_draft_hint_never_overlaps_ime_preedit() {
    let mut f = Fixture::new(false, false, "");
    let hint_visible = |f: &Fixture| {
        f.session.accessibility_dump().iter().any(|n| {
            n.agent_path
                .as_deref()
                .is_some_and(|p| p.ends_with("/input-empty-hint"))
        })
    };
    assert!(hint_visible(&f));
    f.focus();
    f.event(InputPayload::Composition(CompositionInput::Start));
    f.event(InputPayload::Composition(CompositionInput::Update {
        text: "nihao".into(),
        selection: None,
    }));
    assert!(
        f.ui.draft.get_untracked().is_blank(),
        "preedit is not yet committed"
    );
    assert!(f.session.document().context().editor_composing(f.id()));
    assert!(!hint_visible(&f), "the hint must not paint beneath preedit");
    f.event(InputPayload::Composition(CompositionInput::Commit(
        "你好".into(),
    )));
    assert_eq!(f.text(), "你好");
    assert!(!hint_visible(&f));
    f.action(TextEditAction::Undo);
    assert_eq!(f.text(), "");
    assert!(
        !hint_visible(&f),
        "typing focus still owns the empty editor"
    );
}

#[test]
fn real_input_routes_send_settings_and_ime_before_application_shortcuts() {
    for source in [false, true] {
        for ctrl_enter in [false, true] {
            let mut f = Fixture::new(source, ctrl_enter, "abc\n中文👩‍💻");
            f.focus();
            f.key("End", CTRL);
            let before = f.text();
            let selection = f.selection();
            for composition in [
                CompositionInput::Start,
                CompositionInput::Update {
                    text: "nihao".into(),
                    selection: None,
                },
            ] {
                f.event(InputPayload::Composition(composition));
            }
            assert!(f.session.document().context().editor_composing(f.id()));
            for name in [
                "Enter",
                "Escape",
                "ArrowUp",
                "ArrowDown",
                "ArrowLeft",
                "ArrowRight",
            ] {
                for modifiers in [InputModifiers::default(), CTRL] {
                    f.key(name, modifiers);
                }
            }
            assert!(
                f.messages.lock().unwrap().is_empty(),
                "IME key leaked to application"
            );
            assert_eq!(f.text(), before);
            assert_eq!(f.selection(), selection);
            f.event(InputPayload::Composition(CompositionInput::Commit(
                "你好".into(),
            )));
            assert_eq!(f.text(), format!("{before}你好"));
            assert!(!f.session.document().context().editor_composing(f.id()));
            f.key("Enter", if ctrl_enter { CTRL } else { Default::default() });
            assert!(matches!(
                f.messages.lock().unwrap().pop(),
                Some(Msg::Core(Cmd::Send))
            ));
            let sent = f.text();
            f.key(
                "Enter",
                InputModifiers {
                    shift: true,
                    ..Default::default()
                },
            );
            assert_eq!(f.text(), format!("{sent}\n"));
            for name in ["ArrowUp", "ArrowDown"] {
                f.key(
                    name,
                    InputModifiers {
                        shift: true,
                        ..CTRL
                    },
                );
                assert!(
                    f.messages.lock().unwrap().is_empty(),
                    "selection recalled history"
                );
            }
            f.key("ArrowUp", CTRL);
            assert!(matches!(
                f.messages.lock().unwrap().pop(),
                Some(Msg::Core(Cmd::Recall(-1)))
            ));
            f.key("b", CTRL);
            assert!(matches!(
                f.messages.lock().unwrap().pop(),
                Some(Msg::Tool(Tool::Bold))
            ));
            f.key("Escape", Default::default());
            assert!(matches!(
                f.messages.lock().unwrap().pop(),
                Some(Msg::HideInput)
            ));
        }
    }
}

#[test]
fn unfocused_wheel_boundaries_hit_testing_caret_follow_and_edge_drag_use_one_viewport() {
    for source in [false, true] {
        let original = "hello 中文 👩‍💻 e\u{301}\n".repeat(100);
        let mut f = Fixture::new(source, false, &original);
        assert_eq!(f.session.document().context().world().focused(f.id()), None);
        let selection = f.selection();
        f.wheel(-80.0);
        assert!(f.offset() > 0.0, "hover without focus must scroll");
        assert_eq!(f.selection(), selection);
        f.wheel(-1_000_000.0);
        let bottom = f.offset();
        f.wheel(-1_000_000.0);
        assert_eq!(f.offset(), bottom);
        f.wheel(1_000_000.0);
        assert_eq!(f.offset(), 0.0);
        f.wheel(-200.0);
        let b = f.visible_bounds();
        f.click(b.x + 15.0, b.y + 20.0);
        assert!(f.selection().1 > 0, "click must account for scroll");
        f.key("End", CTRL);
        let bottom = f.offset();
        f.wheel(80.0);
        let read = f.offset();
        assert!(read < bottom);
        f.session.flush().unwrap();
        assert_eq!(f.offset(), read);
        f.key("ArrowLeft", Default::default());
        assert!(f.offset() > read, "navigation must resume caret following");
        f.event(InputPayload::Composition(CompositionInput::Start));
        f.event(InputPayload::Composition(CompositionInput::Update {
            text: "ni".into(),
            selection: None,
        }));
        let caret = f
            .input
            .services()
            .text_input()
            .and_then(|c| c.cursor_area)
            .expect("scrolled IME caret");
        assert!(
            caret.y >= b.y - 1.0 && caret.y <= b.y + b.height + 1.0,
            "source={source}: caret={caret:?}, bounds={b:?}"
        );
        f.event(InputPayload::Composition(CompositionInput::End));
        f.key("Home", CTRL);
        f.key(
            "PageDown",
            InputModifiers {
                shift: true,
                ..Default::default()
            },
        );
        let (anchor, focus) = f.selection();
        assert_eq!(anchor, 0);
        assert!(focus > 0);
        f.key(
            "PageUp",
            InputModifiers {
                shift: true,
                ..Default::default()
            },
        );
        assert_eq!(f.selection().0, 0);
        f.key("Home", CTRL);
        f.pointer(PointerPhase::Down, b.x + 15.0, b.y + 20.0);
        f.pointer(PointerPhase::Move, b.x + 20.0, b.y + b.height + 80.0);
        let scroll = f.offset();
        for _ in 0..5 {
            f.input.advance(Duration::from_millis(20));
            f.session
                .document_mut()
                .context_mut()
                .advance_animations(f.input.now());
            f.session.flush().unwrap();
        }
        assert!(
            f.offset() > scroll,
            "stationary drag must continue scrolling: source={source}, before={scroll}, after={}, selection={:?}",
            f.offset(),
            f.selection()
        );
        f.pointer(PointerPhase::Up, b.x + 20.0, b.y + b.height + 80.0);
        assert_eq!(f.text(), original);
        assert_eq!(f.edits, 0);
        assert!(
            f.messages.lock().unwrap().is_empty(),
            "scroll/navigation sent or recalled draft"
        );
        let messages = f
            .session
            .document_mut()
            .context_mut()
            .take_program_messages();
        assert!(messages.iter().all(|m| !matches!(
            m.downcast_ref::<Msg>(),
            Some(Msg::EditorChanged(_) | Msg::Markup(_) | Msg::Core(Cmd::Send))
        )));
    }
}

#[test]
fn host_handoff_preserves_selection_scroll_and_undo_through_tools_and_refresh() {
    let mut f = Fixture::new(true, false, "中文👩‍💻\n".repeat(80).as_str());
    f.focus();
    f.key("End", CTRL);
    f.wheel(100.0);
    let source_scroll = f.offset();
    let source_selection = f.selection();
    f.ui.source_mode.set(false);
    f.focus();
    assert_eq!(
        f.session.document().context().world().focused(f.id()),
        Some(f.editor.stable_id())
    );
    f.key("End", CTRL);
    f.wheel(100.0);
    let rich_scroll = f.offset();
    let rich_selection = f.selection();
    f.ui.source_mode.set(true);
    f.focus();
    assert_eq!(f.offset(), source_scroll);
    assert_eq!(f.selection(), source_selection);
    let before = f.text();
    input::capture_source_selection(f.ui, f.session.document().context(), f.source);
    let mut src = f.ui.source.get_untracked();
    crate::app::apply_source_markup(
        &mut src,
        crate::msg::MarkupEdit::Wrap {
            open: "[b]".into(),
            close: "[/b]".into(),
        },
    );
    f.ui.source.set(src);
    // This is the exact Host handoff order after an application structural edit.
    f.sync(true, true, Vec::new());
    assert!(f.text().contains("[b]"));
    f.action(TextEditAction::Undo);
    assert_eq!(f.text(), before);
    f.action(TextEditAction::Redo);
    assert!(f.text().contains("[b]"));
    f.ui.source_mode.set(false);
    f.focus();
    assert_eq!(f.offset(), rich_scroll);
    assert_eq!(f.selection(), rich_selection);
    f.key("a", CTRL);
    f.sync(
        true,
        false,
        vec![RichEditCommand::SetAttrs(
            nana_ui_core::RichSpanStyle::new().weight(700),
        )],
    );
    assert_eq!(
        f.session.document().context().world().focused(f.id()),
        Some(f.editor.stable_id())
    );
    f.action(TextEditAction::Copy);
    f.action(TextEditAction::Cut);
    assert!(f.text().is_empty());
    f.action(TextEditAction::Paste);
    assert_eq!(f.text(), before);
    assert!(
        f.session
            .document()
            .context()
            .read(f.editor, |e| e
                .value
                .spans()
                .iter()
                .any(|(_, s)| s.shape.weight == Some(700)))
            .unwrap()
    );
    let selection = f.selection();
    let offset = f.offset();
    f.ui.asset_urls.update(|urls| {
        Arc::make_mut(urls).insert("unused".into(), "unused.png".into());
    });
    f.ui.chrome_dark.set(true);
    f.ui.skin
        .set(Arc::new((*f.ui.skin.get_untracked()).clone()));
    f.session
        .document_mut()
        .context_mut()
        .set_theme_tokens(crate::skin_theme::compile(true))
        .unwrap();
    f.session.flush().unwrap();
    assert_eq!(f.selection(), selection);
    assert_eq!(f.offset(), offset);
    f.action(TextEditAction::Undo);
    assert!(f.text().is_empty());
    f.action(TextEditAction::Undo);
    assert_eq!(f.text(), before);
}

#[test]
fn sidebar_drag_and_small_windows_preserve_the_saved_width_and_editor() {
    use nana_ui::runtime::SplitPane;
    use nana_ui_devtools::agent::AgentSession;
    for source in [false, true] {
        let mut f = Fixture::new(source, false, "侧栏调整");
        f.ui.settings
            .update(|s| Arc::make_mut(s).general.input_skin_preview = true);
        f.session.set_viewport(1200, 640, 1.0).unwrap();
        f.focus();
        f.key("End", CTRL);
        f.event(InputPayload::Text(nana_ui_platform::CommittedText::new(
            "后",
        )));
        let selection = f.selection();
        let size = (f.bounds().width, f.bounds().height);
        let pane = Entity::<SplitPane>::from_stable_id(f.ui.input_split.get_untracked().unwrap());
        let drag = |f: &mut Fixture, dx: f32| {
            let handle = f
                .session
                .document()
                .context()
                .read(pane, |p| p.handle.unwrap())
                .unwrap();
            let b = f
                .session
                .document()
                .context()
                .world()
                .layout_box(handle)
                .unwrap();
            let (x, y) = (b.x + b.width / 2.0, b.y + b.height / 2.0);
            f.pointer(PointerPhase::Down, x, y);
            for i in 1..=8 {
                f.pointer(PointerPhase::Move, x + dx * i as f32 / 8.0, y);
            }
            f.pointer(PointerPhase::Up, x + dx, y);
        };
        drag(&mut f, -180.0);
        assert_eq!(f.ui.input_sidebar_width.get_untracked(), 480.0);
        let stored = nanatype_settings::GeometryStore {
            input_sidebar_width: Some(f.ui.input_sidebar_width.get_untracked()),
            ..Default::default()
        };
        let restored: nanatype_settings::GeometryStore =
            serde_json::from_str(&serde_json::to_string(&stored).unwrap()).unwrap();
        assert_eq!(
            nanatype_settings::input_sidebar_width(restored.input_sidebar_width),
            480.0
        );
        for (w, expected) in [(720, 392.0), (920, 480.0), (1200, 480.0)] {
            f.session.set_viewport(w, 480, 1.0).unwrap();
            f.session.flush().unwrap();
            assert_eq!(
                f.session
                    .document()
                    .context()
                    .read(pane, |p| p.model.size())
                    .unwrap(),
                expected
            );
            assert!(input::changed_sidebar_width(f.ui, f.session.document().context()).is_none());
            assert_eq!(f.ui.input_sidebar_width.get_untracked(), 480.0);
            assert_eq!(f.selection(), selection);
            assert_eq!((f.bounds().width, f.bounds().height), size);
        }
        drag(&mut f, -2000.0);
        assert_eq!(f.ui.input_sidebar_width.get_untracked(), 560.0);
        drag(&mut f, 2000.0);
        assert_eq!(
            f.ui.input_sidebar_width.get_untracked(),
            nanatype_settings::INPUT_SIDEBAR_MIN
        );
        f.key("ArrowLeft", Default::default());
        assert_eq!(
            f.ui.input_sidebar_width.get_untracked(),
            nanatype_settings::INPUT_SIDEBAR_MIN + 8.0
        );
        f.action(TextEditAction::Undo);
        assert_eq!(f.text(), "侧栏调整");
    }
}

#[test]
fn fitted_editor_pointer_and_ime_follow_the_preview_at_the_minimum_width() {
    use nana_ui_devtools::agent::AgentSession;
    for source in [false, true] {
        for scale in [1.0, 1.5, 2.0] {
            let mut f = Fixture::new(source, false, "hello world");
            f.ui.settings
                .update(|s| Arc::make_mut(s).general.input_skin_preview = true);
            f.ui.input_sidebar_width.set(560.0);
            f.session.set_viewport(720, 360, scale).unwrap();
            for _ in 0..4 {
                f.session.flush().unwrap();
            }
            let b = f.visible_bounds();
            assert!(
                b.x >= 16.0 && b.x + b.width <= 304.5,
                "editor must fit before the widest allowed sidebar: {b:?}"
            );
            f.click(b.x + b.width - 2.0, b.y + 4.0);
            assert_eq!(
                f.session.document().context().world().focused(f.id()),
                Some(f.node())
            );
            assert_eq!(
                f.selection(),
                (11, 11),
                "far-right click reaches end of first line"
            );
            f.key("Home", CTRL);
            f.event(InputPayload::Composition(CompositionInput::Start));
            f.event(InputPayload::Composition(CompositionInput::Update {
                text: "nihao".into(),
                selection: None,
            }));
            let caret = f
                .input
                .services()
                .text_input()
                .and_then(|c| c.cursor_area)
                .unwrap();
            assert!(
                caret.x >= b.x - 1.0
                    && caret.x <= b.x + b.width + 1.0
                    && caret.y >= b.y - 1.0
                    && caret.y + caret.height <= b.y + b.height + 1.0,
                "IME follows the scaled preview: source={source}, scale={scale}, {caret:?}, {b:?}"
            );
            f.event(InputPayload::Composition(CompositionInput::Commit(
                "你好".into(),
            )));
            assert_eq!(f.text(), "你好hello world");
            f.action(TextEditAction::Undo);
            assert_eq!(f.text(), "hello world");
        }
    }
}

#[test]
fn resizing_and_dock_tabs_preserve_both_editors_selection_and_undo_history() {
    use nana_ui_devtools::agent::AgentSession;
    for source in [false, true] {
        let mut f = Fixture::new(source, false, "窗口调整前");
        f.ui.settings
            .update(|s| Arc::make_mut(s).general.input_skin_preview = true);
        f.focus();
        f.key("End", CTRL);
        f.event(InputPayload::Text(nana_ui_platform::CommittedText::new(
            "，草稿仍在",
        )));
        let text = f.text();
        let selection = f.selection();
        let editor = f.node();
        let size = (f.bounds().width, f.bounds().height);
        for (width, height) in [(720, 360), (1280, 720), (920, 480)] {
            f.session.set_viewport(width, height, 1.0).unwrap();
            f.ui.peer_panel.set(width == 720);
            f.session.flush().unwrap();
            assert_eq!(f.node(), editor);
            assert_eq!(f.text(), text);
            assert_eq!(f.selection(), selection);
            for tab in [
                "input-dock-commands",
                "input-dock-stickers",
                "input-dock-stickers",
            ] {
                let strip = f
                    .session
                    .accessibility_dump()
                    .into_iter()
                    .find(|n| {
                        n.agent_path
                            .as_deref()
                            .is_some_and(|p| p.ends_with("input-dock-tabs"))
                    })
                    .unwrap();
                let strip = Entity::<nana_ui::runtime::Tabs>::from_stable_id(
                    nana_ui::runtime::StableNodeId::new(strip.id).unwrap(),
                );
                let value = if tab == "input-dock-commands" {
                    "commands"
                } else {
                    "stickers"
                };
                let trigger = f
                    .session
                    .document()
                    .context()
                    .read(strip, |t| t.option_node(value).unwrap())
                    .unwrap();
                let b = f
                    .session
                    .document()
                    .context()
                    .world()
                    .layout_box(trigger)
                    .unwrap();
                f.click(b.x + b.width / 2.0, b.y + b.height / 2.0);
                assert_eq!(f.node(), editor);
                assert_eq!(f.text(), text);
                assert_eq!(f.selection(), selection);
                assert_eq!((f.bounds().width, f.bounds().height), size);
            }
        }
        f.action(TextEditAction::Undo);
        assert_eq!(f.text(), "窗口调整前");
        f.action(TextEditAction::Redo);
        assert_eq!(f.text(), text);
    }
}

#[test]
fn secondary_press_mounts_the_production_menu_and_runs_its_selected_edit() {
    for source in [false, true] {
        let mut f = Fixture::new(source, false, "右键中文 👩‍💻");
        f.focus();
        f.key("a", CTRL);
        let selection = f.selection();
        let b = f.visible_bounds();
        let mut press = PointerInput::mouse(PointerPhase::Down, b.x + 20.0, b.y + 20.0);
        press.button = 2;
        press.buttons = 2;
        f.event(InputPayload::Pointer(press));
        assert_eq!(f.selection(), selection);
        let (x, y, keyboard) = f
            .pending
            .drain(..)
            .find_map(|m| match m {
                Msg::EditorContext {
                    source: actual,
                    x,
                    y,
                    keyboard,
                } => {
                    assert_eq!(actual, source);
                    Some((x, y, keyboard))
                }
                _ => None,
            })
            .expect("actual secondary press must dispatch menu request");
        assert!(!keyboard);
        assert!(x > b.x && y > b.y);
        let node = f.node();
        let overlay = mount_editor_menu(
            f.session.document_mut(),
            f.ui,
            node,
            source,
            (790.0, 350.0, true),
            false,
        )
        .unwrap();
        f.session.flush().unwrap();
        let menu_node = f
            .session
            .document()
            .context()
            .world()
            .node(overlay.stable_id())
            .unwrap()
            .children[0];
        let menu = Entity::<nana_ui::runtime::ContextMenu>::from_stable_id(menu_node);
        let disabled = f
            .session
            .document()
            .context()
            .read(menu, |m| {
                m.items.iter().map(|i| i.disabled).collect::<Vec<_>>()
            })
            .unwrap();
        assert_eq!(disabled, vec![true, true, false, false, true, false]);
        let bounds = f
            .session
            .document()
            .context()
            .world()
            .layout_box(menu_node)
            .unwrap();
        assert!(
            bounds.x >= 0.0
                && bounds.y >= 0.0
                && bounds.x + bounds.width <= 796.5
                && bounds.y + bounds.height <= 480.5
        );
        f.key("Enter", Default::default());
        assert!(
            matches!(f.pending.pop(),Some(Msg::EditorAction { source: actual, action:TextEditAction::Cut }) if actual==source)
        );
        assert!(
            f.messages.lock().unwrap().is_empty(),
            "menu Enter must not send draft"
        );
        f.session
            .document_mut()
            .context_mut()
            .remove_view(overlay)
            .unwrap();
        f.action(TextEditAction::Cut);
        assert_eq!(f.input.services().clipboard(), Some("右键中文 👩‍💻"));
        assert!(f.text().is_empty());
        assert_eq!(
            f.session.document().context().world().focused(f.id()),
            Some(node)
        );
        f.action(TextEditAction::Undo);
        assert_eq!(f.text(), "右键中文 👩‍💻");
        f.action(TextEditAction::Redo);
        assert!(f.text().is_empty());
        f.action(TextEditAction::Paste);
        assert_eq!(f.text(), "右键中文 👩‍💻");
        f.key("Home", CTRL);
        let old = f.selection();
        // Right-click the far edge of the fitted editor, beside the sidebar.
        let preview = f
            .session
            .accessibility_dump()
            .into_iter()
            .find(|n| {
                n.agent_path
                    .as_deref()
                    .is_some_and(|p| p.ends_with("/input-preview-scroll"))
            })
            .unwrap();
        let x = (b.x + b.width - 20.0).min(preview.bounds.x + preview.bounds.width - 20.0);
        let mut press = PointerInput::mouse(PointerPhase::Down, x, b.y + 20.0);
        press.button = 2;
        press.buttons = 2;
        f.event(InputPayload::Pointer(press));
        assert_ne!(
            f.selection(),
            old,
            "without a selection, secondary press must position caret"
        );
    }
}

#[test]
fn unicode_navigation_deletion_multiclick_and_ime_geometry_follow_the_visible_caret() {
    for source in [false, true] {
        let mut f = Fixture::new(source, false, "甲👨‍👩‍👧‍👦e\u{301}乙\nabc def");
        f.focus();
        f.key("Home", CTRL);
        f.key("End", Default::default());
        assert_eq!(f.selection().1, "甲👨‍👩‍👧‍👦e\u{301}乙".len());
        f.key("ArrowLeft", Default::default());
        f.key("Backspace", Default::default());
        f.key("Backspace", Default::default());
        assert_eq!(f.text(), "甲乙\nabc def");
        f.key("End", CTRL);
        f.key("ArrowLeft", CTRL);
        assert_eq!(f.selection().1, "甲乙\nabc ".len());
        f.key("End", CTRL);
        f.key("Backspace", CTRL);
        assert_eq!(f.text(), "甲乙\nabc ");
        f.action(TextEditAction::Undo);
        assert_eq!(f.text(), "甲乙\nabc def");
        f.key("a", CTRL);
        f.event(InputPayload::Text(nana_ui_platform::CommittedText::new(
            "hello world\n中文👩‍💻",
        )));
        f.key("Home", CTRL);
        // With the persistent sidebar, the canvas has an outer horizontal
        // scrollbar. Reveal the start before selecting its first word.
        let preview = f
            .session
            .accessibility_dump()
            .into_iter()
            .find(|n| {
                n.agent_path
                    .as_deref()
                    .is_some_and(|p| p.ends_with("input-preview-scroll"))
            })
            .unwrap();
        f.event(InputPayload::Wheel(WheelInput {
            pointer_id: nana_ui_platform::PointerId(0),
            x: 2.0,
            y: preview.bounds.y + 4.0,
            delta_x: 2000.0,
            delta_y: 2000.0,
            unit: WheelUnit::Pixels,
            modifiers: Default::default(),
        }));
        let b = f.visible_bounds();
        f.click(b.x + 15.0, b.y + 10.0);
        f.click(b.x + 15.0, b.y + 10.0);
        assert_eq!(
            f.selection(),
            (0, 5),
            "double click selects word: source={source}"
        );
        f.click(b.x + 15.0, b.y + 10.0);
        let (start, end) = f.selection();
        assert_eq!(start, 0);
        assert_eq!(
            &f.text()[start..end].trim_end_matches('\n'),
            &"hello world",
            "triple click selects paragraph: source={source}"
        );
        f.key("End", CTRL);
        // HostServices receives the same scrolled logical caret used to paint.
        let caret = f
            .input
            .services()
            .text_input()
            .and_then(|c| c.cursor_area)
            .expect("IME caret geometry");
        assert!(
            caret.x >= b.x - 1.0
                && caret.y >= b.y - 1.0
                && caret.x <= b.x + b.width + 1.0
                && caret.y <= b.y + b.height + 1.0,
            "source={source} caret={caret:?} bounds={b:?}"
        );
    }
}

#[cfg(windows)]
#[test]
#[ignore = "explicit acceptance only: temporarily uses the system clipboard and restores it"]
fn windows_system_clipboard_round_trip_and_image_fallback() {
    use nana_ui_platform::HostServices;
    use windows::Win32::System::{Com::*, Ole::*};
    use windows::core::{GUID, Interface};
    // Materialize formats before touching the clipboard: OleGetClipboard can
    // return a proxy onto its live contents. Never log the saved data.
    struct ClipboardRestore(Option<IDataObject>);
    impl Drop for ClipboardRestore {
        fn drop(&mut self) {
            unsafe {
                OleSetClipboard(self.0.as_ref()).expect("restore original clipboard");
                OleFlushClipboard().expect("persist restored clipboard");
                OleUninitialize();
            }
        }
    }
    unsafe {
        OleInitialize(None).unwrap();
    }
    let original = unsafe { OleGetClipboard().unwrap() };
    let snapshot: IDataObject = unsafe { CreateDataCache(None, &GUID::zeroed()).unwrap() };
    let cache: IOleCache = snapshot.cast().unwrap();
    let formats = unsafe { original.EnumFormatEtc(DATADIR_GET.0 as u32).unwrap() };
    loop {
        let mut format = [FORMATETC::default()];
        let mut fetched = 0;
        unsafe {
            formats.Next(&mut format, Some(&mut fetched)).ok().unwrap();
        }
        if fetched == 0 {
            break;
        }
        let medium = unsafe { original.GetData(&format[0]).unwrap() };
        unsafe {
            cache.Cache(&format[0], 0).unwrap();
            cache.SetData(&format[0], &medium, true).unwrap();
        }
    }
    let _restore = ClipboardRestore(Some(snapshot));
    let mut services = EditorClipboard;
    services.write_clipboard("NanaType 离屏验收 👩‍💻").unwrap();
    assert_eq!(
        services.read_clipboard().unwrap().as_deref(),
        Some("NanaType 离屏验收 👩‍💻")
    );
    for source in [false, true] {
        let mut f = Fixture::new(source, false, "");
        f.focus();
        let id = f.id();
        let node = f.node();
        assert!(
            f.session
                .document_mut()
                .context_mut()
                .edit_text_action(id, node, TextEditAction::Paste, &mut services)
                .unwrap()
        );
        f.pump();
        f.session.flush().unwrap();
        assert_eq!(f.text(), "NanaType 离屏验收 👩‍💻");
        f.key("a", CTRL);
        assert!(
            f.session
                .document_mut()
                .context_mut()
                .edit_text_action(id, node, TextEditAction::Cut, &mut services)
                .unwrap()
        );
        f.pump();
        f.session.flush().unwrap();
        assert!(f.text().is_empty());
        assert_eq!(
            services.read_clipboard().unwrap().as_deref(),
            Some("NanaType 离屏验收 👩‍💻")
        );
        f.action(TextEditAction::Undo);
        assert_eq!(f.text(), "NanaType 离屏验收 👩‍💻");
    }
    arboard::Clipboard::new()
        .unwrap()
        .set_image(arboard::ImageData {
            width: 2,
            height: 1,
            bytes: std::borrow::Cow::Owned(vec![255, 0, 0, 255, 0, 255, 0, 255]),
        })
        .unwrap();
    assert!(
        matches!(clipboard_image(),Some(Msg::PasteImage { width:2,height:1,rgba }) if rgba==[255,0,0,255,0,255,0,255])
    );
    let key = KeyInput::named("KeyV", "v", KeyState::Pressed, CTRL);
    assert!(matches!(
        input_shortcut(&key, true, false),
        Some(Msg::PasteImage {
            width: 2,
            height: 1,
            ..
        })
    ));
    let mut f = Fixture::new(false, false, "");
    f.focus();
    let id = f.id();
    let node = f.node();
    f.session
        .document_mut()
        .context_mut()
        .edit_text_action(id, node, TextEditAction::Paste, &mut services)
        .unwrap();
    f.pump();
    assert!(
        f.pending.iter().any(|m| matches!(m, Msg::EditorPaste)),
        "rich image paste must ask application for the image"
    );
}

#[test]
fn inline_images_and_markers_survive_clipboard_undo_scroll_and_resource_refresh() {
    use nanatype_model::{AssetRef, Atom, AtomKind, Document, Inline, Marker};
    let mut f = Fixture::new(false, false, "");
    let asset: AssetRef = "blob:acceptance-image".parse().unwrap();
    let mut doc = Document::from_inlines(vec![
        Inline::Atom(Atom {
            kind: AtomKind::Image,
            asset: asset.clone(),
            height_em: 3.0,
            alt: "image".into(),
            effects: Default::default(),
        }),
        Inline::Marker(Marker::Wait { ms: 120 }),
    ]);
    doc.paragraphs
        .extend(Document::plain(&"long image draft 中文 👩‍💻\n".repeat(80)).paragraphs);
    f.ui.draft.set(Arc::new(doc));
    // A generated solid PNG makes image rendering measurable.
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(8, 8, image::Rgba([255, 25, 85, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let data_url = format!("data:image/png;base64,{}", base64(&png));
    f.ui.asset_urls.update(|urls| {
        Arc::make_mut(urls).insert(asset.to_string(), data_url);
    });
    f.session.flush().unwrap();
    f.focus();
    f.key("End", CTRL);
    assert!(f.offset() > 0.0);
    let bottom = f.offset();
    f.wheel(80.0);
    assert!(f.offset() < bottom);
    f.key("Home", CTRL);
    let initial = f
        .session
        .document()
        .context()
        .read(f.editor, |e| e.value.clone())
        .unwrap();
    assert_eq!(initial.objects().len(), 2);
    assert!(matches!(
        initial.objects()[0].1.content,
        nana_ui_core::RichObjectContent::Image { .. }
    ));
    f.key("a", CTRL);
    f.action(TextEditAction::Copy);
    f.action(TextEditAction::Cut);
    f.action(TextEditAction::Paste);
    let pasted = f
        .session
        .document()
        .context()
        .read(f.editor, |e| e.value.clone())
        .unwrap();
    assert_eq!(
        pasted, initial,
        "internal paste must preserve formatted objects"
    );
    f.action(TextEditAction::Undo);
    assert!(f.text().is_empty());
    f.action(TextEditAction::Redo);
    f.key("Home", CTRL);
    let selection = f.selection();
    f.ui.asset_urls.update(|urls| {
        Arc::make_mut(urls).insert("newly-loaded".into(), "data:,ignored".into());
    });
    f.session.flush().unwrap();
    assert_eq!(f.selection(), selection);
    let out =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/editor-acceptance");
    std::fs::create_dir_all(&out).unwrap();
    f.session
        .screenshot_png(out.join("inline-objects.png"))
        .unwrap();
    let (_, pixels) = f.session.screenshot_rgba().unwrap();
    assert!(
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[0] > 245 && p[1] < 35 && p[2] > 75 && p[2] < 95)
            .count()
            > 1000,
        "inline picture pixels must render"
    );
}

#[test]
fn clicking_source_toolbar_preserves_selection_records_undo_and_restores_typing_focus() {
    let mut f = Fixture::new(true, false, "中文 👩‍💻");
    f.focus();
    f.key("a", CTRL);
    let label = f.ui.now_t("tb-bold");
    let button = f
        .session
        .accessibility_dump()
        .into_iter()
        .find(|n| n.label.as_deref() == Some(label.as_str()))
        .unwrap()
        .bounds;
    f.click(
        button.x + button.width / 2.0,
        button.y + button.height / 2.0,
    );
    let tool = f
        .pending
        .drain(..)
        .find_map(|m| if let Msg::Tool(t) = m { Some(t) } else { None })
        .expect("real toolbar click");
    assert!(matches!(tool, Tool::Bold));
    input::capture_source_selection(f.ui, f.session.document().context(), f.source);
    let mut src = f.ui.source.get_untracked();
    crate::app::apply_source_markup(&mut src, crate::app::markup_for(&tool).unwrap());
    f.ui.source.set(src);
    f.sync(true, true, Vec::new());
    let formatted = f.text();
    assert_eq!(formatted, "[b]中文 👩‍💻[/b]");
    assert_eq!(
        f.session.document().context().world().focused(f.id()),
        Some(f.source.stable_id())
    );
    f.event(InputPayload::Text(nana_ui_platform::CommittedText::new(
        "继续",
    )));
    assert_eq!(f.text(), format!("{formatted}继续"));
    f.key("z", CTRL);
    assert_eq!(f.text(), formatted);
    f.key("z", CTRL);
    assert_eq!(f.text(), "中文 👩‍💻");
}

#[test]
fn layout_switch_preserves_both_editors_selection_rich_content_and_undo() {
    for source_mode in [false, true] {
        let mut f = Fixture::new(source_mode, false, "甲乙丙");
        f.focus();
        f.key("End", CTRL);
        if source_mode {
            f.event(InputPayload::Text(nana_ui_platform::CommittedText::new(
                "[b]中文[/b][pause]",
            )));
        } else {
            f.insert_phrase("[b]中文[/b][size=1.5]大字[/size][pause][sticker=cats/hello]");
        }
        f.key("a", CTRL);
        let (text, draft, selection) = (f.text(), f.ui.draft.get_untracked(), f.selection());
        for preview in [true, false] {
            let switch = f
                .session
                .accessibility_dump()
                .into_iter()
                .find(|n| {
                    n.role == "switch"
                        && n.label.as_deref() == Some(f.ui.now_t("input-skin-preview").as_str())
                })
                .unwrap();
            assert!(switch.bounds.width >= 32.0);
            if preview {
                f.click(
                    switch.bounds.x + switch.bounds.width / 2.0,
                    switch.bounds.y + switch.bounds.height / 2.0,
                );
            } else {
                let id = f.id();
                f.session
                    .document_mut()
                    .context_mut()
                    .focus_node(id, nana_ui::runtime::StableNodeId::new(switch.id).unwrap())
                    .unwrap();
                f.key(" ", Default::default());
            }
            let edit = f
                .pending
                .drain(..)
                .find_map(|msg| match msg {
                    Msg::Edit(edit) => Some(edit),
                    _ => None,
                })
                .expect("switch dispatches a settings edit");
            let mut settings = (*f.ui.settings.get_untracked()).clone();
            edit(&mut settings);
            assert_eq!(settings.general.input_skin_preview, preview);
            f.ui.settings.set(Arc::new(settings));
            f.sync(true, false, Vec::new());
            assert_eq!(f.text(), text);
            assert_eq!(f.ui.draft.get_untracked(), draft);
            assert_eq!(f.selection(), selection);
        }
        f.action(TextEditAction::Undo);
        assert_eq!(f.text(), "甲乙丙");
        f.action(TextEditAction::Redo);
        assert_eq!(f.text(), text);
        assert_eq!(f.ui.draft.get_untracked(), draft);
    }
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                TABLE[(n >> (18 - 6 * i) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

#[test]
fn typography_popovers_keep_selection_and_dispatch_authored_attributes() {
    use nanatype_model::{FontSlot, ParagraphAlign, Size};
    for (panel, choice, expected) in [
        (
            "tb-size",
            "tb-apply-pixels",
            Tool::Size(Some(Size::Px(28.0))),
        ),
        (
            "tb-font",
            "tb-font-accent",
            Tool::Font(Some(FontSlot::Accent)),
        ),
        (
            "tb-align",
            "tb-align-center",
            Tool::Align(Some(ParagraphAlign::Center)),
        ),
    ] {
        let mut f = Fixture::new(false, false, "中文\nsecond");
        f.sync(true, false, vec![RichEditCommand::Select(0..6)]);
        let selection = f.selection();
        let before = f.text();
        let more = f.ui.now_t("tb-more");
        let button = f
            .session
            .accessibility_dump()
            .into_iter()
            .find(|n| n.label.as_deref() == Some(more.as_str()))
            .unwrap()
            .bounds;
        f.click(
            button.x + button.width / 2.0,
            button.y + button.height / 2.0,
        );
        let label = f.ui.now_t(panel);
        let button = f
            .session
            .accessibility_dump()
            .into_iter()
            .find(|n| n.role == "menu-item" && n.label.as_deref() == Some(label.as_str()))
            .unwrap()
            .bounds;
        f.click(
            button.x + button.width / 2.0,
            button.y + button.height / 2.0,
        );
        assert_eq!(f.text(), before, "opening {panel} must not change text");
        assert_eq!(
            f.selection(),
            selection,
            "opening {panel} must retain selection"
        );
        f.input.advance(Duration::from_millis(300));
        f.session
            .document_mut()
            .context_mut()
            .advance_animations(f.input.now());
        f.session.flush().unwrap();
        if let Some(out) = std::env::var_os("NANATYPE_TYPOGRAPHY_CAPTURE_DIR") {
            let out = std::path::PathBuf::from(out);
            std::fs::create_dir_all(&out).unwrap();
            f.session
                .screenshot_png(out.join(format!("{panel}.png")))
                .unwrap();
        }
        let label = f.ui.now_t(choice);
        let button = f
            .session
            .accessibility_dump()
            .into_iter()
            .find(|n| n.label.as_deref() == Some(label.as_str()))
            .unwrap()
            .bounds;
        f.click(
            button.x + button.width / 2.0,
            button.y + button.height / 2.0,
        );
        let tool = f
            .pending
            .drain(..)
            .find_map(|m| if let Msg::Tool(t) = m { Some(t) } else { None })
            .expect("panel choice dispatches a tool");
        if panel == "tb-size" {
            let Tool::Size(Some(Size::Px(px))) = tool else {
                panic!("pixel size command");
            };
            assert!(px >= 4.0 && px <= 400.0);
        } else {
            assert_eq!(tool, expected);
        }
        assert_eq!(f.selection(), selection);
    }
}
