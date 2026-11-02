//! Lenient field readers: chat servers send ids, counts and times as
//! strings or numbers interchangeably.
use serde_json::Value;

/// A string or a number as text (uids and ids come as either).
pub fn string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

/// A number, a numeric string or a bool as an integer; 0 otherwise.
pub fn int(v: &Value) -> i64 {
    match v {
        Value::Number(n) => n
            .as_i64()
            .or_else(|| n.as_f64().map(|f| f as i64))
            .unwrap_or(0),
        Value::String(s) => s.trim().parse().unwrap_or(0),
        Value::Bool(b) => i64::from(*b),
        _ => 0,
    }
}

/// Seconds or milliseconds (sources differ) as milliseconds.
pub fn millis(v: &Value) -> u64 {
    let t = match v {
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        Value::String(s) => s.trim().parse().unwrap_or(0.0),
        _ => 0.0,
    };
    if !t.is_finite() || t <= 0.0 {
        0
    } else if t > 1e12 {
        t as u64
    } else {
        (t * 1000.0) as u64
    }
}

/// Avatars and emoticons come scheme-relative (`//i0.hdslb.com/...`).
pub fn url(v: &Value) -> String {
    let s = string(v);
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("//") {
        format!("https://{rest}")
    } else if let Some(rest) = s.strip_prefix("http://") {
        format!("https://{rest}")
    } else {
        s.to_owned()
    }
}
