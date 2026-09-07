# Constitution amendment for 113 (T035) — applied 2026-09-07

`.specify/memory/constitution.md` now carries all four edits below at version 1.5.2. They were
applied by `/speckit.constitution`, which is the command whose job is to write that file; the
PreToolUse hook that blocks Edit and Write on it (`protected_files.extra` in
`.specify/gates/policy.json`) was left in place. This file stays as the record of what changed
and why, since the constitution itself is outside git — `~/.gitignore` ignores `.specify/`, so
the amendment has no diff to review.

**Version**: 1.5.1 → **1.5.2** (PATCH — two rows added to the Bug Class Registry, two names
added to the never-baselined list, one open item recorded. No principle changed.)

---

## Edit 1 — new sync report, prepended inside the existing HTML comment at line 1

Insert directly after `==================`, above the current `Version change: 1.5.0 → 1.5.1`
block, and demote that block to `Prior sync report:` the way the existing ones are.

```text
Version change: 1.5.1 → 1.5.2
Bump rationale: PATCH — two rows added to the Bug Class Registry, no principle
changed. Both rows record the same shipped class from two directions:

  Undeclared parameter    31 of 81 tools were declared `Parameters<AnyParams>`,
                          which reflects to a client as `{"type": "object"}` with
                          no properties. Those 31 read 143 named keys between them,
                          so the prose was the only parameter documentation for 38%
                          of the surface. Detector: the compiler, now that
                          `AnyParams` is deleted, plus the zero-empty-properties
                          census in tests/binary/schema_census.rs.
  Silently discarded param  `stream_inspect` was documented with a `max_chars`
                          parameter that appears nowhere in crates/*/src. A caller
                          asking for 10,000 characters received everything and no
                          error. Detector: `undeclared-params`.

The never-baselined list grew from six classes to eight.

Open item carried forward, not resolved here: Principle VIII states ≥ 90% coverage
and Release Discipline states ≥ 88%. `scripts/coverage.sh` measures 88.38% on this
branch (87.68% before it), which clears 88 and misses 90, so no plan
can satisfy both MUSTs. The enforceable number is 88% — it is what
`scripts/coverage.sh` and CI check. Aligning Principle VIII down to 88% weakens a
non-negotiable constraint, which the Versioning Policy makes a MAJOR bump, so it
cannot ride a PATCH amendment. Owner: Tom. Resolution needs its own
`/speckit.constitution` run at 2.0.0.
```

## Edit 2 — two rows appended to the Bug Class Registry table

Append below `| Gate reads stale objects | … | \`stale-coverage-objects\` |`:

```text
| Undeclared parameter     | 31 of 81 tools advertised `{"type": "object"}` while reading 143 named keys | compiler + census test    |
| Silently discarded param | `stream_inspect` documented `max_chars`, which no handler ever read        | `undeclared-params`       |
```

Realign the column padding to the widest cell after inserting, then run
`markdownlint-cli2 --fix` and `prettier --write` on the file.

## Edit 3 — the never-baselined list

Replace:

```text
- `error-sentinels`, `self-referential-gates`, `version-consistency`, `binary-path`,
  `empty-config-value`, and `stale-coverage-objects` are never baselined and MUST stay at zero.
```

with:

```text
- `error-sentinels`, `self-referential-gates`, `version-consistency`, `binary-path`,
  `empty-config-value`, `stale-coverage-objects`, `undeclared-params`, and `prose-only-enum`
  are never baselined and MUST stay at zero.
```

`prose-only-enum` earns a place on that list rather than in the table because it has no
shipped instance of its own: it is the guard that keeps the seventeen enums this feature
declared from drifting back into prose, which is what would make SC-004's "count reaches
zero" unmeasurable.

## Edit 4 — the footer

```text
**Version**: 1.5.2 | **Ratified**: 2026-05-01 | **Last Amended**: 2026-09-07
```

---

## Why each detector is named the way it is

The two new classes are one bug seen twice. `Undeclared parameter` is the schema saying
nothing; `Silently discarded param` is what the caller experiences when it says nothing. They
get separate rows because they have separate detectors and either can regress alone: a tool
can declare its properties and still accept anything else (no `deny_unknown_fields`), and a
params struct can be closed while a documented parameter it never declares stays in the docs.

The compiler is a legitimate detector entry. With `AnyParams` and `dispatch_any!` deleted,
there is no type a new tool can name to opt out of a params struct — declaring one is the only
way to compile. The census test covers the other half: a struct that exists but declares
nothing.
