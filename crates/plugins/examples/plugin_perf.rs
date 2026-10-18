//! Standalone, repeatable plugin costs; no application, network or filesystem API.
use nanatype_plugins::LuaSandbox;
use std::time::Instant;
fn main() {
    let source = include_str!("../../../docs/examples/counter-plugin/main.lua");
    let start = Instant::now();
    let mut vm = LuaSandbox::new(source).unwrap();
    let startup = start.elapsed().as_secs_f64() * 1000.;
    vm.event(&serde_json::json!({"type":"init"})).unwrap();
    let mut timings = Vec::new();
    for n in 0..1100 {
        let start = Instant::now();
        vm.event(
            &serde_json::json!({"type":"line_start","id":n,"text":"台词性能测试 Plugin event"}),
        )
        .unwrap();
        if n >= 100 {
            timings.push(start.elapsed().as_secs_f64() * 1000.);
        }
    }
    timings.sort_by(f64::total_cmp);
    println!(
        "{}",
        serde_json::json!({"startup_ms":startup,"events":timings.len(),"median_ms":timings[timings.len()/2],"p95_ms":timings[timings.len()*95/100],"max_ms":timings[timings.len()-1]})
    );
}
