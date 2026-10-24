//! The NanaUI host program: owns one document per window and routes window
//! events, input and messages to [`App`].

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, Instant};

use nana_ui::runtime::view::{entity_ref, with_refs};
use nana_ui::runtime::{
    DocumentId, Entity, FrameworkError, RichTextEditor, RuntimeDocument, Toast, ToastStack,
};
use nana_ui::{
    CompiledTheme, DocumentAccessError, FrameDemand, GpuBackendPolicy, MaterialEffect, RoutedInput,
    RuntimeProgram, RuntimeProgramContext, RuntimeProgramUpdate,
};
use nana_ui_platform::host::WindowCommand;
use nana_ui_platform::{InputPayload, KeyState, WindowEvent, WindowId};
use nanatype_core::Cmd;
use nanatype_model::Overflow;
use nanatype_settings::{Hotkey, InputKey, InputKeys, InputPresence};

use crate::app::{App, INPUT_KEYS};
use crate::clock;
#[cfg(test)]
use crate::msg::Tool;
use crate::msg::{Msg, Shortcut, input_message};
use crate::services::sounds::Keystroke;
use crate::ui::overflow::{self, Measure, PageKey};
use crate::ui::{Ui, danmaku, dialogue, history, input, settings};
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
    /// The `@` / `/` list was placed from a stale layout: lay out once
    /// more. Retries are bounded, should the texts never agree.
    completion_unsettled: bool,
    completion_retries: u8,
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
    /// Language and forced reduced motion each document was last given;
    /// re-sent only when they change (or to a new window), not per message.
    doc_env: HashMap<WindowId, (nanatype_i18n::Locale, bool)>,
    /// The last keystroke click, and whether a key press (rather than IME
    /// text) played it.
    last_keystroke: Option<(Instant, bool)>,
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
    let skin_fps = ui
        .settings
        .with_untracked(|s| s.skin.frame_rate)
        .clamp(30, 60);
    let (clock, tick_clock, render_only, next_at, fps) = match kind {
        Some(WindowKind::Display | WindowKind::PeerDisplay) => {
            let remote = kind == Some(WindowKind::PeerDisplay);
            if remote && !ui.peer.get_untracked().active {
                return FramePolicy::default();
            }
            let track = if remote { ui.remote() } else { ui };
            let follow = if remote { peer_follow } else { follow };
            let typing = dialogue::display_animating(*track, now);
            let movement = crate::ui::sweep::animating(*track)
                || crate::ui::text_light::animating(*track, now)
                || crate::ui::text_scramble::animating(*track, now)
                || follow.wants_frame()
                || (!remote && pager.wants_frame());
            (
                track.now,
                typing,
                movement || crate::ui::shader_effects::animating(*track),
                follow.next_line_at(),
                skin_fps,
            )
        }
        Some(WindowKind::History) => {
            let fade = history::fade_schedule(*ui, now);
            (ui.history_now, fade.animating, false, fade.next_at, 60)
        }
        Some(WindowKind::Settings) => {
            let preview = crate::ui::preview::animating(*ui, now);
            (
                ui.settings_pulse_ms,
                settings::links_pulsing(ui),
                preview,
                None,
                if preview { skin_fps } else { 30 },
            )
        }
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

/// Frames the `@` / `/` list may be laid out again to settle its place.
const COMPLETION_RETRIES: u8 = 3;

/// How close IME text must follow a click to count as the same keystroke.
const KEYSTROKE_DEDUP: Duration = Duration::from_millis(40);

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
    /// Click for a key pressed in the input window. Held keys click once.
    /// Keys an IME swallows may never arrive as presses, so preedit changes
    /// and commits click too, unless a press just did.
    fn keystroke_sound(&mut self, payload: &InputPayload) {
        if !self.app.settings.sound.keystroke {
            return;
        }
        let now = Instant::now();
        // A press skips only an IME click it follows; IME text skips any.
        let recent = |any: bool| {
            self.last_keystroke
                .is_some_and(|(at, key)| (any || !key) && now - at < KEYSTROKE_DEDUP)
        };
        let key = match payload {
            InputPayload::Key(key) => {
                if key.state != KeyState::Pressed
                    || key.repeat
                    || is_modifier(key.physical.0.as_ref())
                    || recent(false)
                {
                    return;
                }
                self.last_keystroke = Some((now, true));
                Keystroke::of(key.logical.0.as_ref())
            }
            // Plain text always follows its own key press, repeats included.
            InputPayload::Composition(
                nana_ui_platform::CompositionInput::Update { .. }
                | nana_ui_platform::CompositionInput::Commit(_),
            ) => {
                if recent(true) {
                    return;
                }
                self.last_keystroke = Some((now, false));
                Keystroke::Normal
            }
            _ => return,
        };
        self.app.keystroke(key);
    }

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
        // Text is cheap to probe; an image is not (arboard decodes the whole
        // picture), so a non-text clipboard keeps Paste enabled and the paste
        // itself finds out.
        let paste_available = arboard::Clipboard::new()
            .is_ok_and(|mut clipboard| clipboard.get_text().map_or(true, |t| !t.is_empty()));
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
            nanatype_diag::report!(crate::diag::EDIT_FAILED, kind = 0u64; "{error}");
        }
    }

    /// Open the Spout senders before rasterizing. A sender that fails waits
    /// before its next try, and the settings page says why.
    #[cfg(windows)]
    fn prepare_outputs(&mut self, context: &RuntimeProgramContext<Msg>) {
        let locale = self.app.ui.locale.get_untracked();
        self.app.spout.prepare(
            &self.app.settings,
            self.app.peer.handle.is_some(),
            context.gpu(),
            locale,
        );
        self.spout_status_changed(context);
    }

    fn spout_status_changed(&mut self, context: &RuntimeProgramContext<Msg>) {
        if self.app.spout.take_status_changed() {
            self.app.refresh_spout_status();
            context.dispatch(Msg::UiChanged);
        }
    }

    fn finish(&mut self) -> RuntimeProgramUpdate {
        crate::ui::collect_effect_garbage();
        for message in crate::ui::shader_effects::take_notices() {
            self.app.notice(message, true);
        }
        self.drive_editor();
        let locale = self.app.ui.locale.get_untracked();
        // The app's reduce-motion setting wins over the system's in every
        // window's runtime, so view transitions and toasts land at once too.
        let forced = self.app.settings.playback.reduced_motion;
        self.doc_env.retain(|id, _| self.docs.contains_key(id));
        for (id, doc) in &mut self.docs {
            if self.doc_env.get(id) == Some(&(locale, forced)) {
                continue;
            }
            self.doc_env.insert(*id, (locale, forced));
            let cx = doc.context_mut();
            nanatype_frame::apply_locale(cx, locale);
            if forced {
                cx.set_reduced_motion(true);
            } else {
                cx.follow_system_reduced_motion();
            }
        }
        self.show_toasts();
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
                shown |= request.push(doc, stack, move |toast| Msg::ToastDismissed(kind, toast));
            }
            // No window to show it in yet (a notice at start-up): keep it
            // for one that opens while it is still fresh.
            if !shown && request.fresh() {
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
        nanatype_frame::apply_locale(context, ui.locale.get_untracked());
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
                    .on_key(editor, key_policy(self.app.ctx.clone(), ui, false))
                    .map_err(err)?;
                context
                    .on_key(source, key_policy(self.app.ctx.clone(), ui, true))
                    .map_err(err)?;
                self.editor = Some(editor);
                self.source = Some(source);
                self.editor_fx = None;
                self.toasts.insert(id, stack);
            }
        }
        self.doc_env.remove(&id);
        self.docs.insert(id, doc);
        Ok(())
    }

    /// Hang the rich editor's `@` / `/` list under the word it completes,
    /// read from the editor's last layout; and close the app's list when
    /// the markup source's own popup was closed.
    fn anchor_completion(&mut self) {
        let ui = self.app.ui;
        if let (Some(doc), Some(source)) = (self.docs.get(&WindowKind::Input.id()), self.source)
            && input::completion::source_dismissed(ui, doc.context().world(), source.stable_id())
        {
            self.app.post(Msg::Complete(crate::msg::Complete::Dismiss));
        }
        let anchor = self.completion_anchor();
        // Placed from a stale layout: lay out once more and place again.
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

    fn completion_anchor(&self) -> Option<input::completion::Anchor> {
        let world = self.docs.get(&WindowKind::Input.id())?.context().world();
        input::completion::anchor(self.app.ui, world, self.editor?.stable_id())
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
        // Retirement freezes the visible viewport as well as the glyph clock.
        if view.text_exit.is_some() {
            return;
        }
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
            let vertical = ui
                .canvas()
                .vertical_text(&self.app.settings, &self.app.skin);
            if layout.is_vertical() != vertical {
                return None;
            }
            // A layout of an older node until the new one is laid out.
            let end = layout.lines.last()?.source.end;
            if end < page.text_trimmed || end > page.text_len {
                return None;
            }
            Some(Measure {
                vertical,
                // TextLayout stores line-relative axes even for vertical writing.
                lines: overflow::line_spans(layout),
                viewport: {
                    let port = world.layout_box(ui.text_port.get_untracked()?)?;
                    if vertical { port.width } else { port.height }
                },
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
            nanatype_diag::report!(crate::diag::EDIT_FAILED, kind = 1u64; "{error}");
        }
    }
    if focus {
        // Visibility bindings must run before focusing the newly shown field.
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
            crate::app::EditorCommand::Edit(command) => cx.rich_edit(editor, command),
            crate::app::EditorCommand::Insert(fragment) => {
                insert_rich_fragment(cx, ui, editor, &fragment)
            }
            crate::app::EditorCommand::Delete => delete_rich_selection(cx, ui, editor),
        };
        if let Err(error) = result {
            nanatype_diag::report!(crate::diag::EDIT_FAILED, kind = 3u64; "{error}");
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

/// NanaUI has no delete command: typing over the selection records the undo
/// step, then the typed character goes too.
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
    // The caret first, so it never lies past the shortened text.
    cx.rich_edit(editor, RichEditCommand::Select(start..start))?;
    cx.update_component(editor, |e, cx| {
        e.value.replace_range(start..start + 1, "");
        cx.emit(RichTextEditorEvent::Changed(e.value.clone()));
    })?;
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

/// The input window's keys before the editor's own editing: the `@` / `/`
/// list's keys while it is open, then the bindings of Settings › Hotkeys.
/// The markup `source` field also pastes clipboard images (the rich editor
/// asks for that itself).
fn key_policy(
    post: RuntimeProgramContext<Msg>,
    ui: Ui,
    source: bool,
) -> impl FnMut(&nana_ui::KeyInput) -> bool + Send + 'static {
    move |key| {
        let completing = ui.completion.with_untracked(Option::is_some);
        let action = {
            let keys = INPUT_KEYS.read().unwrap_or_else(|e| e.into_inner());
            input_key(key, source, completing, &keys)
        };
        let Some((message, consumed)) = action else {
            return false;
        };
        post.dispatch(message);
        consumed
    }
}

/// What a key in the input editor does before the editor's own editing: a
/// message, and whether the key stops there.
fn input_key(
    key: &nana_ui::KeyInput,
    source: bool,
    completing: bool,
    keys: &InputKeys,
) -> Option<(Msg, bool)> {
    if completing {
        if source && moves_caret(key) {
            // The markup field reports no caret moves: close the list rather
            // than leave it on a token the caret has left.
            return Some((Msg::Complete(crate::msg::Complete::Dismiss), false));
        }
        match completion_key(key) {
            // The markup field's own popup closes too.
            Some(message @ Msg::Complete(crate::msg::Complete::Dismiss)) if source => {
                return Some((message, false));
            }
            // It chooses and fills in itself, ahead of the bindings.
            Some(_) if source => return None,
            Some(message) => return Some((message, true)),
            None => {}
        }
    }
    input_shortcut(key, source, keys).map(|m| (m, true))
}

/// A key press as a chord. Letters and digits follow the logical key, so a
/// binding means the same letter on any layout; other keys are named as the
/// W3C code names them.
fn chord_of(key: &nana_ui::KeyInput) -> Hotkey {
    let m = key.modifiers;
    let logical = key.logical.0.as_ref();
    let mut chars = logical.chars();
    let name = match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphabetic() => format!("Key{}", c.to_ascii_uppercase()),
        (Some(c), None) if c.is_ascii_digit() => format!("Digit{c}"),
        (Some(' '), None) => "Space".to_owned(),
        (Some(_), Some(_)) if logical != "Unidentified" => logical.to_owned(),
        _ => key.physical.0.as_ref().to_owned(),
    };
    Hotkey {
        ctrl: m.control,
        shift: m.shift,
        alt: m.alt,
        meta: m.meta,
        key: name,
    }
}

/// Enter and Shift+Enter already break the line in the editor.
fn editor_breaks_line(chord: &Hotkey) -> bool {
    chord.key == "Enter" && !chord.ctrl && !chord.alt && !chord.meta
}

/// The default keys, or the Ctrl+Enter-sends layout.
#[cfg(test)]
fn test_keys(ctrl_enter: bool) -> InputKeys {
    let mut keys = InputKeys::default();
    if ctrl_enter {
        keys.send = Some(Hotkey::ctrl("Enter"));
        keys.newline = Some(Hotkey::bare("Enter"));
    }
    keys
}

/// The open completion list's keys, ahead of every binding: Tab or Enter
/// fills in, ↑/↓ (and Shift+Tab) choose, Esc closes.
fn completion_key(key: &nana_ui::KeyInput) -> Option<Msg> {
    use crate::msg::Complete;
    let m = key.modifiers;
    if key.state != KeyState::Pressed || m.control || m.alt || m.meta {
        return None;
    }
    let action = match (chord_of(key).key.as_str(), m.shift) {
        ("Tab", false) | ("Enter", false) => Complete::Accept(None),
        ("Tab", true) | ("ArrowUp", false) => Complete::Move(-1),
        ("ArrowDown", false) => Complete::Move(1),
        ("Escape", false) => Complete::Dismiss,
        _ => return None,
    };
    Some(Msg::Complete(action))
}

/// Keys that move the caret without typing.
fn moves_caret(key: &nana_ui::KeyInput) -> bool {
    key.state == KeyState::Pressed
        && matches!(
            chord_of(key).key.as_str(),
            "ArrowLeft" | "ArrowRight" | "Home" | "End" | "PageUp" | "PageDown"
        )
}

fn input_shortcut(key: &nana_ui::KeyInput, source: bool, keys: &InputKeys) -> Option<Msg> {
    if key.state != KeyState::Pressed {
        return None;
    }
    let chord = chord_of(key);
    match keys.matching(&chord) {
        Some(InputKey::Newline) if editor_breaks_line(&chord) => None,
        Some(action) => Some(input_message(action)),
        None if source && chord == Hotkey::ctrl("KeyV") => clipboard_image(),
        None => None,
    }
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
            completion_unsettled: false,
            completion_retries: COMPLETION_RETRIES,
            editor_fx: None,
            follow: overflow::Follow::default(),
            peer_follow: overflow::Follow::default(),
            pager: overflow::Pager::default(),
            toasts: HashMap::new(),
            stickers: Default::default(),
            doc_env: HashMap::new(),
            last_keystroke: None,
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
        self.prepare_outputs(_context);
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
        crate::ui::collect_effect_garbage();
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
                    // Broadcast windows have no caption, but macOS still
                    // gives a frameless window its traffic lights.
                    if matches!(
                        kind,
                        WindowKind::Display | WindowKind::PeerDisplay | WindowKind::History
                    ) {
                        self.app
                            .commands
                            .push(WindowCommand::SetNativeWindowControlsVisible {
                                id,
                                visible: false,
                                duration: std::time::Duration::ZERO,
                            });
                    }
                    self.app.windows.record_geometry(kind, geometry);
                    if kind == WindowKind::Input {
                        nanatype_diag::event!(
                            crate::diag::INPUT_READY,
                            since_launch_ms = crate::clock::now_ms()
                        );
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
                        let profile = crate::ui::settings::preview::load_draft(
                            &self.app.settings,
                            &self.app.settings.skin.id,
                            self.app.ui,
                        )
                        .profile(&self.app.skin)
                        .ok();
                        self.app
                            .edit_geometry(|s| kind.remember_size(s, [w, h], profile));
                    }
                }
            }
            WindowEvent::CloseRequested { id } => match WindowKind::from_id(id) {
                Some(WindowKind::Chat) => {
                    self.app.close_chat();
                }
                Some(WindowKind::Settings) | None => {
                    self.app.unofficial_settings_closed();
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
                if id == WindowKind::Settings.id() {
                    self.app.ui.skin_drafts.set(HashMap::new());
                }
                if let Some(kind) = WindowKind::from_id(id) {
                    self.app.windows.closed(kind);
                }
                // The native handle dies with the window; hotkey activation
                // must not touch it afterwards.
                if id == WindowKind::Input.id() {
                    self.app.hotkeys.set_input_window(0);
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
            && let Some(position) = overlay::menu_position(
                kind,
                settings.lock,
                &input.event.payload,
                input.disposition.handled,
            )
            && let Some(handle) = self.app.windows.handle(kind)
            && let Some(menu) = crate::services::overlay_menu::Snapshot::new(kind, self.app.ui)
        {
            crate::services::overlay_menu::show(menu, handle, position, self.app.ctx.clone());
            return Ok(RuntimeProgramUpdate::default());
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
                self.app.edit_geometry(|s| {
                    kind.overlay_mut(s).expect("overlay").size = Some([size.0, size.1]);
                });
            }
            self.app.commands.push(command);
            return Ok(self.finish());
        }
        if WindowKind::from_id(id) == Some(WindowKind::Input) {
            self.keystroke_sound(&input.event.payload);
            if let Some(doc) = self.docs.get(&id) {
                input::sync_composing(self.app.ui, doc.context(), doc.document());
            }
        }
        let InputPayload::Key(key) = &input.event.payload else {
            return Ok(RuntimeProgramUpdate::default());
        };
        if key.state != KeyState::Pressed {
            return Ok(RuntimeProgramUpdate::default());
        }
        if let Some(target) = self.app.ui.recording.get_untracked() {
            let code = key.physical.0.as_ref();
            if code == "Escape" {
                self.app.update(Msg::RecordHotkey(None));
                return Ok(self.finish());
            }
            if !is_modifier(code) {
                let hotkey = match target {
                    // System-wide hotkeys are registered by physical key.
                    Shortcut::Global(_) => {
                        let m = key.modifiers;
                        Hotkey {
                            ctrl: m.control,
                            shift: m.shift,
                            alt: m.alt,
                            meta: m.meta,
                            key: code.to_owned(),
                        }
                    }
                    Shortcut::Input(_) => chord_of(key),
                };
                if !target.is_safe(&hotkey) {
                    let text = self.app.ui.now_t(match target {
                        Shortcut::Global(_) => "hotkeys-unsafe",
                        Shortcut::Input(_) => "hotkeys-unsafe-local",
                    });
                    self.app.notice(text, true);
                    return Ok(self.finish());
                }
                self.app.update(Msg::HotkeyCaptured(hotkey));
                return Ok(self.finish());
            }
        }
        // The editor did not take the hide key (no editor focused).
        if WindowKind::from_id(id) == Some(WindowKind::Input)
            && !input.disposition.handled
            && self.app.settings.hotkeys.input.hide.as_ref() == Some(&chord_of(key))
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
        crate::ui::collect_effect_garbage();
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
            nanatype_diag::report!(crate::diag::SPOUT_FAILED, window = crate::diag::window(kind); "{detail}");
            let locale = self.app.ui.locale.get_untracked();
            let state = crate::ui::link_text::spout_send(locale, key);
            if self.app.spout.status.get(&kind) != Some(&state) {
                self.app.spout.status.insert(kind, state);
                self.app.refresh_spout_status();
                context.dispatch(Msg::UiChanged);
            }
        }
        self.spout_status_changed(context);
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
            nanatype_diag::report!(crate::diag::WINDOW_OUTPUT, window = crate::diag::window(kind); "{status:?}");
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
        if WindowKind::from_id(id) == Some(WindowKind::Input) {
            if self.completion_unsettled {
                return FrameDemand::At(Instant::now());
            }
            // The skin preview's programs read the wall clock: frames only.
            let ui = *self.app.ui;
            if self.app.input_visible()
                && (crate::ui::shader_effects::input_animating(ui)
                    || crate::ui::sweep::input_animating(ui))
            {
                let fps = ui
                    .settings
                    .with_untracked(|s| s.skin.frame_rate)
                    .clamp(30, 60);
                return FrameDemand::Continuous(NonZeroU32::new(fps).expect("non-zero"));
            }
        }
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
        self.prepare_outputs(context);
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
            Some(WindowKind::Input) => self.anchor_completion(),
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
        let shift = InputModifiers {
            shift: true,
            ..plain
        };
        let key =
            |name, modifiers| KeyInput::named("Unidentified", name, KeyState::Pressed, modifiers);
        let enter = test_keys(false);
        let ctrl_enter = test_keys(true);
        assert!(matches!(
            input_shortcut(&key("Enter", plain), false, &enter),
            Some(Msg::Core(Cmd::Send))
        ));
        // Shift+Enter is the newline key, and the editor breaks the line.
        assert!(input_shortcut(&key("Enter", shift), false, &enter).is_none());
        assert!(matches!(
            input_shortcut(&key("Enter", ctrl), false, &ctrl_enter),
            Some(Msg::Core(Cmd::Send))
        ));
        assert!(input_shortcut(&key("Enter", plain), false, &ctrl_enter).is_none());
        assert!(matches!(
            input_shortcut(&key("ArrowUp", ctrl), false, &enter),
            Some(Msg::Core(Cmd::Recall(-1)))
        ));
        assert!(matches!(
            input_shortcut(&key("B", ctrl), false, &enter),
            Some(Msg::Tool(Tool::Bold))
        ));
        for source in [false, true] {
            for keys in [&enter, &ctrl_enter] {
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
                            input_shortcut(&key(name, modifiers), source, keys).is_none(),
                            "{name} {modifiers:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn an_open_completion_list_comes_before_the_bindings() {
        use crate::msg::Complete;
        use nana_ui_platform::{InputModifiers, KeyInput};
        let key =
            |name, modifiers| KeyInput::named("Unidentified", name, KeyState::Pressed, modifiers);
        let plain = InputModifiers::default();
        let ctrl = InputModifiers {
            control: true,
            ..plain
        };
        let keys = test_keys(false);
        let complete = |k: &KeyInput, source| match input_key(k, source, true, &keys) {
            Some((Msg::Complete(c), consumed)) => Some((c, consumed)),
            _ => None,
        };
        // Rich: the app's list takes them.
        assert_eq!(
            complete(&key("Enter", plain), false),
            Some((Complete::Accept(None), true))
        );
        assert_eq!(
            complete(&key("Tab", plain), false),
            Some((Complete::Accept(None), true))
        );
        assert_eq!(
            complete(&key("Escape", plain), false),
            Some((Complete::Dismiss, true))
        );
        assert_eq!(
            complete(&key("ArrowDown", plain), false),
            Some((Complete::Move(1), true))
        );
        // Markup source: its own popup takes them, past the bindings (no
        // Send on Enter); Esc closes the app's list too.
        for name in ["Enter", "Tab", "ArrowDown", "ArrowUp"] {
            assert!(
                input_key(&key(name, plain), true, true, &keys).is_none(),
                "{name}"
            );
        }
        assert_eq!(
            complete(&key("Escape", plain), true),
            Some((Complete::Dismiss, false))
        );
        for source in [false, true] {
            // Chords stay the bindings': Ctrl+↑ still recalls.
            assert!(matches!(
                input_key(&key("ArrowUp", ctrl), source, true, &keys),
                Some((Msg::Core(Cmd::Recall(-1)), true))
            ));
            // Closed, Enter sends and Tab is the editor's.
            assert!(matches!(
                input_key(&key("Enter", plain), source, false, &keys),
                Some((Msg::Core(Cmd::Send), true))
            ));
            assert!(input_key(&key("Tab", plain), source, false, &keys).is_none());
        }
        // Moving the markup caret closes the list but still moves it.
        assert_eq!(
            complete(&key("ArrowLeft", plain), true),
            Some((Complete::Dismiss, false))
        );
        assert!(input_key(&key("ArrowLeft", plain), false, true, &keys).is_none());
    }

    #[test]
    fn rebound_input_keys_replace_the_defaults() {
        use nana_ui_platform::{InputModifiers, KeyInput};
        let alt = InputModifiers {
            alt: true,
            ..Default::default()
        };
        let ctrl = InputModifiers {
            control: true,
            ..Default::default()
        };
        let keys = InputKeys {
            send: Some(Hotkey {
                alt: true,
                ..Hotkey::bare("KeyS")
            }),
            newline: Some(Hotkey::ctrl("Enter")),
            bold: None,
            ..InputKeys::default()
        };
        // Option+S types "ß" on a Mac: the physical key names the chord.
        let send = KeyInput::named("KeyS", "ß", KeyState::Pressed, alt);
        assert!(matches!(
            input_shortcut(&send, false, &keys),
            Some(Msg::Core(Cmd::Send))
        ));
        let enter = |m| KeyInput::named("Enter", "Enter", KeyState::Pressed, m);
        assert!(input_shortcut(&enter(Default::default()), false, &keys).is_none());
        assert!(matches!(
            input_shortcut(&enter(ctrl), false, &keys),
            Some(Msg::InsertNewline)
        ));
        let bold = KeyInput::named("KeyB", "b", KeyState::Pressed, ctrl);
        assert!(input_shortcut(&bold, false, &keys).is_none());
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
            vertical: false,
            lines: vec![
                overflow::LineSpan {
                    bytes: 0..1,
                    top: 0.0,
                    bottom: 40.0,
                },
                overflow::LineSpan {
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
