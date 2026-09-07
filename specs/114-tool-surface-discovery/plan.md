# Implementation Plan: CLI tool discovery and a detector that sees every value set

**Branch**: `114-tool-surface-cost` | **Date**: 2026-09-07 | **Spec**: [spec.md](./spec.md)

## Summary

Two changes, both small, both aimed at the same criticism: the tool surface costs context before it
earns anything.

US1 adds `iris-agentic-dev tool --list` and `iris-agentic-dev tool <name> --schema`, both served off
the same `tool_router` the MCP transport uses, neither needing an IRIS connection. That gives a
bash-only harness progressive disclosure at zero baseline cost — 1.4 KB of names against 104 KB of
MCP listing — and makes a third eval arm possible (`none | iad_cli | iad_mcp`).

US2 repairs the `prose-only-enum` detector, which reports zero because it reads two prose shapes
and eleven dispatcher parameters are written in a third, then declares those eleven value sets. The
declarations are schema-only, so no runtime answer moves.

## Technical Context

**Language/Version**: Rust 2021
**Primary Dependencies**: `rmcp` 3.1.3, `schemars` 1, `clap` 4, `serde_json` — no new crate
**Storage**: N/A
**Testing**: `cargo test --features testing`; `python3 scripts/gates/test_antipatterns.py`
**Target Platform**: macOS, Linux, Windows (same binary as today)
**Project Type**: single (two crates)
**Performance Goals**: `tool --list` under 100 ms cold, no network
**Constraints**: `--list` output under 8 KB for the Merged toolset (FR-005); no IRIS connection on
either discovery path (FR-002)
**Scale/Scope**: 82 registered tools, 342 declared properties, 11 undeclared value sets

## Constitution Check

| Principle                             | Status | Notes                                                                                                                                                                                                          |
| ------------------------------------- | ------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| I. Zero-Install Binary                | PASS   | Two flags on an existing subcommand                                                                                                                                                                            |
| II. ObjectScript Sanity               | N/A    | No ObjectScript                                                                                                                                                                                                |
| III. HTTP-First Execution             | PASS   | Discovery makes no connection at all                                                                                                                                                                           |
| IV. Test-First, Fixture-Driven        | PASS   | Detector canaries are fixtures; CLI tests spawn the binary with no container                                                                                                                                   |
| V. Output Shape Parity                | PASS   | `--schema` emits the schema `tools/list` serves, asserted byte-for-byte (SC-003)                                                                                                                               |
| VI. Environment Guard                 | N/A    | Read-only paths; no tool classification changes                                                                                                                                                                |
| VII. Dependency Minimalism            | PASS   | No new crate                                                                                                                                                                                                   |
| VIII. 90% Coverage Gate               | PASS   | Against the enforceable 88% floor that `scripts/coverage.sh` and CI check. The VIII-vs-Release-Discipline conflict (90 vs 88, measured 88.38%) is the open item in constitution 1.5.2 and is not resolved here |
| IX. Tool Lift Requirement             | N/A    | No new MCP tool. The measurable claim is SC-001, a payload number, not a lift score                                                                                                                            |
| X. ObjectScript Coverage              | N/A    | Pure Rust                                                                                                                                                                                                      |
| XI. No Vacuous Tests                  | PASS   | Every detector canary must be shown failing before the fix lands (US2 is itself a vacuous-gate repair)                                                                                                         |
| XII. Hermetic Test Environment        | PASS   | CLI tests pin env and assert the no-connection path                                                                                                                                                            |
| XIII. Single-Source Failure Detection | PASS   | Catalogue derived from `tool_router`, not a second hand-maintained list                                                                                                                                        |

## Project Structure

### Documentation

```text
specs/114-tool-surface-discovery/
├── spec.md
├── plan.md              # this file
├── research.md          # measurements taken before designing
└── tasks.md
```

### Source

```text
crates/iris-agentic-dev-bin/src/cmd/tool.rs        # --list, --schema, --json; name now optional
crates/iris-agentic-dev-core/src/tools/mod.rs      # tool_catalogue() accessor; 11 enum declarations
crates/iris-agentic-dev-core/src/tools/params/     # enum attrs for converted-tool params
scripts/gates/antipatterns.py                      # prose-only-enum: two new prose shapes + match-arm source
scripts/gates/test_antipatterns.py                 # one canary per new shape
crates/iris-agentic-dev-core/tests/binary/cli_discovery.rs   # spawn tests, no IRIS
crates/iris-agentic-dev-core/tests/unit/test_enum_contract.rs # extend: declared set == match arms
docs/tools.md                                      # document the two flags
```

## Approach

### US1 — discovery off the router

`IrisTools` already exposes `registered_tool_names()`, `tool_description()`, and
`tool_input_schema()`, all reading `tool_router.list_all()` — the same source `list_tools` serves.
Add one accessor that returns the whole catalogue in one pass so `--list` does not call
`list_all()` 82 times, and build `IrisTools` with `None` for the connection on both discovery
paths.

Three decisions worth recording:

1. **`name` becomes `Option<String>`** on `ToolCommand`. `--list` with a name is an error, not a
   silent preference, because an agent that gets a listing when it asked for one tool will read the
   wrong schema.
2. **The one-line summary is the description's first sentence, truncated at 100 characters on a word
   boundary.** Descriptions are multi-sentence and start with the tool's purpose, so the first
   sentence is the summary; truncation keeps FR-005 reachable without an editing pass.
3. **`--list` filters through `TOOL_NAMES`**, the CLI's dispatch list, and a test fails if the two
   sets differ. A listing that advertises a name `call_for_test` cannot dispatch is worse than no
   listing — that exact drift shipped once already (22 names, no dispatch arm).

### US2 — a detector that reads the handler, not the prose

The existing rule matches `mode: a | b | c` and comma lists with a "one of" lead-in. Add:

- **repeated assignment**: three or more `param=value` mentions of the same parameter
  (`what=documents lists…, what=modified lists…` — `iris_info`);
- **bare comma list**: `param: a, b, c` with no lead-in, parentheticals skipped (`iris_doc`);
- **match arms**: the handler branches on a declared parameter with three or more string literal
  arms. This one is prose-independent and is what catches `iris_admin`, whose value list sits under
  the heading "Read actions (always available):" and never names the parameter.

The match-arm source is the authoritative one, and it is also what FR-011 compares the declared
`enum` against — the 113 spec asked for exactly this ("asserted against the values the handler
actually branches on, not just against the prose") and it was only ever applied to the 31 converted
tools.

Existing exemption stays: `not an enum` in the field's doc comment, a sentence written in review.
`iris_system_performance.profile` is the case that needs it.

## Risks

- **False positives from the match-arm shape.** A handler may match strings for something other
  than a closed value set. Mitigated by the doc-comment exemption and by requiring three or more
  arms; each exemption added must say why.
- **Narrowing a set by declaring it.** If a handler accepts more than the prose lists, declaring
  the prose set advertises less than the truth. FR-011's test compares against the arms, so this
  fails rather than ships.
- **`iris_admin.type` may not be a closed set.** It is one of the eleven; resolve by reading the
  handler, and exempt with a reason if the values come from IRIS rather than from a match.
