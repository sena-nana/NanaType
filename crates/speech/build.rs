fn main() {
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    if target == "macos" {
        println!("cargo:rerun-if-changed=native/macos.m");
        cc::Build::new()
            .file("native/macos.m")
            .flag("-fobjc-arc")
            .compile("nanatype_speech_native");
        for framework in ["Speech", "AVFoundation", "Foundation"] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    } else if target == "windows" {
        println!("cargo:rerun-if-changed=native/windows.cpp");
        cc::Build::new()
            .cpp(true)
            .std("c++17")
            .flag_if_supported("/EHsc")
            .file("native/windows.cpp")
            .compile("nanatype_speech_native");
        for lib in ["ole32", "sapi", "uuid"] {
            println!("cargo:rustc-link-lib={lib}");
        }
    }
}
