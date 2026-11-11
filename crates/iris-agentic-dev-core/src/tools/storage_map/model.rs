//! Storage model layer: typed structs built from the element tree.

/// Model version for the JSON contract (data-model.md).
pub const MODEL_VERSION: u32 = 1;

/// FNV-1a 64-bit hash implementation.
///
/// FNV-1a is a non-cryptographic hash used for version digests.
/// Algorithm: start with FNV offset basis, for each byte: xor then multiply by FNV prime.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for &byte in bytes {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Returns a version hex digest (16 lowercase hex digits) for the given text.
pub fn version_hex(text: &str) -> String {
    format!("{:016x}", fnv1a64(text.as_bytes()))
}
