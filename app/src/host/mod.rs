//! The NanaUI host program: owns one document per window and routes window
//! events, input and messages to [`App`].

mod editor_bridge;
mod input_router;
mod overlay;
mod theme_env;
mod viewport_sync;

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Instant;

use nana_ui::runtime::view::{entity_ref, with_refs};
use nana_ui::runtime::{DocumentId, Entity, FrameworkError, RuntimeDocument, ToastStack};
use nana_ui::{
    CompiledTheme, DocumentAccessError, FrameDemand, GpuBackendPolicy, MaterialEffect, RoutedInput,
    RuntimeProgram, RuntimeProgramContext, RuntimeProgramUpdate,
};
use nana_ui_platform::host::WindowCommand;
use nana_ui_platform::{WindowEvent, WindowId};
use nanatype_settings::InputPresence;

use crate::app::App;
use crate::clock;
use crate::msg::{EditorMsg, HostMsg, Msg, StickerMsg, WindowMsg};
use crate::ui::overflow;
use crate::ui::{dialogue, history, input, quick, settings};
use crate::windows::WindowKind;

use editor_bridge::EditorBridge;
use theme_env::ThemeEnv;
use viewport_sync::ViewportSync;

pub struct Host {
    app: App,
    perf: Option<crate::perf::Recorder>,
    docs: HashMap<WindowId, RuntimeDocument>,
    editor: EditorBridge,
    theme_env: ThemeEnv,
    viewport: ViewportSync,
    quick_field: Option<Entity<nana_ui::runtime::TextInput>>,
    history_search_open: bool,
    danmaku_search_open: bool,
    stickers: crate::stickers::playback::Playback,
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

/// The skin frame rate setting, within the rates it offers.
fn skin_fps(ui: crate::ui::Ui) -> u32 {
    let [slow, fast] = nanatype_settings::SKIN_FRAME_RATES;
    let rate = ui.settings.with_untracked(|s| s.skin.frame_rate);
    if rate > slow { fast } else { slow }
}

fn max_frame_rate(kind: Option<WindowKind>) -> Option<NonZeroU32> {
    match kind {
        Some(WindowKind::Input) => Some(NonZeroU32::new(INPUT_FPS).expect("non-zero")),
        Some(WindowKind::History | WindowKind::Danmaku) => {
            Some(NonZeroU32::new(HISTORY_FPS).expect("non-zero"))
        }
        _ => None,
    }
}

const HISTORY_FPS: u32 = 60;
const INPUT_FPS: u32 = 60;

fn frame_policy(
    ui: crate::ui::Ui,
    kind: Option<WindowKind>,
    now: u64,
    follow: &overflow::Follow,
    peer_follows: &[overflow::Follow],
    pager: &overflow::Pager,
) -> FramePolicy {
    let skin_fps = skin_fps(ui);
    let (clock, tick_clock, render_only, next_at, fps) = match kind {
        Some(WindowKind::Display | WindowKind::PeerDisplay(_)) => {
            let seat = kind.and_then(WindowKind::seat);
            if let Some(seat) = seat
                && (!ui.peer.get_untracked().active
                    || seat >= crate::ui::remote_bands(ui.peer_seats.get_untracked()))
            {
                return FramePolicy::default();
            }
            let track = seat.map_or(ui, |seat| ui.lane(seat));
            let scene = dialogue::frame_demand(*track, now);
            let follow_wants = match seat {
                Some(seat) => peer_follows[seat].wants_frame(),
                None => follow.wants_frame() || pager.wants_frame(),
            };
            let follow_next = match seat {
                Some(seat) => peer_follows[seat].next_line_at(),
                None => follow.next_line_at(),
            };
            let next_at = match (scene.next_at, follow_next) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            (
                track.now,
                scene.tick_clock,
                scene.render_only || follow_wants,
                next_at,
                skin_fps,
            )
        }
        Some(WindowKind::History | WindowKind::Danmaku) => {
            if kind == Some(WindowKind::Danmaku)
                && !ui.settings.with_untracked(|s| s.history.chat.separate)
            {
                return FramePolicy::default();
            }
            let stage = if kind == Some(WindowKind::Danmaku) {
                ui.danmaku_log()
            } else {
                *ui
            };
            let scene = history::frame_demand(stage, now);
            (
                stage.history_now,
                scene.tick_clock,
                scene.render_only,
                scene.next_at,
                HISTORY_FPS,
            )
        }
        Some(WindowKind::Settings) => {
            let preview = crate::ui::preview::animating(*ui, now);
            (
                ui.settings_pulse_ms,
                settings::links_pulsing(ui),
                preview,
                crate::ui::preview::playback_deadline(*ui, now),
                if preview { skin_fps } else { 30 },
            )
        }
        Some(WindowKind::Input) => {
            use crate::ui::shader_effects::Wake;
            let wake = crate::ui::shader_effects::input_wake(*ui);
            return FramePolicy {
                demand: if wake == Wake::Continuous || crate::ui::sweep::input_animating(*ui) {
                    FrameDemand::Continuous(NonZeroU32::new(skin_fps).expect("non-zero"))
                } else if let Wake::At(at) = wake {
                    FrameDemand::At(clock::instant(at))
                } else {
                    FrameDemand::OnDemand
                },
                ..FramePolicy::default()
            };
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

/// Read a window's native handle on a managed background worker and post it as `msg`.
fn post_native_handle(
    handle: nana_ui::WindowHandle,
    ctx: &RuntimeProgramContext<Msg>,
    msg: fn(isize) -> Msg,
) {
    crate::plugins::background("native-handle", ctx, move || {
        let hwnd = handle
            .with_native_handle(|h| match h.as_raw() {
                #[cfg(windows)]
                raw_window_handle::RawWindowHandle::Win32(w) => w.hwnd.get(),
                #[cfg(target_os = "macos")]
                raw_window_handle::RawWindowHandle::AppKit(w) => w.ns_view.as_ptr() as isize,
                _ => 0,
            })
            .wait()
            .unwrap_or(0);
        msg(hwnd)
    });
}

impl Host {
    #[cfg(windows)]
    fn prepare_outputs(&mut self, context: &RuntimeProgramContext<Msg>) {
        self.app.note_gpu(context.gpu());
        let locale = self.app.ui.locale.get_untracked();
        let partners = self.app.partner_seats();
        let sizes = self.app.ui.runtime_output_sizes.get_untracked();
        self.app.shell.output.prepare(
            &self.app.settings,
            &sizes,
            partners,
            context.gpu(),
            locale,
            self.app.ui.output_status,
        );
        if let Some(at) = self.app.shell.output.take_cleanup_schedule() {
            self.app.session.scheduler.output_cleanup(at);
        }
    }

    fn finish(&mut self) -> RuntimeProgramUpdate {
        for message in nanatype_frame::ui::settle_effects() {
            self.app.notice(message, true);
        }
        self.editor.drive(&mut self.app, &mut self.docs);
        self.focus_quick();
        if let Some(doc) = self.docs.get_mut(&WindowKind::History.id()) {
            history::focus_search(*self.app.ui, doc, &mut self.history_search_open);
        }
        if let Some(doc) = self.docs.get_mut(&WindowKind::Danmaku.id()) {
            history::focus_search(
                self.app.ui.danmaku_log(),
                doc,
                &mut self.danmaku_search_open,
            );
        }
        self.theme_env.sync_doc_env(&mut self.docs, &self.app);
        let input_visible = self.app.input_visible();
        self.theme_env
            .show_toasts(&mut self.docs, &mut self.app.shell.toasts, input_visible);
        let mut result = self.app.update_result();
        result.redraw = pending_redraw(&self.docs);
        result
    }

    /// Put the caret in the quick bar once it is summoned (and mounted).
    fn focus_quick(&mut self) {
        if !self.app.shell.quick.take_focus() {
            return;
        }
        let (Some(field), Some(doc)) =
            (self.quick_field, self.docs.get_mut(&WindowKind::Quick.id()))
        else {
            return;
        };
        let document = doc.document();
        let cx = doc.context_mut();
        let _ = cx.flush_reactive();
        let _ = cx.focus_node(document, field.stable_id());
    }

    fn mount(&mut self, id: WindowId, kind: WindowKind) -> Result<(), String> {
        let doc_id = DocumentId::new(id.0 + 1).ok_or("document id")?;
        let mut doc = RuntimeDocument::new(doc_id);
        let ui = self.app.ui;
        self.theme_env
            .init_window_theme(id, kind, &mut doc, &self.app);
        let context = doc.context_mut();
        nanatype_frame::apply_locale(context, ui.locale.get_untracked());
        let err = |e: FrameworkError| e.to_string();
        match kind {
            WindowKind::Display => {
                context
                    .mount_view_root(doc_id, move || dialogue::display_window(ui))
                    .map_err(err)?;
            }
            WindowKind::PeerDisplay(seat) => {
                let seat = usize::from(seat);
                context
                    .mount_view_root(doc_id, move || dialogue::peer_window(ui, seat))
                    .map_err(err)?;
            }
            WindowKind::History => {
                context
                    .mount_view_root(doc_id, move || history::history_window(ui))
                    .map_err(err)?;
            }
            WindowKind::Danmaku => {
                context
                    .mount_view_root(doc_id, move || history::danmaku_window(ui))
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
                self.theme_env.register_toasts(id, stack);
            }
            WindowKind::Quick => {
                let (_, field) = context
                    .mount_view_root(doc_id, move || quick::quick_window(ui))
                    .map_err(err)?;
                let post = self.app.ctx.clone();
                context
                    .on_key(field, move |key: &nana_ui::KeyInput| {
                        if let Some(message) = input_router::quick_key(key)
                            .or_else(|| input_router::quick_send_key(key, ui))
                        {
                            post.dispatch(message);
                            true
                        } else {
                            false
                        }
                    })
                    .map_err(err)?;
                self.quick_field = Some(field);
            }
            WindowKind::Input => {
                let (_, (editor, source, stack)) = context
                    .mount_view_root(doc_id, move || input::input_window(ui))
                    .map_err(err)?;
                context
                    .on_key(
                        editor,
                        input_router::key_policy(
                            self.app.ctx.clone(),
                            ui,
                            self.app.hotkey_input.clone(),
                            false,
                        ),
                    )
                    .map_err(err)?;
                context
                    .on_key(
                        source,
                        input_router::key_policy(
                            self.app.ctx.clone(),
                            ui,
                            self.app.hotkey_input.clone(),
                            true,
                        ),
                    )
                    .map_err(err)?;
                self.editor.set_input_entities(editor, source);
                self.theme_env.register_toasts(id, stack);
            }
        }
        self.theme_env.invalidate_doc_env(id);
        self.docs.insert(id, doc);
        Ok(())
    }
}

impl RuntimeProgram for Host {
    type Message = Msg;
    type Error = String;

    fn initialize(context: &RuntimeProgramContext<Msg>) -> Result<(Self, Vec<Msg>), String> {
        // Windows keep presenting while a light plays, so its pipeline can
        // compile without holding the first frame that draws one.
        crate::ui::text_light::compile_off_thread();
        let app = App::new(context);
        let mut host = Self {
            app,
            perf: crate::perf::Recorder::from_env(),
            docs: HashMap::new(),
            editor: EditorBridge::default(),
            theme_env: ThemeEnv::default(),
            viewport: ViewportSync::default(),
            quick_field: None,
            history_search_open: false,
            danmaku_search_open: false,
            stickers: Default::default(),
            last_keystroke: None,
        };
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
        self.editor
            .capture_source_selection(self.app.ui, &self.docs);
        if matches!(
            &message,
            Msg::Editor(EditorMsg::ToggleSource) | Msg::Window(WindowMsg::HideInput)
        ) {
            self.editor.clear_menu(&mut self.docs);
        }
        match message.into_domain() {
            Err(HostMsg::EditorContext {
                source,
                x,
                y,
                keyboard,
            }) => self
                .editor
                .open_menu(&mut self.docs, self.app.ui, source, x, y, keyboard),
            Err(HostMsg::EditorAction { source, action }) => {
                self.editor
                    .handle_action(&mut self.app, &mut self.docs, source, action)
            }
            Err(HostMsg::ToastDismissed(kind, toast)) => {
                self.theme_env.dismiss_toast(&mut self.docs, kind, toast)
            }
            Ok(app_msg) => self.app.update(app_msg),
        }
        #[cfg(windows)]
        self.prepare_outputs(_context);
        self.finish()
    }

    fn gpu_backend_policy() -> GpuBackendPolicy {
        GpuBackendPolicy::CompositionCapable
    }

    fn theme(&self) -> Arc<CompiledTheme> {
        self.app.appearance.theme.clone()
    }

    fn window_material_mode_for(&self, id: WindowId) -> MaterialEffect {
        WindowKind::from_id(id)
            .map(WindowKind::material)
            .unwrap_or(MaterialEffect::Solid)
    }

    fn appearance_backdrop_opacity_for(&self, id: WindowId) -> f32 {
        match WindowKind::from_id(id) {
            Some(WindowKind::Input | WindowKind::Settings | WindowKind::Quick) => 1.0,
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
        self.theme_env.remove_window(id);
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
                    self.app.shell.windows.ready(kind, handle.clone());
                    if matches!(kind, WindowKind::Input | WindowKind::Settings) {
                        self.app.tint_captions();
                    }
                    self.app.apply_zoom(kind);
                    if matches!(
                        kind,
                        WindowKind::Display
                            | WindowKind::PeerDisplay(_)
                            | WindowKind::History
                            | WindowKind::Danmaku
                            | WindowKind::Quick
                    ) {
                        self.app.shell.commands.push(
                            WindowCommand::SetNativeWindowControlsVisible {
                                id,
                                visible: false,
                                duration: std::time::Duration::ZERO,
                            },
                        );
                    }
                    self.app.shell.windows.record_geometry(kind, geometry);
                    if kind == WindowKind::Input {
                        nanatype_diag::event!(
                            crate::diag::INPUT_READY,
                            since_launch_ms = crate::clock::now_ms()
                        );
                        post_native_handle(handle, &self.app.ctx, Msg::InputHwnd);
                    } else if kind == WindowKind::Quick {
                        post_native_handle(handle, &self.app.ctx, Msg::QuickHwnd);
                    }
                }
            }
            WindowEvent::FocusChanged { id, focused: false }
                if id == WindowKind::Quick.id()
                    && matches!(
                        self.app.shell.quick,
                        crate::windows::QuickBarState::Visible { .. }
                    ) =>
            {
                if let Ok(msg) = Msg::QuickHide.into_domain() {
                    self.app.update(msg);
                }
            }
            WindowEvent::Moved { id, geometry } | WindowEvent::Resized { id, geometry } => {
                if let Some(kind) = WindowKind::from_id(id) {
                    viewport_sync::record_overlay_move_or_resize(&mut self.app, kind, geometry);
                }
            }
            WindowEvent::CloseRequested { id } => match WindowKind::from_id(id) {
                Some(WindowKind::Settings) | None => {
                    self.app.shell.windows.close_settings();
                    return RuntimeProgramUpdate {
                        window_commands: vec![WindowCommand::Close(id)],
                        ..RuntimeProgramUpdate::default()
                    };
                }
                Some(WindowKind::Quick) => {
                    if let Ok(msg) = Msg::QuickHide.into_domain() {
                        self.app.update(msg);
                    }
                }
                Some(WindowKind::Input) => {
                    if self.app.settings.general.input_presence == InputPresence::Summon {
                        self.app.shell.summoned = false;
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
                    self.app.shell.windows.closed(kind);
                }
                if id == WindowKind::Input.id() {
                    self.app.shell.hotkeys.set_input_window(0);
                }
                if id == WindowKind::Quick.id() {
                    self.quick_field = None;
                    self.app.quick_closed();
                }
                self.docs.remove(&id);
                self.theme_env.remove_window(id);
            }
            WindowEvent::FileDialogCompleted { result, .. } => {
                self.app.file_chosen(result.id, result.paths);
            }
            WindowEvent::AppearanceChanged { appearance, .. } => {
                self.app.appearance.system_dark =
                    matches!(appearance, nana_ui_platform::SystemAppearance::Dark);
                if self.app.settings.accessibility.chrome == nanatype_settings::ChromeMode::System {
                    self.app.refresh_theme();
                }
            }
            WindowEvent::ReducedMotionChanged { reduced, .. } => {
                self.app.ui.system_reduced_motion.set(reduced);
                self.app.sync_reduced_motion();
            }
            WindowEvent::TextScaleChanged { scale, .. }
                if self.app.ui.system_text_scale.get_untracked() != Some(scale) =>
            {
                self.app.ui.system_text_scale.set(Some(scale));
                self.app.apply_zoom_all();
            }
            WindowEvent::HighContrastChanged { high_contrast, .. } => {
                self.theme_env.clear_doc_env();
                if self.app.appearance.system_high_contrast != high_contrast {
                    self.app.appearance.system_high_contrast = high_contrast;
                    self.app.refresh_theme();
                }
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
        viewport_sync::sync_input_splits(&mut self.app, id, &self.docs);
        if let Some(kind) = WindowKind::from_id(id)
            && let Some(settings) = kind.overlay(&self.app.settings)
            && let Some(position) = overlay::menu_position(
                kind,
                settings.lock,
                &input.event.payload,
                input.disposition.handled,
            )
            && let Some(handle) = self.app.shell.windows.handle(kind)
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
                self.app.shell.windows.frame(kind),
            )
        {
            if let WindowCommand::SetBounds { position, size, .. } = &command {
                let (position, size) = (*position, *size);
                self.app.shell.windows.moved(kind, Some(position), size);
                self.app.set_overlay_size(kind, Some([size.0, size.1]));
            }
            self.app.shell.commands.push(command);
            return Ok(self.finish());
        }
        if WindowKind::from_id(id) == Some(WindowKind::Input) {
            let draft_focused = self
                .docs
                .get(&id)
                .is_some_and(|doc| self.editor.draft_focused(doc));
            if input_router::fence_speech_composition(
                self.app.ui,
                &input.event.payload,
                draft_focused,
            ) {
                self.app.speech_user_edit();
            }
            input_router::keystroke_sound(
                &mut self.app,
                &input.event.payload,
                &mut self.last_keystroke,
            );
            if let Some(doc) = self.docs.get(&id) {
                input::sync_composing(self.app.ui, doc.context(), doc.document());
            }
        }
        if input_router::handle_window_key(
            &mut self.app,
            id,
            input.disposition.handled,
            &input.event.payload,
        ) == Some(true)
        {
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
        let sizes = self.app.ui.runtime_output_sizes.get_untracked();
        self.app
            .shell
            .output
            .config(kind, &self.app.settings, &sizes, self.app.partner_seats())
    }

    fn window_frame_presented(
        &mut self,
        id: WindowId,
        context: &RuntimeProgramContext<Msg>,
    ) -> RuntimeProgramUpdate {
        crate::ui::collect_effect_garbage();
        if let Some(perf) = &mut self.perf {
            perf.frame(id, false, self.docs.get(&id), context.presentation_work());
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
        let paused = self
            .app
            .shell
            .output
            .paused_output(kind, clock::now_ms())
            .cloned();
        let Some(out) = crate::output::resolve(kind, &self.app.settings, self.app.partner_seats())
            .output()
            .or(paused.as_ref())
        else {
            return;
        };
        if let Some(perf) = &mut self.perf {
            perf.frame(
                id,
                true,
                self.docs.get(&id),
                nana_ui::PresentationWork::default(),
            );
        }
        let sizes = self.app.ui.runtime_output_sizes.get_untracked();
        let runtime_size = sizes.get(kind);
        let locale = self.app.ui.locale.get_untracked();
        let outcome = self.app.shell.output.frame(
            kind,
            out,
            runtime_size,
            frame,
            context.gpu(),
            locale,
            self.app.ui.output_status,
        );
        if let Some((_, detail)) = outcome.error {
            nanatype_diag::report!(crate::diag::SPOUT_FAILED, window = crate::diag::window(kind); "{detail}");
        }
        #[cfg(windows)]
        if let Some((sent, ms)) = outcome.perf {
            if let Some(perf) = &mut self.perf {
                perf.spout(id, sent, ms);
            }
            if !sent {
                frame.request_resend();
            }
        }
        if let Some(at) = self.app.shell.output.take_cleanup_schedule() {
            self.app.session.scheduler.output_cleanup(at);
        }
        if paused.is_some()
            && self
                .app
                .shell
                .output
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
        _context: &RuntimeProgramContext<Msg>,
    ) {
        if let Some(kind) = WindowKind::from_id(id) {
            nanatype_diag::report!(crate::diag::WINDOW_OUTPUT, window = crate::diag::window(kind); "{status:?}");
            let locale = self.app.ui.locale.get_untracked();
            if crate::output::resolve(kind, &self.app.settings, self.app.partner_seats())
                .output()
                .is_some()
            {
                self.app.shell.output.status_changed(
                    kind,
                    &status,
                    locale,
                    Some(self.app.ui.output_status),
                );
            }
        }
    }

    fn frame_demand(&self, id: WindowId) -> FrameDemand {
        if WindowKind::from_id(id) == Some(WindowKind::Input) && self.editor.completion_unsettled {
            return FrameDemand::At(Instant::now());
        }
        let demand = frame_policy(
            self.app.ui,
            WindowKind::from_id(id),
            clock::now_ms(),
            &self.viewport.follow,
            &self.viewport.peer_follows,
            &self.viewport.pager,
        )
        .demand;
        let demand = match demand {
            FrameDemand::Continuous(_) | FrameDemand::At(_)
                if WindowKind::from_id(id) == Some(WindowKind::Input)
                    && !self.app.input_visible() =>
            {
                FrameDemand::OnDemand
            }
            demand => demand,
        };
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

    fn max_frame_rate(&self, id: WindowId) -> Option<NonZeroU32> {
        max_frame_rate(WindowKind::from_id(id))
    }

    fn sync_animation_clock(&mut self, epoch: Instant) {
        self.theme_env.sync_animation_clock(epoch);
    }

    fn prepare_window_frame(&mut self, id: WindowId, context: &RuntimeProgramContext<Msg>) {
        if let Some(doc) = self.docs.get_mut(&id) {
            self.theme_env.wake_animation_clock(doc);
        }
        self.theme_env
            .hold_output_theme(id, &mut self.docs, &self.app);
        #[cfg(windows)]
        self.prepare_outputs(context);
        if let Some(perf) = &mut self.perf {
            perf.begin(id);
        }
        let now = clock::now_ms();
        clock::begin_frame(now);
        self.stickers.prepare(
            id,
            context.gpu(),
            now,
            self.app.ui.reduced_motion.get_untracked(),
        );
        if self.stickers.take_rebuild() {
            context.dispatch(Msg::Sticker(StickerMsg::AssetsChanged));
        }
        match WindowKind::from_id(id) {
            Some(WindowKind::Input) => self.editor.anchor_completion(&self.app, &self.docs),
            Some(WindowKind::Display) => {
                self.viewport
                    .follow_track(&mut self.app, &self.docs, now, None)
            }
            Some(WindowKind::PeerDisplay(seat)) if self.app.peer.handle.is_some() => self
                .viewport
                .follow_track(&mut self.app, &self.docs, now, Some(usize::from(seat))),
            Some(WindowKind::Settings) => {
                let ui = *self.app.ui;
                crate::ui::preview::tick_history_playback(ui, now);
                crate::ui::preview::tick_sample_playback(ui, now);
            }
            _ => {}
        }
        let policy = frame_policy(
            self.app.ui,
            WindowKind::from_id(id),
            now,
            &self.viewport.follow,
            &self.viewport.peer_follows,
            &self.viewport.pager,
        );
        let changed = self.docs.get(&id).is_some_and(|doc| {
            let cx = doc.context();
            cx.has_pending_reactive() || cx.world().has_pending_work()
        });
        policy.advance(now, changed);
    }
}

/// Read the same pending work the runtime will drain, including direct editor
/// commands and toasts.
fn pending_redraw(docs: &HashMap<WindowId, RuntimeDocument>) -> nana_ui::RuntimeRedraw {
    nana_ui::RuntimeRedraw::for_windows(
        docs.iter()
            .filter_map(|(id, doc)| nanatype_frame::needs_redraw(doc).then_some(*id)),
    )
}

#[cfg(test)]
mod input_acceptance;
#[cfg(test)]
mod quick_acceptance;

#[cfg(test)]
mod tests {
    use super::input_router::{input_key, input_shortcut};
    use super::*;
    use crate::msg::ChatMsg;
    use crate::msg::Tool;
    use crate::ui::overflow::{Measure, PageKey};
    use nana_ui::runtime::view::{signal, widget};
    use nana_ui::runtime::{LayoutViewport, Text};
    use nana_ui_platform::KeyState;
    use nanatype_core::Cmd;
    use nanatype_settings::{Hotkey, InputKeys};

    fn test_keys(ctrl_enter: bool) -> InputKeys {
        let mut keys = InputKeys::default();
        if ctrl_enter {
            keys.send = Some(Hotkey::ctrl("Enter"));
        }
        keys
    }

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
            Some(Msg::Editor(EditorMsg::Tool(Tool::Bold)))
        ));
        let alt = InputModifiers { alt: true, ..plain };
        assert!(matches!(
            input_shortcut(&key("ArrowUp", alt), false, &enter),
            Some(Msg::Chat(ChatMsg::Select(-1)))
        ));
        assert!(matches!(
            input_shortcut(&key("ArrowDown", alt), true, &enter),
            Some(Msg::Chat(ChatMsg::Select(1)))
        ));
        assert!(matches!(
            input_shortcut(&key("r", alt), false, &ctrl_enter),
            Some(Msg::Chat(ChatMsg::MentionSelected { reply: true }))
        ));
        assert!(matches!(
            input_shortcut(&key("m", alt), false, &enter),
            Some(Msg::Chat(ChatMsg::MentionSelected { reply: false }))
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
            Some((Msg::Editor(EditorMsg::Complete(c)), consumed)) => Some((c, consumed)),
            _ => None,
        };
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
            assert!(matches!(
                input_key(&key("ArrowUp", ctrl), source, true, &keys),
                Some((Msg::Core(Cmd::Recall(-1)), true))
            ));
            assert!(matches!(
                input_key(&key("Enter", plain), source, false, &keys),
                Some((Msg::Core(Cmd::Send), true))
            ));
            assert!(input_key(&key("Tab", plain), source, false, &keys).is_none());
        }
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
        let send = KeyInput::named("KeyS", "ß", KeyState::Pressed, alt);
        assert!(matches!(
            input_shortcut(&send, false, &keys),
            Some(Msg::Core(Cmd::Send))
        ));
        let enter = |m| KeyInput::named("Enter", "Enter", KeyState::Pressed, m);
        assert!(input_shortcut(&enter(Default::default()), false, &keys).is_none());
        assert!(matches!(
            input_shortcut(&enter(ctrl), false, &keys),
            Some(Msg::Editor(EditorMsg::InsertNewline))
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
        ui.peer_lanes[0].display.set(ui.display.get_untracked());
        ui.now.set(123);
        ui.peer_now.set(456);
        let follow = overflow::Follow::default();
        let pager = overflow::Pager::default();
        let now = 1_000_000_000;
        let policy = |kind| {
            frame_policy(
                ui,
                Some(kind),
                now,
                &follow,
                std::slice::from_ref(&follow),
                &pager,
            )
        };
        let local = policy(WindowKind::Display);
        assert!(matches!(local.demand, FrameDemand::Continuous(_)));
        assert!(!local.tick_clock);
        local.advance(now, false);
        assert_eq!(ui.now.get_untracked(), 123);
        local.advance(now, true);
        assert_eq!(ui.now.get_untracked(), now);
        assert_eq!(ui.peer_now.get_untracked(), 456);
        let peer = policy(WindowKind::PARTNER);
        assert!(matches!(peer.demand, FrameDemand::OnDemand));
        peer.advance(now, true);
        assert_eq!(ui.peer_now.get_untracked(), 456);
        ui.peer.update(|peer| Arc::make_mut(peer).active = true);
        let peer = policy(WindowKind::PARTNER);
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
    fn stepped_skin_programs_wake_the_display_once_per_step() {
        let skin = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"))
            .load("nanatype.pixel")
            .unwrap()
            .skin;
        let ui = crate::ui::Ui::new(
            Arc::new(nanatype_settings::Settings::default()),
            skin,
            nanatype_i18n::Locale::ZhCn,
        );
        ui.display.update(|view| Arc::make_mut(view).empty = false);
        let follow = overflow::Follow::default();
        let pager = overflow::Pager::default();
        let now = 10_000;
        let policy = clock::capture_at(now, || {
            frame_policy(
                ui,
                Some(WindowKind::Display),
                now,
                &follow,
                std::slice::from_ref(&follow),
                &pager,
            )
        });
        assert!(!policy.tick_clock);
        assert!(matches!(policy.demand, FrameDemand::At(at) if at == clock::instant(10_667)));
    }

    #[test]
    fn display_frame_policy_stays_live_for_a_final_compositor_reveal() {
        let now = 10_000;
        let ui = crate::ui::Ui::new(
            Arc::new(nanatype_settings::Settings::default()),
            Arc::new(nanatype_theme::Skin::default()),
            nanatype_i18n::Locale::ZhCn,
        );
        ui.display.update(|view| {
            let view = Arc::make_mut(view);
            view.empty = false;
            view.entrance = nanatype_model::Entrance {
                kind: nanatype_model::EntranceKind::Fade,
                ms: 200,
            };
            view.doc = Arc::new(nanatype_model::Document::plain("hi"));
            view.reveal = nanatype_core::Reveal {
                visible_from: 0,
                shown: 0,
                segment: Some(nanatype_core::SegmentReveal {
                    origin_ms: now,
                    start: 0,
                    reveal_ms: Arc::from([0, 100]),
                }),
                unit_times: None,
                hidden_from: 2,
            };
        });
        let follow = overflow::Follow::default();
        let pager = overflow::Pager::default();
        let live = frame_policy(
            ui,
            Some(WindowKind::Display),
            now + 299,
            &follow,
            &[],
            &pager,
        );
        assert!(live.tick_clock);
        assert!(matches!(live.demand, FrameDemand::Continuous(_)));
        let settled = frame_policy(
            ui,
            Some(WindowKind::Display),
            now + 300,
            &follow,
            &[],
            &pager,
        );
        assert!(!settled.tick_clock);
        assert!(matches!(settled.demand, FrameDemand::OnDemand));
    }

    #[test]
    fn history_frame_policy_stays_live_for_a_compositor_reveal() {
        let now = 10_000;
        let ui = crate::ui::Ui::new(
            Arc::new(nanatype_settings::Settings::default()),
            Arc::new(nanatype_theme::Skin::default()),
            nanatype_i18n::Locale::ZhCn,
        );
        let doc = nanatype_model::Document::plain("hi");
        ui.history.set(Arc::new(vec![nanatype_core::HistoryRow {
            key: 1,
            item: nanatype_model::HistoryItem {
                source: nanatype_model::SourceId::local(),
                kind: nanatype_model::SourceKind::Typewriter,
                line: None,
                author: "Nana".into(),
                avatar: None,
                doc,
                at: 0,
                uid: None,
                reply: None,
                event: None,
            },
            reveal: Some(nanatype_core::Reveal {
                visible_from: 0,
                shown: 0,
                segment: Some(nanatype_core::SegmentReveal {
                    origin_ms: now,
                    start: 0,
                    reveal_ms: Arc::from([0, 100]),
                }),
                unit_times: None,
                hidden_from: 2,
            }),
        }]));
        let follow = overflow::Follow::default();
        let pager = overflow::Pager::default();
        let live = frame_policy(ui, Some(WindowKind::History), now, &follow, &[], &pager);
        assert!(live.tick_clock);
        assert!(matches!(live.demand, FrameDemand::Continuous(_)));
        let settled = frame_policy(
            ui,
            Some(WindowKind::History),
            now + 1_000,
            &follow,
            &[],
            &pager,
        );
        assert!(!settled.tick_clock);
        assert!(matches!(settled.demand, FrameDemand::OnDemand));
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
            d.exit = Some(nanatype_core::DisplayExit::Fade {
                at_ms: now,
                ms: 100,
            });
        });
        let mut follow = overflow::Follow::default();
        let peer_follow = overflow::Follow::default();
        let mut pager = overflow::Pager::default();
        let policy = frame_policy(
            ui,
            Some(WindowKind::Display),
            now,
            &follow,
            std::slice::from_ref(&peer_follow),
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
                std::slice::from_ref(&peer_follow),
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
            std::slice::from_ref(&peer_follow),
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
            std::slice::from_ref(&peer_follow),
            &pager,
        );
        assert!(matches!(policy.demand, FrameDemand::Continuous(_)) && !policy.tick_clock);
    }

    #[test]
    fn glyph_motion_keeps_to_each_windows_frame_rate() {
        let rate = |kind| max_frame_rate(kind).map(NonZeroU32::get);
        for kind in [WindowKind::Display, WindowKind::PARTNER] {
            assert_eq!(rate(Some(kind)), None, "{kind:?}");
        }
        assert_eq!(rate(Some(WindowKind::Input)), Some(INPUT_FPS));
        assert_eq!(rate(Some(WindowKind::History)), Some(HISTORY_FPS));
        assert_eq!(rate(Some(WindowKind::Danmaku)), Some(HISTORY_FPS));
        assert_eq!(rate(Some(WindowKind::Settings)), None);
        assert_eq!(rate(Some(WindowKind::Quick)), None);
        assert_eq!(rate(None), None);
    }

    #[test]
    fn danmaku_frame_policy_is_ondemand_when_idle_or_separate_is_off() {
        let now = 10_000;
        let mut settings = nanatype_settings::Settings::default();
        settings.history.fade_after_s = None;
        let ui = crate::ui::Ui::new(
            Arc::new(settings.clone()),
            Arc::new(nanatype_theme::Skin::default()),
            nanatype_i18n::Locale::ZhCn,
        );
        let follow = overflow::Follow::default();
        let pager = overflow::Pager::default();
        assert!(matches!(
            frame_policy(ui, Some(WindowKind::Danmaku), now, &follow, &[], &pager).demand,
            FrameDemand::OnDemand
        ));
        settings.history.chat.separate = true;
        settings.history.fade_after_s = Some(5);
        ui.settings.set(Arc::new(settings));
        let unix_now = crate::clock::unix_of(now);
        let doc = nanatype_model::Document::plain("danmaku");
        ui.danmaku.set(Arc::new(vec![nanatype_core::HistoryRow {
            key: 1,
            item: nanatype_model::HistoryItem {
                source: nanatype_model::SourceId::local(),
                kind: nanatype_model::SourceKind::Chat,
                line: None,
                author: "Viewer".into(),
                avatar: None,
                doc,
                at: unix_now,
                uid: None,
                reply: None,
                event: None,
            },
            reveal: None,
        }]));
        let scheduled = frame_policy(ui, Some(WindowKind::Danmaku), now, &follow, &[], &pager);
        assert!(matches!(scheduled.demand, FrameDemand::At(_)));
        let fading = frame_policy(
            ui,
            Some(WindowKind::Danmaku),
            now + 5_100,
            &follow,
            &[],
            &pager,
        );
        assert!(fading.tick_clock);
        assert!(matches!(fading.demand, FrameDemand::Continuous(_)));
        ui.settings
            .update(|s| Arc::make_mut(s).history.chat.separate = false);
        let disabled = frame_policy(
            ui,
            Some(WindowKind::Danmaku),
            now + 5_100,
            &follow,
            &[],
            &pager,
        );
        assert!(!disabled.tick_clock);
        assert!(matches!(disabled.demand, FrameDemand::OnDemand));
    }

    #[test]
    fn the_skin_frame_rate_setting_keeps_to_the_rates_offered() {
        for (setting, fps) in [(30, 30), (60, 60), (144, 60), (0, 30)] {
            let mut settings = nanatype_settings::Settings::default();
            settings.skin.frame_rate = setting;
            let ui = crate::ui::Ui::new(
                Arc::new(settings),
                Arc::new(nanatype_theme::Skin::default()),
                nanatype_i18n::Locale::ZhCn,
            );
            assert_eq!(skin_fps(ui), fps, "{setting}");
        }
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
