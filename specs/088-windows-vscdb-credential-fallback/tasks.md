# 088 — Tasks

## Phase 1: Unit tests (write first — no Windows required, all pass on macOS)

- [x] T001 Move `crates/iris-agentic-dev-bin/tests/unit/test_vscode_payload.rs` →
      `crates/iris-agentic-dev-core/tests/unit/test_vscode_payload.rs`; update imports
      to `iris_agentic_dev_core::iris::vscode_payload::*`; wire `[[test]]` in core
      `Cargo.toml`

## Phase 2: Move vscode_payload module to core

- [x] T002 Add `rusqlite`, `windows`, `aes-gcm` to
      `crates/iris-agentic-dev-core/Cargo.toml`; `aes-gcm` in regular deps (cross-platform pure
      Rust); `rusqlite` + `windows` under `[target.'cfg(windows)'.dependencies]`
- [x] T003 Create `crates/iris-agentic-dev-core/src/iris/vscode_payload.rs` — moved
      from bin; platform-independent (AES-GCM logic runs on all platforms)
- [x] T004 Add `pub mod vscode_payload;` to
      `crates/iris-agentic-dev-core/src/iris/mod.rs`
- [x] T005 `cargo test --package iris-agentic-dev-core --test test_vscode_payload` — 18 tests pass

## Phase 3: Add vscdb helpers and fallback to server_manager.rs

- [x] T006 Added to `crates/iris-agentic-dev-core/src/iris/server_manager.rs`
      (all `#[cfg(target_os = "windows")]`):
      `current_windows_username`, `vscdb_dpapi_decrypt`, `vscdb_state_db_path`,
      `vscdb_local_state_path`, `vscdb_read_secret`, `pub fn resolve_vscode_secret`
- [x] T007 In `resolve_credential`, after WCM match: Windows fallback calls
      `resolve_vscode_secret`; non-Windows returns WCM result unchanged

## Phase 4: Simplify bin crate

- [x] T008 `check_sm_credential.rs` rewritten — delegates to core's
      `resolve_vscode_secret`; verbose diagnostic wrapper kept; no duplicated
      DPAPI/AES/sqlite logic
- [x] T009 Deleted `crates/iris-agentic-dev-bin/src/cmd/vscode_payload.rs`;
      removed `pub mod vscode_payload` from bin's `cmd/mod.rs`;
      deleted `tests/unit/test_vscode_payload.rs` from bin
- [x] T010 Removed `aes-gcm`, `base64` from bin deps; removed `windows` from bin's
      Windows deps (now in core); `rusqlite` kept in bin for verbose diagnostic read

## Phase 5: Clippy, fmt, tests

- [x] T011 `cargo fmt --all` clean
- [x] T012 `cargo clippy -- -D warnings` clean (exit 0)
- [x] T013 `cargo test --package iris-agentic-dev-core` — green
- [x] T014 `cargo test --package iris-agentic-dev` — green

## Done criteria

- `cargo test` (no `--include-ignored`, macOS) green ✓
- `cargo clippy -- -D warnings` clean ✓
- `cargo fmt --all -- --check` clean ✓
- `check_sm_credential.rs` contains no duplicated vscdb/DPAPI/AES logic ✓
- `vscode_payload.rs` exists in core, deleted from bin ✓
- `resolve_credential` tries vscdb on any Windows WCM error ✓
