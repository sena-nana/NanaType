//! Compile only the pinned application interpreter, never user-provided modules.
use sha2::{Digest, Sha256};
fn main() {
    println!("cargo:rerun-if-changed=runtime/lua.wasm");
    let wasm = std::fs::read("runtime/lua.wasm").expect("pinned Lua interpreter");
    assert_eq!(
        format!("{:x}", Sha256::digest(&wasm)),
        "95f3f19ddb740125883bc41d5ec670cd0828e7b58c8bdad385323cbff497b55c",
        "Lua interpreter differs from the reviewed artifact"
    );
    let mut config = wasmtime::Config::new();
    config.consume_fuel(true).epoch_interruption(true);
    config
        .target(&std::env::var("TARGET").unwrap())
        .expect("supported plugin target");
    let engine = wasmtime::Engine::new(&config).expect("interpreter compiler");
    // Windows build-script main threads have a small default stack. Large
    // interpreter functions need a larger compilation stack in release too.
    let compiled = std::thread::Builder::new()
        .name("compile-pinned-lua".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(move || engine.precompile_module(&wasm))
        .expect("start interpreter compiler")
        .join()
        .expect("interpreter compiler thread")
        .expect("compile pinned Lua interpreter");
    let output = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("lua.cwasm");
    std::fs::write(output, compiled).expect("write application-owned interpreter artifact");
}
