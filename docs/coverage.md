# Coverage

## How it works

Coverage is measured per-file, not as a project total. Each source file declares
a floor in `coverage-floors.toml`. CI fails if any file drops below its floor, or
if a new file appears in the coverage output without a registered floor.

The checker runs on every PR without a live IRIS instance:

```bash
./scripts/check-coverage-floors.sh
```

## Why some floors are low

The MCP handlers in the `tools/` dispatch layer have low floors (`comparison_tools.rs`
11, `admin_tools.rs` 12, `observability.rs` 17, `mod.rs` 35). That is not
a quality gap — it is a measurement gap. Most of those code paths only run when
the MCP server is exercised over the wire: the `test_e2e` integration tests spawn
an `iris-agentic-dev` subprocess, call it via MCP protocol, and verify output. The
subprocess binary is not instrumented in the unit-test run, so its execution never
feeds back into `llvm-cov`.

Files with pure logic (policy gates, `execute_session.rs`, `storage_guard.rs`) have
floors from 87% to 98% and must stay there.

## Adding a new source file

1. Write it. Write unit tests for the pure-logic parts.
2. Run `scripts/check-coverage-floors.sh` locally. It will tell you the measured
   coverage and suggest a floor.
3. Add the entry to `coverage-floors.toml` at `measured - 2`.
4. CI passes.

## Raising a floor

After adding tests that increase a file's coverage:

1. Run `scripts/check-coverage-floors.sh` to see the new number.
2. Raise the floor in `coverage-floors.toml` to `new_measured - 2`.
3. Commit both together.

## The subprocess gap

The full picture requires launching an instrumented binary and merging its profraw
output into the report. `scripts/coverage.sh` has this wired up for local use:

```bash
IRIS_HOST=localhost IRIS_WEB_PORT=52780 bash scripts/coverage.sh
```

That script also enforces the `overall` floor in `coverage-floors.toml` (88 today), which the
CI check does not. Only `iris-agentic-dev-core` is measured; the bin crate has no floors.

Wiring this into CI (so the per-file floors can eventually reflect true coverage) is
tracked as a future project.
