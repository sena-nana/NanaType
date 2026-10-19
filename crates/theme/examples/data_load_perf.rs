//! Declared-data and pipeline binding costs for the Love Letter skin.
//! `cargo run -p nanatype-theme --release --example data_load_perf`
use nanatype_theme::SkinRegistry;
use std::time::Instant;

fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.
}

fn main() {
    let id = "nanatype.love_letter";
    // Cold: nothing parsed yet; prepares GPU payload and binds the stroke data.
    let start = Instant::now();
    let first = SkinRegistry::new(None, "1.0.0").load(id).unwrap();
    let cold = ms(start);
    // Warm: a fresh registry still shares parsed data by content hash.
    let mut warm = Vec::new();
    for _ in 0..20 {
        let start = Instant::now();
        let loaded = SkinRegistry::new(None, "1.0.0").load(id).unwrap();
        warm.push(ms(start));
        assert_eq!(loaded.skin.material_bindings, first.skin.material_bindings);
    }
    warm.sort_by(f64::total_cmp);
    // Import-time validation decompresses and checks every block.
    let registry = SkinRegistry::new(None, "1.0.0");
    let dir = std::env::temp_dir().join(format!("nanatype-data-perf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let archive = dir.join("letter.ntpkg");
    registry.export(id, &archive).unwrap();
    let start = Instant::now();
    registry.inspect_package_archive(&archive).unwrap();
    let import = ms(start);
    let _ = std::fs::remove_dir_all(&dir);
    println!(
        "{}",
        serde_json::json!({
            "skin": id,
            "cold_load_ms": cold,
            "warm_load_median_ms": warm[warm.len() / 2],
            "warm_load_max_ms": warm[warm.len() - 1],
            "import_inspection_ms": import,
        })
    );
}
