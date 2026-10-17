//! Real NanaType Host, native windows and production Spout sender. Isolated paths.
use nana_ui::runtime::{FrameworkError, RuntimeDocument};
use nana_ui::*;
use nana_ui_platform::{
    ApplicationIdentity, ApplicationPaths, PathEnvironment, WindowEvent, WindowId,
};
use nanatype::{host::Host, msg::Msg, windows::WindowKind};
use nanatype_core::Cmd;
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
#[path = "support.rs"]
mod native_outputs;
struct Probe {
    inner: Host,
    state: Arc<Mutex<native_outputs::State>>,
}
impl Probe {
    fn record_frame(&mut self, id: WindowId, frame: &WindowOutputFrame, ms: f64) {
        let mut s = self.state.lock().unwrap();
        let elapsed = s.start.elapsed().as_millis();
        if s.frames.len() < 60000 {
            s.frames.push(native_outputs::FrameRecord {
                ms: elapsed as u64,
                window: id.0,
                revision: frame.content_revision(),
                generation: frame.resource_generation(),
                extent: frame.extent(),
                send_cpu_ms: ms,
            });
        }
    }
}
impl RuntimeProgram for Probe {
    type Message = Msg;
    type Error = String;
    fn initialize(context: &RuntimeProgramContext<Msg>) -> Result<(Self, Vec<Msg>), String> {
        let root = PathBuf::from(std::env::var_os("NANATYPE_NATIVE_PROBE").unwrap());
        let state = Arc::new(Mutex::new(native_outputs::State::new(root.clone())));
        native_outputs::install(state.clone());
        let (inner, messages) = Host::initialize(context)?;
        native_outputs::commands(root, context.clone(), state.clone());
        Ok((Self { inner, state }, messages))
    }
    fn output_trace_enabled(&self, _: WindowId) -> bool {
        true
    }
    fn window_output_trace(
        &mut self,
        id: WindowId,
        t: output::OutputTraceSnapshot,
        _: &RuntimeProgramContext<Msg>,
    ) {
        self.state.lock().unwrap().traces.insert(format!("{}:{}",id.0,t.consumer.0),json!({"producer":t.producer,"resource_generation":t.resource_generation,"revision":t.content_revision,"extent":t.extent,"state":t.state as u64,"deferral":t.deferral_reason as u64,"ui_renders":t.work.ui_renders,"copies":t.work.gpu_copies}));
    }
    fn with_document<R>(
        &self,
        id: WindowId,
        f: impl FnOnce(&RuntimeDocument) -> R,
    ) -> Result<Option<R>, DocumentAccessError> {
        self.inner.with_document(id, f)
    }
    fn with_document_mut<R>(
        &mut self,
        id: WindowId,
        f: impl FnOnce(&mut RuntimeDocument) -> R,
    ) -> Result<Option<R>, DocumentAccessError> {
        self.inner.with_document_mut(id, f)
    }
    fn update(&mut self, message: Msg, arg2: &RuntimeProgramContext<Msg>) -> RuntimeProgramUpdate {
        self.inner.update(message, arg2)
    }
    fn gpu_backend_policy() -> GpuBackendPolicy {
        Host::gpu_backend_policy()
    }
    fn theme(&self) -> Arc<CompiledTheme> {
        self.inner.theme()
    }
    fn window_material_mode_for(&self, id: WindowId) -> MaterialEffect {
        self.inner.window_material_mode_for(id)
    }
    fn appearance_backdrop_opacity_for(&self, id: WindowId) -> f32 {
        self.inner.appearance_backdrop_opacity_for(id)
    }
    fn initialize_window(
        &mut self,
        id: WindowId,
        context: &RuntimeProgramContext<Msg>,
    ) -> Result<(), String> {
        self.inner.initialize_window(id, context)
    }
    fn discard_window(&mut self, id: WindowId) {
        self.inner.discard_window(id)
    }
    fn window_event(
        &mut self,
        event: WindowEvent,
        context: &RuntimeProgramContext<Msg>,
    ) -> RuntimeProgramUpdate {
        self.inner.window_event(event, context)
    }
    fn input_event(
        &mut self,
        id: WindowId,
        input: RoutedInput<'_>,
        arg3: &RuntimeProgramContext<Msg>,
    ) -> Result<RuntimeProgramUpdate, FrameworkError> {
        self.inner.input_event(id, input, arg3)
    }
    fn scene_gpu_renderers(&self, arg1: WindowId) -> Option<nana_ui::SceneGpuRendererRegistry> {
        self.inner.scene_gpu_renderers(arg1)
    }
    fn host_textures(&self, id: WindowId) -> Option<nana_ui::HostTextureRegistry> {
        self.inner.host_textures(id)
    }
    fn window_output(&self, id: WindowId) -> Option<nana_ui::WindowOutputConfig> {
        self.inner.window_output(id)
    }
    fn window_frame_presented(
        &mut self,
        id: WindowId,
        arg2: &RuntimeProgramContext<Msg>,
    ) -> RuntimeProgramUpdate {
        self.inner.window_frame_presented(id, arg2)
    }
    fn window_output_frame(
        &mut self,
        id: WindowId,
        frame: &nana_ui::WindowOutputFrame,
        context: &RuntimeProgramContext<Msg>,
    ) {
        let t = Instant::now();
        self.inner.window_output_frame(id, frame, context);
        self.record_frame(id, frame, t.elapsed().as_secs_f64() * 1000.);
    }
    fn window_output_status(
        &mut self,
        id: WindowId,
        status: nana_ui::WindowOutputStatus,
        context: &RuntimeProgramContext<Msg>,
    ) {
        self.inner.window_output_status(id, status, context)
    }
    fn frame_demand(&self, id: WindowId) -> FrameDemand {
        self.inner.frame_demand(id)
    }
    fn prepare_window_frame(&mut self, id: WindowId, context: &RuntimeProgramContext<Msg>) {
        self.inner.prepare_window_frame(id, context)
    }
}
impl Drop for Probe {
    fn drop(&mut self) {
        nanatype::output::probe::clear();
        std::thread::sleep(Duration::from_millis(100));
        self.state.lock().unwrap().save();
    }
}
pub fn run() {
    let root = PathBuf::from(
        std::env::var_os("NANATYPE_NATIVE_PROBE").expect("isolated evidence directory"),
    );
    let identity = ApplicationIdentity::new(
        "dev.sena-nana.nanatype.probe",
        "NanaType Native Probe",
        "0.1.0",
    );
    let mut env = PathEnvironment::current().unwrap();
    env.executable = root.join("app/native_outputs.exe");
    env.portable_marker = true;
    let paths = ApplicationPaths::resolve_with(&identity, &env).unwrap();
    std::fs::create_dir_all(root.join("commands")).unwrap();
    let skin = nanatype_theme::SkinRegistry::new(None, "0.1.0")
        .load("nanatype.tidal")
        .unwrap();
    let primary = nanatype::windows::descriptor(
        WindowKind::Input,
        "NanaType Native Probe",
        &skin.skin,
        &Default::default(),
    );
    NanaApplication::builder(identity)
        .paths(paths)
        .run::<Probe>(primary)
        .unwrap();
}
