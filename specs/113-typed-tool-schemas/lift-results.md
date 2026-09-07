# 113 release gate — behavioral evidence

Two measurements, both required before the merge, neither one a lift claim:

- **T037 / SC-007** — can a model call each converted tool with nothing but the advertised schema?
- **T036** — does the converted surface still solve the 22-task repair suite as well as 1.3.2 did?

Model for both: `mlx-community/Qwen3-Coder-Next-4bit` on a local MLX server
(`http://localhost:8006`). IRIS: `iris-dev-iris`, web port 52780, namespace `USER`.

Pick a non-thinking model. `LlmClient::complete` reads `choices[].message.content`; a reasoning
model returns the answer in a separate `reasoning` field, so every reply arrives empty and scores as
"response carried no JSON object" — a formatting habit recorded as a schema failure. The first
attempt at T037 used `mlx-community/Qwen3.6-27B-4bit` and produced exactly that.

## T037 — SC-007: 31 of 31 callable from the schema alone

`IAD_SC007=1 cargo test --features testing --test sc007_schema_only -- --include-ignored
--test-threads=1`. The opt-in variable is there because no model server exists in CI or under
`cargo llvm-cov`, and a test that always fails there is a test nobody reads; without it the run
skips and prints where this record lives. One binary spawn per tool, that tool's `description` blanked in the listing the model reads, so the
only parameter documentation left is the schema: property names, types, enums, `required`, and
per-property descriptions. The model gets one plain-English request and returns an arguments object,
which is judged against the schema rather than executed. Raw table: `target/sc007-results.json`.

Every one of the 31 converted tools has a task, so there is no tool passing by absence.

| Tool                      | Verdict  | Arguments the model produced from the schema alone                                                           |
| ------------------------- | -------- | ------------------------------------------------------------------------------------------------------------ |
| `capability_matrix`       | accepted | `{"server":"prod-a","user":"irisowner"}`                                                                     |
| `compare_document`        | accepted | `{"document":"User.Order.cls","server_a":"dev","server_b":"prod"}`                                           |
| `compare_namespace`       | accepted | `{"namespace":"USER","server_a":"dev","server_b":"prod"}`                                                    |
| `global_kill`             | accepted | `{"confirm_token":"8f31c0","global":"^ZTestScratch"}`                                                        |
| `global_preview`          | accepted | `{"count":50,"global":"^Sample.PersonD"}`                                                                    |
| `hl7_schema_inspect`      | accepted | `{"schema":"2.5","segment":"PID"}`                                                                           |
| `hl7_schema_list`         | accepted | `{"namespace":"HSCUSTOM"}`                                                                                   |
| `iris_admin`              | accepted | `{"action":"list_users"}`                                                                                    |
| `iris_business_rule_info` | accepted | `{"action":"get","namespace":"USER","rule_name":"Demo.Router.MainRule"}`                                     |
| `iris_containers`         | accepted | `{"action":"list"}`                                                                                          |
| `iris_credential_list`    | accepted | `{"namespace":"HSCUSTOM"}`                                                                                   |
| `iris_credential_manage`  | accepted | `{"action":"create","id":"EpicAPI","namespace":"USER","password":"s3cret","username":"svc_epic"}`            |
| `iris_database_list`      | accepted | `{"server":"dev"}`                                                                                           |
| `iris_database_stats`     | accepted | `{"db":"USER"}`                                                                                              |
| `iris_interop_query`      | accepted | `{"component":"HL7.Router","limit":20,"namespace":"USER","what":"messages"}`                                 |
| `iris_lookup_manage`      | accepted | `{"action":"set","key":"ADT","namespace":"USER","table":"RouteMap","value":"Epic"}`                          |
| `iris_lookup_transfer`    | accepted | `{"action":"export","namespace":"USER","table":"RouteMap"}`                                                  |
| `iris_message_body`       | accepted | `{"message_id":"4711","namespace":"USER"}`                                                                   |
| `iris_mirror_status`      | accepted | `{"server":"prod-a"}`                                                                                        |
| `iris_namespace_create`   | accepted | `{"db_path":"/usr/irissys/mgr/analytics/","name":"ANALYTICS"}`                                               |
| `iris_namespace_list`     | accepted | `{"server":"dev"}`                                                                                           |
| `iris_production_diff`    | accepted | `{"namespace":"USER","production":"Demo.Router"}`                                                            |
| `iris_production_item`    | accepted | `{"action":"disable","item":"HL7.FileIn","namespace":"USER"}`                                                |
| `iris_system_performance` | accepted | `{"mode":"start","profile":"30mins"}`                                                                        |
| `journal_search`          | accepted | `{"end":"2026-09-02T23:59:59Z","global_pattern":"^Sample","max_entries":100,"start":"2026-09-01T00:00:00Z"}` |
| `mermaid_class`           | accepted | `{"class":"Sample.Person","depth":2,"namespace":"USER"}`                                                     |
| `mermaid_production`      | accepted | `{"namespace":"USER","production":"Demo.Router"}`                                                            |
| `my_access`               | accepted | `{"server":"dev"}`                                                                                           |
| `query_audit_log`         | accepted | `{"end":null,"event_type":"LoginFailure","limit":25,"server":null,"start":null,"user":"_SYSTEM"}`            |
| `resolve_storage`         | accepted | `{"class":"Sample.Person","namespace":"USER"}`                                                               |
| `stream_inspect`          | accepted | `{"namespace":"USER","oid":"5@%Stream.GlobalCharacter"}`                                                     |

What the arguments show, beyond the pass count:

- Enums get used as enums. `iris_admin` picked `list_users`, `iris_system_performance` picked
  `start`, `iris_production_item` picked `disable`, `iris_interop_query` picked `messages` — all
  values out of the advertised list, none invented.
- `global_kill` supplied `confirm_token` unprompted, because the schema marks it required. Under
  1.3.2 the destructive gate's token requirement lived only in the prose.
- `iris_credential_manage` filled all five keys the `create` path needs. That one used to be 14
  undeclared names in an open object.
- `query_audit_log` passed explicit `null`s for the optional filters. Accepted: `Option<T>` fields
  take `null`, which is what makes the closed set safe to iterate over.

A schema-shaped acceptance test, not an execution test — six of the 31 mutate state, and `call_tool`
decides acceptance from exactly the facts the schema carries. See `benchmark::schema_tasks` for the
argument in full.

## T036 — repair-suite regression check

Not a lift claim. The question is only whether closing the schemas broke anything an agent used to
be able to do, and the answer is no: the same 18 of 22 tasks pass before and after, and the same
four fail.

Both passes ran the `jira_bugs` suite (22 tasks) in `mcp` mode against `iris-dev-iris`, one after
the other on the same MLX server, `--task-timeout-s 120 --max-time-s 2400`. BEFORE is the installed
1.3.2 binary (`/opt/homebrew/bin/iris-agentic-dev`), AFTER is `./target/debug/iris-agentic-dev` off
this branch. Same model, same container, same namespace, same tasks.

| Metric    | 1.3.2 (before) | 113 (after) |
| --------- | -------------- | ----------- |
| Tasks     | 22             | 22          |
| Passed    | 18             | 18          |
| Pass rate | 81.8%          | 81.8%       |
| Errored   | 0              | 0           |
| Elapsed   | 854.6 s        | 853.3 s     |
| Retries   | 0              | 0           |

Per-task outcomes are identical — no task flipped in either direction. The same four fail on both
sides: `jira-002` (conditional mandatory validation), `jira-007` (field becomes editable),
`jira-015` (performance degradation), `jira-056` (multi-file contract change). All four are
reasoning failures on the repair itself, not tool-call failures, and all four failed the same way
under 1.3.2. Nothing in this feature touches them.

Two things this rules out, both of them plausible ways a schema change goes wrong:

- **No new rejections.** `additionalProperties: false` plus the `UNKNOWN_PARAMETER` site means a
  call that used to be accepted with a junk key now fails. If the agent had been leaning on any
  such key, the retry count or the error count would have moved. Both stayed at zero.
- **No slowdown from the bigger listing.** The `tools/list` payload grew 35% (see `research.md`),
  and per-task wall clock did not notice: mean delta −0.06 s across the 22, range −3.6 s to +1.6 s,
  which is MLX sampling noise.

Run the same comparison before any future release that changes the advertised surface. Both raw
files stay out of the repo; the numbers above are the record.

One measurement caveat worth writing down. An earlier attempt at this comparison overlapped two
benchmark passes on one MLX server and one IRIS namespace, and the two runs shared output paths.
The pass rate happened to come out the same, but the numbers were discarded rather than reported.
The run lock in `^IRISDEV("telemetry","benchmark_lock",<host>)` is keyed by the host **string**, so
a pass launched with `IRIS_HOST=localhost` and a pass launched with `IRIS_HOST=127.0.0.1` take
different locks and do not block each other. Use one spelling of the host for a before/after pair.
