# Backlog from the empty-success audit

`docs/postmortem-empty-success.md` covers the class and what got fixed. This is what the sweep
turned up that is **not** fixed, so it does not get lost between releases. Each item names the
detector that would catch it, per the Bug Class Registry rule in the constitution.

## Security gates: false permits

Fixed in 1.3.2: `iris_execute_method` outside gate [0], the `##class(...)` / `$classmethod(...)`
call forms bypassing the dotted token list, `check_sql_code_edit` matching raw text,
`iris_query(mode="read", force=true)` skipping both checks, and `policy == None` skipping
gates [1]–[4].

Also fixed in 1.3.2, and worse than this file first recorded it: `iris_admin` never called
`dispatch_gate`, so gates [1]–[4] were absent for every admin action, not just the two PHI ones.
`action="journal_search"` and `action="view_processes"` additionally read `dataPolicy` from the
caller's params, so an agent could hand itself `dataPolicy="allow"` and get a bulk journal dump.
`iris_admin` calls the gate now, gate [2] matches on `action` as well as `tool_name`, and both
actions take the policy from `[policy.<server>]`.

Still open:

- **P6–P13** from the audit — lower-severity permits, each needs its own confirmation against a
  live container before it is worth a code change.

## Security gates: false blocks

These refuse work that should be allowed. Less dangerous than a false permit, but a gate that
blocks legitimate use is a gate people route around.

- **B2 / B3 / B5**: unbounded patterns `^Ens.Rule*`, `^OE*`, `^ORDER*` in the system blocklist.
  `^ORDERS` in an application namespace has nothing to do with the system. `^SYS*` was the same
  shape and is fixed in 1.3.2 (`^SYS` plus `^SYS.*`); the others need the same enumeration against
  a live container before narrowing them.
- **B6, B8**: see the audit notes.

## Tests that cannot fail

- **84 `call_for_test_*_no_iris` tests** assert nothing beyond "the call returned". They are the
  `empty-tests` and `vacuous-tests` classes at scale.
- **Zero-iteration loops** at `crates/iris-agentic-dev-core/tests/unit/symbols_local_tests.rs:181`
  and `:218` — a loop over a collection that may be empty is not a test.
- **11 tests gated on `IRIS_ADMIN_TOOLS`**, which the suite never sets. They have never run.

## The baseline is anchored on line numbers

`scripts/gates/antipatterns-baseline.txt` keys each known finding as `check\tpath:line`. Any
edit above a baselined line makes that entry stale _and_ produces a "new" finding at the shifted
line, so an unrelated change can fail the gate with dozens of findings that are the same known
instances. During the 1.3.2 fix pass this produced 31 new findings and 27 stale lines, none of
them real.

That is the "gate that cries wolf" failure the suite's own docstring warns about. The fix is to
anchor on content rather than position: key on `check + path + fingerprint of the matched line`,
with an occurrence index to keep repeated identical lines distinguishable. It requires
regenerating the whole baseline, so it wants its own change, not a release-eve edit.

## The coverage floors are calibrated against test code

`scripts/coverage.sh` and `scripts/check-coverage-floors.sh` measure every line under
`crates/iris-agentic-dev-core/src`, and that includes the bodies of inline `#[cfg(test)]
mod tests`. A test function is code that runs, so llvm-cov records it as covered — the floor
is then part production coverage, part a count of how many inline tests the file has.

Measured on the 1.3.2 tree: 30,678 lines under `src`, of which roughly 9,846 sit inside test
modules. Overall reads 87.76%; excluding test modules it is **83.89%**. The same arithmetic
runs the other way on the uncovered side — 399 of the 3,755 uncovered lines are assert messages
inside those modules, which only execute when a test fails.

So the number moves for two reasons that have nothing to do with production coverage: adding an
inline test raises the file's measured coverage by the body it contributes and lowers it by the
assert messages it contributes. Moving tests to `tests/unit/` (which 1.3.2 did for
`policy/patterns.rs` and `policy/code_edit_gate.rs`) removes both effects at once, which is why
those two files jumped.

The fix is to exclude `mod tests` regions from the measurement, via
`llvm-cov --ignore-filename-regex` on dedicated test files plus a `#[cfg(test)]`-aware region
filter. Every per-file floor and the `overall` floor are calibrated against the inflated number,
so this cannot land without regenerating all of `coverage-floors.toml` in the same change. Doing
it on release eve would mean shipping floors nobody had read.
