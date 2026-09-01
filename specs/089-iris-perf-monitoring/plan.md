# Implementation Plan: IRIS Mirror Status and Database Free Space

**Branch**: `089-iris-perf-monitoring` | **Date**: 2026-09-01 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/089-iris-perf-monitoring/spec.md`

## Summary

Two deliverables: (1) a new `iris_mirror_status` tool that calls four `%SYSTEM.Mirror`
classmethods in %SYS and returns JSON, and (2) extension of the existing
`iris_database_list` tool to merge `%SYS.DatabaseQuery:FreeSpace` results into each
database entry. Both are read-only, HTTP-first, no helper class required. All APIs
verified against iris-dev-iris (community 2026.2.0L) on 2026-09-01.

## Technical Context

**Language/Version**: Rust 2021 (edition), workspace toolchain (currently Rust 1.92)
**Primary Dependencies**: `rmcp`, `tokio`, `serde_json` — all already in workspace
**Storage**: None — stateless MCP tool calls
**Testing**: `cargo test` (unit), `cargo test -- --include-ignored --test-threads=1` (IRIS integration)
**Target Platform**: Linux/macOS (same as existing iad binary)
**Project Type**: Single Rust workspace (two crates: core + bin)
**Performance Goals**: Same as existing tools — <5s per call, no timeout issues
**Constraints**: Read-only; no write gate needed; %SYS namespace required for both tools
**Scale/Scope**: Instance-level call, no bulk data

## Constitution Check

| Principle                      | Status | Notes                                                                              |
| ------------------------------ | ------ | ---------------------------------------------------------------------------------- |
| I. Zero-Install Binary         | PASS   | No new install step; pure Rust additions                                           |
| II. ObjectScript Sanity        | PASS   | All APIs verified against live iris-dev-iris 2026-09-01                            |
| III. HTTP-First Execution      | PASS   | Both tools use existing HTTP+Atelier path; no Docker required                      |
| IV. Test-First, Fixture-Driven | PASS   | Unit tests first; IRIS integration tests gate each phase                           |
| V. Output Shape Parity         | PASS   | New tools return consistent JSON; extended tool adds fields without breaking shape |
| VI. Environment Guard          | PASS   | Both tools are read-only; no write/destructive gate needed                         |
| VII. Dependency Minimalism     | PASS   | Zero new crates                                                                    |
| VIII. 90% Coverage Gate        | PASS   | Polish phase includes coverage check task                                          |
| IX. Tool Lift Requirement      | N/A    | Internal ops tools; not part of benchmark task set                                 |
| X. ObjectScript Coverage       | N/A    | No ObjectScript classes authored; pure classmethod calls via iris_execute          |

## Project Structure

### Documentation (this feature)

```text
specs/089-iris-perf-monitoring/
├── plan.md              # This file
├── research.md          # Phase 0 output — API verification findings
├── data-model.md        # Phase 1 output
├── contracts/           # Phase 1 output — tool schemas
└── tasks.md             # Phase 2 output (/speckit.tasks)
```

### Source Code (repository root)

```text
crates/iris-agentic-dev-core/src/tools/
├── mod.rs                          # Add iris_mirror_status handler; extend iris_database_list
└── (no new files — both features land in mod.rs)

crates/iris-agentic-dev-core/tests/unit/
└── test_perf_monitoring.rs         # Unit tests: param parsing, JSON shape, error cases

crates/iris-agentic-dev-bin/tests/integration/
└── test_exec_live.rs               # Binary invocation tests (existing file, add cases)

crates/iris-agentic-dev-core/tests/integration/  (or unit/ — whichever hosts live tests)
└── test_mirror_and_freespace.rs    # Live IRIS integration tests (#[ignore])
```

**Structure Decision**: Single Rust workspace, no new files for core logic — both features
are additions to `mod.rs`. Tests split across unit (no IRIS) and integration (#[ignore]).

## Phase Design

### Phase 1: iris_mirror_status (US1)

**Goal**: New tool, returns mirror topology for any IRIS instance.

**Implementation**:

- Register `iris_mirror_status` in the tool list (no parameters needed; namespace always %SYS)
- Handler builds this ObjectScript snippet and calls `iris_execute`:

  ```objectscript
  ZN "%SYS"
  Set tMember=##class(%SYSTEM.Mirror).IsMember()
  Set tName=##class(%SYSTEM.Mirror).MirrorName()
  Set tType=##class(%SYSTEM.Mirror).GetMemberType()
  Set tPrimary=##class(%SYSTEM.Mirror).IsPrimary()
  Write "{""is_member"":",(tMember),",""mirror_name"":",(tName),
        ",""member_type"":",(tType),",""is_primary"":",(tPrimary),"}"
  ```

- Post-process: normalize `member_type="Not Member"` → `null`, `mirror_name=""` → `null`
  when `is_member=false`
- Error path: if execute fails, return `{error: "...", is_member: null}`

**Phase gate**: live IRIS test asserts `{is_member: false}` on iris-dev-iris

### Phase 2: iris_database_list free space (US2)

**Goal**: Extend existing tool, add `size_mb`, `free_space_mb`, `max_size_mb` per DB.

**Column mapping** (verified against iris-dev-iris):

- `SizeInt` → `size_mb` (integer MB)
- `AvailableNum` → `free_space_mb` (float MB)
- `MaxSize` string → `max_size_mb`: parse numeric prefix, `null` if "Unlimited"
- `Free` → `free_pct` (integer 0–100)
- keyed by `DatabaseName`

**Implementation**:

- After existing database list query, run `%SYS.DatabaseQuery:FreeSpace` in %SYS
- Build `HashMap<String, FreeSpaceData>` keyed by `DatabaseName`
- Merge into each existing database entry by matching name
- Graceful degradation: if query throws, add `free_space_note: "unavailable: <err>"` at
  root; individual entries omit free space fields

**Phase gate**: live IRIS test asserts `size_mb` present and numeric on iris-dev-iris

### Phase 3: Polish

- `cargo clippy -- -D warnings`
- `cargo fmt --all`
- `cargo llvm-cov --features testing -- --include-ignored` — assert ≥ 90%
- Update `docs/tools.md` with both tools/extensions
- Update CLAUDE.md Recent Changes section

## Test Strategy (Three Layers — Non-Negotiable)

### Layer 1: Unit Tests (no IRIS)

File: `crates/iris-agentic-dev-core/tests/unit/test_perf_monitoring.rs`

- `iris_mirror_status` param validation (no params required — tool accepts empty call)
- JSON output shape: `{is_member: bool, mirror_name: string|null, member_type: string|null, is_primary: bool}`
- Normalization logic: "Not Member" → null, "" → null
- `iris_database_list` extended shape: entry contains `size_mb`, `free_space_mb`, `max_size_mb`
- MaxSize parsing: "Unlimited" → null, "500MB" → 500, "1024MB" → 1024
- Graceful degradation shape: root `free_space_note` present when query skipped

### Layer 2: Binary Invocation Tests (#[ignore])

File: `crates/iris-agentic-dev-bin/tests/integration/test_exec_live.rs` (existing)

- Spawn `iris-agentic-dev` via `IAD_BINARY`, send `initialize` + `tools/list`
- Assert `iris_mirror_status` appears in tool list
- Assert `iris_database_list` still in tool list (no regression)
- Send `tools/call` for `iris_mirror_status` — assert valid JSON-RPC response shape

### Layer 3: Live IRIS Integration (#[ignore], --test-threads=1)

File: `crates/iris-agentic-dev-core/tests/integration/test_mirror_and_freespace.rs`

- `iris_mirror_status` on iris-dev-iris: assert `is_member=false`, `mirror_name=null`
- `iris_database_list` on iris-dev-iris: assert ≥1 entry has `size_mb` as positive number,
  `free_space_mb` as non-negative float, `max_size_mb` as null or positive number

## Data Model

See [data-model.md](./data-model.md).
