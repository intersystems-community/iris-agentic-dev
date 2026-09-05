# Feature Specification: Antipattern Gates

**Feature Branch**: `096-terminal-objectscript-compat`
**Created**: 2026-09-04
**Status**: Complete

## Overview

Nine detectors, one per bug class that has already shipped in this repository, wired to run at
every gate boundary. The detectors live in `scripts/gates/antipatterns.py`; the shrinking
baseline lives in `scripts/gates/antipatterns-baseline.txt`.

The premise: a bug fixed after a release teaches nothing unless the next instance of the same
class fails a gate. Fixing fourteen hand-rolled `ERROR:` prefix checks was worthwhile; making
the fifteenth impossible to add is what stops the class.

## The bug classes

Each detector names the shipped instance it was written for.

| Check                    | Shipped instance                                                                                 |
| ------------------------ | ------------------------------------------------------------------------------------------------ |
| `vacuous-tests`          | Five `nopws_101` tests defaulted `IAD_BINARY` to a relative path, skipped for all of 1.3.x       |
| `empty-tests`            | Four `gate_macro.rs` tests had a doc comment and no code, and counted as gate coverage           |
| `env-pinning`            | Spawn tests inherit ~60 behavior-changing env vars; the CI job's own settings change meaning     |
| `mcp-subcommand`         | A bare spawn prints usage and exits 2, giving a JSON-RPC reader empty stdout                     |
| `error-sentinels`        | Fourteen hand-rolled `starts_with("ERROR: ")` guards, blind to `ERROR($ZERROR)`/`ERROR($DEVICE)` |
| `device-capture`         | `run^SystemPerformance` moves `$IO`, so the generator captured nothing and reported success      |
| `self-referential-gates` | `BULK_PHI_TOOLS` named `view_message_body`, a tool that never existed; two tests agreed with it  |
| `tool-name-refs`         | `CODE_EDIT_BLOCKED` told callers to use `iris_document`, which has never been a tool             |
| `version-consistency`    | A version-bearing file added without a cross-file assertion drifts silently                      |

## Why a baseline

The first run found 1444 instances. A gate that fails 1444 times is a gate people learn to
bypass, so it enforces _no new instances_ rather than _zero instances_. The gate also fails on
a baseline line that no longer fires, which is what makes the list shrink instead of rot.
Adding a line is a tracked edit that shows up in review.

Three checks carry no baseline and must stay at zero: `error-sentinels`,
`self-referential-gates`, `version-consistency`. Each is cheap to keep clean and each fails
open when violated — a missed failure shape, a gate list that matches nothing, a version that
disagrees.

## Functional Requirements

- **FR-001** Every detector reports `check`, `path:line`, and a message that names the shipped
  bug and the correct alternative.
- **FR-002** The gate exits non-zero on a finding absent from the baseline.
- **FR-003** The gate exits non-zero on a baseline entry that no longer fires.
- **FR-004** `error-sentinels`, `self-referential-gates`, and `version-consistency` are never
  baselined.
- **FR-005** Detectors are read-only: no writes to the worktree, so they can run inside an
  accept block.
- **FR-006** Detectors run on macOS's bash 3.2 and stock `python3` with no third-party
  packages.
- **FR-007** Comment text is masked before pattern matching, so a doc comment quoting a bug as
  an example is not itself a finding.

## Success Criteria

- `python3 scripts/gates/antipatterns.py` exits 0 on a clean tree and 2 on a new instance of
  any of the nine classes.
- The three no-baseline checks report zero findings.
- The gate runs at all three boundaries (agent Stop hook, git pre-commit, CI) through the
  accept block in `tasks.md`.

## Out of Scope

- Fixing the 270 baselined instances. They are known, recorded, and shrink over time.
- Porting the detectors to Rust. Two implementations of one rule reproduce the
  `self-referential-gates` failure this feature exists to catch; where cargo can express a
  rule, the rule belongs in a cargo test instead of here.
