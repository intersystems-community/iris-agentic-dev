# Research: 095-password-persistence

## R-001: ServerEntry struct (verified 2026-09-02)

File: `crates/iris-agentic-dev-core/src/iris/servers_config.rs`

Confirmed fields: `host`, `port`, `namespace`, `username`, `description: Option<String>`, `scheme: Option<String>`.
No `password` field exists. `save_native_config` at line 103 takes `&ServersConfig`.

**Decision**: Add `pub password: Option<String>` with `#[serde(skip_serializing_if = "Option::is_none")]`.
**Rationale**: `skip_serializing_if` keeps existing entries without the key valid on deserialization (serde default is `None`).

## R-002: iris_add_server keychain path (verified 2026-09-02)

File: `crates/iris-agentic-dev-core/src/tools/mod.rs`, lines 7527–7543

Server entry is written to servers.json BEFORE the keychain call (lines 7519–7524).
On `KeychainUnavailable` → returns `KEYCHAIN_FAILED` error, no plaintext fallback.

**Decision**: After detecting `KeychainUnavailable` + non-empty password: mutate the `ServerEntry` in the
loaded config, set `password = Some(p.password.clone())`, call `save_native_config` again, return success
with `stored_plaintext: true`.

**Alternative rejected**: Write to `.iris-agentic-dev.toml` `[instance.*]` — out of scope per spec.

## R-003: Credential resolution (verified 2026-09-02)

File: `crates/iris-agentic-dev-core/src/iris/connection_pool.rs`, lines 199–218

Current: `resolve_credential(name, &entry.username).unwrap_or_default()` — if keychain has no entry, password = "".
Fix: after unwrap, if result is empty, fall back to `entry.password.clone().unwrap_or_default()`.

**Decision**: Two-step fallback (`keychain → entry.password → ""`).
**Rationale**: Keychain always wins when present; plaintext is last resort.

## R-004: SmCredentialError::KeychainUnavailable variant (verified 2026-09-02)

Struct variant: `SmCredentialError::KeychainUnavailable { .. }` — matches `mod.rs:7530`.

## R-005: Binary test environment

Linux CI has no keychain natively → `store_credential` returns `KeychainUnavailable` without mocking.
Binary invocation test with `IAD_BINARY` is the canonical path. No mock flag needed.

## R-006: iris_remove_server password clear

File: `crates/iris-agentic-dev-core/src/tools/mod.rs`, lines 7578+
Pattern: loads config, removes entry by name, saves. Fix: ensure removed entry's `password` is cleared
(setting entry to `None` on the `ServerEntry` before save, or simply removing the entry entirely as today).
Current code removes the entire entry — password goes with it. No change needed; this is already safe.
