//! Windowless acceptance of the quick input bar: its view, keys and the
//! plain text it sends.
use super::*;
use nana_ui::NanaTextShaper;
use nana_ui::runtime::{HeadlessInput, StableNodeId, TextInput};
use nana_ui_devtools::agent::RuntimeAgentSession;
use nana_ui_platform::{CommittedText, CompositionInput, InputModifiers, KeyInput};
use nanatype_settings::{Hotkey, Settings};
use std::sync::Mutex;

struct Fixture {
    ui: crate::ui::Ui,
    session: RuntimeAgentSession,
    field: Entity<TextInput>,
    input: HeadlessInput,
    shaper: NanaTextShaper,
    messages: Arc<Mutex<Vec<Msg>>>,
}

impl Fixture {
    fn new() -> Self {
        Self::with_send(Some(Hotkey::bare("Enter")))
    }

    fn with_send(send: Option<Hotkey>) -> Self {
        let skin = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"))
            .load("nanatype.halo")
            .unwrap()
            .skin;
        let mut settings = Settings::default();
        settings.hotkeys.input.send = send;
        let ui = crate::ui::Ui::new(Arc::new(settings), skin, nanatype_i18n::Locale::ZhCn);
        let id = DocumentId::new(1).unwrap();
        let mut doc = RuntimeDocument::new(id);
        doc.context_mut()
            .set_theme_tokens(crate::ui::kit::theme::compile(false))
            .unwrap();
        let (_, field) = doc
            .context_mut()
            .mount_view_root(id, move || quick::quick_window(ui))
            .unwrap();
        let messages = Arc::new(Mutex::new(Vec::new()));
        let observed = messages.clone();
        // Same callback as Host::mount.
        doc.context_mut()
            .on_key(field, move |key: &KeyInput| {
                quick_key(key)
                    .or_else(|| quick_send_key(key, ui))
                    .map(|m| observed.lock().unwrap().push(m))
                    .is_some()
            })
            .unwrap();
        let (w, h) = crate::windows::QUICK_SIZE;
        let mut session = RuntimeAgentSession::new(doc, w as u32, h as u32).unwrap();
        let input = HeadlessInput::bind(session.document_mut().context_mut(), id);
        let document = session.document().document();
        let cx = session.document_mut().context_mut();
        cx.focus_node(document, field.stable_id()).unwrap();
        session.flush().unwrap();
        Self {
            ui,
            session,
            field,
            input,
            shaper: NanaTextShaper::default(),
            messages,
        }
    }

    fn event(&mut self, payload: InputPayload) {
        self.input.advance(std::time::Duration::from_millis(16));
        self.input
            .route_shaped(
                self.session.document_mut().context_mut(),
                payload,
                Some(&mut self.shaper),
            )
            .unwrap();
        for message in self
            .session
            .document_mut()
            .context_mut()
            .take_program_messages()
        {
            if let Ok(message) = message.downcast::<Msg>() {
                self.messages.lock().unwrap().push(*message);
            }
        }
        self.session.flush().unwrap();
    }

    fn key(&mut self, name: &'static str) {
        self.key_with(name, InputModifiers::default());
    }

    fn key_with(&mut self, name: &'static str, modifiers: InputModifiers) {
        self.event(InputPayload::Key(KeyInput::named(
            "Unidentified",
            name,
            KeyState::Pressed,
            modifiers,
        )));
    }

    fn take(&self) -> Vec<Msg> {
        std::mem::take(&mut *self.messages.lock().unwrap())
    }

    fn value(&self) -> String {
        let cx = self.session.document().context();
        cx.read(self.field, |e| e.state.value.to_string()).unwrap()
    }
}

#[test]
fn the_bar_is_one_focused_field_without_toolbar_or_panels() {
    let f = Fixture::new();
    let doc = f.session.document();
    assert_eq!(
        doc.context().world().focused(doc.document()),
        Some(f.field.stable_id())
    );
    let nodes = f.session.accessibility_dump();
    for key in [
        "tb-bold",
        "tb-source",
        "tb-send",
        "tb-cheatsheet",
        "tray-mode",
    ] {
        let label = f.ui.now_t(key);
        assert!(
            !nodes
                .iter()
                .any(|n| n.label.as_deref() == Some(label.as_str())),
            "{key} in the quick bar"
        );
    }
    let field = f
        .session
        .document()
        .context()
        .world()
        .layout_box(f.field.stable_id())
        .unwrap();
    let (w, h) = crate::windows::QUICK_SIZE;
    assert!(field.width > w / 2.0 && field.y >= 0.0 && field.y + field.height <= h);
}

#[test]
fn configured_opacity_is_applied_to_the_bar_offscreen() {
    let mut f = Fixture::new();
    f.ui.settings.update(|settings| {
        Arc::make_mut(settings).general.quick_opacity = 0.42;
    });
    f.session.flush().unwrap();

    let cx = f.session.document().context();
    let field = cx
        .world()
        .document_order(f.session.document().document())
        .into_iter()
        .find(|id: &StableNodeId| {
            cx.assembly_path(*id)
                .is_some_and(|path| path.rsplit('/').next() == Some("quick-field"))
        })
        .expect("quick field is mounted");
    let bar = cx.world().parent_id(field).expect("quick bar parent");
    let opacity = cx.world().node_style(bar).map(|style| style.layout.opacity);
    assert_eq!(opacity, Some(Some(0.42)));
}

#[test]
fn enter_sends_the_typed_line_and_escape_hides_the_bar() {
    let mut f = Fixture::new();
    f.event(InputPayload::Text(CommittedText::new("hello [b] /shake")));
    assert_eq!(f.value(), "hello [b] /shake");
    assert_eq!(f.ui.quick_text.get_untracked(), "hello [b] /shake");
    f.take();
    f.key("Enter");
    assert!(matches!(
        f.take().as_slice(),
        [Msg::QuickSend(text)] if text == "hello [b] /shake"
    ));
    f.key("Escape");
    assert!(matches!(f.take().as_slice(), [Msg::QuickHide]));
}

#[test]
fn configured_send_key_is_shared_with_the_quick_bar() {
    let mut f = Fixture::with_send(Some(Hotkey::ctrl("Enter")));
    f.event(InputPayload::Text(CommittedText::new("hello")));
    f.take();

    // Enter is no longer a send key in either input mode.
    f.key("Enter");
    assert!(!f.take().iter().any(|m| matches!(m, Msg::QuickSend(_))));

    let ctrl = InputModifiers {
        control: true,
        ..InputModifiers::default()
    };
    f.key_with("Enter", ctrl);
    assert!(matches!(
        f.take().as_slice(),
        [Msg::QuickSend(text)] if text == "hello"
    ));
}

#[test]
fn enter_while_composing_commits_the_ime_text_without_sending() {
    let mut f = Fixture::new();
    for composition in [
        CompositionInput::Start,
        CompositionInput::Update {
            text: "nihao".into(),
            selection: None,
        },
    ] {
        f.event(InputPayload::Composition(composition));
    }
    f.take();
    f.key("Enter");
    assert!(
        !f.take().iter().any(|m| matches!(m, Msg::QuickSend(_))),
        "Enter during composition sent the line"
    );
    f.event(InputPayload::Composition(CompositionInput::Commit(
        "你好".into(),
    )));
    assert_eq!(f.value(), "你好");
}

#[test]
fn the_line_is_sent_as_plain_text_never_as_markup() {
    for text in [
        "hello [b]bold[/b] /shake @someone",
        "[pause 500] 不是标记",
        r"back\slash [",
    ] {
        let markup = nanatype_markup::serialize(&nanatype_model::Document::plain(text));
        let parsed = nanatype_markup::parse(&markup);
        assert_eq!(parsed.doc.plain_text(), text, "{markup}");
        assert_eq!(
            parsed.doc,
            nanatype_model::Document::plain(text),
            "{markup}"
        );
    }
}
