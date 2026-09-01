# 088 — Plan

## Tech Stack

- Rust 2021, `iris-agentic-dev-core` + `iris-agentic-dev-bin`
- `rusqlite` 0.32 (bundled) — Windows target only in core
- `windows` 0.58 with features `Win32_Security_Cryptography`, `Win32_Foundation`,
  `Win32_System_WindowsProgramming` — Windows target only in core
- `aes-gcm` 0.10, `base64` 0.22 — already in bin; move to core (Windows target only)

## Architecture

### Move vscdb logic from bin → core

`crates/iris-agentic-dev-core/src/iris/vscode_payload.rs` (new file, moved from bin):

- All payload decoding: `classify_payload`, `decode_payload`, `DecodedPayload`,
  `PayloadEncoding`, `DPAPI_BLOB_HEADER`, `LOCAL_STATE_KEY_PREFIX`, `hex_preview`,
  `parse_local_state_key`, `decrypt_safe_storage`
- No changes to the logic — pure move

`crates/iris-agentic-dev-core/src/iris/server_manager.rs` (Windows additions):

- `fn vscdb_state_db_path() -> Result<PathBuf, String>` — locate state.vscdb
- `fn vscdb_local_state_path(db_path: &Path) -> Result<PathBuf, String>`
- `fn vscdb_read_secret(db_path: &Path, key: &str) -> Result<(Vec<u8>, String), String>`
- `fn dpapi_decrypt(ciphertext: &[u8], what: &str) -> Result<Vec<u8>, String>` — with
  current Windows username in error message (using `GetUserNameW`)
- `fn current_windows_username() -> Option<String>`
- `pub fn resolve_vscode_secret(server_name: &str, username: &str, db_path_override: Option<&Path>) -> Result<String, String>`
  — the two-stage unseal: DPAPI on Local State key, AES-GCM on value

### Wire fallback in `resolve_credential`

```rust
// Windows-only fallback block after WCM error
#[cfg(target_os = "windows")]
{
    tracing::debug!("WCM lookup failed for '{server_name}', trying state.vscdb fallback");
    return resolve_vscode_secret(server_name, &username_lower, None)
        .map_err(|e| SmCredentialError::KeychainError {
            server_name: server_name.to_string(),
            detail: e,
        });
}
```

### Simplify `check_sm_credential.rs` in bin

- Remove `dpapi_decrypt`, `current_windows_username`, `vscdb_state_db_path`,
  `vscdb_local_state_path`, `vscdb_read_secret` — all now live in core
- `resolve_vscode_secret` imported from core:
  `iris_agentic_dev_core::iris::server_manager::resolve_vscode_secret`
- Remove Windows-specific deps from bin that now live in core:
  `rusqlite`, `windows` (keep only if still needed for other bin-level code)
- `vscode_payload.rs` in bin replaced with re-exports from core, or bin test
  file updated to import from core

### Module wiring in core

`crates/iris-agentic-dev-core/src/iris/mod.rs`:

- Add `pub mod vscode_payload;` under `#[cfg(target_os = "windows")]`

## File Changes

| File                                                             | Change                                                             |
| ---------------------------------------------------------------- | ------------------------------------------------------------------ |
| `crates/iris-agentic-dev-core/src/iris/vscode_payload.rs`        | NEW — moved from bin                                               |
| `crates/iris-agentic-dev-core/src/iris/mod.rs`                   | add `pub mod vscode_payload` (Windows)                             |
| `crates/iris-agentic-dev-core/src/iris/server_manager.rs`        | add vscdb helpers + fallback in `resolve_credential`               |
| `crates/iris-agentic-dev-core/Cargo.toml`                        | add rusqlite, windows, aes-gcm, base64 (Windows target)            |
| `crates/iris-agentic-dev-bin/src/cmd/check_sm_credential.rs`     | strip duplicated code, import from core                            |
| `crates/iris-agentic-dev-bin/src/cmd/vscode_payload.rs`          | DELETE (or thin re-export wrapper for bin-only test compatibility) |
| `crates/iris-agentic-dev-bin/Cargo.toml`                         | remove rusqlite/windows deps now in core                           |
| `crates/iris-agentic-dev-core/tests/unit/test_vscode_payload.rs` | NEW — unit tests moved from bin                                    |
| `crates/iris-agentic-dev-bin/tests/unit/test_vscode_payload.rs`  | DELETE after moving to core                                        |

## Test Layers

1. **Unit (core)** — `test_vscode_payload.rs` moved to core: same 10+ tests for
   `classify_payload`, `decode_payload`, `parse_local_state_key`, `decrypt_safe_storage`,
   `DPAPI_BLOB_HEADER` constants. All pass on macOS (no Windows deps exercised).

2. **Unit (server_manager)** — new tests in `test_vscode_fallback.rs` (or inline):
   - `resolve_credential_falls_back_to_vscdb_on_windows` — inject mock vscdb via
     `db_path_override`; verify password returned
   - `resolve_credential_vscdb_dpapi_error_includes_username` — inject a vscdb
     with a valid-format DPAPI blob; verify error text contains username placeholder
     These are `#[cfg(target_os = "windows")]` `#[ignore]` tests.

3. **Binary invocation** — existing `check-sm-credential` integration test (if any),
   or new `#[ignore]` test: spawn binary with `check-sm-credential <server> <user>`
   pointing at a fixture vscdb, assert output contains "OK" or expected error text.

4. **Live IRIS (Windows only)** — `#[ignore]`; not runnable on macOS CI.
   Documented in spec as manual verification step.

## Dependency notes

- `aes-gcm` and `base64` move from bin to core (Windows target only). Bin keeps
  them only if other non-Windows code in bin needs them. Check first.
- `windows` crate: bin currently has `Win32_Security_Cryptography`,
  `Win32_Foundation`, `Win32_System_WindowsProgramming`. After move, bin may
  have no Windows deps at all — remove the `[target.'cfg(windows)'.dependencies]`
  block from bin's Cargo.toml if so.
