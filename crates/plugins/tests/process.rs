use nanatype_plugins::{DataAnswer, DataReply, MAX_DATA_ROUNDS, PluginProcess, WorkerResponse};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};
fn no_data() -> DataAnswer {
    Arc::new(|r| DataReply::error(r.token, "no data"))
}
fn text(response: &WorkerResponse) -> String {
    let WorkerResponse::Update { update, .. } = response else {
        panic!("{response:?}");
    };
    match &update.views[0].root {
        nanatype_plugins::Node::Text { text, .. } => text.clone(),
        node => panic!("{node:?}"),
    }
}
#[test]
fn independent_worker_receives_events_and_reports_lua_failure_without_crashing_host() {
    let (sender, receiver) = mpsc::channel();
    let source = "return {on_event=function(e) if e.type=='command' then while true do end end end,view=function() return {views={{slot='panel',root={kind='text',id='ready',text='Ready'}}}} end}";
    let worker = PluginProcess::spawn(
        Path::new(env!("CARGO_BIN_EXE_nanatype-plugin-worker")),
        source.into(),
        serde_json::json!({"type": "init"}),
        no_data(),
        move |response| {
            let _ = sender.send(response);
        },
    )
    .unwrap();
    assert!(matches!(
        receiver.recv_timeout(Duration::from_secs(20)).unwrap(),
        WorkerResponse::Update { .. }
    ));
    worker.event(serde_json::json!({"type":"command"})).unwrap();
    assert!(matches!(
        receiver.recv_timeout(Duration::from_secs(2)).unwrap(),
        WorkerResponse::Error { .. }
    ));
}

#[test]
fn data_answers_arrive_as_values_and_chained_requests_are_bounded() {
    let (sender, receiver) = mpsc::channel();
    // Every answer triggers another request: only MAX_DATA_ROUNDS are served per event.
    let source = r#"
        local last, seen = 'none', 0
        return {
            on_event = function(e)
                if e.type == 'init' or e.type == 'command' then
                    nana.data.request('words', {kind = 'pointer', pointer = '/0'})
                elseif e.type == 'data' then
                    seen = seen + 1
                    last = type(e.results[1].value) .. ':' .. tostring(e.results[1].value)
                    nana.data.request('words', {kind = 'pointer', pointer = '/0'})
                end
            end,
            view = function()
                return {views = {{slot = 'panel', root = {kind = 'text', id = 't', text = last .. '#' .. seen}}}}
            end,
        }"#;
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let answer: DataAnswer = Arc::new(move |r| {
        counted.fetch_add(1, Ordering::SeqCst);
        assert_eq!(r.id, "words");
        DataReply {
            token: r.token,
            value: Some(serde_json::json!("hello")),
            error: None,
        }
    });
    let worker = PluginProcess::spawn(
        Path::new(env!("CARGO_BIN_EXE_nanatype-plugin-worker")),
        source.into(),
        serde_json::json!({"type": "init", "reads": ["words"]}),
        answer,
        move |response| {
            let _ = sender.send(response);
        },
    )
    .unwrap();
    // Chained rounds run without waiting for the UI; only the newest view is posted.
    let last = text(&receiver.recv_timeout(Duration::from_secs(20)).unwrap());
    assert_eq!(last, format!("string:hello#{MAX_DATA_ROUNDS}"));
    assert_eq!(calls.load(Ordering::SeqCst), MAX_DATA_ROUNDS);
    worker.event(serde_json::json!({"type":"command"})).unwrap();
    let last = text(&receiver.recv_timeout(Duration::from_secs(5)).unwrap());
    assert_eq!(last, format!("string:hello#{}", 2 * MAX_DATA_ROUNDS));
    assert_eq!(calls.load(Ordering::SeqCst), 2 * MAX_DATA_ROUNDS);
}

fn spawn(executable: &Path, source: &str) -> (PluginProcess, mpsc::Receiver<WorkerResponse>) {
    let (sender, receiver) = mpsc::channel();
    let worker = PluginProcess::spawn(
        executable,
        source.into(),
        serde_json::json!({"type": "init"}),
        no_data(),
        move |response| {
            let _ = sender.send(response);
        },
    )
    .unwrap();
    (worker, receiver)
}

#[cfg(unix)]
fn alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(windows)]
fn alive(pid: u32) -> bool {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };
    // Only inspect the fixture worker started by this test.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut code = 0;
        let running = GetExitCodeProcess(handle, &mut code) != 0 && code == 259;
        CloseHandle(handle);
        running
    }
}

#[test]
fn crashed_or_foreign_workers_report_errors_instead_of_hanging_the_host() {
    // A worker that exits at once (a crash) and one that echoes our own
    // requests back (a protocol violation) both end with a prompt error.
    let executables = if cfg!(windows) {
        // The test harness rejects --plugin-worker and exits without Ready.
        vec![std::env::current_exe().unwrap()]
    } else {
        vec!["/usr/bin/true".into(), "/bin/cat".into()]
    };
    for executable in executables {
        if !executable.exists() {
            continue;
        }
        let started = std::time::Instant::now();
        let (worker, responses) = spawn(&executable, "return {}");
        assert!(
            matches!(
                responses.recv_timeout(Duration::from_secs(5)),
                Ok(WorkerResponse::Error { .. })
            ),
            "{executable:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(5), "{executable:?}");
        // The supervisor finishes right after posting the error.
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while worker.event(serde_json::json!({"type": "line_start"}))
            != Err(nanatype_plugins::EventError::Stopped)
        {
            assert!(
                std::time::Instant::now() < deadline,
                "{executable:?}: a dead worker must be reported as stopped"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[test]
fn dropping_or_failing_a_worker_reaps_its_process_and_a_fresh_one_starts_cleanly() {
    let worker_path = Path::new(env!("CARGO_BIN_EXE_nanatype-plugin-worker"));
    let looping = "return {on_event=function(e) if e.type=='spin' then while true do end end end, view=function() return {} end}";
    let (worker, responses) = spawn(worker_path, looping);
    responses.recv_timeout(Duration::from_secs(20)).unwrap();
    let pid = worker.id();
    assert!(alive(pid));
    worker.event(serde_json::json!({"type": "spin"})).unwrap();
    assert!(matches!(
        responses.recv_timeout(Duration::from_secs(5)).unwrap(),
        WorkerResponse::Error { .. }
    ));
    // The supervisor kills and waits for a failed worker by itself.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while alive(pid) && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!alive(pid), "failed worker {pid} was not reaped");
    drop(worker);
    assert!(
        matches!(
            responses.recv_timeout(Duration::from_secs(5)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ),
        "failed supervisor must exit"
    );
    // Repeated start/stop leaves nothing behind.
    for _ in 0..5 {
        let (worker, responses) = spawn(worker_path, "return {view=function() return {} end}");
        responses.recv_timeout(Duration::from_secs(20)).unwrap();
        let pid = worker.id();
        drop(worker);
        assert!(!alive(pid), "dropped worker {pid} is still running");
        assert!(
            matches!(
                responses.recv_timeout(Duration::from_secs(5)),
                Err(mpsc::RecvTimeoutError::Disconnected)
            ),
            "stopped supervisor must exit"
        );
    }
}

#[test]
fn bursts_coalesce_into_the_newest_view_instead_of_lagging_behind() {
    let (sender, receiver) = mpsc::channel();
    let source = "local n=0 return {on_event=function(e) if e.type=='tick' then n=n+1 end end, view=function() return {views={{slot='panel',root={kind='text',id='t',text=tostring(n)}}}} end}";
    let worker = PluginProcess::spawn(
        Path::new(env!("CARGO_BIN_EXE_nanatype-plugin-worker")),
        source.into(),
        serde_json::json!({"type": "init"}),
        no_data(),
        move |response| {
            let _ = sender.send((std::time::Instant::now(), response));
        },
    )
    .unwrap();
    receiver.recv_timeout(Duration::from_secs(20)).unwrap();
    let started = std::time::Instant::now();
    let mut accepted = 0;
    while accepted < 32 && worker.event(serde_json::json!({"type": "tick"})).is_ok() {
        accepted += 1;
    }
    // Every accepted event is applied; the UI sees few posts and the final count quickly.
    let mut posts = 0;
    loop {
        let (at, response) = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        posts += 1;
        if text(&response) == accepted.to_string() {
            assert!(
                at - started < Duration::from_millis(500),
                "{accepted} queued events took {:?} to reach the UI",
                at - started
            );
            break;
        }
    }
    assert!(
        posts < accepted,
        "{posts} posts for {accepted} events: not coalesced"
    );
}

#[test]
fn heavy_canvases_repaint_less_often_than_light_views() {
    let draw = nanatype_plugins::MAX_DRAW_COMMANDS;
    let heavy = format!(
        "local n=0 return {{on_event=function() n=n+1 end, view=function() local d={{}} for i=1,{draw} do d[i]={{kind='rect',rect={{(i+n)%300,(i*3)%160,6,6}},color={{i%255,80,200,255}},radius=2}} end return {{views={{{{slot='panel',root={{kind='canvas',id='c',width=320,height=180,draw=d}}}}}}}} end}}"
    );
    let light = "local n=0 return {on_event=function() n=n+1 end, view=function() return {views={{slot='panel',root={kind='text',id='t',text=tostring(n)}}}} end}";
    let mut gaps = Vec::new();
    for (source, cost) in [(heavy.as_str(), draw), (light, 0)] {
        let (sender, receiver) = mpsc::channel();
        let worker = PluginProcess::spawn(
            Path::new(env!("CARGO_BIN_EXE_nanatype-plugin-worker")),
            source.into(),
            serde_json::json!({"type": "init"}),
            no_data(),
            move |response| {
                let _ = sender.send((std::time::Instant::now(), response));
            },
        )
        .unwrap();
        let (_, first) = receiver.recv_timeout(Duration::from_secs(20)).unwrap();
        let WorkerResponse::Update { update, .. } = first else {
            panic!("worker failed before measuring paint cost {cost}: {first:?}");
        };
        assert_eq!(update.paint_cost(), cost, "measure the full authored view");
        for _ in 0..30 {
            let _ = worker.event(serde_json::json!({"type": "tick"}));
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut times = Vec::new();
        while let Ok((at, response)) = receiver.recv_timeout(Duration::from_millis(600)) {
            assert!(
                matches!(response, WorkerResponse::Update { .. }),
                "worker failed while measuring paint cost {cost}: {response:?}"
            );
            times.push(at);
        }
        assert!(times.len() >= 2, "too few posts to measure");
        gaps.push(times.windows(2).map(|w| w[1] - w[0]).min().unwrap());
    }
    // 2048 draw commands: four extra 33 ms windows between posts.
    assert!(
        gaps[0] >= Duration::from_millis(150),
        "heavy gap {:?}",
        gaps[0]
    );
    assert!(
        gaps[1] < Duration::from_millis(100),
        "light gap {:?}",
        gaps[1]
    );
}
