//! Real worker processes, not the in-VM benchmark: cold start, end-to-end
//! event latency with 1/4/8 concurrent plugins, bursts against the bounded
//! queue, host-side call cost, worker memory and repeated start/stop.
//!
//! `cargo test -p nanatype-plugins --release --test perf -- --ignored --nocapture`
//! prints one JSON object per measurement; docs/plugin-performance.md records
//! the method and a reference run.
use nanatype_plugins::{DataAnswer, DataReply, EventError, PluginProcess, WorkerResponse};
use std::{
    path::PathBuf,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

const SOURCE: &str = include_str!("../../../docs/examples/counter-plugin/main.lua");

fn worker() -> PathBuf {
    std::env::var_os("NANATYPE_PLUGIN_WORKER")
        .map(PathBuf::from)
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_nanatype-plugin-worker").into())
}

fn no_data() -> DataAnswer {
    Arc::new(|r| DataReply::error(r.token, "no data"))
}

/// Spawn one worker whose posts carry its index and arrival time.
fn spawn(index: usize, posts: mpsc::Sender<(usize, Instant, bool)>) -> PluginProcess {
    PluginProcess::spawn(
        &worker(),
        SOURCE.into(),
        serde_json::json!({"type": "init"}),
        no_data(),
        move |response| {
            let ok = matches!(response, WorkerResponse::Update { .. });
            let _ = posts.send((index, Instant::now(), ok));
        },
    )
    .unwrap()
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.
}

fn stats(name: &str, extra: serde_json::Value, mut samples: Vec<f64>) -> serde_json::Value {
    samples.sort_by(f64::total_cmp);
    let at = |q: f64| samples[((samples.len() - 1) as f64 * q).round() as usize];
    let mut out = serde_json::json!({
        "measure": name,
        "samples": samples.len(),
        "p50_ms": at(0.5),
        "p95_ms": at(0.95),
        "max_ms": samples[samples.len() - 1],
    });
    if let (Some(out), serde_json::Value::Object(extra)) = (out.as_object_mut(), extra) {
        out.extend(extra);
    }
    println!("{out}");
    out
}

#[cfg(unix)]
fn usage(pid: u32) -> Option<(u64, Option<f64>)> {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    Some((
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()?,
        None,
    ))
}
#[cfg(windows)]
fn usage(pid: u32) -> Option<(u64, Option<f64>)> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, FILETIME},
        System::{
            ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
            Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
        },
    };
    // The host reads its own worker's working set; this is not private commit.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut memory = PROCESS_MEMORY_COUNTERS::default();
        let ok = K32GetProcessMemoryInfo(
            handle,
            &mut memory,
            size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        );
        let (mut created, mut exited, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        let times = GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user);
        CloseHandle(handle);
        let ticks = |t: FILETIME| ((t.dwHighDateTime as u64) << 32) | t.dwLowDateTime as u64;
        (ok != 0).then_some((
            memory.WorkingSetSize as u64 / 1024,
            (times != 0).then(|| (ticks(kernel) + ticks(user)) as f64 / 10_000.),
        ))
    }
}
#[cfg(not(any(unix, windows)))]
fn usage(_: u32) -> Option<(u64, Option<f64>)> {
    None
}

#[test]
#[ignore = "measurement; run in release with --ignored --nocapture"]
fn real_worker_processes() {
    println!(
        "{}",
        serde_json::json!({"measure":"configuration", "worker":worker(), "profile":if cfg!(debug_assertions) {"debug/test"} else {"release/test"}})
    );
    // Cold start: process spawn, interpreter instantiation, Init and the init event.
    let mut cold = Vec::new();
    let mut cycles = Vec::new();
    for _ in 0..20 {
        let (send, posts) = mpsc::channel();
        let started = Instant::now();
        let process = spawn(0, send);
        let (_, at, ok) = posts.recv_timeout(Duration::from_secs(20)).unwrap();
        assert!(ok);
        cold.push(ms(at - started));
        drop(process);
        cycles.push(ms(started.elapsed()));
    }
    stats("cold_start", serde_json::json!({}), cold);
    stats("start_stop_cycle", serde_json::json!({}), cycles);

    for plugins in [1, 4, 8] {
        let (send, posts) = mpsc::channel();
        let workers: Vec<_> = (0..plugins).map(|i| spawn(i, send.clone())).collect();
        for _ in 0..plugins {
            posts.recv_timeout(Duration::from_secs(20)).unwrap();
        }
        // Warm up, then broadcast spaced events (beyond the 33 ms post throttle).
        let mut latency = Vec::new();
        let mut host_call = Vec::new();
        for n in 0..230 {
            let sent = Instant::now();
            for w in &workers {
                let call = Instant::now();
                w.event(serde_json::json!({"type": "line_start", "id": n, "text": "台词性能测试 Plugin event"}))
                    .unwrap();
                host_call.push(ms(call.elapsed()));
            }
            for _ in 0..plugins {
                let (_, at, ok) = posts.recv_timeout(Duration::from_secs(5)).unwrap();
                assert!(ok);
                if n >= 30 {
                    latency.push(ms(at - sent));
                }
            }
            std::thread::sleep(Duration::from_millis(45));
        }
        let usage: Vec<_> = workers.iter().filter_map(|w| usage(w.id())).collect();
        let cpu: Option<Vec<_>> = usage.iter().map(|(_, cpu)| *cpu).collect();
        stats(
            "event_end_to_end",
            serde_json::json!({
                "plugins": plugins,
                "worker_rss_kib_max": usage.iter().map(|(rss, _)| rss).max(),
                "worker_rss_kib_total": (!usage.is_empty()).then(|| usage.iter().map(|(rss, _)| rss).sum::<u64>()),
                "worker_cpu_ms_total": cpu.filter(|v| !v.is_empty()).map(|v| v.iter().sum::<f64>()),
                "worker_usage_samples": usage.len(),
            }),
            latency,
        );
        stats(
            "host_event_call",
            serde_json::json!({"plugins": plugins}),
            host_call,
        );

        // Burst: 200 events at once per plugin against the 32-slot queue.
        let mut accepted = 0;
        let mut busy = 0;
        let mut burst_call = Vec::new();
        let started = Instant::now();
        for n in 0..200 {
            for w in &workers {
                let call = Instant::now();
                match w.event(serde_json::json!({"type": "line_start", "id": n})) {
                    Ok(()) => accepted += 1,
                    Err(EventError::Busy) => busy += 1,
                    Err(e) => panic!("{e}"),
                }
                burst_call.push(ms(call.elapsed()));
            }
        }
        // Drain: no post for 500 ms means every queued event was handled.
        let mut updates = 0;
        while posts.recv_timeout(Duration::from_millis(500)).is_ok() {
            updates += 1;
        }
        stats(
            "burst_host_call",
            serde_json::json!({
                "plugins": plugins,
                "accepted": accepted,
                "busy_dropped": busy,
                "posted_updates": updates,
                "drain_ms": ms(started.elapsed()) - 500.,
            }),
            burst_call,
        );
        // Every worker is still healthy after saturation.
        for w in &workers {
            w.event(serde_json::json!({"type": "line_end"})).unwrap();
        }
        for _ in 0..plugins {
            assert!(posts.recv_timeout(Duration::from_secs(5)).unwrap().2);
        }
    }
}
