# Research: 098-server-probe

## IrisConnection Constructor

**Decision**: Use `IrisConnection::new(base_url, namespace, username, password, DiscoverySource::EnvVar)` for ad-hoc probes.
`base_url` format: `"http://{host}:{web_port}"`. `DiscoverySource::AdHoc` does not exist — use `EnvVar` as the closest analog for one-off probes, or introduce a new `DiscoverySource::AdHoc` variant.

**Rationale**: Constructor signature verified at `connection.rs:272`. Takes base_url, namespace, username, password, source. `atelier_url("/")` on the result gives the probe URL.

**Alternatives**: Could reuse `DiscoverySource::EnvVar` as a catch-all for ad-hoc — cleaner to add `AdHoc` variant to make intent clear in probe output.

## Probe Logic Location

**Decision**: Extract into `probe_server(host, web_port, namespace, username, password) -> ProbeResult` in `server_tools.rs` (or a new `probe.rs` submodule). Called from both `iris_test_server` and the fleet loop in `iris_servers`.

**Rationale**: `IrisConnection::probe()` at `connection.rs:356` mutates self and updates fields on the connection (version, atelier_version, system_mode). The shared function wraps this: construct connection → call `.probe()` → extract fields into `ProbeResult`.

**Alternatives**: Inline in both tools (rejected: duplication, spec FR-010 explicitly forbids it).

## Parallel Fleet Probe

**Decision**: `futures::future::join_all(probes)` inside `tokio::time::timeout(Duration::from_secs(5), ...)` per server. `futures` crate already in Cargo.toml (verify before assuming).

**Rationale**: Each server gets its own 5s timeout via `tokio::time::timeout`. `join_all` waits for all to complete (or time out). Total wall time = max(individual timeouts) = 5s, matching FR-009.

**Alternatives**: `FuturesUnordered` + collect — equivalent wall time, slightly more complex. `tokio::join!` — static arity, doesn't work for dynamic pool size. Stick with `join_all`.

## reqwest::Client Reuse

**Decision**: Share the existing `IrisTools.client` (or construct a short-timeout probe client via `IrisConnection::probe_client()`).

**Rationale**: `probe_client()` at `connection.rs:813` builds a client with short timeout specifically for probing — already exists, safe to call per-probe or share one instance across fleet.

## 401 Handling

**Decision**: HTTP 401 → `reachable: true`, `auth: false`, no `iris_version` / `atelier_version`. Matches existing `iris_test_server` behavior.

**Rationale**: Network reachable but credential rejected. Consistent with spec edge case and current behavior.

## TestServerParams Change

`name` becomes `Option<String>` (was required `String`). Add:
- `host: Option<String>`
- `web_port: Option<u16>` (default 52773)
- `username: Option<String>` (default `_SYSTEM`)
- `password: Option<String>` (default `SYS`)

`#[serde(default)]` on the struct or per-field defaults via `#[serde(default = "fn")]`.

## iris_servers probe Parameter

`iris_servers` currently takes no params (`iris_servers(&self)` with no `Parameters`). Need to add `IrisServersParams { probe: Option<bool> }` and change the handler signature.

**Decision**: Add `IrisServersParams` struct in `server_tools.rs`. Wire via `Parameters<server_tools::IrisServersParams>` in mod.rs:10464 dispatch.
