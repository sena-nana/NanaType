//! Local wall-clock offset for history timestamps, without a date library.

use std::sync::OnceLock;

/// Seconds east of UTC, read once.
pub fn offset_seconds() -> i64 {
    static OFFSET: OnceLock<i64> = OnceLock::new();
    *OFFSET.get_or_init(read)
}

#[cfg(windows)]
fn read() -> i64 {
    use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
    const TIME_ZONE_ID_DAYLIGHT: u32 = 2;
    let mut tz = TIME_ZONE_INFORMATION::default();
    let id = unsafe { GetTimeZoneInformation(&mut tz) };
    let mut bias = tz.Bias;
    if id == TIME_ZONE_ID_DAYLIGHT {
        bias += tz.DaylightBias;
    }
    -i64::from(bias) * 60
}

#[cfg(not(windows))]
fn read() -> i64 {
    // `date +%z` prints e.g. +0900.
    std::process::Command::new("date")
        .arg("+%z")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| {
            let s = s.trim();
            let sign = if s.starts_with('-') { -1 } else { 1 };
            let digits = s.trim_start_matches(['+', '-']);
            let h: i64 = digits.get(0..2)?.parse().ok()?;
            let m: i64 = digits.get(2..4)?.parse().ok()?;
            Some(sign * (h * 3600 + m * 60))
        })
        .unwrap_or(0)
}
