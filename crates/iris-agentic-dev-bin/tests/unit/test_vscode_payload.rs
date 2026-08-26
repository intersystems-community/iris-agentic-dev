//! Format and crypto tests for the value VS Code stores under a `secret://…` key.
//!
//! These run on every platform on purpose. The only Windows-specific step in the
//! credential path is unsealing the AES key with `CryptUnprotectData`; every
//! other step — unwrapping Buffer JSON, recognising the `v10` envelope, parsing
//! `Local State`, and the AES-256-GCM open itself — is pure logic. Gating it
//! behind `cfg(windows)` is what let the original bug ship: `cargo test` on a
//! dev machine compiled none of it.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine;
use iris_agentic_dev::cmd::vscode_payload::{
    classify_payload, decode_payload, decrypt_safe_storage, hex_preview, parse_local_state_key,
    DecodedPayload, PayloadEncoding, DPAPI_BLOB_HEADER, LOCAL_STATE_KEY_PREFIX,
};

const TEST_KEY: [u8; 32] = [7u8; 32];
const TEST_NONCE: [u8; 12] = [3u8; 12];

/// A plausible 140-byte DPAPI blob: real 20-byte header + filler.
fn fake_dpapi_blob() -> Vec<u8> {
    let mut b = DPAPI_BLOB_HEADER.to_vec();
    b.extend((0u8..120).map(|i| i.wrapping_mul(7)));
    assert_eq!(b.len(), 140);
    b
}

/// Build a real `v10` envelope the way Chromium's OSCrypt does.
fn seal_v10(plaintext: &str, key: &[u8; 32], nonce: &[u8; 12]) -> Vec<u8> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let sealed = cipher
        .encrypt(Nonce::from_slice(nonce), plaintext.as_bytes())
        .expect("seal");
    let mut out = b"v10".to_vec();
    out.extend_from_slice(nonce);
    out.extend_from_slice(&sealed);
    out
}

/// Wrap bytes the way `JSON.stringify(Buffer)` does.
fn buffer_json(bytes: &[u8]) -> String {
    format!(
        r#"{{"type":"Buffer","data":[{}]}}"#,
        bytes
            .iter()
            .map(|b| b.to_string())
            .collect::<Vec<_>>()
            .join(",")
    )
}

// ---------------------------------------------------------------------------
// The real-world shape: Buffer JSON around a v10 envelope
// ---------------------------------------------------------------------------

/// This is what current VS Code actually writes, and it is the shape behind the
/// field report of a 187-byte value. The decoded result must be reported as
/// `SafeStorage`, NOT as DPAPI ciphertext — handing it to `CryptUnprotectData`
/// is what produced the bogus "different user" diagnosis.
#[test]
fn buffer_json_wrapping_a_v10_envelope_decodes_to_safe_storage_not_dpapi() {
    let envelope = seal_v10("hunter2-hunter2", &TEST_KEY, &TEST_NONCE);
    let stored = buffer_json(&envelope);

    assert_eq!(
        classify_payload(stored.as_bytes()),
        PayloadEncoding::BufferJson
    );

    match decode_payload(stored.as_bytes()).unwrap() {
        DecodedPayload::SafeStorage(inner) => assert_eq!(inner, envelope),
        DecodedPayload::Dpapi(_) => {
            panic!("a v10 envelope must never be reported as DPAPI ciphertext")
        }
    }
}

/// The length arithmetic that ties the model to the field report: a password in
/// the low teens, sealed as v10 and wrapped in Buffer JSON, lands near 187
/// characters of TEXT.
#[test]
fn buffer_json_of_a_teens_length_password_is_about_187_chars() {
    let stored = buffer_json(&seal_v10("correct-horse1", &TEST_KEY, &TEST_NONCE));
    assert!(
        (150..=230).contains(&stored.len()),
        "expected roughly the observed 187 chars, got {}",
        stored.len()
    );
}

#[test]
fn full_chain_from_stored_text_to_plaintext_password() {
    let password = "s3cret-passw0rd";
    let stored = buffer_json(&seal_v10(password, &TEST_KEY, &TEST_NONCE));

    let DecodedPayload::SafeStorage(envelope) = decode_payload(stored.as_bytes()).unwrap() else {
        panic!("expected a safeStorage envelope");
    };
    assert_eq!(
        decrypt_safe_storage(&envelope, &TEST_KEY).unwrap(),
        password
    );
}

// ---------------------------------------------------------------------------
// Envelope handling
// ---------------------------------------------------------------------------

#[test]
fn bare_v10_and_v11_envelopes_are_recognized() {
    for tag in ["v10", "v11"] {
        let mut payload = tag.as_bytes().to_vec();
        payload.extend([0xAA; 60]);
        assert_eq!(
            classify_payload(&payload),
            PayloadEncoding::SafeStorageAesGcm,
            "{tag} should be detected"
        );
        assert!(matches!(
            decode_payload(&payload).unwrap(),
            DecodedPayload::SafeStorage(_)
        ));
    }
}

#[test]
fn envelope_with_the_wrong_key_fails_authentication_without_blaming_the_user() {
    let envelope = seal_v10("hunter2", &TEST_KEY, &TEST_NONCE);
    let wrong_key = [9u8; 32];

    let err = decrypt_safe_storage(&envelope, &wrong_key).unwrap_err();
    assert!(
        err.contains("authentication failed"),
        "should name the GCM failure: {err}"
    );
    assert!(
        !err.contains("different user"),
        "must not blame the user's account: {err}"
    );
}

#[test]
fn envelope_shorter_than_nonce_plus_tag_is_rejected() {
    let err = decrypt_safe_storage(b"v10short", &TEST_KEY).unwrap_err();
    assert!(err.contains("too short"), "got: {err}");
}

#[test]
fn decrypt_rejects_a_key_that_is_not_32_bytes() {
    let envelope = seal_v10("hunter2", &TEST_KEY, &TEST_NONCE);
    let err = decrypt_safe_storage(&envelope, &[1u8; 16]).unwrap_err();
    assert!(err.contains("32-byte"), "got: {err}");
}

#[test]
fn decrypt_rejects_a_payload_that_is_not_an_envelope() {
    let mut not_envelope = vec![0u8; 40];
    not_envelope[0] = 0xFF;
    let err = decrypt_safe_storage(&not_envelope, &TEST_KEY).unwrap_err();
    assert!(err.contains("not a safeStorage envelope"), "got: {err}");
}

// ---------------------------------------------------------------------------
// Local State key extraction
// ---------------------------------------------------------------------------

#[test]
fn local_state_key_is_base64_decoded_and_the_dpapi_prefix_stripped() {
    let sealed_key = b"\x01\x00\x00\x00sealed-key-bytes";
    let mut raw = LOCAL_STATE_KEY_PREFIX.to_vec();
    raw.extend_from_slice(sealed_key);
    let encoded = base64::engine::general_purpose::STANDARD.encode(&raw);
    let json = format!(r#"{{"os_crypt":{{"encrypted_key":"{encoded}"}}}}"#);

    assert_eq!(parse_local_state_key(&json).unwrap(), sealed_key);
}

#[test]
fn local_state_without_os_crypt_reports_the_missing_field() {
    let err = parse_local_state_key(r#"{"profile":{}}"#).unwrap_err();
    assert!(err.contains("os_crypt.encrypted_key"), "got: {err}");
}

#[test]
fn local_state_key_missing_the_dpapi_prefix_is_rejected() {
    let encoded = base64::engine::general_purpose::STANDARD.encode(b"NOPREFIXwhatever");
    let json = format!(r#"{{"os_crypt":{{"encrypted_key":"{encoded}"}}}}"#);
    let err = parse_local_state_key(&json).unwrap_err();
    assert!(err.contains("DPAPI"), "got: {err}");
}

#[test]
fn local_state_that_is_not_json_is_rejected() {
    assert!(parse_local_state_key("not json at all").is_err());
}

// ---------------------------------------------------------------------------
// Legacy and defensive paths
// ---------------------------------------------------------------------------

#[test]
fn raw_dpapi_blob_is_recognized_and_passed_through() {
    let blob = fake_dpapi_blob();
    assert_eq!(classify_payload(&blob), PayloadEncoding::RawDpapi);
    assert_eq!(
        decode_payload(&blob).unwrap(),
        DecodedPayload::Dpapi(blob.clone())
    );
}

#[test]
fn base64_wrapping_a_dpapi_blob_unwraps_to_dpapi() {
    let blob = fake_dpapi_blob();
    let text = base64::engine::general_purpose::STANDARD.encode(&blob);
    assert_eq!(classify_payload(text.as_bytes()), PayloadEncoding::Base64);
    assert_eq!(
        decode_payload(text.as_bytes()).unwrap(),
        DecodedPayload::Dpapi(blob)
    );
}

#[test]
fn unpadded_base64_wrapping_a_dpapi_blob_unwraps_to_dpapi() {
    let blob = fake_dpapi_blob();
    let text = base64::engine::general_purpose::STANDARD_NO_PAD.encode(&blob);
    assert_eq!(text.len(), 187, "the length observed in the field");
    assert_eq!(
        decode_payload(text.as_bytes()).unwrap(),
        DecodedPayload::Dpapi(blob)
    );
}

#[test]
fn buffer_json_that_unwraps_to_neither_cipher_says_so() {
    // 40 bytes of nothing recognizable — must not be silently passed to DPAPI.
    let stored = buffer_json(&[0xFFu8; 40]);
    let err = decode_payload(stored.as_bytes()).unwrap_err();
    assert!(
        err.contains("Buffer JSON"),
        "should name the wrapper: {err}"
    );
    assert!(
        err.contains("neither a safeStorage envelope nor a DPAPI blob"),
        "got: {err}"
    );
    assert!(!err.contains("different user"), "must not blame the user");
}

#[test]
fn buffer_json_rejects_out_of_range_values() {
    let err = decode_payload(br#"{"type":"Buffer","data":[1,999]}"#).unwrap_err();
    assert!(
        err.contains("999"),
        "error should name the bad value: {err}"
    );
}

#[test]
fn buffer_json_tolerates_whitespace() {
    let envelope = seal_v10("pw", &TEST_KEY, &TEST_NONCE);
    let inner = envelope
        .iter()
        .map(|b| b.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let stored = format!(r#"{{ "type": "Buffer", "data": [ {inner} ] }}"#);
    assert_eq!(
        classify_payload(stored.as_bytes()),
        PayloadEncoding::BufferJson
    );
    assert!(matches!(
        decode_payload(stored.as_bytes()).unwrap(),
        DecodedPayload::SafeStorage(_)
    ));
}

#[test]
fn unrecognized_bytes_report_a_hex_preview_rather_than_guessing() {
    let garbage = b"not-real-dpapi-data";
    assert_eq!(classify_payload(garbage), PayloadEncoding::Unknown);

    let err = decode_payload(garbage).unwrap_err();
    assert!(
        err.contains("6e 6f 74"),
        "error should include the hex preview: {err}"
    );
    assert!(!err.contains("different user"), "must not blame the user");
}

#[test]
fn empty_value_is_not_mistaken_for_base64() {
    assert_eq!(classify_payload(b""), PayloadEncoding::Unknown);
    assert!(decode_payload(b"").is_err());
}

#[test]
fn short_alphanumeric_values_are_not_mistaken_for_base64() {
    // Four base64-alphabet characters decode cleanly but are far too short to
    // be any of the real formats; treating them as base64 would hide the cause.
    assert_eq!(classify_payload(b"YWJj"), PayloadEncoding::Unknown);
}

#[test]
fn hex_preview_shows_bytes_and_printable_ascii() {
    let preview = hex_preview(b"AB\x00\xff", 8);
    assert!(preview.contains("41 42 00 ff"), "got: {preview}");
    assert!(
        preview.contains("AB"),
        "printable run should appear: {preview}"
    );
}

#[test]
fn hex_preview_truncates_and_reports_the_full_length() {
    let preview = hex_preview(&[0xABu8; 64], 4);
    assert_eq!(preview.matches("ab").count(), 4, "got: {preview}");
    assert!(
        preview.contains("64"),
        "should report total length: {preview}"
    );
}

#[test]
fn dpapi_header_matches_the_documented_provider_guid() {
    // version 1 (u32 LE) followed by df9d8cd0-1501-11d1-8c7a-00c04fc297eb
    // in the little-endian layout Windows writes.
    assert_eq!(DPAPI_BLOB_HEADER.len(), 20);
    assert_eq!(&DPAPI_BLOB_HEADER[0..4], &[0x01, 0x00, 0x00, 0x00]);
    assert_eq!(&DPAPI_BLOB_HEADER[4..8], &[0xd0, 0x8c, 0x9d, 0xdf]);
}
