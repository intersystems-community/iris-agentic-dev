# Tasks: Antipattern Gates (112)

**Input**: `specs/112-antipattern-gates/spec.md`

Every task below is done. The accept blocks are what keep it done — they execute at each gate
boundary, so a regression fails a gate rather than waiting for someone to re-read this file.

---

## Phase 1: Detectors

- [x] T001 Write nine detectors in `scripts/gates/antipatterns.py`, one per shipped bug class,
      each carrying the instance it was written for
- [x] T002 Write a Rust brace scanner (`body_after`, `test_fns`, `strip_comments`) so
      per-function questions do not rely on regex
- [x] T003 Mask comment text before matching (`mask_comments`) so a doc comment quoting a bug
      is not read as the bug
- [x] T004 Derive the tool registry from `pub const CLASSIFICATION` in
      `scripts/gates/check_tool_names.py`, failing loudly if extraction yields under 50 names
- [x] T005 Record known instances in `scripts/gates/antipatterns-baseline.txt` and fail on
      findings absent from it, plus on baseline lines that no longer fire
- [x] T006 Delete `scripts/gates/antipatterns.sh` — it never parsed under bash 3.2, and a
      second implementation of the same rules is the defect `self-referential-gates` detects

### Acceptance

The gate is clean, and each of the three no-baseline checks is at zero. `--all-findings` is
what proves the detectors still fire at all: a scanner that silently matches nothing passes
every gate, which is the failure mode this whole feature is about.

```accept
# verifies: FR-002, FR-004, FR-005
set -euo pipefail
python3 scripts/gates/antipatterns.py
python3 scripts/gates/antipatterns.py error-sentinels self-referential-gates version-consistency
# The detectors must still be finding the known instances. Zero total findings with a
# non-empty baseline means the scanner broke, not that the tree got clean.
total=$(python3 scripts/gates/antipatterns.py --all-findings | grep -c '^FINDING' || true)
if [ "$total" -lt 100 ]; then
    echo "antipatterns: only $total findings with --all-findings; the baseline has ~270." >&2
    echo "A scanner that matches nothing passes every gate. Check the detectors." >&2
    exit 1
fi
```

---

## Phase 2: Loud skips

- [x] T007 `crates/iris-agentic-dev-core/src/testing.rs`: `require_iad_binary` resolves from
      `CARGO_MANIFEST_DIR` and panics unless `IAD_ALLOW_SKIP` is set
- [x] T008 `tests/mcp_handshake.rs`: seven protocol tests route through `require_iad_binary`
      instead of resolving `target/debug/iris-agentic-dev` and returning
- [x] T009 `tests/integration/test_compile_cmd.rs`: three tests likewise

### Acceptance

A missing binary must fail these tests, not skip them. `IAD_ALLOW_SKIP` is the one way to get
the old behaviour, and asking for it explicitly is the point.

```accept
# verifies: FR-001
set -euo pipefail
python3 scripts/gates/antipatterns.py vacuous-tests
grep -q 'IAD_ALLOW_SKIP' crates/iris-agentic-dev-core/src/testing.rs
# No test may resolve a build artifact through a relative path again. The rule lives in the
# binary-path detector, which knows the resolver's own tests are allowed to write one.
python3 scripts/gates/antipatterns.py binary-path
```

---

## Phase 3: Single-source failure detection

- [x] T010 `generator_error_message` in `src/iris/connection.rs` returns the message with
      whichever of the four prefixes matched removed; `is_generator_error` delegates to it
- [x] T011 Convert all twelve hand-rolled prefix checks to the shared helper
- [x] T012 Rewrite the `dict.rs` unit test to exercise all four shapes rather than the one
      the old hand-rolled form happened to handle

### Acceptance

```accept
# verifies: FR-004
set -euo pipefail
python3 scripts/gates/antipatterns.py error-sentinels
grep -q 'pub fn generator_error_message' crates/iris-agentic-dev-core/src/iris/connection.rs
grep -q 'ERROR($DEVICE): ' crates/iris-agentic-dev-core/src/iris/connection.rs
```
