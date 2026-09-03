# Implementation Plan: 101-nopws-connectivity

**Branch**: `101-nopws-connectivity` | **Date**: 2026-09-02 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/101-nopws-connectivity/spec.md`

## Summary

AI-branch IRIS builds (irishealth-ai, iris-ai, 2026.3+) ship without an embedded web
server (`WebServer=0` in iris.cpf). iad currently has no fallback when Atelier REST is
unavailable on these builds — operators see a raw "connection refused" with no explanation.
This feature closes three gaps: (1) a `nopws = true` TOML flag and `ssh_host` field in
`WorkspaceConfig`, with clear error messages from `iris_test_server` and auto-detection via
iris.cpf probing; (2) an `iris_execute` docker exec early-branch mirroring the existing
`iris_compile` fallback, plus an `execution_path` field in every `iris_execute` response;
(3) a bundled `nopws-setup` skill covering detection, webgateway sidecar setup, and
first-boot password clearing.

Partial support already exists: `docker_only = true` sentinel URL routing, `derive_capabilities()`
version-string detection, and `iris_compile` docker exec fallback. This spec extends that
foundation without changing any existing Atelier path.

## Technical Context

**Language/Version**: Rust 2021 edition (stable toolchain, `aarch64-apple-darwin`)
**Primary Dependencies**: `rmcp`, `tokio`, `serde`/`serde_json`/`toml`, `reqwest`,
`std::process::Command` — all already in workspace. Zero new crates.
**Storage**: N/A — no database; config lives in `.iris-agentic-dev.toml`
**Testing**: `cargo test` (unit), `cargo test -- --include-ignored` (integration),
`cargo llvm-cov` (coverage gate)
**Target Platform**: macOS arm64/x86_64, Linux x86_64, Windows x86_64 (docker exec path
is Linux/macOS only per spec Out of Scope)
**Project Type**: Single Rust workspace (two crates: core + bin)
**Performance Goals**: docker exec timeout is 30 s (from existing `execute()` in
`connection.rs` line 707 — `tokio::time::timeout(Duration::from_secs(30), ...)`). Note:
the spec assumption of 10 s (section "Assumptions") is incorrect — implementation will
use 30 s consistent with the existing execute() timeout.
**Constraints**: Zero new install steps; binary stays statically linked; no mock IRIS in
any test that touches IRIS behavior
**Scale/Scope**: Additive changes to four existing files + two new test files + one skill
file; no schema migration

### WorkspaceConfig.docker_only and derive_capabilities

`WorkspaceConfig` is a flat `#[derive(Debug, Deserialize, Default, Clone)]` struct in
`crates/iris-agentic-dev-core/src/iris/workspace_config.rs`. The existing `docker_only:
bool` field (with `#[serde(default)]`) routes all execution through `docker exec iris
session` by setting a sentinel base URL (`http://127.0.0.1:1`). Two new fields are added
immediately after it: `nopws: bool` and `ssh_host: Option<String>`.

`derive_capabilities()` (in `tools/mod.rs` ~line 2196) detects `2026.2.0AI` version
strings and sets `no_pws = true` at connect time — this is the version-heuristic path.
The new `nopws = true` config flag is a pre-connection declaration that enables docker
exec routing before any version probe. Both must be ORed in the early-branch:

```rust
// no_pws routing = version_heuristic (v.contains("2026.2.0AI")) OR config.nopws
// Both must be ORed; config.nopws enables the path even when version is unknown
if docker_only || no_pws {  // same pattern as iris_compile (tools/mod.rs ~line 3262)
```

### docker exec path in connection.rs

`IrisConnection::execute()` at `connection.rs:685` runs
`docker exec -i <container> iris session IRIS -U <namespace>`. It reads `IRIS_CONTAINER`
fresh on each call, pipes ObjectScript lines, and applies a **30-second timeout** via
`tokio::time::timeout(Duration::from_secs(30), child.wait_with_output())`. When
`IRIS_CONTAINER` is unset, it returns `Err("DOCKER_REQUIRED")`.

For SSH (FR-009), a new path in `IrisConnection` constructs:
`ssh -o StrictHostKeyChecking=no <ssh_host> docker exec -i <container> iris session IRIS -U <ns>`.
`ssh_host: Option<String>` is added to `IrisConnection` and populated from
`WorkspaceConfig.ssh_host` in `workspace_config_to_connection()`.

### iris_compile fallback — the pattern iris_execute will mirror

`iris_compile` (`tools/mod.rs` ~lines 3245–3301) reads `(docker_only, no_pws)` from the
locked `ConnectionState`, branches before any HTTP attempt when either is true, calls
`iris.execute()`, and returns with `method: "docker_exec"`. The `iris_execute` fix applies
the identical early-branch, adding `execution_path: "docker_exec_local"` (or
`"docker_exec_ssh"` when `ssh_host` is set) alongside the existing `method` field.
`method` is preserved for backward compatibility.

## Constitution Check

_GATE: Must pass before Phase 0 research. Re-check after Phase 1 design._

| Principle                      | Status | Notes                                                                                                                                                                                                                                                                                       |
| ------------------------------ | ------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| I. Zero-Install Binary         | PASS   | No new install step. Zero new crates. Binary stays statically linked.                                                                                                                                                                                                                       |
| II. ObjectScript Sanity        | N/A    | No new ObjectScript APIs introduced. docker exec path pipes code line by line, no class/method calls. auto-detection uses shell `grep`, not ObjectScript.                                                                                                                                   |
| III. HTTP-First Execution      | PASS   | `nopws = true` is opt-in; existing Atelier path is unchanged. The early-branch only fires when `docker_only \|\| no_pws`. No new Docker-required tool added to Merged tier.                                                                                                                 |
| IV. Test-First, Fixture-Driven | PASS   | tasks.md has all three required test layers written before implementation in every phase. Round-trip tests (T004), binary invocation tests (T016), and live IRIS integration tests (T011, T017, T028) are each written and confirmed FAILING before the corresponding implementation tasks. |
| V. Output Shape Parity         | PASS   | `execution_path` is additive to the existing `iris_execute` response. `method` field preserved for backward compat. New `iris_test_server` fields (`nopws`, `web_available`, `nopws_detected`, `nopws_evidence`) are additive. No existing keys removed or renamed.                         |
| VI. Environment Guard          | N/A    | No new write-capable tools introduced. `iris_execute` and `iris_compile` are already gated. The docker exec fallback is execution-only, not data-write.                                                                                                                                     |
| VII. Dependency Minimalism     | PASS   | Zero new Rust crate dependencies. All changes use existing workspace crates: `tokio` (async), `serde`/`serde_json`/`toml` (config), `reqwest` (HTTP probe), `std::process::Command` (docker exec / ssh). Verified in research.md section 9.                                                 |
| VIII. 90% Coverage Gate        | PASS   | T041 in Phase 9 (Polish) is the explicit coverage-gate task: `cargo llvm-cov --summary-only -p iris-agentic-dev-core -- --include-ignored` must show TOTAL ≥ 90.00%. Non-optional; phase does not complete below threshold.                                                                 |
| IX. Tool Lift Requirement      | PASS   | `iris_execute` and `iris_test_server` are both agent-facing MCP tools gaining new response fields and behavioral routing. Lift ≥ +0.20 required before merge. T_LIFT task in tasks.md: run GEPA eval harness on both tools; record A/B results in `lift-results.md`.                        |
| X. ObjectScript Coverage Gate  | N/A    | Pure Rust feature. No new ObjectScript classes or routines shipped.                                                                                                                                                                                                                         |

_A plan with any FAIL gate MUST NOT proceed to implementation._

No FAIL gates found. All gates pass or are N/A.

## Project Structure

### Documentation (this feature)

```text
specs/101-nopws-connectivity/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── spec.md              # Feature specification
├── tasks.md             # Task breakdown (test-first, 9 phases)
├── contracts/           # iris_execute_101.md response shapes + error codes
└── quickstart.md        # Scenarios A–E (local docker, ssh remote, webgateway, etc.)
```

### Source Code (repository root)

```text
crates/iris-agentic-dev-core/src/iris/
├── workspace_config.rs        # Add nopws: bool, ssh_host: Option<String> to WorkspaceConfig;
│                              # update workspace_config_to_connection() to propagate ssh_host
└── connection.rs              # Add ssh_host: Option<String> to IrisConnection;
                               # add SSH docker exec command construction

crates/iris-agentic-dev-core/src/tools/
├── mod.rs                     # iris_execute early-branch + execution_path field;
│                              # iris_compile unchanged (already has fallback);
│                              # iris_test_server nopws/web_available/nopws_detected fields;
│                              # probe_nopws_via_docker_exec() helper;
│                              # iris_doc / iris_source_control NOPWS_ATELIER_REQUIRED guard
└── server_tools.rs            # iris_test_server NoPWS message + web probe logic

tests/
├── integration/
│   └── nopws_101.rs           # Live IRIS integration tests (#[ignore], --test-threads=1)
├── binary/
│   └── nopws_101.rs           # Binary invocation tests (#[ignore], IAD_BINARY)
└── skills/
    └── nopws_skill_test.rs    # Skill keyword presence test

skills/skills/iris-agentic-dev/
└── nopws-setup/
    └── SKILL.md               # NoPWS detection, webgateway sidecar, docker_only, SSH,
                               # first-boot password clearing (≤300 lines)
```

**Structure Decision**: Single Rust workspace (existing layout). All changes are additive
to existing files. Three new test files and one new skill file. No new source modules.

## Phase Plan

| Phase | Name                                              | Priority   | Gate                                                                     |
| ----- | ------------------------------------------------- | ---------- | ------------------------------------------------------------------------ |
| 1     | Setup                                             | —          | Container running, clean build, clippy baseline                          |
| 2     | Foundational: WorkspaceConfig + IrisConnection    | BLOCKS ALL | Round-trip tests green (T004/T009)                                       |
| 3     | US1: NoPWS flag and clear error messages          | P1 / MVP   | T011 PASS (iris_test_server community returns nopws_detected:false)      |
| 4     | US2: iris_execute fallback + execution_path       | P1 / MVP   | T017 PASS (execution_path:atelier present), T016 PASS (binary test)      |
| 5     | US3: Remote NoPWS via SSH                         | P2         | T023 PASS (SSH command construction unit test)                           |
| 6     | US4: NoPWS auto-detection                         | P2         | T028 PASS (community container returns nopws_detected:false)             |
| 7     | US5: nopws-setup skill                            | P1         | T032 PASS (skill file exists, all 6 keywords present), /no-ai-slop clean |
| 8     | US2 extension: Atelier-required tool NoPWS errors | —          | T034 PASS (iris_doc returns NOPWS_ATELIER_REQUIRED)                      |
| 9     | Polish: fmt, clippy, coverage gate, docs          | —          | cargo llvm-cov ≥ 90.00%, all tests green                                 |

MVP stop-point: after Phases 1–4 and 7 complete, all P1 stories are testable.
Phases 5, 6, and 8 are the P2 / extension pass. Phase 9 is the release gate.
