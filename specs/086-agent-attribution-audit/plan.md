# Implementation Plan: Agent Attribution and Audit

**Branch**: `086-agent-attribution-audit` | **Date**: 2026-08-27 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/086-agent-attribution-audit/spec.md`

## Summary

An IRIS operator cannot currently tell an agent's work from a programmer's. Every HTTP caller
arrives through the Web Gateway, which flattens caller identity to `CSPa24.so`, and this tool
sent no `User-Agent` at all — so the Postman-vs-Chrome technique the customer asked about had
nothing to distinguish.

This feature closes that in three parts. Every IRIS-bound request carries a caller marker in
`User-Agent` naming the product, version, caller mode, an operator label, and the connected
MCP client — so agent traffic is filterable in any web-server access log. A guide documents
which native IRIS audit events to enable (`RoutineChange`, the `%SQL/*` family, `Login`),
what those records do and do not contain, and that distinct per-environment credentials
remain the enforcement mechanism. And, opt-in per connection and off by default, the tool
writes its own `%SYS.Audit` records that carry the tool name and MCP client identity the
native records omit — refusing to create the audit event definition itself, because that is a
`%SYS` security write an agent should not perform.

The platform is moving in the same direction but not far enough for this case: IRIS 2026.3.0
adds `%System/%MCP/ToolCall` and `%System/%MCP/ToolDiscovery` (DP-452957) for MCP servers **IRIS
hosts**, and even there recording needs an audit policy applied to the server or tool set, not
just the enabled event type (DP-445295). An external MCP server reaching IRIS over Atelier REST
produces
none of them, so the guide documents both trails and our own event stays under
`EventSource='iris-agentic-dev'` rather than colliding with the platform's `%MCP` namespace
(research R13).

Technically this is small: a shared HTTP-client constructor so no IRIS-bound client is
anonymous, one `tokio::task_local!` alongside the existing `CALL_START` to carry MCP client
identity into the call, one `$SYSTEM.Security.Audit` call at the choke point that already
writes the local audit entry, one camelCase key on the existing `[policy.<server-name>]`
block, and a documentation guide under contract test. No new dependencies.

## Technical Context

**Language/Version**: Rust 2021 edition (workspace: `iris-agentic-dev-core`, `iris-agentic-dev-bin`)
**Primary Dependencies**: `rmcp` 3.1.3 (MCP server, `RequestContext`/`Peer::peer_info`),
`tokio` (`task_local!`), `reqwest` (HTTP clients), `tokio-tungstenite` (WS handshake),
`serde`/`serde_json`/`toml` (config), `tracing` (warn-once). **No new crates.**
**Storage**: None added. Config in `.iris-agentic-dev.toml`; audit records live in IRIS
`%SYS.Audit` and in the existing local JSONL audit log.
**Testing**: `cargo test` (unit), `#[ignore]` subprocess tests via `IAD_BINARY`, `#[ignore]`
live-IRIS integration tests against `iris-dev-iris` (`localhost:52780`) with
`--test-threads=1`. Coverage via `cargo llvm-cov … -- --include-ignored`.
**Target Platform**: macOS, Linux, Windows (single static binary; distroless container image)
**Project Type**: Single Rust workspace, two crates
**Performance Goals**: Marker construction adds no measurable per-request cost. Audit
emission measured at under `$ZHOROLOG` resolution for 10 writes — no latency budget needed.
**Constraints**: The tool MUST NOT write `%SYS` security configuration. Audit write failure
MUST NOT fail the tool call. The Docker-exec transport carries no header and must warn rather
than lose attribution silently. Audit config is global container state, so tests restore it.
Audit read-back goes through the `%SYS.Audit` List query, never a `SELECT` (DP-449511 — selects
can read stale indices). The operator label is ASCII-only, because `%SYS.Audit::Export()` throws
`<ILLEGAL VALUE>` on a Unicode character anywhere in a record (DP-446307).
**Scale/Scope**: 9 IRIS-bound client construction sites, 1 WS handshake, 1 config key, 1
audit event definition, 1 new documentation guide, 25 functional requirements.

## Constitution Check

_GATE: Must pass before Phase 0 research. Re-check after Phase 1 design._

| Principle                      | Status | Notes                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| ------------------------------ | ------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| I. Zero-Install Binary         | PASS   | No new install step. The audit event definition is an operator action documented in the guide, not an install prerequisite; with it absent, emission is inert and the tool still works.                                                                                                                                                                                                                                                                         |
| II. ObjectScript Sanity        | PASS   | Every API verified against live IRIS 2026.2 in [research.md](./research.md): `$SYSTEM.Security.Audit` 5-arg signature, its `1`/`0` return semantics for enabled/disabled/missing, the `Security.Events` API surface, the empty error text, `%SYS.Audit` column set, and the `RoutineChange` empty-client-field limitation.                                                                                                                                      |
| III. HTTP-First Execution      | PASS   | No new Docker-required tool. The Docker-exec path is only _detected_, to warn that attribution is unavailable there (FR-011).                                                                                                                                                                                                                                                                                                                                   |
| IV. Test-First, Fixture-Driven | PASS   | All three layers required by the spec's Test Requirements. Config key parsed from a TOML **string** through `ConnectionPolicyRaw` (the #110 pattern). Marker unit tests already exist and were written before the code they cover.                                                                                                                                                                                                                              |
| V. Output Shape Parity         | PASS   | No new tool and no changed response shape. The warn-once and the audit-failure counter surface through the existing connection status.                                                                                                                                                                                                                                                                                                                          |
| VI. Environment Guard          | PASS   | Emission is off by default and enabled per connection through the existing `[policy.<server-name>]` block. FR-024 forbids the `%SYS` write outright, and FR-025 keeps the non-configurable code-edit refusal.                                                                                                                                                                                                                                                   |
| VII. Dependency Minimalism     | PASS   | Zero new crates — see R9. MCP client identity uses `tokio::task_local!`, already used at `tools/mod.rs:60`.                                                                                                                                                                                                                                                                                                                                                     |
| VIII. 90% Coverage Gate        | PASS   | Polish phase carries the `cargo llvm-cov … -- --include-ignored` task. New code is small and each branch (marker with/without label, docker-exec warn, emission on/off/failed) is directly reachable from a test.                                                                                                                                                                                                                                               |
| IX. Tool Lift Requirement      | N/A    | This feature adds no MCP tool and changes no tool description, so there is no lift to measure. It changes an HTTP header, a config key, and documentation. Recorded here rather than left implicit. **Exception basis**: the constitution's N/A path covers "internal-only / not invoked by agents directly" infra changes; this is infrastructure (header + docs), not an agent-callable tool. No amendment required per the constitution's exception wording. |
| X. ObjectScript Coverage       | N/A    | Pure Rust. The only ObjectScript is the `Security.Events.Create` recipe printed for the operator to run; it ships as documentation, not as a compiled class.                                                                                                                                                                                                                                                                                                    |

_A plan with any FAIL gate MUST NOT proceed to implementation._ No FAIL gates.

## Project Structure

### Documentation (this feature)

```text
specs/086-agent-attribution-audit/
├── plan.md              # This file
├── spec.md              # Feature specification (FR-001 … FR-025)
├── research.md          # Phase 0 — measured IRIS facts and design decisions R1–R13
├── data-model.md        # Phase 1 — entities: caller marker, audit record, config key
├── quickstart.md        # Phase 1 — operator walkthrough, end to end
├── contracts/
│   ├── caller-marker.md     # User-Agent grammar and sanitizing rules
│   ├── audit-record.md      # %SYS.Audit field mapping and EventData format
│   └── policy-config.md     # [policy.<server-name>] key contract
├── checklists/
│   └── requirements.md  # Spec quality checklist (all items pass)
└── tasks.md             # Phase 2 — /speckit.tasks output, NOT created here
```

### Source Code (repository root)

```text
crates/iris-agentic-dev-core/
├── src/
│   ├── iris/
│   │   ├── connection.rs        # CallerMode, user_agent(), sanitized_agent_label();
│   │   │                        #   add shared iris_http_client() constructor,
│   │   │                        #   MCP-peer task-local read, docker-exec warn-once
│   │   ├── discovery.rs         # 4 anonymous clients (134, 223, 429, 582) → constructor
│   │   ├── ws_session.rs        # cookie client (335) + WS handshake (118) → marker
│   │   ├── audit_log.rs         # existing local JSONL log — precedent, unchanged
│   │   ├── iris_audit.rs        # NEW: $SYSTEM.Security.Audit emission + refuse-and-instruct
│   │   └── workspace_config.rs  # ConnectionPolicyRaw / ConnectionPolicy: new camelCase key
│   └── tools/
│       ├── mod.rs               # task_local! beside CALL_START; set in call_tool from
│       │                        #   context.peer.peer_info(); emit beside write_audit_entry
│       ├── search.rs            # sync_client (109) → constructor
│       └── doc.rs               # batch_client (359) → constructor
└── tests/
    ├── unit/
    │   ├── test_user_agent.rs           # exists, 7 tests, green
    │   ├── test_iris_audit.rs           # NEW: EventData format, refuse-and-instruct text
    │   └── test_policy_audit_config.rs  # NEW: TOML round-trip for the new key
    └── integration/
        └── test_iris_audit_live.rs      # NEW: #[ignore], live %SYS.Audit read-back + negative

crates/iris-agentic-dev-bin/
├── src/main.rs                          # set_caller_mode wiring (exists, lines 80-86)
└── tests/integration/
    ├── test_exec_live.rs                # test_user_agent_visible_to_iris (exists, green)
    └── test_attribution_stdio.rs        # NEW: #[ignore], IAD_BINARY subprocess wiring test

docs/
└── agent-attribution.md                 # NEW: the guide (FR-013 … FR-019)
```

**Structure Decision**: The existing two-crate layout is unchanged. Attribution belongs in
`iris-agentic-dev-core/src/iris/` next to `connection.rs` and `audit_log.rs`, because that is
where connection identity and the local audit trail already live. One new module,
`iris/iris_audit.rs`, keeps the `%SYS.Audit` emission separable from the local JSONL log so
the two can be tested independently even though `call_tool` drives both from one place.

## Complexity Tracking

No Constitution Check violations, so nothing to justify.

The one place the plan deliberately diverges from the spec's stated reasoning is recorded in
research R3: FR-012's rationale clause claims a per-request header is required. The
requirement stands, but the implementation uses a task-local read at client-construction
time, which is equivalent and touches no `.send()` call site. This is a simplification, not
an added abstraction.

Two constraints arrived from open IRIS defects after the spec was written and are recorded in
research R7 and R7a rather than as new requirements: read audit records through the List query,
and keep the label ASCII. Neither adds a component; both narrow existing behavior.
