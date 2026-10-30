// Build-script helpers both products share, brought into each `build.rs` with
// `include!("../crates/frame/build_support.rs")`: the build id, Windows
// resources and the development copy of the DX12 shader compiler.
//
// * The Common Controls 6 manifest is embedded through linker arguments, so
//   no resource compiler is needed. `TaskDialogIndirect` exists only in
//   comctl32 v6; without the manifest the loader binds v5 and a process that
//   reaches it dies before any window opens.
// * The icon and version information need a resource compiler. When none is
//   installed a development build only warns; release CI sets
//   `NANATYPE_REQUIRE_RESOURCES=1` to make it an error.

/// What a product's executable says about itself on Windows.
#[allow(dead_code)]
struct Product {
    /// The cargo binary target.
    bin: &'static str,
    /// The `.ico`, relative to the crate.
    icon: &'static str,
    /// Product name and file description.
    name: &'static str,
}

/// Exports `NANATYPE_BUILD_ID` (default `dev`) and returns it.
fn build_id(product: &Product) -> String {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../crates/frame/build_support.rs");
    println!("cargo:rerun-if-changed={}", product.icon);
    println!("cargo:rerun-if-env-changed=NANATYPE_REQUIRE_RESOURCES");
    println!("cargo:rerun-if-env-changed=NANATYPE_BUILD_ID");
    let build = std::env::var("NANATYPE_BUILD_ID").unwrap_or_else(|_| "dev".into());
    println!("cargo:rustc-env=NANATYPE_BUILD_ID={build}");
    build
}

fn targets_windows() -> bool {
    std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
}

fn targets_msvc() -> bool {
    std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
}

/// The Common Controls 6 activation context for the targets `link_arg`
/// names (`cargo:rustc-link-arg-bin=NAME=`, `cargo:rustc-link-arg-examples=`).
fn common_controls(link_arg: &str) {
    println!("{link_arg}/MANIFEST:EMBED");
    println!(
        "{link_arg}/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
    );
    println!("{link_arg}/MANIFESTUAC:level='asInvoker' uiAccess='false'");
}

/// An 8 MiB main-thread stack for the targets `link_arg` names. Windows'
/// default is 1 MiB, and the UI runs on the main thread inside window
/// callbacks: unoptimized, a skin preview's frames came close to that.
/// Only the reservation grows; pages are committed as they are touched.
fn main_stack(link_arg: &str) {
    println!("{link_arg}/STACK:8388608");
}

/// The development copy of the shader compiler, then the icon and version
/// resources (when building on Windows, which has the resource compiler).
fn windows_resources(product: &Product, build: &str) {
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    copy_dxc_for_dev(&out_dir);
    #[cfg(windows)]
    embed_resources(&out_dir, build, product);
    #[cfg(not(windows))]
    let _ = (build, product);
}

/// Icon and version information.
#[cfg(windows)]
fn embed_resources(out_dir: &std::path::Path, build: &str, product: &Product) {
    let manifest_dir =
        std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let icon = manifest_dir.join(product.icon);
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let mut parts: Vec<u16> = version
        .split(['.', '-', '+'])
        .take(3)
        .map(|p| p.parse().unwrap_or(0))
        .collect();
    parts.resize(4, 0);
    let numeric = parts
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let rc = format!(
        r#"#pragma code_page(65001)
1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {numeric}
PRODUCTVERSION {numeric}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "sena-nana"
      VALUE "FileDescription", "{name}"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "{bin}"
      VALUE "OriginalFilename", "{bin}.exe"
      VALUE "ProductName", "{name}"
      VALUE "ProductVersion", "{version} ({build})"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        icon = icon.display().to_string().replace('\\', "/"),
        name = product.name,
        bin = product.bin,
    );
    let rc_path = out_dir.join(format!("{}.rc", product.bin));
    std::fs::write(&rc_path, rc).expect("write rc");
    let required = std::env::var("NANATYPE_REQUIRE_RESOURCES").as_deref() == Ok("1");
    match embed_resource::compile(&rc_path, embed_resource::NONE).manifest_optional() {
        Ok(()) => {}
        Err(error) if required => panic!("icon/version resources: {error}"),
        Err(error) => println!(
            "cargo:warning=icon and version info not embedded ({error}); install the Windows SDK"
        ),
    }
}

/// NanaUI compiles its DX12 shaders with `dxcompiler.dll` when it sits next
/// to the executable (about twice as fast as the FXC fallback). For local
/// builds, copy it from the Windows SDK into the profile directory; packaged
/// builds ship it through their `nana-package.toml`.
fn copy_dxc_for_dev(out_dir: &std::path::Path) {
    let Some(profile_dir) = out_dir.ancestors().nth(3) else {
        return;
    };
    let target = profile_dir.join("dxcompiler.dll");
    if target.exists() {
        return;
    }
    let kits = std::path::Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    let Ok(entries) = std::fs::read_dir(kits) else {
        return;
    };
    let mut versions: Vec<std::path::PathBuf> = entries.flatten().map(|e| e.path()).collect();
    versions.sort();
    if let Some(dll) = versions
        .iter()
        .rev()
        .map(|v| v.join("x64").join("dxcompiler.dll"))
        .find(|p| p.is_file())
    {
        let _ = std::fs::copy(dll, target);
    }
}
