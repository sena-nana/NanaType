use super::*;
use std::collections::HashMap;
use windows::Win32::{Foundation::HANDLE, Graphics::Direct3D12::ID3D12Fence};
struct Pending {
    fence: ID3D12Fence,
    ready: u64,
    release: u64,
    start: Instant,
    ready_ms: Option<f64>,
}
#[derive(serde::Serialize)]
pub struct FrameRecord {
    pub ms: u64,
    pub window: u64,
    pub revision: u64,
    pub generation: u64,
    pub extent: [u32; 2],
    pub send_cpu_ms: f64,
}
#[derive(serde::Serialize)]
struct SendRecord {
    suspended: bool,
    ms: u64,
    window: u64,
    generation: u64,
    slot: usize,
    ready: u64,
    release: u64,
    status: &'static str,
    pool_opens: u64,
    released_without_copy: u64,
    sent: u64,
    skipped: u64,
    failed: u64,
}
pub struct State {
    pub start: Instant,
    pub root: PathBuf,
    pub frames: Vec<FrameRecord>,
    pub traces: HashMap<String, Value>,
    sends: Vec<SendRecord>,
    lifecycle: Vec<Value>,
    pools: HashMap<u64, (u64, ID3D12Fence)>,
    pending: Vec<Pending>,
    retired: Vec<f64>,
    ready_to_release: Vec<f64>,
    errors: Vec<String>,
    stop: bool,
    pub actions: Vec<Value>,
}
impl State {
    pub fn new(root: PathBuf) -> Self {
        Self {
            start: Instant::now(),
            root,
            frames: Vec::with_capacity(60000),
            traces: HashMap::new(),
            sends: Vec::with_capacity(60000),
            lifecycle: vec![],
            pools: HashMap::new(),
            pending: vec![],
            retired: vec![],
            ready_to_release: vec![],
            errors: vec![],
            stop: false,
            actions: vec![],
        }
    }
    fn poll(&mut self) {
        let mut i = 0;
        while i < self.pending.len() {
            let p = &mut self.pending[i];
            let value = unsafe { p.fence.GetCompletedValue() };
            let elapsed = p.start.elapsed().as_secs_f64() * 1000.;
            if value == u64::MAX {
                self.errors.push("shared fence reported device loss".into());
                self.pending.swap_remove(i);
                continue;
            }
            if value >= p.ready && p.ready_ms.is_none() {
                p.ready_ms = Some(elapsed);
            }
            if value >= p.release {
                self.retired.push(elapsed);
                self.ready_to_release.push(elapsed - p.ready_ms.unwrap());
                self.pending.swap_remove(i);
            } else {
                i += 1;
            }
        }
    }
    fn summary(&self) -> Value {
        json!({"elapsed_ms":self.start.elapsed().as_millis(),"frames":self.frames.len(),"sends":self.sends.len(),"retired":self.retired.len(),"pending":self.pending.len(),"errors":self.errors,"actions":self.actions,"shader_cache":nanatype::ui::shader_effects::cache_stats(),"sweep_cache":nanatype::ui::sweep::cache_counts(),"traces":self.traces})
    }
    pub fn save(&mut self) {
        self.poll();
        self.stop = true;
        std::fs::write(self.root.join("native-report.json"),serde_json::to_vec_pretty(&json!({"summary":self.summary(),"frames":self.frames,"sends":self.sends,"lifecycle":self.lifecycle,"release_observed_ms":self.retired,"ready_to_release_observed_ms":self.ready_to_release,"measurement":"Fence polling at 1 ms on a monitor thread; values are observed upper bounds from send return, not GPU timestamps."})).unwrap()).unwrap();
        self.pools.clear();
    }
}
pub fn install(state: Arc<Mutex<State>>) {
    let events = state.clone();
    nanatype::output::probe::install_lifecycle(Box::new(move |event, kind, cpu_ms| {
        let mut s = events.lock().unwrap();
        let ms = s.start.elapsed().as_millis();
        s.lifecycle
            .push(json!({"event":event,"window":kind.map(|k| k.id().0),"ms":ms,"cpu_ms":cpu_ms}));
    }));
    let observation = state.clone();
    nanatype::output::probe::install(Box::new(
        move |kind, gpu, frame, sender, result, suspended| {
            let mut s = observation.lock().unwrap();
            let id = kind.id().0;
            let now = s.start.elapsed().as_millis();
            if s.pools
                .get(&id)
                .is_none_or(|(generation, _)| *generation != frame.pool_generation)
            {
                let hal = unsafe { gpu.wgpu().device().as_hal::<wgpu::hal::api::Dx12>() }
                    .expect("DX12 native export");
                let mut fence: Option<ID3D12Fence> = None;
                // Borrowed handle is live for this callback. The opened COM interface owns
                // its reference and may be queried by the monitor after the callback.
                unsafe {
                    hal.raw_device()
                        .OpenSharedHandle(HANDLE(frame.fence), &mut fence)
                }
                .unwrap();
                s.pools.insert(id, (frame.pool_generation, fence.unwrap()));
            }
            let fence = s.pools[&id].1.clone();
            s.pending.push(Pending {
                fence,
                ready: frame.ready_value,
                release: frame.release_value,
                start: Instant::now(),
                ready_ms: None,
            });
            let status = sender.status();
            if let Err(e) = result {
                s.errors.push(e.to_string());
            }
            let outcome = match result.as_ref().map(|r| r.status) {
                Ok(nanalive_spout::SpoutPublishStatus::Sent) => "Sent",
                Ok(nanalive_spout::SpoutPublishStatus::SkippedAccessTimeout) => {
                    "SkippedAccessTimeout"
                }
                Ok(nanalive_spout::SpoutPublishStatus::BackendUnavailable) => "BackendUnavailable",
                Ok(nanalive_spout::SpoutPublishStatus::InvalidFrame) => "InvalidFrame",
                Ok(nanalive_spout::SpoutPublishStatus::Failed) => "Failed",
                Err(_) => "Error",
            };
            if s.sends.len() < 60000 {
                s.sends.push(SendRecord {
                    suspended,
                    ms: now as u64,
                    window: id,
                    generation: frame.pool_generation,
                    slot: frame.slot,
                    ready: frame.ready_value,
                    release: frame.release_value,
                    status: outcome,
                    pool_opens: sender.pool_opens(),
                    released_without_copy: sender.released_without_copy(),
                    sent: status.sent_count,
                    skipped: status.skipped_access_timeout_count,
                    failed: status.failed_count,
                });
            }
        },
    ));
    std::thread::spawn(move || {
        let mut last = Instant::now();
        loop {
            {
                let mut s = state.lock().unwrap();
                if s.stop {
                    break;
                }
                s.poll();
                if last.elapsed() > Duration::from_secs(1) {
                    let summary = s.summary();
                    let path = s.root.join("live.json");
                    drop(s);
                    let _ = std::fs::write(path, serde_json::to_vec(&summary).unwrap());
                    last = Instant::now();
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    });
}
pub fn commands(root: PathBuf, context: RuntimeProgramContext<Msg>, state: Arc<Mutex<State>>) {
    std::thread::spawn(move || {
        loop {
            if state.lock().unwrap().stop {
                break;
            }
            let mut files: Vec<_> = std::fs::read_dir(root.join("commands"))
                .unwrap()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "json"))
                .collect();
            files.sort();
            for file in files {
                let v: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
                let op = v["op"].as_str().unwrap();
                let message = match op {
                    "skin" => {
                        let id = v["id"].as_str().unwrap().to_owned();
                        Msg::Edit(Box::new(move |s| s.skin.id = id))
                    }
                    "send" => Msg::Core(Cmd::SendMarkup(v["text"].as_str().unwrap().to_owned())),
                    "skip" => Msg::Core(Cmd::Skip),
                    "clear" => Msg::Core(Cmd::ClearDisplay),
                    "outputs" => {
                        let enabled = v["enabled"].as_bool().unwrap();
                        Msg::Edit(Box::new(move |s| {
                            s.output.display.enabled = enabled;
                            s.output.history.enabled = enabled;
                        }))
                    }
                    "visible" => {
                        let visible = v["visible"].as_bool().unwrap();
                        Msg::Edit(Box::new(move |s| {
                            s.overlays.display.visible = visible;
                            s.overlays.history.visible = visible;
                        }))
                    }
                    "all_visible" => {
                        let visible = v["visible"].as_bool().unwrap();
                        Msg::Edit(Box::new(move |s| {
                            s.overlays.input.visible = visible;
                            s.overlays.display.visible = visible;
                            s.overlays.history.visible = visible;
                        }))
                    }
                    "motion" => {
                        let enabled = v["enabled"].as_bool().unwrap();
                        Msg::Edit(Box::new(move |s| s.skin.advanced_effects = enabled))
                    }
                    "size" => {
                        let width = v["width"].as_u64().unwrap() as u32;
                        Msg::Edit(Box::new(move |s| {
                            s.output.display.size = Some([width, 320]);
                            s.output.history.size = Some([width, 360]);
                        }))
                    }
                    "quit" => Msg::Quit,
                    _ => panic!("unknown probe command"),
                };
                {
                    let mut s = state.lock().unwrap();
                    let ms = s.start.elapsed().as_millis();
                    s.actions.push(json!({"ms":ms,"command":v}));
                }
                context.dispatch(message);
                std::fs::rename(&file, file.with_extension("done")).unwrap();
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    });
}
