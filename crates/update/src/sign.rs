use crate::Error;
use crate::manifest::Manifest;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};

/// The release signing key's public half, hex encoded. The private half is
/// the `NANATYPE_UPDATE_KEY` repository secret; see docs/updates.md.
pub const PUBLIC_KEY: &str = "b74c785e0bcd1aae298c3d3b616124b992602060d19515be9664f1788fcb713d";

pub use nanatype_model::codec::hex;

/// [`nanatype_model::codec::unhex`], ignoring surrounding whitespace (a
/// signature file may end in a newline).
pub fn unhex<const N: usize>(text: &str) -> Option<[u8; N]> {
    nanatype_model::codec::unhex(text.trim())
}

/// The hex signature of `manifest` under the hex-encoded 32-byte `secret`.
pub fn sign(manifest: &[u8], secret: &str) -> Option<String> {
    let key = SigningKey::from_bytes(&unhex::<32>(secret)?);
    Some(hex(&key.sign(manifest).to_bytes()))
}

/// Check `signature` over `manifest` with `public_key`, then parse it.
pub fn verify(manifest: &[u8], signature: &[u8], public_key: &str) -> Result<Manifest, Error> {
    let bad = |why: &str| Error::BadManifest(why.into());
    let key = unhex::<32>(public_key)
        .and_then(|k| VerifyingKey::from_bytes(&k).ok())
        .ok_or_else(|| bad("no release key configured"))?;
    let signature = std::str::from_utf8(signature)
        .ok()
        .and_then(unhex::<64>)
        .ok_or_else(|| bad("malformed signature"))?;
    key.verify_strict(manifest, &Signature::from_bytes(&signature))
        .map_err(|_| bad("signature does not match"))?;
    Manifest::parse(manifest)
}
