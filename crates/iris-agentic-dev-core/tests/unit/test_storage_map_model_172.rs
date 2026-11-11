//! Storage map model tests (T033)

use iris_agentic_dev_core::tools::storage_map::model::*;

/// T033: FNV-1a 64 test vectors
#[test]
fn test_fnv1a64_empty() {
    // Published FNV-1a 64 test vector for empty string
    assert_eq!(fnv1a64(b""), 0xcbf29ce484222325);
}

#[test]
fn test_fnv1a64_single_char() {
    // Published FNV-1a 64 test vector for "a"
    assert_eq!(fnv1a64(b"a"), 0xaf63dc4c8601ec8c);
}

#[test]
fn test_version_hex() {
    // Version hex should be 16 lowercase hex digits
    assert_eq!(version_hex(""), "cbf29ce484222325");
    assert_eq!(version_hex("a"), "af63dc4c8601ec8c");

    // Check format: 16 hex digits
    let version = version_hex("test string");
    assert_eq!(version.len(), 16);
    assert!(version.chars().all(|c| c.is_ascii_hexdigit()));
}
