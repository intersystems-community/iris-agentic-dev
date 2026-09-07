# Research — measured before designing

Every number here came from spawning the binary, sending `initialize` + `tools/list` over stdio,
and measuring the response. Compact JSON, no whitespace. Token estimates are bytes ÷ 3.6, which is
close enough for a budget and not close enough to quote as a token count.

## Payload, 1.3.2 against this tree

|                     | 1.3.2 (installed) | after 113 | Δ          |
| ------------------- | ----------------- | --------- | ---------- |
| whole `tools/list`  | 77,078 B          | 104,282 B | **+35.3%** |
| `inputSchema` bytes | 35,195 B          | 62,399 B  | **+77.3%** |
| `description` bytes | 35,062 B          | 35,062 B  | **0.0%**   |
| names alone         | 1,387 B           | 1,387 B   | —          |
| tools               | 81                | 81        | —          |

The five largest entries after 113, as total / description / schema:

| Tool                 | Total | Desc  | Schema |
| -------------------- | ----- | ----- | ------ |
| `iris_admin`         | 6,878 | 1,812 | 4,974  |
| `iris_doc`           | 6,216 | 1,843 | 4,302  |
| `iris_interop_query` | 5,624 | 903   | 4,624  |
| `iris_execute`       | 3,624 | 2,121 | 1,418  |
| `iris_query`         | 2,524 | 705   | 1,758  |

`iris_admin` went from 1,978 B to 6,878 B: its schema was 74 B (an open object) and is now 4,974 B
across 26 declared properties and a 25-value enum.

## The descriptions did not move

Zero bytes. 113 added the structural documentation and left the prose that was carrying the same
facts. That prose is now duplicated — but only semantically, which is why the diet is not a script:
verbatim overlap between descriptions and their own schemas is 2,220 B of 35,062 B (6%). The rest is
restatement in sentences. `iris_admin` spends most of 1,812 B re-listing action names and per-action
parameters that are now a 25-value enum and 26 properties with their own descriptions.

## Mechanical savings, no information lost

| Change                                                  | Payload   | Cumulative |
| ------------------------------------------------------- | --------- | ---------- |
| baseline                                                | 104,364 B | —          |
| drop `$schema` (81 × 57 B)                              | 99,747 B  | −4.4%      |
| flatten 258 `anyOf:[{T},{"null"}]` to `type:[T,"null"]` | 95,111 B  | −8.9%      |

Still +23% over 1.3.2, and 23,189 B of what remains is per-property descriptions — the content that
made SC-007 pass. Deferred to its own change: it alters what every client receives.

## Toolset tiers are not scoped

| `IRIS_TOOLSET` | Tools | Payload   |
| -------------- | ----- | --------- |
| `baseline`     | 84    | 93,065 B  |
| `nostub`       | 80    | 91,985 B  |
| `merged`       | 81    | 104,364 B |

Three near-identical tiers. None is a task-scoped subset, so the profile mechanism exists and buys
nothing yet.

## The CLI already dispatches; it cannot discover

`iris-agentic-dev tool iris_info -a '{"what":"metadata"}' --envelope` against `iris-dev-iris`:
40–60 ms wall per call, three consecutive runs, process spawn included. `elapsed_ms` inside the
envelope reports 5–6 ms, so the rest is process start and connection setup. Latency is not an
argument against a CLI arm.

`TOOL_NAMES` in `cmd/tool.rs` holds 82 names and is printed only on the error path, when a caller
already guessed wrong. There is no `--list` and no `--schema`.

## Enum declaration is schema-only

`iris_interop_query` advertises `what` as `enum: [logs, queues, messages]`. Sending
`{"what":"bogus"}` returns the handler's own answer:

```json
{
  "error": "iris_interop_query: what must be logs, queues, or messages",
  "error_code": "INVALID_ACTION"
}
```

Not a deserialization failure. `#[schemars(extend("enum" = [...]))]` on a `String` field changes the
advertised schema and nothing else, which is what makes FR-010 achievable rather than aspirational.

## The detector's blind spot

`prose-only-enum` reads two shapes: `PIPE_VALUES` (`mode: a | b | c`) and `ONE_OF_VALUES` (a comma
list introduced by "one of" or "valid values"). Eleven dispatcher parameters are written in neither:

| Tool                  | Param    | Prose shape                                                               |
| --------------------- | -------- | ------------------------------------------------------------------------- |
| `iris_info`           | `what`   | repeated `what=documents lists all docs, what=modified lists…`            |
| `iris_doc`            | `mode`   | `mode: get (fetch source), put (write, auto SCM checkout), delete, head…` |
| `iris_admin`          | `type`   | value list under a heading, parameter never named                         |
| `iris_debug`          | `action` | —                                                                         |
| `iris_global`         | `action` | —                                                                         |
| `iris_macro`          | `action` | —                                                                         |
| `iris_production`     | `action` | —                                                                         |
| `iris_source_control` | `action` | —                                                                         |
| `kb`                  | `action` | —                                                                         |
| `skill`               | `action` | —                                                                         |
| `skill_community`     | `action` | —                                                                         |

All eleven are in the 50 tools 113 never converted, so the rule has only ever been exercised against
the 31 it was written alongside. Eleven other dispatcher-style parameters do advertise their sets, so
the split is exactly the converted/unconverted line.

The constitution says this detector MUST stay at zero. It is at zero because it does not fire.
That is the `self-referential-gates` class, already in the registry — a gate whose reading of the
tree is narrower than the tree.

## What the repaired detector reports

Four sources added to `prose-only-enum` — repeated `param=value` assignments, a bare colon-comma
list with parenthetical glosses stripped, the handler's own `match` arms, and a two-value set
written "a or b" in the field's doc comment. Eleven findings on this tree:

| Tool                  | Param    | Values the prose or the arms name                                              |
| --------------------- | -------- | ------------------------------------------------------------------------------ |
| `iris_doc`            | `mode`   | get, put, delete, head, fragment, compiled, list, insert, delete_lines         |
| `iris_info`           | `what`   | documents, modified, namespace, metadata, jobs, csp_apps, csp_debug, sa_schema |
| `iris_macro`          | `action` | list, signature, location, definition, expand                                  |
| `iris_debug`          | `action` | map_int, error_logs, capture, source_map                                       |
| `skill`               | `action` | list, describe, search, forget, propose                                        |
| `skill_community`     | `action` | list, install                                                                  |
| `kb`                  | `action` | index, recall                                                                  |
| `agent_info`          | `what`   | stats, history                                                                 |
| `iris_source_control` | `action` | status, menu, checkout, execute                                                |
| `iris_global`         | `action` | get, set, kill, list                                                           |
| `iris_production`     | `action` | status, start, stop, update, check, recover                                    |

Two of the spec's eleven are not on this list, and one that is was not in the spec:

- `iris_admin.action` already declares its 25-value enum. 113 converted it, and its doc comment says
  the enum is the dispatch table. It was never a finding; it only looked like one.
- `iris_admin.type` is a finding the detector cannot state, for a reason worth its own entry below.
- `agent_info.what` is new. It is one of the nine tools the Merged tier prunes, so a census that
  reads only the default listing does not see it either.

## Two ways a parameter goes invisible rather than unmatched

Both were found while repairing the detector, and both are the same bug shape as the gate itself: the
reader is narrower than the tree, so the answer is "nothing here" rather than "cannot parse this".

**A description with backslash line continuations.** `TOOL_DESC` had no `re.S`, and `iris_admin`
writes its description as one string split over 24 lines with `\` continuations. The regex never
matched, so `tool_blocks` yielded no entry and the whole tool was skipped — 92 of 93 `#[tool(...)]`
attributes parsed, and the missing one was the worst offender on the tree. Fixed with `re.S` plus a
continuation-collapsing pass, and pinned by a canary.

**A field whose wire name is a Rust keyword.** `iris_admin` writes `pub r#type: Option<String>`.
`FIELD_DECL` anchored the name at `[a-z_]`, so `r#type` matched nothing and `type` was not in the
struct's field index at all. Every check that asks "does this parameter declare an enum" answered by
skipping it. Fixed by making `r#` optional and stripping it, so the index is keyed by the wire name.

`iris_admin.type` still produces no finding after the fix, and that is the honest answer: its two
values live in a parenthetical inside the field's doc comment — "applications of this type (REST,
CSP)" — and a
detector that reads parentheticals as value sets would fire on most of the tree. It is declared in
T023 anyway, because the set is closed — the spec named it, not the gate.

## What the twelve answer today for a value outside the set

Measured against `iris-dev-iris` on 2026-09-07, before any enum was declared, and now pinned in
`tests/integration/enum_rejection.rs`. Every value sent was `zzbogus`.

| Tool.param                   | `error_code`     | Carried in | Names the valid values |
| ---------------------------- | ---------------- | ---------- | ---------------------- |
| `iris_doc.mode`              | `INVALID_PARAMS` | `error`    | yes                    |
| `iris_info.what`             | `INVALID_PARAM`  | `error`    | yes                    |
| `iris_macro.action`          | `INVALID_PARAM`  | `error`    | yes                    |
| `iris_debug.action`          | `INVALID_PARAM`  | `error`    | yes                    |
| `skill.action`               | `INVALID_PARAM`  | `error`    | yes                    |
| `skill_community.action`     | `INVALID_PARAM`  | `error`    | yes                    |
| `kb.action`                  | `INVALID_PARAM`  | `error`    | yes                    |
| `agent_info.what`            | `INVALID_PARAM`  | `error`    | yes                    |
| `iris_source_control.action` | `INVALID_PARAM`  | `error`    | yes                    |
| `iris_global.action`         | `INVALID_ACTION` | `message`  | yes                    |
| `iris_production.action`     | `INVALID_ACTION` | `error`    | yes                    |
| `iris_admin.type`            | none             | —          | not an error at all    |

Every one names its values, which is the part that matters for a caller who ignored the schema. But
three error codes and two carrier fields across eleven dispatchers is not a contract a client can
code against: `iris_global` alone answers in `message` where the other ten answer in `error`, so a
client that reads `error` gets `null` from exactly one tool. That is a finding, not a task for this
feature — repairing it in the same change would leave this test unable to tell a deliberate fix from
a regression. It is recorded here for its own spec.

`iris_admin.type` is the twelfth and behaves differently on purpose: it filters, so an unmatched
value returns `success: true` with an empty list. Declaring its enum must not promote that into an
error, and a test says so.

## The schema a test reads is not the schema a client receives

`list_tools` runs `normalize_schema_openapi3` on the way out, which rewrites
`"type": ["string", "null"]` into `anyOf` **and moves the type-specific siblings into the non-null
branch** — `format`, `minimum`, `pattern`, `items`, and `enum` among them. So for an optional
parameter the advertised enum is at `properties.<name>.anyOf[0].enum`, not `properties.<name>.enum`.

Two consequences, both now written down in code rather than rediscovered:

- `tool_input_schema()` stays on the raw router path, because the enum-reading tests want the
  pre-rewrite form. `tool_catalogue()` and `tools/list` are normalized.
- The catalogue test bridges them by applying `normalize_schema_openapi3` itself (which is why the
  function is now `pub`). `capability_matrix` is the tool that fails first if either path drops it.
