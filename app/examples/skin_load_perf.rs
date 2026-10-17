//! Configuration pipeline benchmark, separate from retained GPU frame timing.
//! cargo run -p nanatype --example skin_load_perf -- <output.json>
use nanatype_theme::{MANIFEST, SkinRegistry};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc, time::Instant};

fn measure(mut f: impl FnMut(), count: usize) -> Value {
    let mut ms: Vec<_> = (0..count)
        .map(|_| {
            let start = Instant::now();
            f();
            start.elapsed().as_secs_f64() * 1000.
        })
        .collect();
    ms.sort_by(f64::total_cmp);
    json!({"iterations": count, "median_ms": ms[count/2], "p95_ms": ms[count*95/100]})
}

fn main() {
    let out = PathBuf::from(std::env::args().nth(1).expect("output JSON"));
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    nanatype::services::media::init(&out.parent().unwrap().join("load-perf-media")).unwrap();
    let start = Instant::now();
    let mut registry = SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
    let cold = start.elapsed().as_secs_f64() * 1000.;
    let ids: Vec<_> = registry.entries().iter().map(|e| e.id.clone()).collect();
    let mut skins = vec![];
    let mut manifest_bytes = 0;
    let mut compact_export_bytes = 0;
    let mut expanded_export_bytes = 0;
    let mut decoded_bytes = 0;
    for id in &ids {
        let start = Instant::now();
        let a = registry.load(id).unwrap();
        let first_load_ms = start.elapsed().as_secs_f64() * 1000.;
        let b = registry.load(id).unwrap();
        manifest_bytes += a.files.read(MANIFEST).unwrap().len();
        compact_export_bytes += nanatype_theme::standalone_manifest(&a.skin)
            .unwrap()
            .to_string()
            .len();
        expanded_export_bytes += nanatype_theme::TomlTable::try_from(&*a.skin)
            .unwrap()
            .to_string()
            .len();
        decoded_bytes += a
            .skin
            .shader_programs
            .values()
            .flat_map(|p| &p.textures)
            .map(|t| t.bytes.len())
            .sum::<usize>();
        let timing = measure(
            || {
                std::hint::black_box(registry.load(id).unwrap());
            },
            5,
        );
        skins.push(
            json!({"id": id, "load": timing, "first_load_ms": first_load_ms,
            "shared_validated_skin": Arc::ptr_eq(&a.skin, &b.skin),
            "programs": a.skin.shader_programs.len()}),
        );
    }
    let all = measure(
        || {
            for id in &ids {
                std::hint::black_box(registry.load(id).unwrap());
            }
        },
        5,
    );
    // Warm materialized image URLs before measuring picker refreshes.
    std::hint::black_box(nanatype::ui::skin_rows(
        &registry,
        nanatype_i18n::Locale::ZhCn,
    ));
    let picker = measure(
        || {
            std::hint::black_box(nanatype::ui::skin_rows(
                &registry,
                nanatype_i18n::Locale::ZhCn,
            ));
        },
        5,
    );
    let rescan = measure(|| registry.rescan(), 5);
    let result = json!({"skin_count": ids.len(), "cold_registry_ms": cold,
        "manifest_bytes": manifest_bytes, "decoded_texture_bytes": decoded_bytes,
        "compact_export_bytes": compact_export_bytes, "expanded_export_bytes": expanded_export_bytes,
        "load_all": all, "warm_picker": picker, "rescan": rescan, "skins": skins});
    std::fs::write(&out, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
    println!(
        "{} skins; cold {cold:.1} ms; load-all {:.2} ms; picker {:.2} ms; rescan {:.2} ms",
        ids.len(),
        result["load_all"]["median_ms"].as_f64().unwrap(),
        result["warm_picker"]["median_ms"].as_f64().unwrap(),
        result["rescan"]["median_ms"].as_f64().unwrap()
    );
}
