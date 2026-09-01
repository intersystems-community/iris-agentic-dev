# 088 — Windows vscdb credential fallback

## Overview

On Windows, VS Code's Server Manager extension stores IRIS credentials in
`state.vscdb` (safeStorage / AES-256-GCM), not in Windows Credential Manager.
`resolve_credential` in `iris-agentic-dev-core` only checks Windows Credential
Manager, so every Server Manager credential on Windows shows as `not_configured`
in the MCP server even when credentials are correctly stored in VS Code.

**Origin:** Robbie Luman (ISC) reported `credential_status: "not_configured"` for
all 16 Server Manager servers despite VS Code having credentials. The `check-sm-credential`
diagnostic (which reads `state.vscdb`) found the entries but `CryptUnprotectData`
failed — a separate user-context issue. The `not_configured` result in the MCP server
is the core bug: the vscdb path is never tried.

## Functional Requirements

- FR-001: On Windows, `resolve_credential` MUST try the `state.vscdb` safeStorage
  path when Windows Credential Manager returns any error (`CredentialNotFound`,
  `KeychainUnavailable`, or `KeychainError`). WCM is not where SM stores credentials
  on Windows, so any WCM failure is reason to try vscdb.

## Clarifications

### Session 2026-09-01

- Q: On Windows, when WCM returns `KeychainError` (not just NotFound/Unavailable), should we also try vscdb? → A: Yes — fall through on any WCM error on Windows.
- FR-002: The vscdb credential read MUST use the same two-stage unseal as
  `check-sm-credential`: DPAPI on the Local State AES key, then AES-256-GCM on
  the stored value.
- FR-003: If DPAPI fails (user mismatch), the error MUST include the current
  Windows username so the caller can diagnose which user the process is running as.
- FR-004: The vscdb fallback path MUST be code in `iris-agentic-dev-core` (not
  `iris-agentic-dev-bin`), so the MCP server — which is `core` — can use it.
- FR-005: `check_sm_credential.rs` in `iris-agentic-dev-bin` MUST import from
  core's vscdb module instead of carrying its own parallel implementation.
- FR-006: The fallback is Windows-only (`#[cfg(target_os = "windows")]`). No
  behaviour change on macOS or Linux.

## Non-Functional Requirements

- NFR-001: No new public API surface beyond what is needed for the fallback and the
  bin crate's diagnostic command.
- NFR-002: `rusqlite` and `windows` crate deps added to core under
  `[target.'cfg(target_os = "windows")'.dependencies]` only — do not pull them
  into non-Windows builds.
- NFR-003: `cargo test` (non-ignored, macOS) must remain green — all new
  Windows-path code is `#[cfg(target_os = "windows")]`.

## User Stories

### US1 — MCP server resolves SM credentials on Windows (P1)

As an IRIS developer on Windows with Server Manager credentials stored in VS Code,
when I start the MCP server, it finds my credentials automatically without requiring
a `.iris-agentic-dev.toml` password entry.

Acceptance criteria:

- `resolve_credential("iservice-base", "rluman")` returns the password when the
  credential is in `state.vscdb` and DPAPI succeeds (same Windows user).
- `check_config` reports `credential_status: "resolved"` for that server.
- If DPAPI fails, the error message includes the current Windows username.

### US2 — Diagnostic command uses shared core implementation (P2)

As a developer or support engineer, running `check-sm-credential` uses the same
code path as the MCP server, so a passing diagnostic means the MCP server will
also succeed (and a failing diagnostic explains why both fail).

Acceptance criteria:

- `check_sm_credential.rs` in bin delegates to `iris_agentic_dev_core::iris::vscode_payload`
  and the server_manager vscdb functions — no parallel implementation in bin.
- `check-sm-credential` output is unchanged (same diagnostic text).

## Edge Cases

- `state.vscdb` not found (VS Code not installed, or Cursor fork): log at debug,
  return the original WCM error — do not surface a second confusing error.
- Both WCM and vscdb fail: return the vscdb error (it is more informative on Windows).
- vscdb entry exists but DPAPI fails: return error with current Windows username.
- vscdb entry exists, DPAPI succeeds, AES-GCM fails (corrupt entry): return
  AES error — do not silently fall through.

## Out of Scope

- Fixing the DPAPI user-mismatch itself (a Windows user-context issue, not fixable
  in this codebase).
- Windows Credential Manager write path.
- Any non-Windows credential change.
