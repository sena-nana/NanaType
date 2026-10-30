//! Windows resources for NanaType Editor (see `crates/frame/build_support.rs`).

include!("../crates/frame/build_support.rs");

const PRODUCT: Product = Product {
    bin: "nanatype-editor",
    icon: "assets/nanatype-editor.ico",
    name: "NanaType Editor",
};

fn main() {
    let build = build_id(&PRODUCT);
    if !targets_windows() {
        return;
    }
    if targets_msvc() {
        common_controls(&format!("cargo:rustc-link-arg-bin={}=", PRODUCT.bin));
        main_stack(&format!("cargo:rustc-link-arg-bin={}=", PRODUCT.bin));
    }
    windows_resources(&PRODUCT, &build);
}
