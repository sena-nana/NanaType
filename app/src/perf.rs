//! Opt-in native-window observations. No frame-path file I/O; report on exit.
use nana_ui::runtime::RuntimeDocument;
use nana_ui_platform::WindowId;
use std::{collections::HashMap, path::PathBuf, time::Instant};

#[derive(Default)]
struct Window {
    prepared: Option<Instant>,
    presented: u64,
    outputs: u64,
    presented_after_6s: u64,
    outputs_after_6s: u64,
    revision: Option<u64>,
    text_shaped: u64,
    layout_nodes: u64,
    draw_calls: Option<u64>,
    upload_bytes: Option<u64>,
    samples: Vec<f64>,
    spout_sent: u64,
    spout_skipped: u64,
    spout_ms: Vec<f64>,
}

pub struct Recorder {
    path: PathBuf,
    started: Instant,
    windows: HashMap<WindowId, Window>,
}
impl Recorder {
    pub fn from_env() -> Option<Self> {
        Some(Self {
            path: std::env::var_os("NANATYPE_PERF").map(PathBuf::from)?,
            started: Instant::now(),
            windows: HashMap::new(),
        })
    }
    pub fn begin(&mut self, id: WindowId) {
        self.windows.entry(id).or_default().prepared = Some(Instant::now());
    }
    /// A frame handed to Spout: whether it was taken, and how long it held
    /// the UI thread.
    pub fn spout(&mut self, id: WindowId, sent: bool, ms: f64) {
        if self.started.elapsed().as_millis() < 2000 {
            return;
        }
        let window = self.windows.entry(id).or_default();
        if sent {
            window.spout_sent += 1;
        } else {
            window.spout_skipped += 1;
        }
        if window.spout_ms.len() < 10_000 {
            window.spout_ms.push(ms);
        }
    }
    pub fn frame(&mut self, id: WindowId, output: bool, doc: Option<&RuntimeDocument>) {
        // Discard startup and initial pipeline compilation consistently.
        if self.started.elapsed().as_millis() < 2000 {
            return;
        }
        let window = self.windows.entry(id).or_default();
        if output {
            window.outputs += 1;
            window.outputs_after_6s += u64::from(self.started.elapsed().as_millis() >= 6000);
        } else {
            window.presented += 1;
            window.presented_after_6s += u64::from(self.started.elapsed().as_millis() >= 6000);
        }
        if let Some(start) = window.prepared.take() {
            if window.samples.len() < 10_000 {
                window.samples.push(start.elapsed().as_secs_f64() * 1000.0);
            }
            if let Some(doc) = doc {
                let cx = doc.context();
                let work = cx.last_work_counters();
                let revision = cx.world().pending_work_revision();
                if window.revision != Some(revision) {
                    window.revision = Some(revision);
                    window.text_shaped += work.text_shaped as u64;
                    window.layout_nodes += work.layout_nodes as u64;
                }
                sum(&mut window.draw_calls, work.draw_calls);
                sum(&mut window.upload_bytes, work.gpu_upload_bytes);
            }
        }
    }
}
fn quantile(sorted: &[f64], percent: usize) -> Option<f64> {
    (!sorted.is_empty()).then(|| sorted[sorted.len() * percent / 100])
}
fn sum(total: &mut Option<u64>, value: Option<usize>) {
    if let Some(value) = value {
        *total = Some(total.unwrap_or(0) + value as u64);
    }
}
impl Drop for Recorder {
    fn drop(&mut self) {
        let windows:Vec<_>=self.windows.iter_mut().map(|(id,w)| {
            w.samples.sort_by(f64::total_cmp);
            w.spout_ms.sort_by(f64::total_cmp);
            let median=quantile(&w.samples,50);
            // Stutter is in the tail: a median hides the frame that stalled.
            let slow=w.samples.iter().filter(|ms|**ms>33.0).count();
            serde_json::json!({"window":format!("{:?}",crate::windows::WindowKind::from_id(*id)),"presented_frames":w.presented,"output_frames":w.outputs,
                "presented_after_6s":w.presented_after_6s,"output_after_6s":w.outputs_after_6s,
                "prepare_to_callback_median_ms":median,"prepare_to_callback_p95_ms":quantile(&w.samples,95),
                "prepare_to_callback_p99_ms":quantile(&w.samples,99),"prepare_to_callback_max_ms":w.samples.last(),"frames_over_33ms":slow,
                "spout_sent":w.spout_sent,"spout_skipped":w.spout_skipped,
                "spout_send_median_ms":quantile(&w.spout_ms,50),"spout_send_max_ms":w.spout_ms.last(),
                "text_shaped":w.text_shaped,"layout_nodes":w.layout_nodes,"draw_calls":w.draw_calls,"gpu_upload_bytes":w.upload_bytes})
        }).collect();
        let report = serde_json::json!({"schema":1,"warmup_ms":2000,"elapsed_ms":self.started.elapsed().as_millis(),"windows":windows,
            "measurement":"Native frame/output callbacks. Prepare-to-callback is host wall time, not a GPU timestamp. Missing GPU counters are null. See gpu_perf for GPU timestamps."});
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_vec_pretty(&report) {
            let _ = std::fs::write(&self.path, json);
        }
    }
}
