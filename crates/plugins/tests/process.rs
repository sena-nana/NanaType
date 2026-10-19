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
    let mut last = String::new();
    for _ in 0..=MAX_DATA_ROUNDS {
        last = text(&receiver.recv_timeout(Duration::from_secs(20)).unwrap());
    }
    assert_eq!(last, format!("string:hello#{MAX_DATA_ROUNDS}"));
    assert!(
        receiver.recv_timeout(Duration::from_millis(500)).is_err(),
        "requests beyond the round budget wait for the next host event"
    );
    assert_eq!(calls.load(Ordering::SeqCst), MAX_DATA_ROUNDS);
    worker.event(serde_json::json!({"type":"command"})).unwrap();
    for _ in 0..=MAX_DATA_ROUNDS {
        receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2 * MAX_DATA_ROUNDS);
}
