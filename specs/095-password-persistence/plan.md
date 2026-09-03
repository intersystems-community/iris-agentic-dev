# Implementation Plan: iris_add_server Password Persistence Fallback

**Branch**: `095-password-persistence` | **Date**: 2026-09-02 | **Spec**: [spec.md](spec.md)

## Summary

When `store_credential` fails with `SmCredentialError::KeychainUnavailable`, write the
password into `ServerEntry.password` in servers.json instead of returning `KEYCHAIN_FAILED`.
Pool credential resolution falls back to `entry.password` after a keychain miss, so the server
is usable immediately. `iris_servers` never exposes the value; `iris_remove_server` clears it.

## Technical Context

**Language/Version**: Rust 2021 (workspace `edition = "2021"`)
**Primary Dependencies**: `serde`/`serde_json` (existing), `keyring` (existing, via `server_manager`)
**Storage**: `~/.config/iris-agentic-dev/servers.json` (JSON, user-private)
**Testing**: `cargo test`, `cargo llvm-cov --include-ignored` for coverage
**Target Platform**: Linux (CI/MCP headless), macOS (dev), keychain absent on Linux
**Project Type**: Single Rust workspace (two crates)
**Performance Goals**: N/A — config-only path, no hot loop
**Constraints**: Password value must never appear in any tool response field
**Scale/Scope**: Single file change to `ServerEntry` struct; two callsites updated

## Constitution Check

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Zero-Install Binary | PASS | No new dependencies; no install step |
| II. ObjectScript Sanity | N/A | No ObjectScript in this feature |
| III. HTTP-First Execution | N/A | Config-only, no new tools requiring Docker |
| IV. Test-First, Fixture-Driven | PASS | Unit JSON round-trip + binary invocation tests defined in spec |
| V. Output Shape Parity | PASS | `iris_servers` response shape unchanged; new fields on `iris_add_server` response are additive |
| VI. Environment Guard | N/A | No new write tools; `iris_add_server` already not gated |
| VII. Dependency Minimalism | PASS | No new crates |
| VIII. 90% Coverage Gate | PASS | Polish phase includes `cargo llvm-cov --include-ignored` ≥ 90% check |
| IX. Tool Lift Requirement | N/A | Enhancement to existing tool path, not a new tool; no lift benchmark required |
| X. ObjectScript Coverage | N/A | Pure Rust feature |

## Project Structure

### Documentation (this feature)

```text
specs/095-password-persistence/
├── plan.md              ← this file
├── research.md          ← Phase 0 output (below)
├── data-model.md        ← Phase 1 output (below)
├── contracts/           ← Phase 1 output (below)
└── tasks.md             ← Phase 2 output (/speckit.tasks)
```

### Source Code (files changed)

```text
crates/iris-agentic-dev-core/src/iris/
├── servers_config.rs        # ServerEntry: add password field (FR-001)
└── connection_pool.rs       # load_pool: fallback to entry.password after keychain miss (FR-004)

crates/iris-agentic-dev-core/src/tools/
├── mod.rs                   # iris_add_server: plaintext fallback branch (FR-002, FR-003)
│                            # iris_remove_server: clear password on remove (FR-005)
│                            # iris_servers: add has_plaintext_credential field (FR-006)
└── server_tools.rs          # no change — AddServerParams.password already exists

docs/
└── connecting.md            # document plaintext fallback (FR-007)

tests/
└── binary_095_password_persistence.rs   # binary invocation test (layer 2)
```

---

## Phase 0: Research

### R-001: `ServerEntry` struct — verified state

File: `crates/iris-agentic-dev-core/src/iris/servers_config.rs`

Fields confirmed (lines 7–15):
- `pub host: String`
- `pub port: u16`
- `pub namespace: String`
- `pub username: String`
- `pub description: Option<String>`
- `pub scheme: Option<String>`
- **No `password` field** — confirmed absent

`save_native_config` signature (line 103): `pub fn save_native_config(cfg: &ServersConfig) -> Result<(), Box<dyn std::error::Error>>`

Adding `pub password: Option<String>` with `#[serde(skip_serializing_if = "Option::is_none")]` ensures backward-compatible deserialization.

### R-002: `iris_add_server` keychain error handling — verified state

File: `crates/iris-agentic-dev-core/src/tools/mod.rs`, lines 7527–7543

Current path:
1. `server_manager::store_credential(&p.name, &p.username, &p.password)` called
2. On `KeychainUnavailable` → sets `is_unavailable = true`, returns `KEYCHAIN_FAILED` error with hint to edit toml
3. **No plaintext fallback exists** — confirmed

The entry is already saved to servers.json (line 7519-7524 saves before the keychain call). So the fix is: after detecting `KeychainUnavailable`, reload the saved entry, set `password = Some(p.password.clone())`, re-save, return success.

### R-003: Credential resolution in pool — verified state

File: `crates/iris-agentic-dev-core/src/iris/connection_pool.rs`, lines 199–218

Current path for iad-native entries:
1. `IrisConnection::new(... "", ...)` — empty password placeholder
2. `server_manager::resolve_credential(name, &entry.username).unwrap_or_default()` — keychain lookup
3. `conn.password = password` — assigns result

Fix: after keychain lookup fails (returns empty string or Err), check `entry.password` as fallback:

```rust
let password = server_manager::resolve_credential(name, &entry.username)
    .unwrap_or_default();
let password = if password.is_empty() {
    entry.password.clone().unwrap_or_default()
} else {
    password
};
```

### R-004: `SmCredentialError::KeychainUnavailable` variant name — verified

Confirmed at `mod.rs:7530`: `SmCredentialError::KeychainUnavailable { .. }` — struct variant with named fields (wildcard `..`).

### R-005: Test strategy for Linux CI (no keychain)

Linux CI has no keychain natively — `store_credential` returns `KeychainUnavailable` without any mock flag needed. Binary invocation test (`#[ignore]`, `IAD_BINARY`) runs the full path in CI. On macOS dev machines, keychain is present and the test goes to keychain path — CI is canonical.

No `IAD_MOCK_KEYCHAIN_UNAVAILABLE` env var needed. The spec mentioned it as a possibility but CI's natural state is sufficient.

---

## Phase 1: Design

### data-model.md

**`ServerEntry`** (servers_config.rs) — add one field:

```rust
pub struct ServerEntry {
    pub host: String,
    pub port: u16,
    pub namespace: String,
    pub username: String,
    pub description: Option<String>,
    pub scheme: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,   // ← NEW
}
```

JSON round-trip guarantee:
- Entry without `"password"` key → `password: None` (serde default)
- Entry with `"password": "SYS"` → `password: Some("SYS")`
- Serialized entry with `password: None` omits key (skip_serializing_if)

**Error codes** (unchanged):
- `KEYCHAIN_FAILED` — still returned when keychain fails for non-availability reasons
- New response shape for plaintext fallback path — `added: true, stored_plaintext: true, warning, note`

### contracts/iris_add_server_response.md

**Keychain success path** (unchanged):
```json
{ "added": true, "name": "...", "note": "Restart iad for the pool to include this server." }
```

**Plaintext fallback path** (new):
```json
{
  "added": true,
  "name": "...",
  "stored_plaintext": true,
  "warning": "Password stored in plaintext in servers.json — use Server Manager for production credentials.",
  "note": "Restart iad for the pool to include this server."
}
```

**Other keychain error** (unchanged):
```json
{ "error_code": "KEYCHAIN_FAILED", "keychain_unavailable": false, "message": "..." }
```

**`iris_servers` response** (additive change):
```json
{
  "name": "...", "host": "...", "port": 1972, "namespace": "USER",
  "username": "_SYSTEM", "source": "iad-native", "reachable": null,
  "has_plaintext_credential": true
}
```
`has_plaintext_credential` is `false` (or omitted) when `ServerEntry.password` is `None`.
Password value is never included.

---

## Complexity Tracking

No violations to justify.
