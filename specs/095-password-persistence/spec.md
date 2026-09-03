# Feature Specification: iris_add_server Password Persistence Fallback

**Feature Branch**: `095-password-persistence`
**Created**: 2026-09-02
**Status**: Draft

## Overview

`iris_add_server` stores credentials in the OS keychain. In MCP contexts (Claude
Desktop, headless CI, Remote SSH), the keychain is unavailable. Today the tool adds
the server entry to `~/.config/iris-agentic-dev/servers.json` but returns a
`KEYCHAIN_FAILED` error — the server is registered with no password and cannot
connect until the user manually edits config.

**Codebase state (verified 2026-09-02)**:

- `ServerEntry` (servers_config.rs) has no `password` field — only `host`, `port`,
  `namespace`, `username`, `description`, `scheme`.
- `iris_add_server` calls `server_manager::store_credential`. When it fails with
  `KeychainUnavailable`, the current error hint says "Add host/port/username/password
  to .iris-agentic-dev.toml instead" — but there is no code that does this.
- `.iris-agentic-dev.toml` `[instance.*]` blocks already support `password:
Option<String>` (workspace_config.rs `InstanceConfig`). This is the toml-based
  credential path, but `iris_add_server` does not use it.

The fix: when keychain storage fails with `KeychainUnavailable`, fall back to writing
the password into the `ServerEntry` in servers.json (add `password: Option<String>`
to the struct). Return `{added: true, stored_plaintext: true, warning: "..."}` — not
an error. The server is usable immediately after pool reload.

---

## User Scenarios & Testing

### User Story 1 — Add server in MCP context without keychain (Priority: P1)

An agent is helping a developer connect to a remote IRIS instance. The developer
tells the agent the host, port, namespace, username, and password. The agent calls
`iris_add_server`. The MCP server is running in Claude Desktop (no keychain access).
Today: `KEYCHAIN_FAILED` error — server is half-added with no credentials. After
this fix: server is added with password stored in servers.json, response includes a
`stored_plaintext` warning, agent reports success and advises the user to consider
Server Manager for production credentials.

**Independent Test**: Set `IAD_BINARY` and run `iris_add_server` in a subprocess
with the `IAD_MOCK_KEYCHAIN_UNAVAILABLE=1` env var (or similar test hook); assert
`servers.json` contains the password field and response contains `stored_plaintext:
true`.

**Acceptance Scenarios**:

1. Given keychain is unavailable, When `iris_add_server` is called with a password,
   Then the response is `{added: true, stored_plaintext: true, warning: "..."}` —
   not a `KEYCHAIN_FAILED` error.
2. Given keychain is available, When `iris_add_server` is called, Then password goes
   to keychain as before — no behavior change, no `stored_plaintext` field.
3. Given keychain fails for a non-availability reason (locked, permissions), When
   `iris_add_server` is called, Then `KEYCHAIN_FAILED` is still returned — plaintext
   fallback applies only to `KeychainUnavailable`.
4. Given a server was added via plaintext fallback, When iad connects to that server,
   Then it reads the password from `ServerEntry.password` and authenticates.
5. Given no password is provided in a no-keychain context, When `iris_add_server` is
   called, Then the server is added without a password; the response notes no
   credential was stored.

---

## Functional Requirements

- **FR-001**: Add `pub password: Option<String>` to `ServerEntry` in
  `crates/iris-agentic-dev-core/src/iris/servers_config.rs`, with
  `#[serde(skip_serializing_if = "Option::is_none")]`. Existing entries without the
  field deserialize normally — `None`.

- **FR-002**: In `iris_add_server` (mod.rs), when `store_credential` returns
  `SmCredentialError::KeychainUnavailable` and `p.password` is non-empty, write
  `password: Some(p.password.clone())` into the `ServerEntry` before calling
  `servers_config::save_native_config`.

- **FR-003**: Success response in the plaintext-fallback path:
  `{added: true, name: "...", stored_plaintext: true, warning: "Password stored in
plaintext in servers.json — use Server Manager for production credentials.",
note: "Restart iad for the pool to include this server."}`.

- **FR-004**: Credential resolution in the connection pool: after a keychain miss,
  check `ServerEntry.password` as a fallback. Keychain wins if present; plaintext
  field is last resort. Locate the resolution point in connection_pool.rs or
  discovery.rs where the pool builds `IrisConnection` objects.

- **FR-005**: `iris_remove_server` removes the entire server entry from servers.json,
  which implicitly removes the password (no code change needed — existing
  `iris_remove_server` behavior already satisfies this).

- **FR-006**: `iris_servers` listing never exposes the password value. The existing
  response shape `{name, host, port, namespace, username, source, reachable}` is
  unchanged. Add `has_plaintext_credential: bool` to signal to the user that Server
  Manager migration is recommended.

- **FR-007**: Update `docs/connecting.md` to document the plaintext fallback behavior
  and recommend Server Manager for production credentials.

---

## Key Entities

- **`ServerEntry`** (`crates/iris-agentic-dev-core/src/iris/servers_config.rs`):
  add `pub password: Option<String>`.
- **`iris_add_server`** (`crates/iris-agentic-dev-core/src/tools/mod.rs:7491`):
  add plaintext fallback branch after `SmCredentialError::KeychainUnavailable`.
- **`AddServerParams`**
  (`crates/iris-agentic-dev-core/src/tools/server_tools.rs:19`): no change —
  `password: String` already present.
- **Credential resolution**: wherever the pool or discovery builds `IrisConnection`
  from a `ServerEntry`, add fallback to `entry.password` after keychain lookup.

---

## Test Layers (per project constitution)

1. **Unit / JSON round-trip**: parse a `ServerEntry` JSON string containing a
   `"password"` field; assert deserialized `password == Some("SYS")`. Parse one
   without `"password"`; assert `password == None`. Covers serde silent-drop.

2. **Binary invocation** (`#[ignore]`, `IAD_BINARY`): spawn `iris-agentic-dev` in a
   real headless environment (Linux CI has no keychain by default — no mock flag needed),
   send `tools/call iris_add_server` with a password. Assert JSON-RPC response contains
   `stored_plaintext: true` and that `servers.json` written to temp dir contains the
   `password` field. On macOS dev machines where keychain exists, this test is expected
   to be skipped or fail to the keychain path — CI (Linux) is the canonical environment.

3. **No live IRIS integration needed**: this feature is config-only — authentication
   correctness is covered by existing connection tests.

---

## Success Criteria

- `iris_add_server` succeeds end-to-end in a headless MCP context with no keychain.
- A server added via plaintext fallback can be used immediately (after pool reload
  — per Gap #2 / spec 093) to connect and run tools.
- Existing servers using keychain storage are unaffected.
- The plaintext password value is never returned in any tool response.
- `iris_servers` response shape unchanged for existing callers.

---

## Out of Scope

- Encrypting the password at rest in servers.json (Server Manager's job).
- Writing the password to `.iris-agentic-dev.toml` `[instance.*]` blocks — that
  path already works for manual config; this spec only fixes `iris_add_server`.
- Interactive `--password-stdin` flow (not applicable in MCP subprocess context).
- SSH tunnel management.

---

## Assumptions

- `servers.json` lives in `~/.config/iris-agentic-dev/servers.json` (user-private
  dir, 600 on Unix). Plaintext storage is acceptable for dev credentials; production
  should use Server Manager.
- The hot-reload gap (Gap #2, spec 093) is separate. After this fix the server is in
  config but the running pool requires a restart. Noted in the response.
- `KeychainUnavailable` is the correct variant to key the fallback on — other
  keychain errors (locked, permission denied) still surface as errors.
