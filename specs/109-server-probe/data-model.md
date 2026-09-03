# Data Model: 098-server-probe

## ProbeResult

New struct in `server_tools.rs` (or `probe.rs`):

```rust
pub struct ProbeResult {
    pub reachable: bool,
    pub auth: bool,                      // false on HTTP 401
    pub iris_version: Option<String>,
    pub atelier_version: Option<String>,
    pub namespace: Option<String>,
    pub latency_ms: Option<u64>,
    pub error: Option<String>,
}
```

## TestServerParams (updated)

```rust
pub struct TestServerParams {
    /// Named server from pool. If omitted, ad-hoc params (host) are required.
    pub name: Option<String>,
    /// Ad-hoc: hostname or IP to probe directly (skips pool lookup).
    pub host: Option<String>,
    /// Ad-hoc: web port. Default: 52773.
    pub web_port: Option<u16>,
    /// Ad-hoc: IRIS username. Default: "_SYSTEM".
    pub username: Option<String>,
    /// Ad-hoc: IRIS password. Default: "SYS".
    pub password: Option<String>,
}
```

Validation logic (in handler, not struct):

- `host` present → ad-hoc probe, ignore `name`
- `name` present, no `host` → named-server probe (existing path)
- neither → structured error: `"Provide either a server name or host/web_port parameters."`

## IrisServersParams (new)

```rust
pub struct IrisServersParams {
    /// When true, probe all servers in parallel (5s timeout each).
    pub probe: Option<bool>,
}
```

## IrisServersProbeEntry (JSON shape when probe=true)

Extends the existing server entry shape:

```json
{
  "name": "myserver",
  "host": "localhost",
  "port": 52780,
  "namespace": "USER",
  "username": "_SYSTEM",
  "source": "iad-native",
  "reachable": true,
  "latency_ms": 42,
  "error": null
}
```

When `probe=false` (default), `reachable` is `null`, `latency_ms` absent, `error` absent — identical to today.

## DiscoverySource

Consider adding `DiscoverySource::AdHoc` variant to label ad-hoc probe connections clearly. Not required — `EnvVar` can be reused. Prefer `AdHoc` for clarity.

## Error Codes

| Code               | When used                                                                                                                                                                                                                                                                                                                                        |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `SERVER_NOT_FOUND` | Named server (`name` param) is not present in the loaded pool.                                                                                                                                                                                                                                                                                   |
| `MISSING_PARAMS`   | Neither `name` nor `host` was provided to `iris_test_server`. Use this instead of `SERVER_NOT_FOUND` for the missing-both-params case so callers can distinguish "server not in pool" from "no params at all". Do **not** use `INVALID_PARAMS` here — `INVALID_PARAMS` signals malformed values; `MISSING_PARAMS` signals absent required input. |
