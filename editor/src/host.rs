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
        nanatype_frame::ui::collect_effect_garbage();
        for message in nanatype_frame::ui::shader_effects::take_notices() {
            self.app.notice(message, true);
        }
        self.show_toasts();
        let pending = self.doc.as_ref().is_some_and(|doc| {
            let cx = doc.context();
            cx.has_pending_reactive() || cx.world().has_pending_work()
        });
        RuntimeProgramUpdate {
            window_commands: std::mem::take(&mut self.app.commands),
            redraw: if pending {
                nana_ui::RuntimeRedraw::Window(WindowId::PRIMARY)
            } else {
                nana_ui::RuntimeRedraw::None
            },
            exit: false,
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
            // No window yet: keep it for one that opens while it is fresh.
            if !shown && request.fresh() {
                self.app.toasts.push(request);
            }
        }
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
        };
        host.mount()?;
        let ctx = context.clone();
        nanatype_frame::instance::serve(nanatype_frame::instance::Product::EDITOR, move || {
            ctx.dispatch(Msg::SecondInstance)
        });
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
                let window = context.window();
                nanatype_frame::shell::tint_caption(&window, &self.app.theme);
                self.app.window = Some(window);
            }
            WindowEvent::CloseRequested { .. } => {
                // Unsaved work asks first (save and close, close, or cancel).
                let dirty = self
                    .app
                    .ui
                    .package_editor
                    .with_untracked(|v| v.as_ref().is_some_and(|v| v.dirty));
                if dirty {
                    self.app.update(Msg::PackageEditor(Action::Close));
                    return self.finish();
                }
                return RuntimeProgramUpdate {
                    exit: true,
                    ..RuntimeProgramUpdate::default()
                };
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
            _ => {}
        }
        self.finish()
    }

    fn input_event(
        &mut self,
        _: WindowId,
        _: RoutedInput<'_>,
        _: &RuntimeProgramContext<Msg>,
    ) -> Result<RuntimeProgramUpdate, FrameworkError> {
        Ok(RuntimeProgramUpdate::default())
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
        RuntimeProgramUpdate::default()
    }

    fn frame_demand(&self, _: WindowId) -> FrameDemand {
        if self.previewing {
            FrameDemand::Continuous(NonZeroU32::new(30).expect("non-zero"))
        } else if let Some(at) =
            crate::ui::preview::playback_deadline(*self.app.ui, nanatype_frame::clock::now_ms())
        {
            FrameDemand::At(nanatype_frame::clock::instant(at))
        } else {
            FrameDemand::OnDemand
        }
    }

    fn prepare_window_frame(&mut self, _: WindowId, _: &RuntimeProgramContext<Msg>) {
        let now = nanatype_frame::clock::now_ms();
        nanatype_frame::clock::begin_frame(now);
        let ui = *self.app.ui;
        crate::ui::preview::tick_history_playback(ui, now);
        crate::ui::preview::tick_sample_playback(ui, now);
        self.previewing = crate::ui::preview::animating(*self.app.ui, now);
        if self.previewing {
            self.app.ui.now.set_if_changed(now);
        }
    }
}
