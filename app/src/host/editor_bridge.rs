//! Isolation boundary for NanaUI rich-text / source editor bridging, context
//! menus, clipboard caching, and completion popup placement.

use std::collections::HashMap;
use std::sync::Arc;

use nana_ui::runtime::{Entity, FrameworkError, RichTextEditor, RuntimeDocument};
use nana_ui_platform::WindowId;

use crate::app::{App, EditorCommand};
use crate::msg::{EditorMsg, HostMsg, Msg};
use crate::ui::input;
use crate::windows::WindowKind;

const COMPLETION_RETRIES: u8 = 3;

/// Cached clipboard service for editor context-menu Cut/Copy/Paste without
/// re-allocating `arboard::Clipboard::new()` on every action.
#[derive(Default)]
pub(super) struct EditorClipboard {
    inner: Option<arboard::Clipboard>,
}

impl EditorClipboard {
    fn ensure(&mut self) -> Option<&mut arboard::Clipboard> {
        if self.inner.is_none() {
            self.inner = arboard::Clipboard::new().ok();
        }
        self.inner.as_mut()
    }

    fn paste_text_available(&mut self) -> bool {
        self.ensure()
            .is_some_and(|cb| cb.get_text().map_or(true, |t| !t.is_empty()))
    }
}

impl nana_ui_platform::HostServices for EditorClipboard {
    fn set_cursor(&mut self, _: nana_ui_platform::CursorIcon) {}
    fn set_text_input(&mut self, _: Option<&nana_ui_platform::TextInputContext>) {}

    fn read_clipboard(&mut self) -> Result<Option<String>, nana_ui_platform::HostServiceError> {
        let Some(clipboard) = self.ensure() else {
            return Err(nana_ui_platform::HostServiceError::Failed);
        };
        match clipboard.get_text() {
            Ok(text) => Ok(Some(text)),
            Err(arboard::Error::ContentNotAvailable) => Ok(None),
            Err(_) => Err(nana_ui_platform::HostServiceError::Failed),
        }
    }

    fn write_clipboard(&mut self, text: &str) -> Result<(), nana_ui_platform::HostServiceError> {
        let Some(clipboard) = self.ensure() else {
            return Err(nana_ui_platform::HostServiceError::Failed);
        };
        clipboard
            .set_text(text)
            .map_err(|_| nana_ui_platform::HostServiceError::Failed)
    }
}

/// An image on the clipboard with no text beside it (text pastes normally).
pub(super) fn clipboard_image() -> Option<Msg> {
    let mut clipboard = arboard::Clipboard::new().ok()?;
    if clipboard.get_text().is_ok_and(|t| !t.is_empty()) {
        return None;
    }
    let image = clipboard.get_image().ok()?;
    Some(Msg::Editor(EditorMsg::PasteImage {
        width: image.width as u32,
        height: image.height as u32,
        rgba: image.bytes.into_owned(),
    }))
}

pub(super) struct EditorBridge {
    pub editor: Option<Entity<RichTextEditor>>,
    pub source: Option<Entity<nana_ui::runtime::TextArea>>,
    pub editor_menu: Option<Entity<nana_ui::runtime::OverlayHost>>,
    pub completion_unsettled: bool,
    completion_retries: u8,
    editor_fx: Option<(usize, bool, usize)>,
    clipboard: EditorClipboard,
}

impl Default for EditorBridge {
    fn default() -> Self {
        Self {
            editor: None,
            source: None,
            editor_menu: None,
            completion_unsettled: false,
            completion_retries: COMPLETION_RETRIES,
            editor_fx: None,
            clipboard: EditorClipboard::default(),
        }
    }
}

impl EditorBridge {
    pub fn set_input_entities(
        &mut self,
        editor: Entity<RichTextEditor>,
        source: Entity<nana_ui::runtime::TextArea>,
    ) {
        self.editor = Some(editor);
        self.source = Some(source);
        self.editor_fx = None;
    }

    pub fn clear_menu(&mut self, docs: &mut HashMap<WindowId, RuntimeDocument>) {
        if let Some(menu) = self.editor_menu.take()
            && let Some(doc) = docs.get_mut(&WindowKind::Input.id())
        {
            let _ = doc.context_mut().remove_view(menu);
        }
    }

    pub fn draft_focused(&self, doc: &RuntimeDocument) -> bool {
        let focused = doc.context().world().focused(doc.document());
        focused.is_some()
            && (focused == self.editor.map(|e| e.stable_id())
                || focused == self.source.map(|e| e.stable_id()))
    }

    pub fn capture_source_selection(
        &self,
        ui: crate::ui::Ui,
        docs: &HashMap<WindowId, RuntimeDocument>,
    ) {
        if let (Some(source), Some(doc)) = (self.source, docs.get(&WindowKind::Input.id())) {
            input::capture_source_selection(ui, doc.context(), source);
        }
    }

    fn editor_target(&self, source: bool) -> Option<nana_ui::runtime::StableNodeId> {
        if source {
            self.source.map(|e| e.stable_id())
        } else {
            self.editor.map(|e| e.stable_id())
        }
    }

    pub fn open_menu(
        &mut self,
        docs: &mut HashMap<WindowId, RuntimeDocument>,
        ui: crate::ui::Ui,
        source: bool,
        x: f32,
        y: f32,
        keyboard: bool,
    ) {
        let Some(target) = self.editor_target(source) else {
            return;
        };
        let paste_available = self.clipboard.paste_text_available();
        let Some(doc) = docs.get_mut(&WindowKind::Input.id()) else {
            return;
        };
        if let Some(menu) = self.editor_menu.take() {
            let _ = doc.context_mut().remove_view(menu);
        }
        self.editor_menu =
            mount_editor_menu(doc, ui, target, source, (x, y, keyboard), paste_available).ok();
    }

    pub fn handle_action(
        &mut self,
        app: &mut App,
        docs: &mut HashMap<WindowId, RuntimeDocument>,
        source: bool,
        action: nana_ui::runtime::TextEditAction,
    ) {
        if source != app.ui.source_mode.get_untracked() {
            return;
        }
        if !matches!(
            action,
            nana_ui::runtime::TextEditAction::Copy | nana_ui::runtime::TextEditAction::SelectAll
        ) {
            app.draft.bump_version();
            app.speech_user_edit();
        }
        let Some(target) = self.editor_target(source) else {
            return;
        };
        if source
            && action == nana_ui::runtime::TextEditAction::Paste
            && let Some(message) = clipboard_image()
        {
            self.clear_menu(docs);
            if let Ok(app_msg) = message.into_domain() {
                app.update(app_msg);
            }
            app.draft.focus_editor = true;
            return;
        }
        let Some(doc) = docs.get_mut(&WindowKind::Input.id()) else {
            return;
        };
        let document = doc.document();
        let cx = doc.context_mut();
        if let Some(menu) = self.editor_menu.take() {
            let _ = cx.remove_view(menu);
        }
        if let Err(error) = cx.edit_text_action(document, target, action, &mut self.clipboard) {
            nanatype_diag::report!(crate::diag::EDIT_FAILED, kind = 0u64; "{error}");
        }
    }

    /// Apply pending editor commands and refresh per-glyph presentation chains.
    pub fn drive(&mut self, app: &mut App, docs: &mut HashMap<WindowId, RuntimeDocument>) {
        let (commands, focus, source_undoable) = app.draft.take_bridge_state();
        let (Some(editor), Some(doc)) = (self.editor, docs.get_mut(&WindowKind::Input.id())) else {
            return;
        };
        sync_editor(
            doc,
            app.ui,
            editor,
            self.source,
            commands,
            focus,
            source_undoable,
        );
        let cx = doc.context_mut();
        let reduced = app.ui.reduced_motion.get_untracked();
        let key = (
            Arc::as_ptr(&app.appearance.skin) as usize,
            reduced,
            crate::ui::rich::attr_version(),
        );
        if self.editor_fx != Some(key) {
            let effects = crate::ui::rich::effect_table(&app.appearance.skin.effects, reduced);
            if cx
                .set_rich_presentation_chains(editor.stable_id(), effects, None)
                .is_ok()
            {
                self.editor_fx = Some(key);
            }
        }
    }

    /// Hang the rich editor's `@` / `/` list under the word it completes,
    /// and close the app's list when the markup source's own popup was closed.
    pub fn anchor_completion(&mut self, app: &App, docs: &HashMap<WindowId, RuntimeDocument>) {
        let ui = app.ui;
        if let (Some(doc), Some(source)) = (docs.get(&WindowKind::Input.id()), self.source)
            && input::completion::source_dismissed(ui, doc.context().world(), source.stable_id())
        {
            app.post(Msg::Editor(EditorMsg::Complete(
                crate::msg::Complete::Dismiss,
            )));
        }
        let anchor = self.completion_anchor(app, docs);
        if anchor.is_some_and(|a| !a.settled) {
            self.completion_retries = self.completion_retries.saturating_sub(1);
            self.completion_unsettled = self.completion_retries > 0;
        } else {
            self.completion_retries = COMPLETION_RETRIES;
            self.completion_unsettled = false;
        }
        let at = anchor.map(|a| a.at);
        if ui.completion_at.get_untracked() != at {
            ui.completion_at.set(at);
        }
    }

    fn completion_anchor(
        &self,
        app: &App,
        docs: &HashMap<WindowId, RuntimeDocument>,
    ) -> Option<input::completion::Anchor> {
        let world = docs.get(&WindowKind::Input.id())?.context().world();
        input::completion::anchor(app.ui, world, self.editor?.stable_id())
    }
}

pub(super) fn mount_editor_menu(
    doc: &mut RuntimeDocument,
    ui: crate::ui::Ui,
    target: nana_ui::runtime::StableNodeId,
    source: bool,
    (x, y, keyboard): (f32, f32, bool),
    paste_available: bool,
) -> Result<Entity<nana_ui::runtime::OverlayHost>, FrameworkError> {
    use nana_ui::runtime::{ContextMenuEvent, TextEditAction};
    let document = doc.document();
    let cx = doc.context_mut();
    let mut root = target;
    while let Some(parent) = cx.world().parent_id(root) {
        root = parent;
    }
    let bounds = cx
        .world()
        .layout_box(root)
        .ok_or(FrameworkError::MissingView(root))?;
    let mut enabled = TextEditAction::ALL.map(|action| cx.text_edit_action_enabled(target, action));
    enabled[4] &= paste_available;
    let mut menu = input::editor_context_menu(ui, x, y, enabled);
    if keyboard {
        menu.highlighted = menu.items.iter().position(|item| !item.disabled);
    }
    menu.place_in(bounds);
    let overlay = cx.create_component(document, nana_ui::runtime::OverlayHost::new())?;
    let result = (|| {
        let menu = cx.create_component(document, menu)?;
        cx.append_child(overlay, menu)?;
        cx.activate_overlay(overlay, menu)?;
        cx.on(menu, move |_, event: &ContextMenuEvent, cx| {
            if let ContextMenuEvent::Select(key) = event
                && let Some(action) = TextEditAction::ALL
                    .into_iter()
                    .find(|a| a.key() == key.as_ref())
            {
                cx.dispatch_program_all(Msg::Host(HostMsg::EditorAction { source, action }));
            }
        })?;
        Ok(overlay)
    })();
    if result.is_err() {
        let _ = cx.remove_view(overlay);
    }
    result
}

/// Commit application edits before bindings observe their new values.
pub(super) fn sync_editor(
    doc: &mut RuntimeDocument,
    ui: crate::ui::Ui,
    editor: Entity<RichTextEditor>,
    source: Option<Entity<nana_ui::runtime::TextArea>>,
    commands: Vec<EditorCommand>,
    focus: bool,
    source_undoable: bool,
) {
    let document = doc.document();
    let cx = doc.context_mut();
    if source_undoable && let Some(source) = source {
        let value = ui.source.get_untracked();
        if let Err(error) = cx.edit_text_area_value(
            source,
            value.text,
            nana_ui::runtime::TextSelection::new(value.selection.0, value.selection.1),
        ) {
            nanatype_diag::report!(crate::diag::EDIT_FAILED, kind = 1u64; "{error}");
        }
    }
    if focus {
        if let Err(error) = cx.flush_reactive() {
            nanatype_diag::report!(crate::diag::EDIT_FAILED, kind = 2u64; "{error}");
        }
        let target = if ui.source_mode.get_untracked() {
            source.map(|s| s.stable_id())
        } else {
            Some(editor.stable_id())
        };
        if let Some(target) = target {
            let _ = cx.focus_node(document, target);
        }
    }
    for command in commands {
        let result = match command {
            EditorCommand::Edit(command) => cx.rich_edit(editor, *command),
            EditorCommand::Insert(fragment) => insert_rich_fragment(cx, ui, editor, &fragment),
            EditorCommand::Delete => delete_rich_selection(cx, ui, editor),
        };
        if let Err(error) = result {
            nanatype_diag::report!(crate::diag::EDIT_FAILED, kind = 3u64; "{error}");
        }
    }
}

fn insert_rich_fragment(
    cx: &mut nana_ui::runtime::AppContext,
    ui: crate::ui::Ui,
    editor: Entity<RichTextEditor>,
    fragment: &nana_ui_core::RichText,
) -> Result<bool, FrameworkError> {
    use nana_ui::runtime::{RichEditCommand, RichTextEditorEvent};
    if fragment.text().is_empty() || cx.read(editor, |e| e.read_only || e.disabled)? {
        return Ok(false);
    }
    let selection = cx.read(editor, |e| e.selection())?;
    let start = selection.0.min(selection.1);
    let end = start + fragment.text().len();
    ui.editor_transaction.set(true);
    let result = (|| {
        cx.rich_edit(
            editor,
            RichEditCommand::Select(start..selection.0.max(selection.1)),
        )?;
        cx.rich_edit(editor, RichEditCommand::InsertText(fragment.text().into()))
    })();
    ui.editor_transaction.set(false);
    result?;
    cx.update_component(editor, |e, cx| {
        e.value.replace_with(start..end, fragment);
        cx.emit(RichTextEditorEvent::Changed(e.value.clone()));
    })?;
    cx.rich_edit(editor, RichEditCommand::Select(end..end))?;
    Ok(true)
}

fn delete_rich_selection(
    cx: &mut nana_ui::runtime::AppContext,
    ui: crate::ui::Ui,
    editor: Entity<RichTextEditor>,
) -> Result<bool, FrameworkError> {
    use nana_ui::runtime::{RichEditCommand, RichTextEditorEvent};
    if cx.read(editor, |e| e.read_only || e.disabled)? {
        return Ok(false);
    }
    let selection = cx.read(editor, |e| e.selection())?;
    let start = selection.0.min(selection.1);
    if start == selection.0.max(selection.1) {
        return Ok(false);
    }
    ui.editor_transaction.set(true);
    let typed = cx.rich_edit(editor, RichEditCommand::InsertText(" ".into()));
    ui.editor_transaction.set(false);
    typed?;
    cx.rich_edit(editor, RichEditCommand::Select(start..start))?;
    cx.update_component(editor, |e, cx| {
        e.value.replace_range(start..start + 1, "");
        cx.emit(RichTextEditorEvent::Changed(e.value.clone()));
    })?;
    Ok(true)
}
