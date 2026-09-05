# Postmortem: the tool that always succeeded and never returned anything

`iris_system_performance` shipped in 1.3.x. Every call returned `success: true` and an empty
result. Nobody's tests failed, CI was green, and the bug was found by using the tool.

This is the analysis of why it escaped, what else it turned out to be hiding, and what now
fails a gate.

## What actually broke

`execute_via_generator` captures output by opening a temp file and making it the current
device. `run^SystemPerformance` moves `$IO`. So the routine ran, wrote its output somewhere
else, and the capture file stayed empty.

The HTTP request succeeded. The temp class compiled. The SQL procedure returned. Nothing in
the `Result` chain had anything to complain about, because at the transport layer nothing went
wrong. The tool read an empty string, found no `ERROR:` prefix in it, and reported success.

Two lines fix it:

```objectscript
Set tIO = $IO
Do run^SystemPerformance(...)
Use tIO
```

## Why no test caught it

The interesting part isn't the missing `Use tIO`. It's that a full green suite couldn't have
caught it, for five separate reasons.

**The tests that would have caught it never ran.** Five `nopws_101` tests defaulted `IAD_BINARY`
to `./target/debug/iris-agentic-dev`. A test's working directory is the crate root, not the
workspace root, so that path never resolved. Each test hit its `if !bin.exists() { return; }`
guard and printed `ok`. They did this for all of 1.3.x. There is no difference in the output
between a test that passed and a test that skipped.

**Some test bodies were empty.** Four tests in `gate_macro.rs` had a doc comment and no code.
They counted in every coverage report and in my head as gate coverage.

**Success was asserted, but nothing about the payload.** The live test asserted the call
returned `success: true`. An empty result satisfies that.

**Failure detection was duplicated fourteen times.** Fourteen call sites each hand-rolled
`starts_with("ERROR: ")`. The generator actually produces four prefixes: `ERROR:`,
`ERROR($ZERROR):`, `ERROR($DEVICE):`, and a no-space `ERROR:<SENTINEL>:` form from
tool-generated ObjectScript. Adding a new shape meant fourteen places went blind at once, and
there was no single place to add it.

**The environment decided what the tests proved.** No spawn site used `env_clear()`. Every one
was a deny-list, so ~60 behavior-changing env vars leaked in from whoever ran the suite — which
in CI means the CI job's own configuration chose which code path the test exercised.

## What else was hiding

Once I went looking for the shape rather than the instance, the same pattern turned up in nine
more places.

The literal same bug — a callee that moves `$IO` — in four `Ens.Director` call sites.

Five variants of "a refusal reported as an empty success": `my_access`, the user-roles decode,
the coverage stop path, `skill_forget`, and `global_preview`. `global_preview` was the worst of
them: a failed preview still minted a `confirm_token`, so the confirmation step attested to a
preview that never happened.

And two gates that were asserting against themselves rather than against reality.
`BULK_PHI_TOOLS` listed `view_message_body` — a tool that has never existed in this codebase —
and two tests agreed with the constant, because they compared it to a literal list in the test
rather than to the router's registry. The gate had been dead since it was written.
`CODE_EDIT_BLOCKED` told callers to use `iris_document`, also not a tool.

The source-control probe failed open: when it couldn't determine whether source-control hooks
were installed, it answered "no hooks", which is the permissive direction on the one axis where
being wrong matters.

## The generalization

Every one of these is the same failure at a different layer: **something reported success
because nothing had reported failure.**

- Transport success stood in for operation success.
- A missing resource stood in for a passing test.
- An empty body stood in for a satisfied assertion.
- An unrunnable probe stood in for a permissive answer.
- A constant compared to itself stood in for a verified contract.

The common cure is that absence must be an error, not a default. `.unwrap_or_default()` on an
IRIS call is the smallest expression of the whole bug class: it turns "IRIS refused" into
"IRIS returned nothing" and then into `success: true`.

One more, which I want on the record because it nearly repeated the whole story: the first
version of the detector suite was a bash script. It never parsed under macOS bash 3.2 — a
`python3 - <<'PY'` heredoc nested inside `$( )` mis-parses when the body contains single quotes
— so every check inside it silently reported success. A gate that cannot run is not a gate
that passed.

## What now fails a gate

Nine detectors in `scripts/gates/antipatterns.py`, one per class above, running at all three
boundaries: the agent Stop hook, git pre-commit, and CI. Details in
`specs/112-antipattern-gates/spec.md`.

The first run found 1444 instances. A gate that fails 1444 times is a gate people learn to
bypass, so known instances live in `scripts/gates/antipatterns-baseline.txt` and the gate
enforces _no new instances_. It also fails on a baseline line that no longer fires, so the list
shrinks instead of rotting. Three checks are never baselined and stay at zero:
`error-sentinels`, `self-referential-gates`, `version-consistency`.

Constitution v1.5.0 adds the three principles this cost me: XI No Vacuous Tests, XII Hermetic
Test Environment, XIII Single-Source Failure Detection — plus the Bug Class Registry, whose
rule is that a fix without a detector is incomplete. It closes the instance and leaves the
class open.

## The one-line version

If a tool can return an empty result and call it success, it will, and the test suite will
agree with it.
