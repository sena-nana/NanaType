#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// The unoptimized binary's __eh_frame outgrows the 16 MB that ld's compact
// unwind table can address; that only slows panic unwinding.
#![cfg_attr(all(debug_assertions, target_os = "macos"), allow(linker_messages))]

fn main() -> std::process::ExitCode {
    nanatype::run()
}
