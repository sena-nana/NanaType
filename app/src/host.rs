//! The NanaUI host program: owns one document per window and routes window
//! events, input and messages to [`App`].

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use nana_ui::runtime::view::{entity_ref, with_refs};
use nana_ui::runtime::{
    DocumentId, Entity, FrameworkError, RichTextEditor, RuntimeDocument, Toast, ToastDismissed,
    ToastStack,
};
use nana_ui::{
    CompiledTheme, DocumentAccessError, FrameDemand, GpuBackendPolicy, MaterialEffect, RoutedInput,
    RuntimeProgram, RuntimeProgramContext, RuntimeProgramUpdate,
};
use nana_ui_platform::host::WindowCommand;
use nana_ui_platform::{InputPayload, KeyState, WindowEvent, WindowId};
use nanatype_core::Cmd;
use nanatype_model::Overflow;
use nanatype_settings::{Hotkey, InputPresence};

use crate::app::{App, SEND_KEY};
use crate::clock;
use crate::msg::{Msg, Tool};
use crate::ui::overflow::{self, LineSpan, Measure, PageKey};
use crate::ui::{danmaku, dialogue, history, input, settings};
use crate::windows::WindowKind;

mod overlay;

pub struct Host {
    app: App,
    perf: Option<crate::perf::Recorder>,
    docs: HashMap<WindowId, RuntimeDocument>,
    /// The input window's rich editor.
    editor: Option<Entity<RichTextEditor>>,
    source: Option<Entity<nana_ui::runtime::TextArea>>,
    editor_menu: Option<Entity<nana_ui::runtime::OverlayHost>>,
    /// Skin and reduced-motion the editor's effect preview was built for.
    editor_fx: Option<(usize, bool, usize)>,
    /// The typewriter's scroll through long text.
    follow: overflow::Follow,
    peer_follow: overflow::Follow,
    /// The typewriter's automatic page breaks.
    pager: overflow::Pager,
    /// The toast stack of each control window (settings, input, chat).
    toasts: HashMap<WindowId, Entity<ToastStack>>,
    stickers: crate::stickers::playback::Playback,
}

/// Application-clock work is separate from rendering-only work (GPU sweep,
/// compositor tracks and the text follower). Recompute at each callback.
#[derive(Default)]
struct FramePolicy {
    clock: Option<nana_ui::runtime::view::Signal<u64>>,
    tick_clock: bool,
    demand: FrameDemand,
}

impl FramePolicy {
    fn advance(&self, now: u64, changed: bool) {
        if (self.tick_clock || changed)
            && let Some(clock) = self.clock
        {
            clock.set_if_changed(now);
        }
    }
}

fn frame_policy(
    ui: crate::ui::Ui,
    kind: Option<WindowKind>,
    now: u64,
    follow: &overflow::Follow,
    peer_follow: &overflow::Follow,
    pager: &overflow::Pager,
) -> FramePolicy {
    let (clock, tick_clock, render_only, next_at, fps) = match kind {
        Some(WindowKind::Display | WindowKind::PeerDisplay) => {
            let remote = kind == Some(WindowKind::PeerDisplay);
            if remote && !ui.peer.get_untracked().active {
                return FramePolicy::default();
            }
            let track = if remote { ui.remote() } else { ui };
            let follow = if remote { peer_follow } else { follow };
            let typing = dialogue::display_animating(track, now);
            let movement = crate::ui::sweep::animating(track)
                || follow.wants_frame()
                || (!remote && pager.wants_frame());
            (
                track.now,
                typing,
                movement || crate::ui::shader_effects::animating(track),
                follow.next_line_at(),
                if typing || movement { 60 } else { 30 },
            )
        }
        Some(WindowKind::History) => {
            let fade = history::fade_schedule(ui, now);
            (ui.history_now, fade.animating, false, fade.next_at, 60)
        }
        Some(WindowKind::Input) => (ui.pulse_ms, input::mirror_chip_pulsing(ui), false, None, 30),
        Some(WindowKind::Settings) => (
            ui.settings_pulse_ms,
            settings::links_pulsing(ui),
            crate::ui::shader_effects::preview_animating(ui),
            None,
            30,
        ),
        _ => return FramePolicy::default(),
    };
    FramePolicy {
        clock: Some(clock),
        tick_clock,
        demand: if tick_clock || render_only {
            FrameDemand::Continuous(NonZeroU32::new(fps).expect("non-zero"))
        } else {
            next_at.map_or(FrameDemand::OnDemand, |at| {
                FrameDemand::At(clock::instant(at))
            })
        },
    }
}

/// The tag every document shapes its text in by default, so Han characters
/// take the active language's glyph shapes instead of the font's first guess.
fn default_language(tag: &str) -> Option<nana_ui::runtime::LanguageTag> {
    nana_ui::runtime::LanguageTag::new(tag)
}

fn is_modifier(code: &str) -> bool {
    matches!(
        code,
        "ShiftLeft"
            | "ShiftRight"
            | "ControlLeft"
            | "ControlRight"
            | "AltLeft"
            | "AltRight"
            | "MetaLeft"
            | "MetaRight"
            | "OSLeft"
            | "OSRight"
    )
}

impl Host {
    fn editor_target(&self, source: bool) -> Option<nana_ui::runtime::StableNodeId> {
        if source {
            self.source.map(|e| e.stable_id())
        } else {
            self.editor.map(|e| e.stable_id())
        }
    }

    fn open_editor_menu(&mut self, source: bool, x: f32, y: f32, keyboard: bool) {
        let Some(target) = self.editor_target(source) else {
            return;
        };
        let ui = self.app.ui;
        let Some(doc) = self.docs.get_mut(&WindowKind::Input.id()) else {
            return;
        };
        if let Some(menu) = self.editor_menu.take() {
            let _ = doc.context_mut().remove_view(menu);
        }
        let paste_available = arboard::Clipboard::new().is_ok_and(|mut clipboard| {
            clipboard.get_text().is_ok_and(|text| !text.is_empty()) || clipboard.get_image().is_ok()
        });
        self.editor_menu =
            mount_editor_menu(doc, ui, target, source, (x, y, keyboard), paste_available).ok();
    }

    fn editor_action(&mut self, source: bool, action: nana_ui::runtime::TextEditAction) {
        if source != self.app.ui.source_mode.get_untracked() {
            return;
        }
        let Some(target) = self.editor_target(source) else {
            return;
        };
        if source
            && action == nana_ui::runtime::TextEditAction::Paste
            && let Some(message) = clipboard_image()
        {
            if let Some(menu) = self.editor_menu.take()
                && let Some(doc) = self.docs.get_mut(&WindowKind::Input.id())
            {
                let _ = doc.context_mut().remove_view(menu);
            }
            self.app.update(message);
            self.app.focus_editor = true;
            return;
        }
        let Some(doc) = self.docs.get_mut(&WindowKind::Input.id()) else {
            return;
        };
        let document = doc.document();
        let cx = doc.context_mut();
        if let Some(menu) = self.editor_menu.take() {
            let _ = cx.remove_view(menu);
        }
        if let Err(error) = cx.edit_text_action(document, target, action, &mut EditorClipboard) {
            crate::debug_log(&format!("editor action: {error}"));
        }
    }

    fn finish(&mut self) -> RuntimeProgramUpdate {
        crate::ui::shader_effects::collect_garbage();
        crate::ui::sweep::collect_garbage();
        for message in crate::ui::shader_effects::take_notices() {
            self.app.notice(message, true);
        }
        self.drive_editor();
        let language = self.app.ui.locale.get_untracked().tag();
        // The app's reduce-motion setting wins over the system's in every
        // window's runtime, so view transitions and toasts land at once too.
        let forced = self.app.settings.playback.reduced_motion;
        for doc in self.docs.values_mut() {
            let cx = doc.context_mut();
            cx.set_default_language(default_language(language));
            if forced {
                cx.set_reduced_motion(true);
            } else {
                cx.follow_system_reduced_motion();
            }
        }
        self.show_toasts();
        if self.app.theme_changed {
            self.app.theme_changed = false;
            let theme = self.app.theme.clone();
            for doc in self.docs.values_mut() {
                let _ = doc.context_mut().set_theme_tokens(theme.clone());
            }
        }
        let mut result = self.app.update_result();
        result.redraw = pending_redraw(&self.docs);
        result
    }

    /// Apply the app's editor commands, and keep the editor's per-glyph
    /// effect preview on the skin's effect table.
    fn drive_editor(&mut self) {
        let commands = std::mem::take(&mut self.app.rich_commands);
        let (Some(editor), Some(doc)) = (self.editor, self.docs.get_mut(&WindowKind::Input.id()))
        else {
            return;
        };
        sync_editor(
            doc,
            self.app.ui,
            editor,
            self.source,
            commands,
            std::mem::take(&mut self.app.focus_editor),
            std::mem::take(&mut self.app.source_edit_undoable),
        );
        let cx = doc.context_mut();
        let reduced = self.app.ui.reduced_motion.get_untracked();
        let key = (
            Arc::as_ptr(&self.app.skin) as usize,
            reduced,
            crate::ui::rich::attr_version(),
        );
        if self.editor_fx != Some(key) {
            let effects = crate::ui::rich::effect_table(&self.app.skin.effects, reduced);
            if cx
                .set_rich_presentation_chains(editor.stable_id(), effects, None)
                .is_ok()
            {
                self.editor_fx = Some(key);
            }
        }
    }

    /// Push the app's new toasts into the settings window (when open) and
    /// the input window (while it is visible). A toast that times out or is
    /// closed comes back as [`Msg::ToastDismissed`].
    fn show_toasts(&mut self) {
        let requests = std::mem::take(&mut self.app.toasts);
        if requests.is_empty() {
            return;
        }
        let input_visible = self.app.input_visible();
        for request in requests {
            let mut shown = false;
            for kind in [WindowKind::Settings, WindowKind::Input, WindowKind::Chat] {
                let wanted = match kind {
                    WindowKind::Input => input_visible,
                    WindowKind::Chat => true,
                    _ => request.settings,
                };
                let id = kind.id();
                let (true, Some(stack), Some(doc)) = (
                    wanted,
                    self.toasts.get(&id).copied(),
                    self.docs.get_mut(&id),
                ) else {
                    continue;
                };
                let toast = Toast::new(request.title.as_str(), request.tone)
                    .timeout(std::time::Duration::from_millis(request.ms))
                    .dismissible(true);
                let cx = doc.context_mut();
                let pushed = cx.push_toast(stack, toast).and_then(|toast| {
                    cx.on(toast, move |_, _: &ToastDismissed, vcx| {
                        vcx.dispatch_program_all(Msg::ToastDismissed(kind, toast));
                    })
                });
                match pushed {
                    Ok(()) => shown = true,
                    Err(e) => crate::debug_log(&format!("toast: {e}")),
                }
            }
            // No window to show it in yet (a notice at start-up): keep it
            // for one that opens while it is still fresh.
            if !shown && request.at.elapsed() < std::time::Duration::from_millis(request.ms) {
                self.app.toasts.push(request);
            }
        }
    }

    fn dismiss_toast(&mut self, kind: WindowKind, toast: Entity<Toast>) {
        let id = kind.id();
        if let (Some(stack), Some(doc)) = (self.toasts.get(&id).copied(), self.docs.get_mut(&id)) {
            let _ = doc.context_mut().remove_toast(stack, toast);
        }
    }

    fn mount(&mut self, id: WindowId, kind: WindowKind) -> Result<(), String> {
        let doc_id = DocumentId::new(id.0 + 1).ok_or("document id")?;
        let mut doc = RuntimeDocument::new(doc_id);
        let ui = self.app.ui;
        let context = doc.context_mut();
        let _ = context.set_theme_tokens(self.app.theme.clone());
        let err = |e: FrameworkError| e.to_string();
        match kind {
            WindowKind::Display => {
                context
                    .mount_view_root(doc_id, move || dialogue::display_window(ui))
                    .map_err(err)?;
            }
            WindowKind::PeerDisplay => {
                context
                    .mount_view_root(doc_id, move || dialogue::peer_window(ui))
                    .map_err(err)?;
            }
            WindowKind::History => {
                context
                    .mount_view_root(doc_id, move || history::history_window(ui))
                    .map_err(err)?;
            }
            WindowKind::Settings => {
                let (_, stack) = context
                    .mount_view_root(doc_id, move || {
                        let stack = entity_ref::<ToastStack>();
                        let view =
                            crate::ui::feedback::with_toasts(settings::settings_window(ui), stack);
                        with_refs(view, stack)
                    })
                    .map_err(err)?;
                self.toasts.insert(id, stack);
            }
            WindowKind::Chat => {
                let (_, stack) = context
                    .mount_view_root(doc_id, move || {
                        let stack = entity_ref::<ToastStack>();
                        with_refs(
                            crate::ui::feedback::with_toasts_limit(
                                danmaku::chat_window(ui),
                                stack,
                                1,
                            ),
                            stack,
                        )
                    })
                    .map_err(err)?;
                self.toasts.insert(id, stack);
            }
            WindowKind::Input => {
                let (_, (editor, source, stack)) = context
                    .mount_view_root(doc_id, move || input::input_window(ui))
                    .map_err(err)?;
                context
                    .on_key(editor, key_policy(self.app.ctx.clone(), false))
                    .map_err(err)?;
                context
                    .on_key(source, key_policy(self.app.ctx.clone(), true))
                    .map_err(err)?;
                self.editor = Some(editor);
                self.source = Some(source);
                self.editor_fx = None;
                self.toasts.insert(id, stack);
            }
        }
        doc.context_mut()
            .set_default_language(default_language(self.app.ui.locale.get_untracked().tag()));
        self.docs.insert(id, doc);
        Ok(())
    }

    /// Read the typewriter's last layout: scroll along with the typing, or
    /// tell the core where a full box turns the page.
    fn follow_track(&mut self, now: u64, remote: bool) {
        let ui = if remote {
            self.app.ui.remote()
        } else {
            self.app.ui
        };
        let view = ui.display.get_untracked();
        let mut mode = if remote {
            Overflow::Scroll
        } else {
            self.app.skin.dialogue.overflow
        };
        let mirror = self.app.settings.general.input_mode == nanatype_settings::InputMode::Mirror;
        if mode == Overflow::Page && (mirror || view.line.is_none()) {
            // Nothing to page while the text follows the editor.
            mode = Overflow::Scroll;
        }
        let key = (!view.empty && mode != Overflow::Clip).then(|| PageKey::of(&view));
        let page = overflow::published_on(ui.track_slot).filter(|p| Some(p.key) == key);
        let measure = page.as_ref().and_then(|page| {
            let world = self.docs.get(&ui.display_kind().id())?.context().world();
            let (_, layout) = world.text_layout(ui.text_node.get_untracked()?)?;
            // A layout of an older node until the new one is laid out.
            let end = layout.lines.last()?.source.end;
            if end < page.text_trimmed || end > page.text_len {
                return None;
            }
            Some(Measure {
                lines: layout
                    .lines
                    .iter()
                    .map(|l| LineSpan {
                        bytes: l.source.clone(),
                        top: l.bounds.y,
                        bottom: l.bounds.y + l.bounds.height,
                    })
                    .collect(),
                viewport: world.layout_box(ui.text_port.get_untracked()?)?.height,
                glyphs: &page.glyphs,
            })
        });
        let reduced = ui.reduced_motion.get_untracked();
        let (follow, pager) = match mode {
            Overflow::Page => (None, key),
            _ => (key, None),
        };
        let follower = if remote {
            &mut self.peer_follow
        } else {
            &mut self.follow
        };
        let y = follower.frame(follow, measure.as_ref(), now, reduced);
        if !remote
            && let Some(breaks) = self.pager.frame(pager, measure.as_ref())
            && let Some(line) = view.line
        {
            self.app.post(Msg::Core(Cmd::PageBreaks { line, breaks }));
        }
        if ui.text_scroll.get_untracked() != y {
            ui.text_scroll.set(y);
        }
    }

    fn open_overlays(&mut self) {
        for kind in [WindowKind::Display, WindowKind::History] {
            if !self.app.windows.is_open(kind) {
                let title = self.app.ui.now_t(kind.title_key());
                let desc =
                    crate::windows::descriptor(kind, &title, &self.app.skin, &self.app.settings);
                self.app.commands.push(WindowCommand::Open {
                    id: kind.id(),
                    settings: desc,
                });
            }
        }
        // The chat panel comes back if it was open at quit.
        if self.app.settings.integrations.bilibili.panel_open {
            self.app.open_chat();
        }
    }
}

fn mount_editor_menu(
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
                cx.dispatch_program_all(Msg::EditorAction { source, action });
            }
        })?;
        Ok(overlay)
    })();
    if result.is_err() {
        let _ = cx.remove_view(overlay);
    }
    result
}

/// Commit application edits before bindings observe their new values. This is
/// also the focus handoff used by mode switches and toolbar operations.
fn sync_editor(
    doc: &mut RuntimeDocument,
    ui: crate::ui::Ui,
    editor: Entity<RichTextEditor>,
    source: Option<Entity<nana_ui::runtime::TextArea>>,
    commands: Vec<crate::app::EditorCommand>,
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
            crate::debug_log(&format!("source edit: {error}"));
        }
    }
    if focus {
        // Visibility bindings must run before focusing the newly shown field.
        if let Err(error) = cx.flush_reactive() {
            crate::debug_log(&format!("editor bindings: {error}"));
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
            crate::app::EditorCommand::Edit(command) => cx.rich_edit(editor, command),
            crate::app::EditorCommand::Insert(fragment) => {
                insert_rich_fragment(cx, ui, editor, &fragment)
            }
        };
        if let Err(error) = result {
            crate::debug_log(&format!("rich edit: {error}"));
        }
    }
}

/// NanaUI exposes text insertion and attributed document replacement separately.
/// Record a single native undo step, then restore the fragment's spans, ruby and
/// objects before reporting the final value to the application.
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
        // Select breaks coalescing with preceding typing without changing it.
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
    // Following typing starts a new undo step and reads the final attributes.
    cx.rich_edit(editor, RichEditCommand::Select(end..end))?;
    Ok(true)
}

#[cfg(test)]
mod input_acceptance;

/// The menu uses the Runtime's editing path with the same OS pasteboard as keys.
struct EditorClipboard;
impl nana_ui_platform::HostServices for EditorClipboard {
    fn set_cursor(&mut self, _: nana_ui_platform::CursorIcon) {}
    fn set_text_input(&mut self, _: Option<&nana_ui_platform::TextInputContext>) {}
    fn read_clipboard(&mut self) -> Result<Option<String>, nana_ui_platform::HostServiceError> {
        let mut clipboard =
            arboard::Clipboard::new().map_err(|_| nana_ui_platform::HostServiceError::Failed)?;
        match clipboard.get_text() {
            Ok(text) => Ok(Some(text)),
            Err(arboard::Error::ContentNotAvailable) => Ok(None),
            Err(_) => Err(nana_ui_platform::HostServiceError::Failed),
        }
    }
    fn write_clipboard(&mut self, text: &str) -> Result<(), nana_ui_platform::HostServiceError> {
        arboard::Clipboard::new()
            .and_then(|mut clipboard| clipboard.set_text(text))
            .map_err(|_| nana_ui_platform::HostServiceError::Failed)
    }
}

/// An image on the clipboard with no text beside it (text pastes normally).
fn clipboard_image() -> Option<Msg> {
    let mut clipboard = arboard::Clipboard::new().ok()?;
    if clipboard.get_text().is_ok_and(|t| !t.is_empty()) {
        return None;
    }
    let image = clipboard.get_image().ok()?;
    Some(Msg::PasteImage {
        width: image.width as u32,
        height: image.height as u32,
        rgba: image.bytes.into_owned(),
    })
}

/// The input window's keys before the editor's own editing: send, recall,
/// hide and the formatting shortcuts. The markup `source` field also pastes
/// clipboard images (the rich editor asks for that itself).
fn key_policy(
    post: RuntimeProgramContext<Msg>,
    source: bool,
) -> impl FnMut(&nana_ui::KeyInput) -> bool + Send + 'static {
    move |key| {
        let Some(message) = input_shortcut(key, source, SEND_KEY.load(Ordering::Relaxed) == 1)
        else {
            return false;
        };
        post.dispatch(message);
        true
    }
}

fn input_shortcut(key: &nana_ui::KeyInput, source: bool, ctrl_enter: bool) -> Option<Msg> {
    if key.state != KeyState::Pressed {
        return None;
    }
    let m = key.modifiers;
    let logical = key.logical.0.as_ref();
    let plain = m.control && !m.shift && !m.alt && !m.meta;
    let unmodified = !m.control && !m.shift && !m.alt && !m.meta;
    let msg = match logical {
        "Enter" if !ctrl_enter && unmodified => Msg::Core(Cmd::Send),
        "Enter" if ctrl_enter && plain => Msg::Core(Cmd::Send),
        "ArrowUp" if plain => Msg::Core(Cmd::Recall(-1)),
        "ArrowDown" if plain => Msg::Core(Cmd::Recall(1)),
        "Escape" if unmodified => Msg::HideInput,
        "b" | "B" if plain => Msg::Tool(Tool::Bold),
        "i" | "I" if plain => Msg::Tool(Tool::Italic),
        "u" | "U" if plain => Msg::Tool(Tool::Underline),
        "v" | "V" if plain && source => clipboard_image()?,
        _ => return None,
    };
    Some(msg)
}

impl RuntimeProgram for Host {
    type Message = Msg;
    type Error = String;

    fn initialize(context: &RuntimeProgramContext<Msg>) -> Result<(Self, Vec<Msg>), String> {
        let app = App::new(context);
        let mut host = Self {
            app,
            perf: crate::perf::Recorder::from_env(),
            docs: HashMap::new(),
            editor: None,
            source: None,
            editor_menu: None,
            editor_fx: None,
            follow: overflow::Follow::default(),
            peer_follow: overflow::Follow::default(),
            pager: overflow::Pager::default(),
            toasts: HashMap::new(),
            stickers: Default::default(),
        };
        // The primary window exists before `initialize_window` is ever
        // called, so its document is built here.
        host.stickers.prepare(
            WindowKind::Input.id(),
            context.gpu(),
            clock::now_ms(),
            host.app.ui.reduced_motion.get_untracked(),
        );
        host.mount(WindowKind::Input.id(), WindowKind::Input)?;
        Ok((host, Vec::new()))
    }

    fn with_document<R>(
        &self,
        id: WindowId,
        f: impl FnOnce(&RuntimeDocument) -> R,
    ) -> Result<Option<R>, DocumentAccessError> {
        Ok(self.docs.get(&id).map(f))
    }

    fn with_document_mut<R>(
        &mut self,
        id: WindowId,
        f: impl FnOnce(&mut RuntimeDocument) -> R,
    ) -> Result<Option<R>, DocumentAccessError> {
        Ok(self.docs.get_mut(&id).map(f))
    }

    fn update(
        &mut self,
        message: Msg,
        _context: &RuntimeProgramContext<Msg>,
    ) -> RuntimeProgramUpdate {
        if let (Some(source), Some(doc)) = (self.source, self.docs.get(&WindowKind::Input.id())) {
            input::capture_source_selection(self.app.ui, doc.context(), source);
        }
        if matches!(&message, Msg::ToggleSource | Msg::HideInput)
            && let Some(menu) = self.editor_menu.take()
            && let Some(doc) = self.docs.get_mut(&WindowKind::Input.id())
        {
            let _ = doc.context_mut().remove_view(menu);
        }
        match message {
            Msg::EditorContext {
                source,
                x,
                y,
                keyboard,
            } => self.open_editor_menu(source, x, y, keyboard),
            Msg::EditorAction { source, action } => self.editor_action(source, action),
            Msg::ToastDismissed(kind, toast) => self.dismiss_toast(kind, toast),
            message => self.app.update(message),
        }
        #[cfg(windows)]
        self.app.spout.prepare(
            &self.app.settings,
            self.app.peer.handle.is_some(),
            _context.gpu(),
        );
        self.finish()
    }

    fn gpu_backend_policy() -> GpuBackendPolicy {
        GpuBackendPolicy::CompositionCapable
    }

    fn theme(&self) -> Arc<CompiledTheme> {
        self.app.theme.clone()
    }

    fn window_material_mode_for(&self, id: WindowId) -> MaterialEffect {
        WindowKind::from_id(id)
            .map(WindowKind::material)
            .unwrap_or(MaterialEffect::Solid)
    }

    fn appearance_backdrop_opacity_for(&self, id: WindowId) -> f32 {
        match WindowKind::from_id(id) {
            Some(WindowKind::Input | WindowKind::Settings | WindowKind::Chat) => 1.0,
            _ => 0.0,
        }
    }

    fn initialize_window(
        &mut self,
        id: WindowId,
        context: &RuntimeProgramContext<Msg>,
    ) -> Result<(), String> {
        let kind = WindowKind::from_id(id).ok_or_else(|| format!("unknown window {id:?}"))?;
        if self.docs.contains_key(&id) {
            return Ok(());
        }
        self.stickers.prepare(
            id,
            context.gpu(),
            clock::now_ms(),
            self.app.ui.reduced_motion.get_untracked(),
        );
        self.mount(id, kind)
    }

    fn discard_window(&mut self, id: WindowId) {
        self.stickers.discard(id);
        self.docs.remove(&id);
        self.toasts.remove(&id);
        crate::ui::shader_effects::collect_garbage();
        crate::ui::sweep::collect_garbage();
    }

    fn window_event(
        &mut self,
        event: WindowEvent,
        context: &RuntimeProgramContext<Msg>,
    ) -> RuntimeProgramUpdate {
        match event {
            WindowEvent::Ready { id, geometry } => {
                if let Some(kind) = WindowKind::from_id(id) {
                    let handle = context.window();
                    self.app.windows.ready(kind, handle.clone());
                    if kind == WindowKind::Chat {
                        self.app.chat_resized(geometry.logical_size.0);
                    }
                    if matches!(
                        kind,
                        WindowKind::Input | WindowKind::Settings | WindowKind::Chat
                    ) {
                        self.app.tint_captions();
                    }
                    self.app.windows.record_geometry(kind, geometry);
                    if kind == WindowKind::Input {
                        crate::debug_log("input window ready");
                        self.open_overlays();
                        let post = self.app.ctx.clone();
                        std::thread::spawn(move || {
                            let hwnd = handle
                                .with_native_handle(|h| match h.as_raw() {
                                    #[cfg(windows)]
                                    raw_window_handle::RawWindowHandle::Win32(w) => w.hwnd.get(),
                                    #[cfg(target_os = "macos")]
                                    raw_window_handle::RawWindowHandle::AppKit(w) => {
                                        // The platform handle is the content view. Activation
                                        // resolves its window on the main thread.
                                        w.ns_view.as_ptr() as isize
                                    }
                                    _ => 0,
                                })
                                .wait()
                                .unwrap_or(0);
                            post.dispatch(Msg::InputHwnd(hwnd));
                        });
                    }
                }
            }
            WindowEvent::Moved { id, geometry } | WindowEvent::Resized { id, geometry } => {
                if let Some(kind) = WindowKind::from_id(id) {
                    self.app.windows.record_geometry(kind, geometry);
                    // An unlocked window keeps the size chosen by the user.
                    let (w, h) = geometry.logical_size;
                    if kind == WindowKind::Chat {
                        self.app.chat_resized(w);
                    }
                    let unlocked = kind
                        .overlay(&self.app.settings)
                        .is_some_and(|o| o.lock != nanatype_settings::Lock::Locked);
                    let current = self.app.window_size(kind);
                    if unlocked
                        && matches!(
                            kind,
                            WindowKind::Display | WindowKind::PeerDisplay | WindowKind::History
                        )
                        && ((current.0 - w).abs() > 1.0 || (current.1 - h).abs() > 1.0)
                    {
                        self.app.edit(|s| {
                            if let Some(o) = kind.overlay_mut(s) {
                                o.size = Some([w, h]);
                            }
                        });
                    }
                }
            }
            WindowEvent::CloseRequested { id } => match WindowKind::from_id(id) {
                Some(WindowKind::Chat) => {
                    self.app.close_chat();
                }
                Some(WindowKind::Settings) | None => {
                    if matches!(
                        self.app.ui.bilibili.get_untracked().account,
                        nanatype_bilibili::AccountState::Qr { .. }
                    ) {
                        self.app.bilibili.cancel_login();
                    }
                    self.app
                        .ui
                        .bilibili
                        .update(|s| std::sync::Arc::make_mut(s).confirm = false);
                    return RuntimeProgramUpdate {
                        window_commands: vec![WindowCommand::Close(id)],
                        ..RuntimeProgramUpdate::default()
                    };
                }
                Some(WindowKind::Input) => {
                    if self.app.settings.general.input_presence == InputPresence::Summon {
                        self.app.summoned = false;
                    } else {
                        self.app.edit(|s| s.overlays.input.visible = false);
                    }
                }
                Some(kind) => self.app.edit(|s| {
                    if let Some(o) = kind.overlay_mut(s) {
                        o.visible = false;
                    }
                }),
            },
            WindowEvent::Closed { id } => {
                if let Some(kind) = WindowKind::from_id(id) {
                    self.app.windows.closed(kind);
                }
                self.docs.remove(&id);
                self.toasts.remove(&id);
            }
            WindowEvent::FileDialogCompleted { result, .. } => {
                self.app.file_chosen(result.id, result.paths);
            }
            WindowEvent::AppearanceChanged { appearance, .. } => {
                self.app.system_dark =
                    matches!(appearance, nana_ui_platform::SystemAppearance::Dark);
                if self.app.settings.skin.chrome == nanatype_settings::ChromeMode::System {
                    self.app.refresh_theme();
                }
            }
            WindowEvent::ReducedMotionChanged { reduced, .. } => {
                self.app.ui.system_reduced_motion.set(reduced);
                self.app.sync_reduced_motion();
            }
            _ => {}
        }
        self.finish()
    }

    fn input_event(
        &mut self,
        id: WindowId,
        input: RoutedInput<'_>,
        _: &RuntimeProgramContext<Msg>,
    ) -> Result<RuntimeProgramUpdate, FrameworkError> {
        if WindowKind::from_id(id) == Some(WindowKind::Input)
            && let Some(doc) = self.docs.get(&id)
            && let Some(width) = input::changed_sidebar_width(self.app.ui, doc.context())
        {
            self.app.ui.input_sidebar_width.set(width);
            self.app.windows.geometry.input_sidebar_width = Some(width);
            self.app.windows.geometry_dirty = true;
            self.app.save_geometry();
        }
        if let Some(kind) = WindowKind::from_id(id)
            && let Some(settings) = kind.overlay(&self.app.settings)
            && let Some(command) = overlay::command(
                kind,
                settings.lock,
                &input.event.payload,
                input.disposition.handled,
                self.app.windows.frame(kind),
            )
        {
            if let WindowCommand::SetBounds { position, size, .. } = &command {
                let (position, size) = (*position, *size);
                // Record immediately so consecutive wheel events accumulate
                // before the native resize notification arrives.
                self.app.windows.moved(kind, Some(position), size);
                self.app.edit(|s| {
                    kind.overlay_mut(s).expect("overlay").size = Some([size.0, size.1]);
                });
            }
            self.app.commands.push(command);
            return Ok(self.finish());
        }
        let InputPayload::Key(key) = &input.event.payload else {
            return Ok(RuntimeProgramUpdate::default());
        };
        if key.state != KeyState::Pressed {
            return Ok(RuntimeProgramUpdate::default());
        }
        if self.app.ui.recording.get_untracked().is_some() {
            let code = key.physical.0.as_ref();
            if code == "Escape" {
                self.app.update(Msg::RecordHotkey(None));
                return Ok(self.finish());
            }
            if !is_modifier(code) {
                let m = key.modifiers;
                let hotkey = Hotkey {
                    ctrl: m.control,
                    shift: m.shift,
                    alt: m.alt,
                    meta: m.meta,
                    key: code.to_owned(),
                };
                if !hotkey.is_safe_global() {
                    let text = self.app.ui.now_t("hotkeys-unsafe");
                    self.app.notice(text, true);
                    return Ok(self.finish());
                }
                self.app.update(Msg::HotkeyCaptured(hotkey));
                return Ok(self.finish());
            }
        }
        if WindowKind::from_id(id) == Some(WindowKind::Input)
            && key.logical.0 == "Escape"
            && key.modifiers == nana_ui_platform::InputModifiers::default()
            && !input.disposition.handled
        {
            self.app.update(Msg::HideInput);
            return Ok(self.finish());
        }
        Ok(RuntimeProgramUpdate::default())
    }

    fn scene_gpu_renderers(&self, _: WindowId) -> Option<nana_ui::SceneGpuRendererRegistry> {
        Some(crate::ui::sweep::renderers())
    }

    fn host_textures(&self, id: WindowId) -> Option<nana_ui::HostTextureRegistry> {
        self.stickers.registry(id)
    }

    fn window_output(&self, id: WindowId) -> Option<nana_ui::WindowOutputConfig> {
        let kind = WindowKind::from_id(id)?;
        self.app
            .spout
            .config(kind, &self.app.settings, self.app.peer.handle.is_some())
    }

    fn window_frame_presented(
        &mut self,
        id: WindowId,
        _: &RuntimeProgramContext<Msg>,
    ) -> RuntimeProgramUpdate {
        crate::ui::shader_effects::collect_garbage();
        crate::ui::sweep::collect_garbage();
        if let Some(perf) = &mut self.perf {
            perf.frame(id, false, self.docs.get(&id));
        }
        RuntimeProgramUpdate::default()
    }

    fn window_output_frame(
        &mut self,
        id: WindowId,
        frame: &nana_ui::WindowOutputFrame,
        context: &RuntimeProgramContext<Msg>,
    ) {
        let Some(kind) = WindowKind::from_id(id) else {
            return;
        };
        let paused = self.app.spout.paused_output(kind, clock::now_ms()).cloned();
        let Some(out) =
            crate::output::resolve(kind, &self.app.settings, self.app.peer.handle.is_some())
                .output()
                .or(paused.as_ref())
        else {
            return;
        };
        if let Some(perf) = &mut self.perf {
            perf.frame(id, true, self.docs.get(&id));
        }
        if let Some((key, detail)) = self.app.spout.frame(kind, out, frame, context.gpu()) {
            crate::debug_log(&format!("spout {kind:?}: {detail}"));
            let locale = self.app.ui.locale.get_untracked();
            let state = crate::ui::link_text::spout_send(locale, key);
            if self.app.spout.status.get(&kind) != Some(&state) {
                self.app.spout.status.insert(kind, state);
                self.app.refresh_spout_status();
                context.dispatch(Msg::UiChanged);
            }
        }
        if paused.is_some()
            && self
                .app
                .spout
                .paused_output(kind, clock::now_ms())
                .is_none()
        {
            context.dispatch(Msg::OutputCleanup);
        }
    }

    fn window_output_status(
        &mut self,
        id: WindowId,
        status: nana_ui::WindowOutputStatus,
        context: &RuntimeProgramContext<Msg>,
    ) {
        if let Some(kind) = WindowKind::from_id(id) {
            crate::debug_log(&format!("output {kind:?}: {status:?}"));
            let locale = self.app.ui.locale.get_untracked();
            let previous = self.app.ui.status.get_untracked();
            if crate::output::resolve(kind, &self.app.settings, self.app.peer.handle.is_some())
                .output()
                .is_some()
            {
                self.app.spout.status_changed(kind, &status, locale);
            }
            self.app.refresh_spout_status();
            if !Arc::ptr_eq(&previous, &self.app.ui.status.get_untracked()) {
                context.dispatch(Msg::UiChanged);
            }
        }
    }

    fn frame_demand(&self, id: WindowId) -> FrameDemand {
        let demand = frame_policy(
            self.app.ui,
            WindowKind::from_id(id),
            clock::now_ms(),
            &self.follow,
            &self.peer_follow,
            &self.pager,
        )
        .demand;
        match (
            demand,
            self.stickers.next_at(
                id,
                clock::now_ms(),
                self.app.ui.reduced_motion.get_untracked(),
            ),
        ) {
            (FrameDemand::Continuous(fps), _) => FrameDemand::Continuous(fps),
            (FrameDemand::At(at), Some(sticker)) => {
                FrameDemand::At(at.min(clock::instant(sticker)))
            }
            (_, Some(sticker)) => FrameDemand::At(clock::instant(sticker)),
            (demand, None) => demand,
        }
    }

    fn prepare_window_frame(&mut self, id: WindowId, context: &RuntimeProgramContext<Msg>) {
        #[cfg(windows)]
        self.app.spout.prepare(
            &self.app.settings,
            self.app.peer.handle.is_some(),
            context.gpu(),
        );
        if let Some(perf) = &mut self.perf {
            perf.begin(id);
        }
        let now = clock::now_ms();
        self.stickers.prepare(
            id,
            context.gpu(),
            now,
            self.app.ui.reduced_motion.get_untracked(),
        );
        if self.stickers.take_rebuild() {
            context.dispatch(Msg::AssetsChanged);
        }
        match WindowKind::from_id(id) {
            Some(WindowKind::Display) => self.follow_track(now, false),
            Some(WindowKind::PeerDisplay) if self.app.peer.handle.is_some() => {
                self.follow_track(now, true)
            }
            _ => {}
        }
        let policy = frame_policy(
            self.app.ui,
            WindowKind::from_id(id),
            now,
            &self.follow,
            &self.peer_follow,
            &self.pager,
        );
        let changed = self.docs.get(&id).is_some_and(|doc| {
            let cx = doc.context();
            cx.has_pending_reactive() || cx.world().has_pending_work()
        });
        policy.advance(now, changed);
    }
}

/// Read the same pending work the runtime will drain, including direct editor
/// commands and toasts. This avoids a second, incomplete message-to-window map.
fn pending_redraw(docs: &HashMap<WindowId, RuntimeDocument>) -> nana_ui::RuntimeRedraw {
    nana_ui::RuntimeRedraw::for_windows(docs.iter().filter_map(|(id, doc)| {
        let cx = doc.context();
        (cx.has_pending_reactive() || cx.world().has_pending_work()).then_some(*id)
    }))
}

#[cfg(test)]
mod redraw_tests {
    use super::*;
    use nana_ui::runtime::view::{signal, widget};
    use nana_ui::runtime::{LayoutViewport, Text};

    #[test]
    fn input_shortcuts_match_exact_chords_and_leave_selection_keys_alone() {
        use nana_ui_platform::{InputModifiers, KeyInput};
        let plain = InputModifiers::default();
        let ctrl = InputModifiers {
            control: true,
            ..plain
        };
        let key =
            |name, modifiers| KeyInput::named("Unidentified", name, KeyState::Pressed, modifiers);
        assert!(matches!(
            input_shortcut(&key("Enter", plain), false, false),
            Some(Msg::Core(Cmd::Send))
        ));
        assert!(
            input_shortcut(
                &key(
                    "Enter",
                    InputModifiers {
                        shift: true,
                        ..plain
                    }
                ),
                false,
                false
            )
            .is_none()
        );
        assert!(matches!(
            input_shortcut(&key("Enter", ctrl), false, true),
            Some(Msg::Core(Cmd::Send))
        ));
        assert!(matches!(
            input_shortcut(&key("ArrowUp", ctrl), false, false),
            Some(Msg::Core(Cmd::Recall(-1)))
        ));
        for source in [false, true] {
            for ctrl_enter in [false, true] {
                for modifiers in [
                    InputModifiers {
                        shift: true,
                        ..ctrl
                    },
                    InputModifiers { alt: true, ..ctrl },
                    InputModifiers { meta: true, ..ctrl },
                ] {
                    for name in ["ArrowUp", "ArrowDown", "Enter", "b", "i", "u"] {
                        assert!(
                            input_shortcut(&key(name, modifiers), source, ctrl_enter).is_none(),
                            "{name} {modifiers:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn gpu_only_frames_leave_application_clocks_idle_and_keep_tracks_independent() {
        let mut skin = nanatype_theme::Skin::default();
        skin.dialogue.sweep.enabled = true;
        let ui = crate::ui::Ui::new(
            Arc::new(nanatype_settings::Settings::default()),
            Arc::new(skin),
            nanatype_i18n::Locale::ZhCn,
        );
        ui.display.update(|view| Arc::make_mut(view).empty = false);
        ui.peer_display.set(ui.display.get_untracked());
        ui.now.set(123);
        ui.peer_now.set(456);
        let follow = overflow::Follow::default();
        let pager = overflow::Pager::default();
        let now = 1_000_000_000;
        let policy = |kind| frame_policy(ui, Some(kind), now, &follow, &follow, &pager);
        let local = policy(WindowKind::Display);
        assert!(matches!(local.demand, FrameDemand::Continuous(_)));
        assert!(!local.tick_clock);
        local.advance(now, false);
        assert_eq!(ui.now.get_untracked(), 123);
        local.advance(now, true);
        assert_eq!(ui.now.get_untracked(), now);
        assert_eq!(ui.peer_now.get_untracked(), 456);
        let peer = policy(WindowKind::PeerDisplay);
        assert!(matches!(peer.demand, FrameDemand::OnDemand));
        peer.advance(now, true);
        assert_eq!(ui.peer_now.get_untracked(), 456);
        ui.peer.update(|peer| Arc::make_mut(peer).active = true);
        let peer = policy(WindowKind::PeerDisplay);
        assert!(matches!(peer.demand, FrameDemand::Continuous(_)));
        peer.advance(now, false);
        assert_eq!(ui.peer_now.get_untracked(), 456);
        ui.reduced_motion.set(true);
        assert!(matches!(
            policy(WindowKind::Display).demand,
            FrameDemand::OnDemand
        ));
    }

    #[test]
    fn completed_box_motion_sleeps_until_the_followers_next_line() {
        let ui = crate::ui::Ui::new(
            Arc::new(nanatype_settings::Settings::default()),
            Arc::new(nanatype_theme::Skin::default()),
            nanatype_i18n::Locale::ZhCn,
        );
        let now = 1_000_000_000;
        ui.display.update(|d| {
            let d = Arc::make_mut(d);
            d.empty = false;
            d.fade = Some((now, 100));
        });
        let mut follow = overflow::Follow::default();
        let peer_follow = overflow::Follow::default();
        let mut pager = overflow::Pager::default();
        let policy = frame_policy(
            ui,
            Some(WindowKind::Display),
            now,
            &follow,
            &peer_follow,
            &pager,
        );
        assert!(policy.tick_clock && matches!(policy.demand, FrameDemand::Continuous(_)));
        let now = now + 200;
        assert!(matches!(
            frame_policy(
                ui,
                Some(WindowKind::Display),
                now,
                &follow,
                &peer_follow,
                &pager
            )
            .demand,
            FrameDemand::OnDemand
        ));
        let key = PageKey {
            line: Some(nanatype_model::LineId(1)),
            visible_from: 0,
        };
        let glyphs = vec![
            overflow::Glyph {
                byte: 0,
                unit: Some(0),
                at: Some(0),
            },
            overflow::Glyph {
                byte: 1,
                unit: Some(1),
                at: Some(now + 100),
            },
        ];
        let measure = Measure {
            lines: vec![
                LineSpan {
                    bytes: 0..1,
                    top: 0.0,
                    bottom: 40.0,
                },
                LineSpan {
                    bytes: 1..2,
                    top: 40.0,
                    bottom: 80.0,
                },
            ],
            viewport: 40.0,
            glyphs: &glyphs,
        };
        follow.frame(Some(key), Some(&measure), now, false);
        let policy = frame_policy(
            ui,
            Some(WindowKind::Display),
            now,
            &follow,
            &peer_follow,
            &pager,
        );
        assert!(!policy.tick_clock);
        assert!(matches!(policy.demand, FrameDemand::At(at) if at == clock::instant(now + 100)));
        pager.frame(Some(key), None);
        let policy = frame_policy(
            ui,
            Some(WindowKind::Display),
            now,
            &follow,
            &peer_follow,
            &pager,
        );
        assert!(matches!(policy.demand, FrameDemand::Continuous(_)) && !policy.tick_clock);
    }

    #[test]
    fn local_signal_changes_redraw_one_window_global_changes_redraw_both() {
        let local = signal(0);
        let global = signal(0);
        let mut docs = HashMap::new();
        let mut shaper = nana_ui::NanaTextShaper::default();
        for n in 1..=2 {
            let id = DocumentId::new(n).unwrap();
            let mut doc = RuntimeDocument::new(id);
            doc.context_mut()
                .mount_view_root(id, move || {
                    widget(Text::new("")).bind(move |t: &mut Text| {
                        t.value =
                            format!("{} {}", if n == 1 { local.get() } else { 0 }, global.get());
                    })
                })
                .unwrap();
            doc.flush(LayoutViewport::new(200.0, 100.0), &mut shaper)
                .unwrap();
            docs.insert(WindowId(n), doc);
        }
        assert_eq!(pending_redraw(&docs), nana_ui::RuntimeRedraw::None);
        local.set(1);
        assert_eq!(
            pending_redraw(&docs),
            nana_ui::RuntimeRedraw::Window(WindowId(1))
        );
        for doc in docs.values_mut() {
            doc.flush(LayoutViewport::new(200.0, 100.0), &mut shaper)
                .unwrap();
        }
        global.set(1);
        assert_eq!(
            pending_redraw(&docs),
            nana_ui::RuntimeRedraw::Windows(vec![WindowId(1), WindowId(2)])
        );
    }
}
