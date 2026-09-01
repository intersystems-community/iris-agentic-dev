//! Decoding the value VS Code stores under a `secret://…` key in `state.vscdb`.
//!
//! VS Code does not store raw DPAPI ciphertext. `EncryptionMainService.encrypt`
//! is `JSON.stringify(safeStorage.encryptString(value))`, so the `value` column
//! holds TEXT of the form `{"type":"Buffer","data":[…]}`. On Windows the bytes
//! inside that array are a Chromium `safeStorage` envelope: the ASCII tag `v10`
//! (or `v11`), a 12-byte GCM nonce, the AES-256-GCM ciphertext, and a 16-byte
//! tag. The AES key itself lives in `%APPDATA%\Code\Local State` under
//! `os_crypt.encrypted_key`, base64-encoded, prefixed with the ASCII bytes
//! `DPAPI`, and sealed with `CryptProtectData`.
//!
//! So DPAPI is used — one level in, on the *key*, never on the secret. Calling
//! `CryptUnprotectData` on the stored value cannot succeed for any user on any
//! machine.
//!
//! This module is deliberately platform-independent so every format and crypto
//! step is covered by `cargo test` on any dev machine. Only the DPAPI unseal of
//! the AES key is Windows-gated, in `server_manager`.

use base64::Engine;

/// Header Windows writes at the front of every DPAPI blob: a `u32` version of
/// 1, then the provider GUID `df9d8cd0-1501-11d1-8c7a-00c04fc297eb` in
/// little-endian layout.
pub const DPAPI_BLOB_HEADER: [u8; 20] = [
    0x01, 0x00, 0x00, 0x00, // version
    0xd0, 0x8c, 0x9d, 0xdf, // GUID Data1 (LE)
    0x01, 0x15, // Data2 (LE)
    0xd1, 0x11, // Data3 (LE)
    0x8c, 0x7a, 0x00, 0xc0, 0x4f, 0xc2, 0x97, 0xeb, // Data4
];

/// ASCII prefix on the base64-decoded `os_crypt.encrypted_key` in `Local State`.
pub const LOCAL_STATE_KEY_PREFIX: &[u8] = b"DPAPI";

/// `v10`/`v11` tag length.
const ENVELOPE_TAG_LEN: usize = 3;
/// AES-GCM nonce length used by Chromium's OSCrypt.
const GCM_NONCE_LEN: usize = 12;
/// AES-GCM authentication tag length.
const GCM_TAG_LEN: usize = 16;
/// Shortest possible envelope: tag + nonce + empty ciphertext + auth tag.
const MIN_ENVELOPE_LEN: usize = ENVELOPE_TAG_LEN + GCM_NONCE_LEN + GCM_TAG_LEN;

/// Shortest plausible base64 payload. A DPAPI blob is never smaller than its
/// own 20-byte header, so anything under ~28 characters is something else.
const MIN_BASE64_LEN: usize = 28;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayloadEncoding {
    /// Raw DPAPI ciphertext — ready for `CryptUnprotectData` as-is.
    RawDpapi,
    /// Base64 text wrapping DPAPI ciphertext.
    Base64,
    /// `{"type":"Buffer","data":[…]}` from Electron's `Buffer.toJSON()`.
    /// This is what current VS Code writes.
    BufferJson,
    /// Chromium `safeStorage` AES-GCM envelope (`v10`/`v11` tag).
    SafeStorageAesGcm,
    Unknown,
}

/// What a decoded payload actually turned out to be. The caller must branch on
/// this: the two variants need completely different key material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodedPayload {
    /// Feed to `CryptUnprotectData` directly.
    Dpapi(Vec<u8>),
    /// AES-256-GCM envelope. Needs the `Local State` key; see
    /// [`parse_local_state_key`] and [`decrypt_safe_storage`].
    SafeStorage(Vec<u8>),
}

pub fn classify_payload(value: &[u8]) -> PayloadEncoding {
    if value.starts_with(&DPAPI_BLOB_HEADER) {
        return PayloadEncoding::RawDpapi;
    }
    if value.starts_with(b"v10") || value.starts_with(b"v11") {
        return PayloadEncoding::SafeStorageAesGcm;
    }
    if looks_like_buffer_json(value) {
        return PayloadEncoding::BufferJson;
    }
    if looks_like_base64(value) {
        return PayloadEncoding::Base64;
    }
    PayloadEncoding::Unknown
}

/// Unwrap the stored `ItemTable.value` down to actual ciphertext.
///
/// Wrappers are peeled and the result re-classified, because unwrapping Buffer
/// JSON or base64 usually reveals a `v10` envelope rather than a DPAPI blob.
/// Returning "DPAPI ciphertext" unconditionally is what made the earlier
/// revision report a misleading cause.
pub fn decode_payload(value: &[u8]) -> Result<DecodedPayload, String> {
    decode_payload_inner(value, 0)
}

fn decode_payload_inner(value: &[u8], depth: u8) -> Result<DecodedPayload, String> {
    if depth > 2 {
        return Err("value is wrapped more deeply than expected; giving up".to_string());
    }
    match classify_payload(value) {
        PayloadEncoding::RawDpapi => Ok(DecodedPayload::Dpapi(value.to_vec())),
        PayloadEncoding::SafeStorageAesGcm => Ok(DecodedPayload::SafeStorage(value.to_vec())),
        PayloadEncoding::Base64 => {
            let inner = decode_base64(value)?;
            reclassify_unwrapped(inner, "base64", depth)
        }
        PayloadEncoding::BufferJson => {
            let inner = decode_buffer_json(value)?;
            reclassify_unwrapped(inner, "Buffer JSON", depth)
        }
        PayloadEncoding::Unknown => Err(format!(
            "value is in an unrecognized format ({} bytes): {}. Expected an Electron \
             Buffer JSON object, a \"v10\"/\"v11\" safeStorage envelope, base64, or raw \
             DPAPI ciphertext. Please report this output.",
            value.len(),
            hex_preview(value, 16)
        )),
    }
}

fn reclassify_unwrapped(
    inner: Vec<u8>,
    wrapper: &str,
    depth: u8,
) -> Result<DecodedPayload, String> {
    match classify_payload(&inner) {
        PayloadEncoding::Unknown => Err(format!(
            "unwrapped {wrapper} to {} bytes, but they are neither a safeStorage \
             envelope nor a DPAPI blob: {}. Please report this output.",
            inner.len(),
            hex_preview(&inner, 16)
        )),
        _ => decode_payload_inner(&inner, depth + 1),
    }
}

fn decode_base64(value: &[u8]) -> Result<Vec<u8>, String> {
    match base64::engine::general_purpose::STANDARD.decode(value) {
        Ok(d) => Ok(d),
        Err(_) => base64::engine::general_purpose::STANDARD_NO_PAD
            .decode(value)
            .map_err(|e| format!("value looked like base64 but did not decode: {e}")),
    }
}

fn decode_buffer_json(value: &[u8]) -> Result<Vec<u8>, String> {
    let text = std::str::from_utf8(value)
        .map_err(|e| format!("Buffer JSON value is not valid UTF-8: {e}"))?;
    let parsed: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("Buffer JSON did not parse: {e}"))?;

    let data = parsed
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or_else(|| "Buffer JSON has no \"data\" array".to_string())?;

    let mut out = Vec::with_capacity(data.len());
    for item in data {
        let n = item
            .as_u64()
            .ok_or_else(|| format!("Buffer JSON \"data\" holds a non-integer entry: {item}"))?;
        let byte = u8::try_from(n)
            .map_err(|_| format!("Buffer JSON \"data\" entry {n} is outside byte range 0-255"))?;
        out.push(byte);
    }
    Ok(out)
}

/// Pull the DPAPI-sealed AES key out of `%APPDATA%\Code\Local State`.
///
/// Returns the bytes to hand to `CryptUnprotectData`; the ASCII `DPAPI` prefix
/// is stripped. Unsealing it yields the 32-byte AES-256 key.
pub fn parse_local_state_key(local_state_json: &str) -> Result<Vec<u8>, String> {
    let parsed: serde_json::Value = serde_json::from_str(local_state_json)
        .map_err(|e| format!("Local State did not parse as JSON: {e}"))?;

    let encoded = parsed
        .get("os_crypt")
        .and_then(|c| c.get("encrypted_key"))
        .and_then(|k| k.as_str())
        .ok_or_else(|| {
            "Local State has no os_crypt.encrypted_key — this VS Code install may \
             predate safeStorage"
                .to_string()
        })?;

    let raw = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| format!("os_crypt.encrypted_key is not valid base64: {e}"))?;

    let stripped = raw.strip_prefix(LOCAL_STATE_KEY_PREFIX).ok_or_else(|| {
        format!(
            "os_crypt.encrypted_key does not start with the expected \"DPAPI\" prefix: {}",
            hex_preview(&raw, 8)
        )
    })?;

    if stripped.is_empty() {
        return Err("os_crypt.encrypted_key holds no ciphertext after the prefix".to_string());
    }
    Ok(stripped.to_vec())
}

/// Open a `v10`/`v11` envelope with the unsealed AES-256 key.
pub fn decrypt_safe_storage(envelope: &[u8], aes_key: &[u8]) -> Result<String, String> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Key, Nonce};

    if aes_key.len() != 32 {
        return Err(format!(
            "expected a 32-byte AES-256 key from Local State, got {} bytes",
            aes_key.len()
        ));
    }
    if envelope.len() < MIN_ENVELOPE_LEN {
        return Err(format!(
            "safeStorage envelope is too short: {} bytes, need at least {MIN_ENVELOPE_LEN}",
            envelope.len()
        ));
    }
    if !(envelope.starts_with(b"v10") || envelope.starts_with(b"v11")) {
        return Err(format!(
            "not a safeStorage envelope: {}",
            hex_preview(envelope, 8)
        ));
    }

    let nonce = &envelope[ENVELOPE_TAG_LEN..ENVELOPE_TAG_LEN + GCM_NONCE_LEN];
    let sealed = &envelope[ENVELOPE_TAG_LEN + GCM_NONCE_LEN..];

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(aes_key));
    let plaintext = cipher
        .decrypt(Nonce::from_slice(nonce), sealed)
        .map_err(|_| {
            "AES-GCM authentication failed — the Local State key does not match this \
             value. The credential may have been written by a different VS Code \
             install, or Local State was replaced after the secret was stored."
                .to_string()
        })?;

    String::from_utf8(plaintext).map_err(|e| format!("decrypted bytes are not UTF-8: {e}"))
}

fn looks_like_buffer_json(value: &[u8]) -> bool {
    let head = &value[..64.min(value.len())];
    let Ok(text) = std::str::from_utf8(head) else {
        return false;
    };
    let trimmed = text.trim_start();
    trimmed.starts_with('{') && trimmed.contains("\"Buffer\"")
}

fn looks_like_base64(value: &[u8]) -> bool {
    if value.len() < MIN_BASE64_LEN {
        return false;
    }
    value
        .iter()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
}

/// Render the first `max` bytes as hex plus printable ASCII, for diagnostics.
/// Callers pass ciphertext only — never decrypted material.
pub fn hex_preview(value: &[u8], max: usize) -> String {
    let shown = &value[..max.min(value.len())];
    let hex = shown
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ");
    let ascii: String = shown
        .iter()
        .map(|&b| if b.is_ascii_graphic() { b as char } else { '.' })
        .collect();
    format!(
        "{hex} |{ascii}| (first {} of {} bytes)",
        shown.len(),
        value.len()
    )
}
