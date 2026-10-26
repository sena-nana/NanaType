//! Fixed input workload; print reproducible measurements without product telemetry.
use nanatype_editor::session;
use nanatype_theme::editor::ProjectKind;
use std::time::Instant;
fn workload(signed: bool, toml: bool) -> serde_json::Value {
    let mut s = session::create(ProjectKind::Lua, "editor.perf").unwrap();
    let asset = s.import("large.txt", vec![b'a'; 16 * 1024 * 1024]).unwrap();
    let base = if toml {
        (0..16000)
            .map(|i| format!("key_{i:05} = {i}\n"))
            .collect::<String>()
    } else {
        format!("-- {}\nreturn {{}}\n", "x".repeat(256 * 1024))
    };
    let path = if toml {
        s.import("large.toml", base.as_bytes().to_vec()).unwrap()
    } else {
        s.edit("main.lua", &base).unwrap();
        "main.lua".into()
    };
    if signed {
        s.sign(
            &nanatype_theme::SealIdentity::generate().unwrap(),
            &std::collections::BTreeSet::from([asset]),
        )
        .unwrap();
    }
    let dir = tempfile::tempdir().unwrap();
    s.save(dir.path()).unwrap();
    s.select(&path);
    let mut times = Vec::new();
    const WARMUP: usize = 200;
    const SAMPLES: usize = 1000;
    for i in 0..WARMUP + SAMPLES {
        let text = format!("{base}{} {i}\n", if toml { "#" } else { "--" });
        let start = Instant::now();
        s.edit(&path, &text).unwrap();
        let _ = s.view(None, false);
        let elapsed = start.elapsed().as_secs_f64() * 1000.;
        if i >= WARMUP {
            times.push(elapsed);
        }
    }
    times.sort_by(f64::total_cmp);
    serde_json::json!({"syntax":if toml {"toml"}else{"lua"},"signed_asset": signed,"source_bytes":base.len(),"asset_bytes":16*1024*1024,"warmup":WARMUP,"samples":times.len(),"edit_and_view_p50_ms":times[SAMPLES/2],"edit_and_view_p95_ms":times[SAMPLES*95/100]})
}
fn main() {
    println!(
        "{}",
        serde_json::json!({"unsigned_lua":workload(false,false),"protected_asset_lua":workload(true,false),"unsigned_toml":workload(false,true)})
    );
}
