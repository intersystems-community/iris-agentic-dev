# Phase 0 Research: 101-nopws-connectivity

**Date**: 2026-09-02
**Branch**: `093-toml-hot-reload` (plan authored here; impl targets `101-nopws-connectivity`)

---

## 1. WorkspaceConfig current state

File: `crates/iris-agentic-dev-core/src/iris/workspace_config.rs`

`WorkspaceConfig` is a flat `#[derive(Debug, Deserialize, Default, Clone)]` struct. All
fields are declared as named struct fields with `#[serde(default)]` where bool. Current
relevant fields:

```rust
pub docker_only: bool,           // #[serde(default)] — already present
pub container: Option<String>,   // container name for docker exec
pub host: Option<String>,        // HTTP host (skips docker discovery)
pub web_port: Option<u16>,       // #[serde(alias = "port")]
pub web_prefix: Option<String>,  // path prefix for webgateway
pub username: Option<String>,
pub password: Option<String>,
```

**Fields to add**: `nopws: bool` (default false) and `ssh_host: Option<String>`.

There are no tests in `workspace_config.rs` using `toml::from_str` for these fields
yet — the existing round-trip tests use struct literals, which cannot catch serde
silent-drop (the #110 pattern). The FR-011 test must use `toml::from_str`.

Existing tests that construct `WorkspaceConfig` directly (struct literals) appear in
lines 1088–1300. They will need the two new fields added to the literal, or the struct
will need `..Default::default()` fill — but those tests already use field-by-field
construction. The simplest fix: add `nopws: false, ssh_host: None` to each literal, or
add `#[allow(dead_code)]` and rely on `Default`. The cleanest approach matches what 085
did: new fields go in `Default::default()`, and the struct-literal tests are updated to
include the new fields.

---

## 2. iris_compile docker exec fallback pattern

File: `crates/iris-agentic-dev-core/src/tools/mod.rs`, lines 3245–3301

The pattern reads `docker_only` and `no_pws` from the locked `ConnectionState`, then
branches before any HTTP attempt:

```rust
let (docker_only, no_pws) = {
    let conn_lock = self.connection.lock().unwrap();
    let docker_only = conn_lock.iris.as_ref()
        .map(|i| i.base_url == "http://127.0.0.1:1" || i.base_url.starts_with("http://127.0.0.1:1/"))
        .unwrap_or(false);
    let no_pws = conn_lock.iris.as_ref()
        .and_then(|i| i.version.as_deref())
        .map(|v| v.contains("2026.2.0AI"))
        .unwrap_or(false);
    (docker_only, no_pws)
};

if docker_only || no_pws {
    let code = format!(r#"do $SYSTEM.OBJ.Compile("{}","{}")"#, ...);
    let result = iris.execute(&code, &namespace).await;
    return match result {
        Ok(output) => ok_json(json!({ "success": ..., "method": "docker_exec", ... })),
        Err(e) if e.to_string() == "DOCKER_REQUIRED" => err_json("DOCKER_REQUIRED", "..."),
        Err(e) => err_json("COMPILE_FAILED", &e.to_string()),
    };
}
```

`iris.execute()` is the docker exec path on `IrisConnection`. It calls
`docker exec -i <container> iris session IRIS -U <namespace>` with `\r\n` line endings
and `Halt\r\n` terminator. When `IRIS_CONTAINER` is unset, it returns
`Err("DOCKER_REQUIRED".into())`.

**For `iris_execute`**: the same pattern applies. The current `iris_execute` code
(lines 4072–4370) tries HTTP first via `execute_via_generator`, then falls through to
`iris.execute()` as docker fallback. The gap: there is **no early skip** before HTTP
when `docker_only || no_pws` is true. The fix: add the same early-branch block from
`iris_compile`, returning `execution_path: "docker_exec_local"` (or
`"docker_exec_ssh"` when `ssh_host` is set).

`iris_execute` current response fields: `success`, `output`, `namespace`, `method`
(`"http"` or `"docker"`), `auth_user`, `service_account_env`, optionally
`error_code`, `session_state`, `sql_translated`, `translated_code`,
`translation_warning`. **Add `execution_path`** alongside `method` (keeping `method`
for backward compat — it is in the existing response shape).

---

## 3. iris_test_server current shape

File: `crates/iris-agentic-dev-core/src/tools/server_tools.rs` + `mod.rs` lines 7621–7675.

`TestServerParams` has only `name: String`. The implementation:

1. Builds an Atelier URL from the pool connection.
2. Makes a GET request.
3. Returns: `{ name, reachable, error?, http_status?, latency_ms, auth?, atelier_version?,
iris_version? }`.

**Fields to add** (FR-003): `nopws: bool`, `web_available: bool`, `nopws_detected: bool`,
`nopws_evidence: Option<String>`. The `web_available` field mirrors `reachable` but is
semantically distinct — `reachable` is the raw TCP/HTTP result; `web_available` signals
whether Atelier REST is usable.

NoPWS auto-detection (FR-005): when web probe fails and `nopws` is not set from config,
attempt `docker exec <container> grep WebServer <path>` probing two paths:

- `/usr/irissys/iris.cpf` (Ubuntu-based images)
- `/usr/local/etc/irissys/iris.cpf` (Alpine-based images)

Also attempt TCP probe of superserver port 1972.

The `nopws` value in the config is accessible via the pool connection's source metadata.
The implementation will need to look up whether the configured WorkspaceConfig for the
named server has `nopws = true` — or fall through to auto-detection.

---

## 4. derive_capabilities() — existing NoPWS detection

File: `crates/iris-agentic-dev-core/src/tools/mod.rs`, lines 2196–2243.

```rust
pub fn derive_capabilities(iris_version: Option<&str>, docker_only: bool, ...) {
    let no_pws = iris_version.map(|v| v.contains("2026.2.0AI")).unwrap_or(false);
    let atelier_rest = !docker_only && !no_pws;
    // ...
    json!({ "private_web_server": !no_pws, "atelier_rest": atelier_rest, "compile_path": ..., })
}
```

This is called at connect time to expose capabilities to the caller. The `no_pws` flag
is derived **from the IRIS version string** — it requires a successful connection probe
to know the version. For the `nopws = true` config field (FR-001), the flag is set
**before any connection attempt**, enabling the docker exec route even if version is
unknown. The two mechanisms complement each other.

Note: `iris_compile` does NOT call `derive_capabilities()` directly — it re-derives
`no_pws` from `conn_lock.iris.version` at dispatch time. Same approach for `iris_execute`.

---

## 5. SSH path for docker exec

`IrisConnection::execute()` runs `docker exec -i <container> iris session IRIS -U <ns>`.
For SSH (FR-009), the command becomes:

```
ssh -o StrictHostKeyChecking=no <ssh_host> docker exec -i <container> iris session IRIS -U <ns>
```

`StrictHostKeyChecking=no` is required because the MCP process is non-interactive. SSH
uses the system's SSH config for keys, ProxyJump chains, etc. — iad does not manage
credentials.

**Implementation**: the `IrisConnection::execute()` method or a new `execute_ssh()` method
will check for a `ssh_host` field. Since `ssh_host` is in `WorkspaceConfig`, not
`IrisConnection`, the calling code in `iris_execute` will need to read it from the config
store and pass it to the execution method, or embed it in a new helper struct.

The cleanest approach: add `ssh_host: Option<String>` to `IrisConnection` itself (or pass
it as a parameter to `execute()`). The connection is already built from `WorkspaceConfig`
in `workspace_config_to_connection()` — that's where `ssh_host` should be wired in.

---

## 6. Skills format

File: `skills/skills/iris-agentic-dev/SKILL.md`

Skills are Markdown files with YAML frontmatter:

```yaml
---
name: <skill-name>
description: <one-line description for skill selection>
author: tdyar
managed_by: iris-agentic-dev
---
```

Body is Markdown with H2 sections, tables, and fenced code blocks. Skill keyword phrases
appear in the `description` field — that is what the skill selector matches against. The
skills directory for this project is `skills/skills/iris-agentic-dev/`. A new skill goes
in `skills/skills/iris-agentic-dev/nopws-setup/SKILL.md` or as a standalone file at
`skills/skills/iris-agentic-dev/nopws-setup.md`.

---

## 7. Connection struct and ssh_host propagation

`IrisConnection` is in `connection.rs`. It holds: `base_url`, `namespace`, `username`,
`password`, `version`, `source`. Adding `ssh_host: Option<String>` here is the right
place because `execute()` lives on `IrisConnection`.

`workspace_config_to_connection()` already maps `WorkspaceConfig` fields into
`IrisConnection` — it sets `IRIS_CONTAINER` env and returns `None` (docker discovery) or
`Some(IrisConnection)`. The `docker_only = true` branch returns an IrisConnection with
sentinel URL `http://127.0.0.1:1`. The `nopws = true` branch should do the same (or
merge with `docker_only`): when `nopws = true`, the behavioral effect is identical to
`docker_only = true` **unless** the web port is actually reachable (FR-003 scenario 3).

---

## 8. Test structure summary

Three required test layers (from spec):

1. **Unit / TOML round-trip** — `toml::from_str` with `nopws = true` and
   `ssh_host = "baystate"` → assert struct fields set. Location:
   `workspace_config.rs` tests block.

2. **Binary invocation** (`#[ignore]`, `IAD_BINARY`) — send `initialize` + `tools/call`
   for `iris_test_server` over stdio JSON-RPC; assert `nopws` field present. Location:
   `tests/binary_invocation/` or existing binary test module.

3. **Live IRIS integration** (`#[ignore]`, `iris-dev-iris`, `--test-threads=1`) —
   `iris_test_server` against community container returns `nopws_detected: false`;
   docker exec fallback test with web port pointed at closed port. Location:
   `tests/integration/`.

---

## 9. No new crate dependencies

All changes use existing crates: `tokio` (async), `serde`/`serde_json`/`toml` (config),
`reqwest` (HTTP probe), `std::process::Command` (docker exec / ssh). No new Cargo.toml
entries required. Constitution VII: PASS.

---

## 10. Verified IRIS API usage

The docker exec path does NOT call any ObjectScript API — it runs `iris session IRIS` and
pipes ObjectScript code line by line. No class or method verification is needed for the
execution path itself.

For `iris_test_server`, the auto-detection path reads iris.cpf via `docker exec grep` —
this is a shell command, not an ObjectScript API. No IRIS API verification required.

Constitution II: PASS (no new ObjectScript APIs introduced).
