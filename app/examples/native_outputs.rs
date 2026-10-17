//! Real OBS/Spout validation uses isolated paths and the production Host.
#[cfg(all(windows, debug_assertions))]
#[path = "native_outputs/program.rs"]
mod program;
#[cfg(all(windows, debug_assertions))]
fn main() {
    program::run();
}
#[cfg(not(all(windows, debug_assertions)))]
fn main() {
    eprintln!("native_outputs requires a Windows debug build and DX12");
}
