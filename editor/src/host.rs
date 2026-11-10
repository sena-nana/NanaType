//! The NanaUI host program: the editor window's document, its events and
//! messages, routed to [`App`].

use std::num::NonZeroU32;
use std::sync::Arc;

use nana_ui::runtime::view::{entity_ref, with_refs};
use nana_ui::runtime::{DocumentId, Entity, FrameworkError, RuntimeDocument, ToastStack};
use nana_ui::{
    CompiledTheme, DocumentAccessError, FrameDemand, GpuBackendPolicy, MaterialEffect, RoutedInput,
    RuntimeProgram, RuntimeProgramContext, RuntimeProgramUpdate,
};
use nana_ui_platform::host::WindowCommand;
use nana_ui_platform::{WindowEvent, WindowId};

use crate::app::App;
use crate::msg::Msg;
use crate::session::Action;

pub struct Host {
    app: App,
    doc: Option<RuntimeDocument>,
    toasts: Option<Entity<ToastStack>>,
    /// The preview has effects that move, as of the last frame.
    previewing: bool,
    canvas_drag: Option<crate::canvas::Drag>,
    /// Find was just opened: its field takes the focus once it is laid out.
    focus_search: bool,
    #[cfg(target_os = "macos")]
    _open_documents: objc2::rc::Retained<crate::open_documents::Delegate>,
}

impl Host {
    fn mount(&mut self) -> Result<(), String> {
        let id = DocumentId::new(WindowId::PRIMARY.0 + 1).ok_or("document id")?;
        let mut doc = RuntimeDocument::new(id);
        let ui = self.app.ui;
        let context = doc.context_mut();
        let _ = context.set_theme_tokens(self.app.theme.clone());
        nanatype_frame::apply_locale(context, ui.locale.get_untracked());
        let (_, toasts) = context
            .mount_view_root(id, move || {
                let stack = entity_ref::<ToastStack>();
                with_refs(crate::ui::window::editor_window(ui, stack), stack)
            })
            .map_err(|e: FrameworkError| e.to_string())?;
        self.toasts = Some(toasts);
        self.doc = Some(doc);
        Ok(())
    }

    fn finish(&mut self) -> RuntimeProgramUpdate {
        for message in nanatype_frame::ui::settle_effects() {
            self.app.notice(message, true);
        }
        self.show_toasts();
        let pending = self.doc.as_ref().is_some_and(nanatype_frame::needs_redraw);
        RuntimeProgramUpdate {
            window_commands: std::mem::take(&mut self.app.commands),
            redraw: if pending {
                nana_ui::RuntimeRedraw::Window(WindowId::PRIMARY)
            } else {
                nana_ui::RuntimeRedraw::None
            },
            exit: self.app.exit_requested,
        }
    }

    /// A toast that times out or is closed comes back as [`Msg::ToastDismissed`].
    fn show_toasts(&mut self) {
        let requests = std::mem::take(&mut self.app.toasts);
        for request in requests {
            let shown = match (self.toasts, self.doc.as_mut()) {
                (Some(stack), Some(doc)) => request.push(doc, stack, Msg::ToastDismissed),
                _ => false,
            };
            request.retry_unless(shown, &mut self.app.toasts);
        }
    }
}

impl Host {
    /// Carry out a keyboard command.
    fn run(
        &mut self,
        command: crate::shortcuts::Command,
        context: &RuntimeProgramContext<Msg>,
    ) -> Result<(), FrameworkError> {
        use crate::session::Mode;
        use crate::shortcuts::Command;
        use crate::ui::{Drawer, shell};
        let ui = self.app.ui;
        match command {
            Command::Save | Command::Export => {
                // Leaving the field writes what was typed in it; the save
                // queued behind that write then waits for it.
                if let Some(doc) = &mut self.doc {
                    let id = doc.document();
                    doc.context_mut().clear_focus(id)?;
                }
                context.dispatch(Msg::PackageEditor(if command == Command::Save {
                    Action::Save
                } else {
                    Action::Export
                }));
            }
            Command::Undo => self.app.update(Msg::PackageEditor(Action::Undo)),
            Command::Redo => self.app.update(Msg::PackageEditor(Action::Redo)),
            Command::Find => {
                let mode = shell::current(ui);
                let source = matches!(mode, Mode::Files | Mode::Lua)
                    || mode == Mode::Design && ui.split_source.get_untracked();
                if !source {
                    ui.mode.set(Mode::Files);
                }
                ui.find_open.set(true);
                self.focus_search = true;
            }
            Command::Dismiss => {
                ui.find_open.set_if_changed(false);
            }
            Command::Workspace(mode) => {
                let open = ui
                    .package_editor
                    .with_untracked(|v| v.as_ref().is_some_and(|v| mode.available(v)));
                if open
                    && shell::current(ui) != mode
                    && let Some(msg) = shell::select_mode(ui, mode)
                {
                    self.app.update(msg);
                }
            }
            Command::ToggleSide => ui.side_open.update(|open| *open = !*open),
            Command::ToggleProblems => ui.drawer.update(|d| {
                *d = if *d == Drawer::Problems {
                    Drawer::Closed
                } else {
                    Drawer::Problems
                }
            }),
            Command::Close => self.app.update(Msg::PackageEditor(Action::Close)),
            Command::DebugStart | Command::DebugStop => {
                let lua = ui
                    .package_editor
                    .with_untracked(|v| v.as_ref().is_some_and(|v| Mode::Lua.available(v)));
                if lua {
                    self.app
                        .update(Msg::PackageEditor(if command == Command::DebugStart {
                            Action::DebugStart
                        } else {
                            Action::DebugStop
                        }));
                }
            }
        }
        Ok(())
    }

    /// Give find's field the focus once it exists.
    fn focus_search_field(&mut self) {
        if !self.focus_search {
            return;
        }
        let Some(doc) = &mut self.doc else {
            return;
        };
        let Some(target) = self.app.ui.search_field.get_untracked() else {
            return;
        };
        self.focus_search = false;
        let document = doc.document();
        let _ = doc.context_mut().apply_accessibility_action(
            document,
            nana_ui::runtime::AccessibilityActionRequest {
                target,
                action: nana_ui::runtime::AccessibilityAction::Focus,
            },
        );
    }
}

impl RuntimeProgram for Host {
    type Message = Msg;
    type Error = String;

    fn initialize(context: &RuntimeProgramContext<Msg>) -> Result<(Self, Vec<Msg>), String> {
        let launch = crate::Launch::from_args(std::env::args().skip(1));
        let app = App::new(context, launch.paths());
        let mut host = Self {
            app,
            doc: None,
            toasts: None,
            previewing: false,
            canvas_drag: None,
            focus_search: false,
            #[cfg(target_os = "macos")]
            _open_documents: crate::open_documents::install(context)?,
        };
        host.mount()?;
        let ctx = context.clone();
        nanatype_frame::instance::serve_with_open(
            nanatype_frame::instance::Product::EDITOR,
            move |path| {
                ctx.dispatch(Msg::SecondInstance);
                if let Some(path) = path {
                    ctx.dispatch(Msg::Open(path));
                }
            },
        );
        Ok((host, launch.open.map(Msg::Open).into_iter().collect()))
    }

    fn with_document<R>(
        &self,
        id: WindowId,
        f: impl FnOnce(&RuntimeDocument) -> R,
    ) -> Result<Option<R>, DocumentAccessError> {
        Ok((id == WindowId::PRIMARY)
            .then_some(())
            .and(self.doc.as_ref())
            .map(f))
    }

    fn with_document_mut<R>(
        &mut self,
        id: WindowId,
        f: impl FnOnce(&mut RuntimeDocument) -> R,
    ) -> Result<Option<R>, DocumentAccessError> {
        Ok((id == WindowId::PRIMARY)
            .then_some(())
            .and(self.doc.as_mut())
            .map(f))
    }

    fn update(&mut self, message: Msg, _: &RuntimeProgramContext<Msg>) -> RuntimeProgramUpdate {
        match message {
            Msg::ToastDismissed(toast) => {
                if let (Some(stack), Some(doc)) = (self.toasts, self.doc.as_mut()) {
                    let _ = doc.context_mut().remove_toast(stack, toast);
                }
            }
            Msg::SecondInstance => {
                self.app
                    .commands
                    .push(WindowCommand::Focus(WindowId::PRIMARY));
            }
            message => self.app.update(message),
        }
        self.finish()
    }

    fn gpu_backend_policy() -> GpuBackendPolicy {
        GpuBackendPolicy::CompositionCapable
    }

    fn theme(&self) -> Arc<CompiledTheme> {
        self.app.theme.clone()
    }

    fn window_material_mode_for(&self, _: WindowId) -> MaterialEffect {
        MaterialEffect::Solid
    }

    fn appearance_backdrop_opacity_for(&self, _: WindowId) -> f32 {
        1.0
    }

    fn initialize_window(
        &mut self,
        id: WindowId,
        _: &RuntimeProgramContext<Msg>,
    ) -> Result<(), String> {
        if id != WindowId::PRIMARY || self.doc.is_some() {
            return Ok(());
        }
        self.mount()
    }

    fn discard_window(&mut self, id: WindowId) {
        if id == WindowId::PRIMARY {
            self.doc = None;
            self.toasts = None;
        }
        nanatype_frame::ui::collect_effect_garbage();
    }

    fn window_event(
        &mut self,
        event: WindowEvent,
        context: &RuntimeProgramContext<Msg>,
    ) -> RuntimeProgramUpdate {
        match event {
            WindowEvent::Ready { .. } => {
                // The app paints its own high contrast, as the setting says.
                if let Some(doc) = self.doc.as_mut() {
                    doc.context_mut().set_high_contrast(false);
                }
                let window = context.window();
                nanatype_frame::shell::tint_caption(&window, &self.app.theme);
                self.app.window = Some(window);
                self.app.system_text_scale = context.text_scale();
                self.app.apply_zoom();
            }
            WindowEvent::FocusChanged { focused: false, .. }
            | WindowEvent::VisibilityChanged { hidden: true, .. } => {
                self.canvas_drag = None;
                self.app.ui.canvas_preview.set(None);
                self.app.ui.canvas_focus.set(false);
            }
            WindowEvent::CloseRequested { .. } => {
                self.app.update(Msg::PackageEditor(Action::Exit));
                return self.finish();
            }
            WindowEvent::FileDialogCompleted { result, .. } => {
                self.app.file_chosen(result.id, result.paths);
            }
            WindowEvent::AppearanceChanged { appearance, .. } => {
                self.app.system_dark =
                    matches!(appearance, nana_ui_platform::SystemAppearance::Dark);
                self.app.refresh_theme();
                if let Some(doc) = self.doc.as_mut() {
                    let _ = doc.context_mut().set_theme_tokens(self.app.theme.clone());
                }
            }
            WindowEvent::ReducedMotionChanged { reduced, .. } => {
                self.app.ui.reduced_motion.set(reduced);
            }
            WindowEvent::TextScaleChanged { scale, .. } => {
                self.app.system_text_scale = Some(scale);
                self.app.apply_zoom();
            }
            WindowEvent::HighContrastChanged { high_contrast, .. } => {
                self.app.system_high_contrast = high_contrast;
                self.app.refresh_theme();
                if let Some(doc) = self.doc.as_mut() {
                    let cx = doc.context_mut();
                    cx.set_high_contrast(false);
                    let _ = cx.set_theme_tokens(self.app.theme.clone());
                }
            }
            _ => {}
        }
        self.finish()
    }

    fn input_event(
        &mut self,
        _: WindowId,
        input: RoutedInput<'_>,
        context: &RuntimeProgramContext<Msg>,
    ) -> Result<RuntimeProgramUpdate, FrameworkError> {
        let ui = self.app.ui;
        if let nana_ui_platform::InputPayload::Key(key) = &input.event.payload
            && !input.disposition.prevent_default
            && ui.package_editor.with_untracked(Option::is_some)
        {
            let typing = self
                .doc
                .as_ref()
                .is_some_and(|doc| doc.context().focused_text_editor(doc.document()).is_some());
            if let Some(command) = crate::shortcuts::command(key, typing) {
                self.run(command, context)?;
            }
        }
        if let Some(doc) = &self.doc
            && let Some(id) = ui.source_node.get_untracked()
            && let Some(entity) = doc.context().view_entity::<nana_ui::runtime::TextArea>(id)
            && let Ok(area) = doc.context().read(entity, Clone::clone)
            && let Some(view) = ui.package_editor.get_untracked()
            && let Some(path) = &view.selected
        {
            ui.source_views.update(|m| {
                m.insert(path.clone(), area);
            });
        }
        if let Some(doc) = &self.doc {
            crate::ui::shell::sync_widths(ui, doc.context());
        }
        if let Some(action) = crate::canvas::input(ui, &input.event.payload, &mut self.canvas_drag)
        {
            self.app.update(Msg::PackageEditor(action));
        }
        if matches!(&input.event.payload,nana_ui_platform::InputPayload::Pointer(p) if p.phase==nana_ui_platform::PointerPhase::Down)
            && ui.canvas_focus.get_untracked()
            && let Some(doc) = &mut self.doc
        {
            let id = doc.document();
            doc.context_mut().clear_focus(id)?;
        }
        Ok(self.finish())
    }

    fn scene_gpu_renderers(&self, _: WindowId) -> Option<nana_ui::SceneGpuRendererRegistry> {
        Some(nanatype_frame::ui::sweep::renderers())
    }

    fn window_frame_presented(
        &mut self,
        _: WindowId,
        _: &RuntimeProgramContext<Msg>,
    ) -> RuntimeProgramUpdate {
        nanatype_frame::ui::collect_effect_garbage();
        self.focus_search_field();
        if self.app.ui.canvas_enabled.get_untracked()
            && let Some(doc) = &self.doc
        {
            let parts = nanatype_frame::ui::editor_geometry::collect(
                self.app.ui.preview_host,
                doc.context(),
            );
            self.app.ui.canvas_geometry.set_if_changed(parts);
        }
        self.finish()
    }

    fn frame_demand(&self, _: WindowId) -> FrameDemand {
        if self.previewing {
            FrameDemand::Continuous(NonZeroU32::new(30).expect("non-zero"))
        } else if let Some(at) = crate::ui::preview::playback_deadline(
            self.app.ui.preview_host,
            nanatype_frame::clock::now_ms(),
        )
        .into_iter()
        .chain(self.app.maintenance_at())
        .min()
        {
            FrameDemand::At(nanatype_frame::clock::instant(at))
        } else {
            FrameDemand::OnDemand
        }
    }

    fn prepare_window_frame(&mut self, _: WindowId, _: &RuntimeProgramContext<Msg>) {
        self.app.maintenance();
        let now = nanatype_frame::clock::now_ms();
        nanatype_frame::clock::begin_frame(now);
        let ui = self.app.ui.preview_host;
        crate::ui::preview::tick_history_playback(ui, now);
        crate::ui::preview::tick_sample_playback(ui, now);
        self.previewing = crate::ui::preview::animating(self.app.ui.preview_host, now);
        if self.previewing {
            self.app.ui.now.set_if_changed(now);
        }
    }
}
