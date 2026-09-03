# Implementation Plan: Terminal-Mode ObjectScript Compatibility

**Branch**: `096-terminal-objectscript-compat` | **Date**: 2026-09-02 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/096-terminal-objectscript-compat/spec.md`

## Summary

`iris_execute` has two execution paths: HTTP primary (`execute_via_generator`, class
method body, `{}` works) and docker exec fallback (`execute`, IRIS terminal,
line-by-line, `{}` causes `<SYNTAX>`). When agents are on NoPWS containers or when HTTP
fails, the docker exec path is the only option — and block syntax fails silently with a
raw `<SYNTAX>` error.

This feature adds a pre-submission guard (`contains_terminal_block_syntax`) to
`write_gate.rs`, called in `iris_execute` immediately before the docker exec fallback.
On detection the tool returns `TERMINAL_SYNTAX_UNSUPPORTED` with an actionable message
and the `.mac` + `iris_compile` escape hatch, without making any IRIS call. The
`iris_execute` tool description is also updated to document both paths and the escape
hatch so agents can plan accordingly.

## Technical Context

**Language/Version**: Rust 2021 edition (stable toolchain, `aarch64-apple-darwin`)
**Primary Dependencies**: `rmcp`, `tokio`, `serde_json`, `anyhow` — all already in
workspace. No new crates.
**Storage**: N/A — no persistent state; the guard is a pure string scanner.
**Testing**: `cargo test` (unit, no IRIS); `cargo test -- --include-ignored
--test-threads=1` (integration, live IRIS at localhost:52780); binary invocation tests
via `IAD_BINARY` env var.
**Target Platform**: macOS arm64 + x86_64, Linux x86_64, Windows x86_64 (no
platform-specific code in this feature).
**Project Type**: Single Rust workspace — `iris-agentic-dev-core` crate (tools + MCP
server), `iris-agentic-dev-bin` crate (CLI entry point).
**Performance Goals**: Guard must complete in under 1ms (pure string scan). No
latency budget impact on the HTTP path.
**Constraints**: Zero false positives on valid terminal-mode code. Guard must not fire
on the HTTP path. No IRIS call when guard fires.
**Scale/Scope**: Single function (~40–60 lines) + single call site in
`iris_execute`. Small, self-contained change.

## Constitution Check

_GATE: Must pass before Phase 0 research. Re-check after Phase 1 design._

| Principle                      | Status | Notes                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| ------------------------------ | ------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| I. Zero-Install Binary         | PASS   | No new install step; pure Rust string scanner, no new crates, binary unchanged in size budget                                                                                                                                                                                                                                                                                                                                                                          |
| II. ObjectScript Sanity        | N/A    | No new ObjectScript APIs. Guard is Rust-side only. The terminal `<SYNTAX>` behavior is a known IRIS constraint, not an API to verify.                                                                                                                                                                                                                                                                                                                                  |
| III. HTTP-First Execution      | PASS   | No new Docker-required tools added to Merged tier. This spec adds a guard on an EXISTING docker exec fallback path. The HTTP path is unchanged and never applies the check.                                                                                                                                                                                                                                                                                            |
| IV. Test-First, Fixture-Driven | PASS   | Unit tests (no IRIS): happy paths, false-positive cases, detection cases. Binary invocation test: description update. Live IRIS integration: compile-and-run escape hatch (P2). Tests written before implementation.                                                                                                                                                                                                                                                   |
| V. Output Shape Parity         | PASS   | New `TERMINAL_SYNTAX_UNSUPPORTED` error uses existing `{success, error_code, error}` shape, consistent with all other `iris_execute` errors.                                                                                                                                                                                                                                                                                                                           |
| VI. Environment Guard          | N/A    | No new write-capable tool. `iris_execute` is already write-gated.                                                                                                                                                                                                                                                                                                                                                                                                      |
| VII. Dependency Minimalism     | PASS   | Zero new Rust crates. State machine for string literal tracking is ~40 lines using only `std`.                                                                                                                                                                                                                                                                                                                                                                         |
| VIII. 90% Coverage Gate        | PASS   | Polish phase includes `cargo llvm-cov --include-ignored` coverage-check task targeting ≥ 90%. Unit tests cover all guard branches; integration test covers docker exec path end-to-end.                                                                                                                                                                                                                                                                                |
| IX. Tool Lift Requirement      | N/A    | This is a safety guard on an existing tool, not a new MCP tool. No new entry in `tools/list`. No lift benchmark required per constitution exception for internal-only changes. **Tool Lift N/A approval**: `iris_execute` is an existing tool with unchanged user-facing description. The change is entirely internal routing logic (HTTP remains primary, docker exec is fallback). No description text changes, so lift benchmarking is not applicable to this spec. |
| X. ObjectScript Coverage       | N/A    | Pure Rust feature. No new ObjectScript shipped.                                                                                                                                                                                                                                                                                                                                                                                                                        |

_No FAIL gates. Cleared to proceed to implementation._

## Project Structure

### Documentation (this feature)

```text
specs/096-terminal-objectscript-compat/
├── plan.md              # This file
├── spec.md              # Feature specification
├── research.md          # Phase 0 output (execution path analysis, detection rules)
├── data-model.md        # Phase 1 output (error code registry, function contract)
└── tasks.md             # Phase 2 output (/speckit.tasks — NOT created by /speckit.plan)
```

### Source Code (repository root)

```text
crates/iris-agentic-dev-core/src/tools/
├── write_gate.rs           # ADD: contains_terminal_block_syntax()
├── mod.rs                  # MODIFY: guard call before docker exec fallback (~line 4307)
│                           # MODIFY: iris_execute tool description (~line 4073)
└── (test files)            # ADD: unit tests for contains_terminal_block_syntax

docs/
└── tools.md                # ADD (P2): compile-and-run escape hatch section
```

**Structure Decision**: Single Rust project. All changes are in `iris-agentic-dev-core`.
No new files added except doc section (P2). Tests are inline (unit) and in the existing
test module structure.

## Complexity Tracking

> No Constitution Check violations. Section left blank per template instruction.
