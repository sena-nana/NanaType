//! Write the signed update manifest for a folder of packaged files.
//!
//! NANATYPE_UPDATE_KEY=<hex secret> \
//!     cargo run -p nanatype-update --example manifest -- <dir> <version>
//!
//! Writes `<dir>/latest.json` and `<dir>/latest.json.sig`, after checking
//! that the signature verifies against the compiled-in public key.
use nanatype_update::{MANIFEST_NAME, Manifest, PUBLIC_KEY, SIG_SUFFIX};
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(dir), Some(version)) = (args.next(), args.next()) else {
        fail("usage: manifest <dir> <version>");
    };
    let dir = PathBuf::from(dir);
    let version = version.trim_start_matches('v');
    let secret = std::env::var("NANATYPE_UPDATE_KEY").unwrap_or_else(|_| {
        fail("NANATYPE_UPDATE_KEY is not set");
    });
    let manifest = Manifest::from_dir(&dir, version).unwrap_or_else(|e| fail(&e.to_string()));
    if manifest.assets.is_empty() {
        fail("no packaged files found");
    }
    let json = serde_json::to_vec_pretty(&manifest).expect("manifest serializes");
    let signature = nanatype_update::sign(&json, &secret)
        .unwrap_or_else(|| fail("NANATYPE_UPDATE_KEY is not a 64-digit hex key"));
    if let Err(e) = nanatype_update::verify(&json, signature.as_bytes(), PUBLIC_KEY) {
        fail(&format!("the key does not match PUBLIC_KEY: {e}"));
    }
    std::fs::write(dir.join(MANIFEST_NAME), &json).expect("write manifest");
    std::fs::write(dir.join(format!("{MANIFEST_NAME}{SIG_SUFFIX}")), signature)
        .expect("write signature");
    for asset in &manifest.assets {
        println!(
            "{} {:?} {:?} {}",
            asset.name, asset.target, asset.kind, asset.sha256
        );
    }
}

fn fail(why: &str) -> ! {
    eprintln!("{why}");
    std::process::exit(1);
}
