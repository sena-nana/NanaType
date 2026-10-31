//! Windows resources for NanaType (see `crates/frame/build_support.rs`), and
//! the link fixups only NanaType's libraries need. `TaskDialogIndirect` is
//! pulled in by the tray stack, so the Common Controls 6 manifest matters here.

include!("../crates/frame/build_support.rs");

const PRODUCT: Product = Product {
    bin: "nanatype",
    icon: "assets/nanatype.ico",
    name: "NanaType",
};

fn main() {
    let build = build_id(&PRODUCT);
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        // cargo run has no .app bundle. TCC still requires the usage strings
        // in the executable's Info.plist when requesting microphone/speech access.
        println!("cargo:rerun-if-changed=../packaging/Info.extra.plist");
        let plist = include_str!("../packaging/Info.extra.plist").replace("<dict>",
            "<dict><key>CFBundleIdentifier</key><string>dev.sena-nana.nanatype</string><key>CFBundleName</key><string>NanaType</string>");
        let path = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap())
            .join("speech-info.plist");
        std::fs::write(&path, plist).expect("write development Info.plist");
        println!(
            "cargo:rustc-link-arg-bin=nanatype=-Wl,-sectcreate,__TEXT,__info_plist,{}",
            path.display()
        );
    }
    if !targets_windows() {
        return;
    }
    if targets_msvc() {
        // Prebuilt libvpx omits vpxmd.pdb; Rust debug information is retained.
        println!("cargo:rustc-link-arg=/IGNORE:4099");

        if std::env::var("CARGO_CFG_TARGET_FEATURE")
            .unwrap_or_default()
            .split(',')
            .any(|feature| feature == "crt-static")
        {
            // Bind libvpx's DLL-imported CRT functions to Rust's static CRT.
            println!("cargo:rustc-link-arg=/NODEFAULTLIB:MSVCRT");
            for symbol in ["_beginthreadex", "fopen", "round"] {
                println!("cargo:rustc-link-arg=/INCLUDE:{symbol}");
            }
            // These are LINK's expected local-import fixups for that archive.
            println!("cargo:rustc-link-arg=/IGNORE:4217,4286");
        }

        common_controls(&format!("cargo:rustc-link-arg-bin={}=", PRODUCT.bin));
        main_stack(&format!("cargo:rustc-link-arg-bin={}=", PRODUCT.bin));
        // Standalone snapshot/performance tools link the same tray stack.
        common_controls("cargo:rustc-link-arg-examples=");
        main_stack("cargo:rustc-link-arg-examples=");
    }
    windows_resources(&PRODUCT, &build);
}
