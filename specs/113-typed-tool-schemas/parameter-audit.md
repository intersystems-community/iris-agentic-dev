# Parameter audit — 113-typed-tool-schemas

Running record of the defects the conversion turned up. Declaring a parameter forces you to read
what the handler does with it, and that reading found bugs that the open `{"type": "object"}` schema
had been hiding. T027/T028 complete this file with the four-way audit (docs ↔ description ↔ schema ↔
handler); the findings below were written down as they were hit, so they are not lost if a batch is
revisited.

Each finding says whether it is in scope for this feature. This feature changes what tools
**advertise**; it does not change what they **do**. A runtime defect found here gets recorded and
reported, not fixed in the same commit.

## F1 — `journal_search`: `start` and `end` never filter (out of scope)

`journal_search_impl` builds the scan with

```objectscript
If ts<"2026-09-07 00:30:16" Continue
If ts>"2026-09-07 00:00:00" Continue
```

`<` and `>` are **numeric** operators in ObjectScript. Both sides collapse to their leading number —
the year — so `"2026-09-07 00:00:00" < "2026-09-07 00:30:16"` evaluates to 0. Within one calendar
year the two filters do nothing; the tool returns the oldest records in the journal no matter what
window was asked for. Proven live on `iris-dev-iris`: `start` an hour after every returned record
changes nothing, and `Write ("2026-09-07 00:00:00"<"2026-09-07 00:30:16")` writes `0`.

This is the `max_chars` pattern with a different name: a documented parameter accepted and silently
discarded. The string comparison operator is `]]`, or the timestamps have to be converted to
`$HOROLOG` before comparing.

Pinned by `objectscript_numeric_comparison_is_why_time_filters_do_nothing` in
`tests/integration/params_batch4.rs`, which fails if IRIS ever compares these as strings.

## F2 — `journal_search`: any skipped record spins an IRIS job for 90 seconds (out of scope)

The scan advances the cursor at the bottom of the loop:

```objectscript
While (rec'="")&&(cnt<100) {
  Set ts=rec.TimeStamp
  If ts<"…" Continue
  …
  Set rec=rec.Next
}
```

`Continue` jumps back to the `While` condition without executing `Set rec=rec.Next`, so the first
record that fails a filter is re-tested forever. The call burns one IRIS process until the HTTP
client gives up — measured at 90 seconds on `iris-dev-iris` — and comes back as `IRIS_UNREACHABLE`,
which points the caller at the network rather than at the loop.

Reachable today with any `global_pattern` that misses (`{"global_pattern": "^NOSUCH"}`) or any
`start`/`end` in a different year than the journal's records. The fix is to advance the cursor before
`Continue`, or to restructure the filter as a positive `If … { Write … }`.

Consequence for this feature: `start`, `end` and `global_pattern` have no discriminating live test in
`tests/integration/params_batch4.rs`, because every such test would have to send a value that skips a
record and then wait out the 90-second timeout. They are covered by the source contract
(`batch4_handlers_read_their_contract`) and by F1's language test.

## F3 — `iris_system_performance.profile` is not a closed set (in scope, resolved)

The docs table and the handler's error message both list six profiles, which reads like an enum.
`sysperf_profile_or_default` accepts **any** name of letters, digits and underscore, because an
instance can carry site-defined profiles; the six names are only what IRIS ships. Declaring an enum
would reject calls that work today.

Resolved by declaring `profile` a plain string and asserting the absence of an enum in
`system_performance_profile_is_not_declared_as_an_enum` — otherwise US2 would have added one from the
prose.

## F4 — `read_keys` could not see a camelCase parameter (in scope, resolved)

`iris_agentic_dev_core::testing::read_keys` extracts a handler's read set from the `p.get("…")`
literals in its body, and every schema-versus-handler comparison in this feature is built on it. Its
regex accepted only `[a-z_][a-z0-9_]*`, so `p.get("acknowledgePhi")` and `p.get("dataPolicy")` — two
real parameters of `iris_message_body` — were invisible to it.

Nothing failed. That is the problem: a helper that under-reports the read set makes "the schema
declares what the handler reads" pass by omission, which is the same failure mode as the missing
schema. Fixed in batch 5 by broadening the name class to `[A-Za-z_][A-Za-z0-9_]*`, with the reason
in a comment at the call site so it is not narrowed again.

## F5 — `iris_admin.roles` accepted an array and discarded it (in scope, resolved)

`create_user` and `update_user` read `p.get("roles").and_then(|v| v.as_str())`, so `roles` is a
comma-separated string. Three payloads in `tests/integration/test_handlers_live.rs` sent
`"roles": []`, and the tests passed — the array was discarded and the user was created with no
roles. Declaring `roles: Option<String>` turned that into a deserialization failure, which is how it
was found.

The payloads now send `"%All"`. No production caller is known to have sent an array, but the tool
accepted one silently for as long as `AnyParams` was in place, which is exactly the `max_chars`
pattern: a shape the schema did not forbid and the handler did not read.

## F6 — four tools document `server` in prose but never read it (in scope, documented)

`iris_credential_list`, `iris_credential_manage`, `iris_lookup_manage` and `iris_lookup_transfer` all
call `self.iris_arc()` directly rather than resolving a connection from the pool, so none of them
reads a `server` key. Every other tool in the batch neighbourhood takes one.

This is the `max_chars` pattern with the polarity reversed: not a documented parameter that the
handler ignores, but a capability the surrounding tools have and these four silently lack. Declaring
`server` on them anyway would be worse than leaving it off — a caller passing
`server: "prod"` would be answered from the default connection and never told.

Resolved by omission plus a guard: batch 6 declares no `server` property on these four, and
`server_is_absent_from_every_tool_in_this_batch` in `tests/binary/schema_batch6.rs` fails if one
appears. Wiring the handlers to the pool is a behaviour change and out of scope here (the spec's
"preserve current runtime behavior exactly"); the guard is what makes the gap visible instead of
invisible.

## F7 — an unrecognized `iris_lookup_manage` action reports the gate, not the typo

With the destructive tier closed — the default, since it is never inferred — `iris_lookup_manage`
with `action: "nonsense"` answers `DESTRUCTIVE_TOOLS_DISABLED`, not `INVALID_ACTION`. The gate
classifies anything outside the four read actions as destructive and refuses before the dispatcher
runs.

That is the fail-closed default working correctly and nothing here changes it. It is recorded because
it shapes what the live tests can prove: `INVALID_ACTION` for this tool is only observable with the
destructive tier open, so `lookup_manage_read_actions_honor_table_key_and_namespace` opens a second
session for that one assertion and pins the gated answer in the first.

## F8 — the suite's own tests called `iris_interop_query` with a parameter that does not exist

Three tests sent `query_type` to `iris_interop_query`. The handler reads `what`. `query_type` appears
nowhere in `crates/*/src`, and one of the three sent `query_type: "error_log"`, a value `what` does
not accept either. All three passed for as long as they have existed: `AnyParams` accepted the key,
the handler saw no `what`, defaulted to `logs`, and answered successfully. The assertions only
required a `success` or `error_code` field, so a right-looking answer to the wrong question satisfied
them.

The typed struct turned all three red at once — `unknown field 'query_type'`, which is the whole point
of the feature arriving in the place least expected.

Fixed: `tests/interop_e2e_tests.rs` now sends `what: "logs"` and `what: "queues"`, and
`tests/unit/test_tools_mod_unit.rs` sends `what: "messages"`. Each site carries a comment naming what
it used to say, so the next reader does not re-invent the parameter.

This is the strongest evidence in the audit that prose-only parameter documentation does not work.
The invented name was not in a client's payload or a user's config — it was in this repository's own
test suite, written by someone reading the tool description.

## F9 — thirty-five live-dispatch tests sent parameters no handler reads

F8 was three tests. Turning `deny_unknown_fields` on across all 43 remaining params structs found
thirty-five more, all in `tests/integration/test_handlers_live.rs`, between them sending eighteen
distinct parameter names that appear nowhere in `crates/*/src`:

| Sent                | Tool                                                                                | What the tool actually reads                   |
| ------------------- | ----------------------------------------------------------------------------------- | ---------------------------------------------- |
| `limit`             | `iris_search`                                                                       | nothing — the tool has no result cap           |
| `doc_type`          | `iris_search`                                                                       | `category`                                     |
| `document`          | `iris_search`                                                                       | `documents` (an array)                         |
| `log_type`, `store` | `iris_get_log`                                                                      | nothing — no log-type selector exists          |
| `max_lines`         | `iris_get_log`                                                                      | `limit`                                        |
| `table_name`        | `iris_lookup_manage`                                                                | `table`                                        |
| `item_name`         | `iris_production_item`                                                              | `item`                                         |
| `full_status`       | `iris_production`                                                                   | `full`                                         |
| `params`            | `iris_query`                                                                        | `parameters`                                   |
| `workspace`         | `iris_symbols_local`                                                                | `workspace_path`                               |
| `max_results`       | `iris_symbols`                                                                      | `limit`                                        |
| `action`            | `iris_table_info`                                                                   | nothing — columns and indexes always come back |
| `path`              | `kb_index`                                                                          | `workspace_path`                               |
| `class_name`        | `resolve_dynamic_dispatch`                                                          | `package_prefix`                               |
| `name`              | `iris_credential_manage`                                                            | `id`                                           |
| `what`              | `agent_history`                                                                     | nothing — `limit` is the only parameter        |
| `namespace`         | `kb`, `kb_recall`, `skill`, `skill_community`, `iris_get_log`, `iris_symbols_local` | nothing — these tools are not namespace-scoped |

Two of these are worth naming individually.

`test_dispatch_iris_search_with_limit` is a test whose name is its whole subject, and `iris_search` has
never had a `limit`. The test passed because the key was discarded and the search returned everything.
It now pins `documents`, a scoping parameter the tool does read.

`test_dispatch_agent_history_with_calls` built its history by calling `iris_info` with
`{"action": "version"}`. `iris_info` takes `what`. The setup call failed, its error went into a
`let _ =`, and the test then asserted on an empty history — so the "with calls" case had never once
run with calls in it. The setup now sends `{"what": "namespace"}`.

Nothing here changes runtime behaviour: every one of these keys was already being ignored. What
changed is that the suite now says out loud what it is testing. A renamed parameter used to cost
nothing; from here it costs a red test.

## F10 — five `iris_search` e2e tests sent `max_results`, and one was named for enforcing it

`max_results` belongs to `iris_doc(mode="list")`, where it is documented and clamped to 1000.
`iris_search` has no result cap at all — size is handled by the log store, not by a parameter. Five
tests in `tests/integration/test_e2e.rs` sent `max_results` to `iris_search` anyway.

`e2e_search_max_results_respected` is the one worth naming. Its entire subject was that the cap is
honoured:

```rust
if result["success"] == true {
    assert!(results.len() <= 2, "max_results=2 must not return more: …");
}
```

The open schema accepted the key, the handler never read it, and the search returned an uncapped set —
which the assertion then measured as compliant, because two is more than the number of results a
scopeless search comes back with. This is `max_chars` exactly: a parameter name that exists only in the
caller's head, plus a test that reads as proof it works.

Resolved: the four other call sites drop the key, and the test is now
`e2e_search_refuses_max_results_because_it_is_not_a_search_parameter`, asserting the refusal. A test
that claims to check a cap has to fail when there is no cap.

| Tool          | Parameter     | Documented | Advertised | Read | Resolution                                               | Wrong source   |
| ------------- | ------------- | ---------- | ---------- | ---- | -------------------------------------------------------- | -------------- |
| `iris_search` | `max_results` | no         | no         | no   | key removed from 4 calls, 5th inverted to assert refusal | the test suite |

## F11 — the PHI acknowledgement in an e2e test never arrived

`iris_message_body`'s acknowledgement parameter is camelCase on the wire: the PHI gate reads
`acknowledgePhi` out of the raw arguments before dispatch (`src/policy/gate.rs`), the params struct
renames the field to match, and `docs/tools.md` documents it that way in all six places it appears.

`e2e_message_body_nonexistent_id_returns_error` sent `acknowledge_phi`. The key was discarded, the
acknowledgement was never made, and the test passed on the `DATA_POLICY_BLOCKED` early return instead
of on the path it was written for. Same shape as F8: a right-looking answer to a different question.

| Tool                | Parameter         | Documented        | Advertised        | Read              | Resolution                  | Wrong source   |
| ------------------- | ----------------- | ----------------- | ----------------- | ----------------- | --------------------------- | -------------- |
| `iris_message_body` | `acknowledge_phi` | no (camelCase is) | no (camelCase is) | no (camelCase is) | test sends `acknowledgePhi` | the test suite |

## F12 — `iris_containers` was sent a namespace it has never had

Two tests in `tests/integration/test_live_reload_e2e.rs` called
`iris_containers(action="select", name=…, namespace="USER")`. Selecting a container rewrites the whole
connection, and the namespace comes from the connection config — the tool reads `action` and `name` and
nothing else. Neither the docs nor the description mention a namespace. The key was accepted and
dropped for as long as the tool has existed.

| Tool              | Parameter   | Documented | Advertised | Read | Resolution                  | Wrong source   |
| ----------------- | ----------- | ---------- | ---------- | ---- | --------------------------- | -------------- |
| `iris_containers` | `namespace` | no         | no         | no   | key removed from both calls | the test suite |

## F13 — `struct_fields` reported one struct's fields as another's (in scope, resolved)

`iris_agentic_dev_core::testing::struct_fields` found a struct's declaration and then took everything
up to the next `\n}`. `pub struct NoParams {}` closes on its own line, so the search ran straight past
it into the following declaration and returned `GetLogParams`'s four fields — `id`, `limit`, `offset`,
`server` — as the fields of every no-argument tool.

The four-way audit is what surfaced it: seven tools were reported as reading four parameters they do
not advertise. The helper, not the tools, was wrong. Fixed by balancing braces from the opening `{`
instead of scanning for a newline.

Worth recording because of the direction of the error. A helper that over-reports a read set produces
loud false failures, which get fixed. A helper that under-reports one produces silent passes — F4 was
that case. This feature's guards are only as good as the two functions underneath them, so both
failure modes are now pinned in `tests/unit/test_testing_helpers.rs`.

## F14 — `iris_doc.compiled_type` documented a value the handler rejects (in scope, resolved)

The parameter accepts `"INT"` and nothing else: `"OBJ"` returns `INVALID_PARAMS`, because compiled
`.obj` retrieval was never implemented. The description offered both.

Declaring the enum forced the question. Declaring `["INT", "OBJ"]` would advertise a value that
always fails, which is worse than prose — a client would offer it in completion. Resolved by declaring
`["INT"]` and rewriting the description to say why: `Only "INT" (the default) is implemented; "OBJ" is
rejected with INVALID_PARAMS, so the enum offers the one value that works.`

| Tool       | Parameter       | Documented  | Advertised | Read | Resolution                                        | Wrong source           |
| ---------- | --------------- | ----------- | ---------- | ---- | ------------------------------------------------- | ---------------------- |
| `iris_doc` | `compiled_type` | INT and OBJ | `["INT"]`  | yes  | enum narrowed, docs row and description corrected | the docs and the prose |

`docs/tools.md` said `"INT"` \| `"OBJ"`; it now says INT is the only value and names the error OBJ
returns. That fix is held in place by a guard rather than by memory:
`no_documented_value_falls_outside_the_declared_enum` in `tests/unit/test_docs_contract.rs` pulls the
quoted values out of every parameter row whose schema declares an `enum` and fails on any the enum
does not contain. The enum contract test proves declared-equals-branched; this proves
documented-is-within-declared. Both directions of the value set are pinned now, which is what was
missing when `"OBJ"` was written down.

## F15 — `iris_add_server.scheme` has no validation site at all

Every other enum in FR-006 was extracted from the handler's `match` or `if` arms, so the declared set
is provably the set the code accepts. `scheme` has no such site: no branch, no comparison, no
rejection. The value is written into the server entry and later interpolated into a URL, so
`scheme: "gopher"` is stored and produces `gopher://host:port` on the next call.

Declared `["http", "https"]` from the defaults in `src/iris/servers_config.rs`
(`scheme: Some("http".to_string())` and the `https` sibling) rather than from a comparison that does
not exist, and `the_one_parameter_with_no_branch_site_is_pinned_another_way` in
`tests/unit/test_enum_contract.rs` states that out loud so the exception cannot be quietly copied to a
second parameter.

Adding runtime validation is a behaviour change and out of scope. The enum narrows what clients offer;
it does not narrow what the handler accepts.

| Tool              | Parameter | Documented | Advertised          | Read | Resolution                                     | Wrong source                        |
| ----------------- | --------- | ---------- | ------------------- | ---- | ---------------------------------------------- | ----------------------------------- |
| `iris_add_server` | `scheme`  | yes        | `["http", "https"]` | yes  | enum from the defaults; no runtime check added | neither — the gap is in the handler |

## F16 — a shared docs heading credited one table to three tools

`docs/tools.md` documents `kb`, `kb_index` and `kb_recall` under a single heading, and the extractor
attributed every parameter row under it to all three tools. The audit then reported `kb_index.action`
and `kb_recall.path` as undeclared promises, when each name is in fact advertised by exactly the tool
that takes it.

A parser artifact, not a product defect, and it is recorded because it was three of the ten
disagreements the first audit run reported — a reader comparing counts would otherwise assume three
tools were fixed. `documented_but_unadvertised` now fails only when **none** of the tools a heading
names advertises the parameter.

## Batch sign-off

Every batch's parameters were compared against `docs/tools.md`, the advertised schema, and the
handler's read set. `the_four_parameter_sources_agree_for_every_tool` in
`tests/unit/test_docs_contract.rs` reports zero disagreements across 93 tool entries, and re-runs on
every build — the table below is a record of when each batch was cleared, not the enforcement.

| Batch | Tools                                                                                                                  | Audited | Findings               |
| ----- | ---------------------------------------------------------------------------------------------------------------------- | ------- | ---------------------- |
| 1     | `compare_document`, `compare_namespace`, `global_preview`, `global_kill`                                               | yes     | —                      |
| 2     | `iris_namespace_list`, `iris_namespace_create`, `iris_database_list`, `iris_database_stats`, `iris_containers`         | yes     | F12                    |
| 3     | `iris_admin`                                                                                                           | yes     | F5                     |
| 4     | `iris_system_performance`, `iris_mirror_status`, `journal_search`, `query_audit_log`, `my_access`, `capability_matrix` | yes     | F1, F2, F3             |
| 5     | `iris_interop_query`, `iris_production_item`, `iris_production_diff`, `iris_message_body`, `iris_business_rule_info`   | yes     | F4, F8, F11            |
| 6     | `iris_credential_list`, `iris_credential_manage`, `iris_lookup_manage`, `iris_lookup_transfer`                         | yes     | F6, F7                 |
| 7     | `hl7_schema_list`, `hl7_schema_inspect`, `mermaid_class`, `mermaid_production`, `resolve_storage`, `stream_inspect`    | yes     | —                      |
| 8     | the 50 already-typed tools (`deny_unknown_fields` sweep)                                                               | yes     | F9, F10, F13, F14, F15 |

F16 is a defect in the audit's own docs parser and belongs to no batch.

### Unresolved rows

None. F1 and F2 are runtime defects in `journal_search` that this feature records and does not fix —
they are out of scope by the spec's "preserve current runtime behavior exactly" and need their own
spec. Every row that concerns what a tool **advertises** is resolved.
