//! Small, stable encodings shared across crates: names derived from them
//! end up on disk, so their output must never change.

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0100_0000_01b3;

/// 64-bit FNV-1a: stable across runs and builds, unlike `DefaultHasher`.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    fnv1a64_seeded(FNV_OFFSET, bytes)
}

/// FNV-1a from another offset basis, for a second, independent hash.
pub fn fnv1a64_seeded(seed: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(seed, |hash, b| {
        (hash ^ u64::from(*b)).wrapping_mul(FNV_PRIME)
    })
}

/// Lowercase hex.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Exactly `N` bytes of hex, either case.
pub fn unhex<const N: usize>(text: &str) -> Option<[u8; N]> {
    if text.len() != N * 2 || !text.is_ascii() {
        return None;
    }
    let mut bytes = [0; N];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv_matches_the_reference_vectors() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn hex_round_trips() {
        assert_eq!(hex(&[0x00, 0xab, 0xff]), "00abff");
        assert_eq!(unhex::<3>("00ABff"), Some([0x00, 0xab, 0xff]));
        assert_eq!(unhex::<3>("00abf"), None);
        assert_eq!(unhex::<1>("zz"), None);
    }
}
