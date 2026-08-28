# Contract: `[policy.<server-name>]` audit key

**Feature**: 086-agent-attribution-audit | Covers FR-020, FR-022

## The key

```toml
[policy.prod-iris]
mcpTemplate = "Live"
irisAudit = true       # NEW — emit %SYS.Audit records for every tool call on this connection

[policy.dev-iris]
irisAudit = false      # default; may be omitted entirely
```

| Key         | Type | Default | Scope          |
| ----------- | ---- | ------- | -------------- |
| `irisAudit` | bool | `false` | One connection |

Per-connection scope is the point: the customer's ask is to treat environments differently, and
`[policy.<server-name>]` is the block that already expresses per-environment behavior.

## Struct binding

Added to `ConnectionPolicyRaw` (`crates/iris-agentic-dev-core/src/iris/workspace_config.rs:213`):

```rust
#[serde(rename = "irisAudit", default)]
pub iris_audit: bool,
```

and surfaced on `ConnectionPolicy` alongside `allow`, `mcp_template`, `data_policy`,
`global_blocklist`, `data_policy_kill_allowlist`. camelCase in TOML, snake_case in Rust, matching
every existing key in this block.

## Test contract

The key MUST be exercised by parsing a TOML **string** through the real deserializer:

```rust
let cfg: FleetConfig = toml::from_str(
    "[policy.prod]\nirisAudit = true\n"
).unwrap();
assert!(cfg.policy["prod"].iris_audit);
```

A test that constructs `ConnectionPolicyRaw { iris_audit: true, .. }` proves nothing — it cannot
catch a missing or misspelled `#[serde(rename)]`, which is exactly how #110 shipped a TOML key
that serde silently ignored.

Required assertions:

1. `irisAudit = true` parses to `true`.
2. `irisAudit = false` parses to `false`.
3. The key absent parses to `false` (the `default`).
4. A wrong-case spelling (`irisaudit`, `iris_audit`) does not silently enable auditing.
5. The key on one connection does not affect another connection in the same file.

## Wiring contract

A parsed config field that nothing reads is the #111 failure. So beyond the round-trip test, a
subprocess test (`IAD_BINARY`, `#[ignore]`) starts the binary with a config that sets
`irisAudit = true`, calls a tool over stdio, and asserts the emission path was taken — and a
matching run with the key absent asserts it was not.

## Interaction with existing policy behavior

`irisAudit` is independent of `allow`, `mcpTemplate` and `dataPolicy`. It changes what gets
recorded, never what is permitted. In particular:

- A call blocked by a gate is still a call worth recording; emission describes the attempt and
  its outcome.
- `irisAudit = true` does not imply the local JSONL audit log is on, and vice versa. The local
  log is keyed off the presence of a policy block (`audit_log.rs::should_write`); this key is
  keyed off its own value.
