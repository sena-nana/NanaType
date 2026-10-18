use nanatype_plugins::{PluginProcess, WorkerResponse};
use std::{path::Path, sync::mpsc, time::Duration};
#[test]
fn independent_worker_receives_events_and_reports_lua_failure_without_crashing_host() {
    let (sender, receiver) = mpsc::channel();
    let source = "return {on_event=function(e) if e.type=='command' then while true do end end end,view=function() return {views={{slot='panel',root={kind='text',id='ready',text='Ready'}}}} end}";
    let worker = PluginProcess::spawn(
        Path::new(env!("CARGO_BIN_EXE_nanatype-plugin-worker")),
        source.into(),
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
