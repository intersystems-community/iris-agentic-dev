# Implementation Plan: Mirror Management Tools

**Branch**: `097-mirror-management` | **Date**: 2026-09-02 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/097-mirror-management/spec.md`

## Summary

Add two `iris_admin` actions — `mirror_add_async` and `mirror_failover` — that let ops
agents join an IRIS instance to an existing mirror set as an async (DR) member and promote
a backup to primary. Both actions use the `execute_via_generator` + `ZN "%SYS"` pattern
established by `iris_mirror_status_impl`. Gate classification: `mirror_add_async` →
`WriteClass::Write`; `mirror_failover` → `WriteClass::Destructive`. API signatures
(`SYS.Mirror.JoinMirrorAsAsyncMember`, `SYS.Mirror.BecomePrimary`) are verified against
iris-dev-iris in `research.md`. No new crate dependencies. Three test layers: unit, binary
invocation, live IRIS integration.

---

## Technical Context

**Language/Version**: Rust 2021 edition (stable toolchain, aarch64-apple-darwin)
**Primary Dependencies**: `rmcp`, `tokio`, `serde`/`serde_json`, `reqwest` — all
workspace-present; no new crates required
**Storage**: N/A — stateless tool; IRIS is the data store
**Testing**: `cargo test`, `cargo llvm-cov`, `cargo clippy`, `cargo fmt`; integration
tests use `#[ignore]` + `--include-ignored --test-threads=1`
**Target Platform**: macOS arm64/x86_64, Linux x86_64, Windows x86_64 (single binary)
**Project Type**: Single Rust workspace (two crates: core + bin)
**Performance Goals**: Same as existing `iris_admin` actions — no new latency requirements;
IRIS mirror API calls are low-frequency ops-automation actions
**Constraints**: Must not require any new IRIS-side class installs; must work against
IRIS 2016.1+ via Atelier REST / `execute_via_generator`
**Scale/Scope**: Two new match arms in `mod.rs`, two new `impl` functions in
`admin_tools.rs`, two new rows in `write_gate.rs` mixed table, updated tool description
and INVALID_ACTION fallthrough

---

## Constitution Check

_GATE: Must pass before Phase 0 research. Re-check after Phase 1 design._

| Principle | Status | Notes |
| --- | --- | --- |
| I. Zero-Install Binary | PASS | No new crate, no IRIS class install; pure `execute_via_generator` path via Atelier REST |
| II. ObjectScript Sanity | PASS | `SYS.Mirror.JoinMirrorAsAsyncMember` and `SYS.Mirror.BecomePrimary` verified live against iris-dev-iris 2026.2 — see `research.md` §"SYS.Mirror Write Classmethods (verified live)" |
| III. HTTP-First Execution | PASS | Both actions use `execute_via_generator`; no Docker exec path; `iris_admin` is already in `Merged` tier |
| IV. Test-First, Fixture-Driven | PASS | Unit tests (None iris → IRIS_UNREACHABLE), binary invocation tests (gate blocking), and live IRIS tests (#[ignore]) are all specified in tasks.md T012–T015a, T023–T024 before implementation tasks T017–T018, T026–T027 |
| V. Output Shape Parity | PASS | Both actions follow `{success, error_code, error}` error shape; success shapes documented in `data-model.md`; no existing counterpart to collide with |
| VI. Environment Guard | PASS | `mirror_add_async` classified `WriteClass::Write` (requires `IRIS_WRITE_TOOLS_ENABLED`); `mirror_failover` classified `WriteClass::Destructive` (requires `IRIS_DESTRUCTIVE_TOOLS_ENABLED`); gate entries go in `write_gate.rs` mixed table for `iris_admin` BEFORE dispatch arms exist (T008) |
| VII. Dependency Minimalism | PASS | No new crates; all required functionality is in the existing workspace (`serde_json`, `reqwest`, `tokio`) |
| VIII. 90% Coverage Gate | PASS | Polish phase (T032) runs `cargo llvm-cov --include-ignored` and asserts ≥ 90%; live IRIS integration tests (T020, T_FAILOVER_LIVE) cover the happy/error paths |
| IX. Tool Lift Requirement | PASS | Benchmark task required in `src/benchmark/tasks/` for both new actions; lift ≥ +0.20 must be measured and recorded in `specs/097-mirror-management/lift-results.md` before merge; lift phase precedes Polish phase (T034) |
| X. ObjectScript Coverage | N/A | No new ObjectScript classes shipped; ObjectScript is executed inline as string literals in Rust; the `execute_via_generator` codepath is already covered by the existing live IRIS integration test suite |

_No FAIL gates. Plan may proceed to implementation._

---

## Project Structure

### Documentation (this feature)

```text
specs/097-mirror-management/
├── plan.md              # This file
├── spec.md              # Feature specification
├── research.md          # Phase 0 — SYS.Mirror API verification (complete)
├── data-model.md        # Phase 1 — Rust structs, JSON shapes, error codes (complete)
├── tasks.md             # Phase 2 — 35 tasks T001–T034 + T015a (complete)
├── lift-results.md      # Required before merge (Principle IX)
└── benchmark-results.md # Coverage/benchmark pass (T034) — note: tasks.md uses this name; constitution requires lift-results.md — BOTH must be present
```

### Source Code (this feature touches)

```text
crates/iris-agentic-dev-core/src/
├── tools/
│   ├── write_gate.rs           # Add mirror_add_async + mirror_failover to mixed("iris_admin") table (~line 525)
│   ├── admin_tools.rs          # Add iris_mirror_add_async_impl, iris_mirror_failover_impl (~line 573 region)
│   └── mod.rs                  # Add dispatch arms (~line 7351), update tool description (~7164), update INVALID_ACTION fallthrough (~7351)
└── benchmark/
    └── tasks/
        ├── mirror-add-097.json     # Lift benchmark task for mirror_add_async
        └── mirror-failover-097.json # Lift benchmark task for mirror_failover

tests/
└── binary_097_mirror.rs        # New binary invocation test file (T014, T015, T024)
```

**Structure Decision**: Single Rust workspace; new code is additive to existing files. No
new modules required. Binary invocation tests go in a new `tests/binary_097_mirror.rs` file,
following the pattern of other `tests/binary_*.rs` files for `#[ignore]` gate tests.

---

## Complexity Tracking

No constitution violations. No complexity justifications required.

---

## Phase Plan

### Phase 0: Research (COMPLETE)

Research is complete. `research.md` contains:

- Verified `SYS.Mirror.JoinMirrorAsAsyncMember` signature (7 params including 2 by-reference
  output params) against iris-dev-iris 2026.2
- Verified `SYS.Mirror.BecomePrimary()` signature and semantics — returns `%Boolean`, not
  `%Status`
- Confirmed `%SYSTEM.Mirror` (read) vs `SYS.Mirror` (write) class distinction
- ObjectScript code templates for both actions including pre-flight checks
- Gate classifications for both actions
- Alternatives considered and rejected (`SYS.Mirror.AddFailoverMember` for async,
  `SYS.Mirror.Promote` for failover)
- Community IRIS constraint: iris-dev-iris returns `IsMember()=0`; full round-trip tests
  require `IRIS_MIRROR_PRIMARY` env var

**Phase 0 gate**: PASS — no NEEDS CLARIFICATION items remain.

### Phase 1: Design (COMPLETE)

Design artifacts are complete:

- `data-model.md`: `MirrorAddAsyncParams` struct, `MirrorFailoverParams` struct,
  `MirrorAddAsyncResult` and `MirrorFailoverResult` JSON shapes, error code registry,
  Rust struct sketches with `serde` defaults
- `tasks.md`: 35 tasks (T001–T034 + T015a) in TDD order — tests before implementation
  in every phase

**Key design decisions:**

1. **Parameter name**: `instance_name` (the IRIS instance name on the primary failover member),
   NOT `arbiter_host`. The tasks.md T017/T018 contain a naming inconsistency — the spec and
   data-model.md use `instance_name` which is correct per the verified API signature.
2. **Confirmation guard**: `mirror_failover` requires `confirm: true` in params to prevent
   accidental agent invocations (FR-006), in addition to the destructive gate.
3. **Pre-flight checks**: both actions check `%SYSTEM.Mirror.IsMember()` before calling the
   write API — `mirror_add_async` exits early with `ALREADY_MEMBER` if already joined;
   `mirror_failover` exits early with `NOT_MIRROR_MEMBER` if not joined or `ALREADY_PRIMARY`
   if already primary.
4. **Version mismatch detection**: pattern-match `"version"` or `"incompatible"` (case-insensitive)
   in the ObjectScript error string from `$System.Status.GetErrorText(tSC)` — covered by unit
   test T015a.
5. **lift-results.md naming**: the constitution (Principle IX) requires `lift-results.md`;
   tasks.md T034 references `benchmark-results.md`. Both files will be written — `lift-results.md`
   is the primary artifact for the constitution gate.

**Phase 1 gate**: PASS — data model complete, no open design questions.

### Phase 2: Implementation (tasks.md)

Tasks execute in this order:

**Phase 1 (Setup): T001–T005**

- Verify iris-dev-iris running; record baseline test count and coverage.
- Read `iris_mirror_status_impl` (admin_tools.rs:573) and `write_gate.rs:524` mixed table
  to internalize the exact patterns to follow.

**Phase 2 (Gate classification): T006–T010** — GATE

- Write gate classification unit test (T006), confirm it fails (T007).
- Add `mirror_add_async` and `mirror_failover` to the `iris_admin` mixed table in
  `write_gate.rs` (T008):
  ```rust
  ("mirror_add_async", WriteClass::Write),
  ("mirror_failover", WriteClass::Destructive),
  ```
- Confirm T006 passes (T009), run clippy (T010).
- **Gate**: must pass before any dispatch arms exist — this ensures the gate is enforced
  even if the dispatch arm is wired incorrectly.

**Phase 3 (mirror_add_async): T011–T022**

- Read research.md for verified signature (T011).
- Unit tests first: None-iris → IRIS_UNREACHABLE (T012), missing mirror_name → INVALID_PARAMS
  (T013), version-mismatch detection unit test (T015a).
- Binary invocation tests: without IRIS_WRITE_TOOLS_ENABLED → WRITE_TOOLS_DISABLED (T014),
  with gate enabled but no IRIS → IRIS_UNREACHABLE (T015).
- Unit test: mirror_failover without IRIS_WRITE_TOOLS_ENABLED (T016 — also covers write gate
  consistency across both actions).
- Implement `iris_mirror_add_async_impl` in `admin_tools.rs` (T017).
- Add dispatch arm + update tool description + INVALID_ACTION fallthrough in `mod.rs` (T018,
  FR-011).
- Confirm binary tests pass (T019).
- Live IRIS test against iris-dev-iris: assert structured error, not crash (T020–T021).
- Clippy clean (T022).

**Phase 4 (mirror_failover): T023–T029**

- Unit tests first: None-iris → IRIS_UNREACHABLE (T023), without IRIS_DESTRUCTIVE_TOOLS_ENABLED
  → DESTRUCTIVE_TOOLS_DISABLED (T024).
- Confirm T023–T024 fail (T025).
- Implement `iris_mirror_failover_impl` in `admin_tools.rs` (T026).
- Add dispatch arm in `mod.rs` (T027).
- Confirm T023–T024 pass (T028).
- Clippy clean (T029).

**Phase 4b (Missing AC coverage): T_VERSION_MISMATCH, T_SSL_REQUIRED, T_FAILOVER_LIVE**

- These close AC3 (version mismatch), AC5 (SSL required), and US2 AC1 (live failover)
  acceptance scenarios from the spec. Run after Phase 4 complete.

**Phase 5 (Polish): T030–T034** — RELEASE GATE

- Full test suite (T030), integration suite with `--test-threads=1` (T031).
- Coverage gate: `cargo llvm-cov --include-ignored` ≥ 90% (T032).
- Format + clippy clean (T033).
- Tool lift benchmark: write benchmark tasks in `src/benchmark/tasks/`, run A/B lift
  measurement for both new actions, record in `lift-results.md` and `benchmark-results.md`
  (T034). **Lift ≥ +0.20 required before merge.**

### Phase Gates

| Gate | Condition | Blocks |
| --- | --- | --- |
| Gate 0 | research.md complete, no NEEDS VERIFICATION | Phase 1 design |
| Gate 1 | Constitution Check all PASS | Phase 2 implementation |
| Gate 2 | T008 gate entries in write_gate.rs + T009 classification test passes | T017/T026 dispatch arms |
| Gate 3 | T019 binary tests pass (mirror_add_async wired) | T020 live IRIS test |
| Gate 4 | T028 binary tests pass (mirror_failover wired) | Phase 5 polish |
| Gate 5 (RELEASE) | T032 coverage ≥ 90% + T034 lift ≥ +0.20 in lift-results.md | Merge to main |

---

## Key Risk: Naming Bug in tasks.md T017/T018

The implementation task descriptions in T017 and T018 list `arbiter_host` as a parameter
name in the function signature comment. The correct parameter name — per the verified API
signature in research.md and the data-model.md struct — is `instance_name`. When implementing
T017/T018, use `instance_name`, not `arbiter_host`. The tasks.md descriptions are advisory;
the spec and data-model.md are authoritative.

## Key Risk: lift-results.md vs benchmark-results.md

T034 in tasks.md names the output file `benchmark-results.md`. Constitution Principle IX
requires `lift-results.md`. Both files must be written at T034 time. The constitution gate
is `lift-results.md`; `benchmark-results.md` is supplementary.

## Key Risk: INVALID_ACTION fallthrough (FR-011)

The current INVALID_ACTION message in `mod.rs` (~line 7351) does not mention `mirror_add_async`
or `mirror_failover`. This is a known gap (FR-011). T018 and T027 must update both the tool
description string and the INVALID_ACTION error text. Failing to update the INVALID_ACTION
fallthrough means agents get a misleading error when they mistype the action name.
