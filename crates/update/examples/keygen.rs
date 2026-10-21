//! Generate a release signing key pair.
//!
//! cargo run -p nanatype-update --example keygen
//!
//! Prints the public key for `PUBLIC_KEY` in src/sign.rs and the secret for
//! the `NANATYPE_UPDATE_KEY` repository secret. Keep the secret offline.
fn main() {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).expect("system randomness");
    let key = ed25519_dalek::SigningKey::from_bytes(&seed);
    println!(
        "public {}",
        nanatype_update::hex(key.verifying_key().as_bytes())
    );
    println!("secret {}", nanatype_update::hex(&seed));
}
