//! The Steam ids are configuration, not source: `NANATYPE_STEAM_APP_ID`
//! comes from the environment or the gitignored `.cargo/config.toml`
//! (`[env]`, see `.cargo/config.example.toml` and docs/steam.md). A release
//! build that links Steam refuses to guess it; a debug build falls back to
//! Valve's public test app, Spacewar (480).
use std::{env, fs, path::Path};

const SPACEWAR: u32 = 480;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    for var in [
        "NANATYPE_STEAM_APP_ID",
        "NANATYPE_STEAM_TAG_SKIN",
        "NANATYPE_STEAM_TAG_SOUNDS",
    ] {
        println!("cargo:rerun-if-env-changed={var}");
    }
    let linked = env::var_os("CARGO_FEATURE_STEAMWORKS").is_some();
    let release = env::var("PROFILE").is_ok_and(|p| p != "debug");
    let app_id = match env::var("NANATYPE_STEAM_APP_ID") {
        Ok(raw) => raw
            .trim()
            .parse::<u32>()
            .ok()
            .filter(|id| *id > 0)
            .unwrap_or_else(|| panic!("NANATYPE_STEAM_APP_ID must be a Steam app id, not `{raw}`")),
        Err(_) if linked && release => panic!(
            "a release Steam build needs NANATYPE_STEAM_APP_ID; set it in .cargo/config.toml [env] (see docs/steam.md)"
        ),
        Err(_) => {
            if linked {
                println!(
                    "cargo:warning=NANATYPE_STEAM_APP_ID is not set; using Spacewar ({SPACEWAR}) for this debug build"
                );
            }
            SPACEWAR
        }
    };
    let tag = |var: &str, default: &str| env::var(var).unwrap_or_else(|_| default.to_owned());
    let out = Path::new(&env::var_os("OUT_DIR").expect("OUT_DIR")).join("ids.rs");
    fs::write(
        out,
        format!(
            "/// The Steam app this build belongs to.\npub const APP_ID: u32 = {app_id};\n\
             /// The Workshop tag skins are published under.\npub const TAG_SKIN: &str = {:?};\n\
             /// The Workshop tag sound packs are published under.\npub const TAG_SOUNDS: &str = {:?};\n",
            tag("NANATYPE_STEAM_TAG_SKIN", "Skin"),
            tag("NANATYPE_STEAM_TAG_SOUNDS", "Sound Pack"),
        ),
    )
    .expect("write ids.rs");
}
