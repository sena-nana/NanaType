//! Windows resources.
//!
//! * The Common Controls 6 manifest is embedded through linker arguments, so
//!   no resource compiler is needed. `TaskDialogIndirect` (pulled in by the
//!   tray stack) exists only in comctl32 v6; without the manifest the loader
//!   binds v5 and the process dies before any window opens.
//! * The icon and version information need a resource compiler. When none is
//!   installed a development build only warns; release CI sets
//!   `NANATYPE_REQUIRE_RESOURCES=1` to make it an error.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/nanatype.ico");
    println!("cargo:rerun-if-env-changed=NANATYPE_REQUIRE_RESOURCES");
    println!("cargo:rerun-if-env-changed=NANATYPE_BUILD_ID");
    let build = env::var("NANATYPE_BUILD_ID").unwrap_or_else(|_| "dev".into());
    println!("cargo:rustc-env=NANATYPE_BUILD_ID={build}");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // Prebuilt libvpx omits vpxmd.pdb; Rust debug information is retained.
        println!("cargo:rustc-link-arg=/IGNORE:4099");

        if env::var("CARGO_CFG_TARGET_FEATURE")
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

        // Standalone snapshot/performance tools link the same tray stack.
        // They need the embedded activation context too, including when copied.
        for target in [
            "cargo:rustc-link-arg-bin=nanatype=",
            "cargo:rustc-link-arg-examples=",
        ] {
            println!("{target}/MANIFEST:EMBED");
            println!(
                "{target}/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
            );
            println!("{target}/MANIFESTUAC:level='asInvoker' uiAccess='false'");
        }
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    copy_dxc_for_dev(&out_dir);
    // The resource compiler crate is a Windows-host build dependency.
    #[cfg(windows)]
    embed_resources(&out_dir, &build);
}

/// Icon and version information.
#[cfg(windows)]
fn embed_resources(out_dir: &std::path::Path, build: &str) {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let icon = manifest_dir.join("assets").join("nanatype.ico");
    let version = env::var("CARGO_PKG_VERSION").unwrap_or_default();
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
      VALUE "FileDescription", "NanaType"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "nanatype"
      VALUE "OriginalFilename", "nanatype.exe"
      VALUE "ProductName", "NanaType"
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
    );
    let rc_path = out_dir.join("nanatype.rc");
    fs::write(&rc_path, rc).expect("write rc");
    let required = env::var("NANATYPE_REQUIRE_RESOURCES").as_deref() == Ok("1");
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
/// builds ship it through `packaging/nana-package.toml`.
fn copy_dxc_for_dev(out_dir: &std::path::Path) {
    let Some(profile_dir) = out_dir.ancestors().nth(3) else {
        return;
    };
    let target = profile_dir.join("dxcompiler.dll");
    if target.exists() {
        return;
    }
    let kits = std::path::Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    let Ok(entries) = fs::read_dir(kits) else {
        return;
    };
    let mut versions: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    versions.sort();
    if let Some(dll) = versions
        .iter()
        .rev()
        .map(|v| v.join("x64").join("dxcompiler.dll"))
        .find(|p| p.is_file())
    {
        let _ = fs::copy(dll, target);
    }
}
