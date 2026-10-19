//! Helpers shared by the measurement examples: `mod common;` in each one.

/// Sample count, median, 95th percentile and maximum of millisecond timings;
/// `null` when nothing was measured.
pub fn distribution(mut samples: Vec<f64>) -> serde_json::Value {
    if samples.is_empty() {
        return serde_json::Value::Null;
    }
    samples.sort_by(f64::total_cmp);
    serde_json::json!({
        "samples": samples.len(),
        "median_ms": samples[samples.len() / 2],
        "p95_ms": samples[samples.len() * 95 / 100],
        "max_ms": samples.last(),
    })
}
