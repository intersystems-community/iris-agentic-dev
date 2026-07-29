# Tools

Most tools work over the Atelier REST API and connect to any IRIS instance — no Docker
required unless noted. Tools marked ✦ require `IRIS_CONTAINER`. Tools marked 🔒 are
write-gated (suppressed on Live instances unless `IRIS_ALLOW_PROD=1`).

---

## Code

| Tool                    | What it does                                                                                                              |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `iris_compile`          | Compile a class, routine, or wildcard. Returns errors with line numbers.                                                  |
| `iris_doc`              | Read, write, delete, insert, or check any IRIS document. Supports stale-edit guards via `expected`.                       |
| `iris_execute`          | Run ObjectScript, return output. Code-editing calls are blocked — see [Code-edit guard](#code-edit-guard).                |
| `iris_execute_method`   | Invoke a `ClassMethod` directly by class+method+args, no boilerplate. String-returning methods only (v1).                 |
| `iris_query`            | Execute SQL, return rows as JSON. `mode=explain\|count\|write` for query plans, row-count estimates, and gated DML.       |
| `iris_test`             | Run `%UnitTest` tests, return structured pass/fail results. Set `coverage=true` to also measure line coverage inline.     |
| `iris_coverage`         | Measure ObjectScript line coverage via `%Monitor.System.LineByLine`. `mode=run` is all-in-one. See [Coverage](#coverage). |
| `iris_global`           | Read, write, kill, or list IRIS global nodes. Gated — see [Data safety gates](#data-safety-gates).                        |
| `iris_source_control` ✦ | Check lock status, checkout, execute SCM actions. CheckIn is opt-in via `IRIS_SCM_ALLOW_CHECKIN=1`.                       |

### Code-edit guard

`iris_execute` rejects any code that matches class- or routine-editing patterns. The
check runs before execution — a compound line that mixes innocent data work with one
blocked token is rejected entirely and nothing executes.

Blocked patterns include: `%Dictionary.*Definition`, `$system.OBJ` (Load, Compile,
Delete, and variants), `%RoutineMgr`, and direct writes to code-storage globals
(`^rOBJ`, `^rINDEX`, `^%occRoutine`, etc.).

The error response includes a `matched` field naming the specific token that triggered
the block, and a `remediation` field pointing to the correct tools:

- To write or delete a class or routine: `iris_doc` with `mode=put` or `mode=delete`.
  `iris_doc` is SCM-checkout-gated and auditable.
- To compile: `iris_compile`.

The guard is non-configurable and applies to all connections.

---

## Search and introspection

| Tool                            | What it does                                                                                                                                                   |
| ------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `iris_symbols`                  | Search classes and methods via `%Dictionary`.                                                                                                                  |
| `iris_symbols_local`            | Search `.cls`/`.mac`/`.inc` files on disk by glob pattern — no IRIS connection required.                                                                       |
| `docs_introspect`               | Deep class inspection: methods, properties, XData, superclasses.                                                                                               |
| `iris_search`                   | Full-text search across the namespace. Supports regex, category filters, and scoped document lists. Requires a document scope — namespace-wide grep times out. |
| `iris_info`                     | Namespace discovery: documents, jobs, CSP apps, metadata.                                                                                                      |
| `iris_macro`                    | Macro inspection: list, signature, definition, expand.                                                                                                         |
| `iris_table_info`               | Inspect a SQL table: class-projected vs. DDL, backing storage globals, optional row count.                                                                     |
| `resolve_dynamic_dispatch`      | Resolve `$classmethod`/`##class({var})` polymorphic dispatch to compiled candidate classes, with confidence scores.                                            |
| `extract_message_map_routing`   | Extract a compiled Ensemble `MessageMap` routing table (MessageType → Method) from a BusinessProcess/Router.                                                   |
| `find_subclass_implementations` | Find all concrete subclass implementations of a method across the full inheritance hierarchy.                                                                  |

---

## Debugging

| Tool           | What it does                                                                    |
| -------------- | ------------------------------------------------------------------------------- |
| `iris_debug`   | Map INT offsets to source lines, fetch error logs, capture error state.         |
| `iris_get_log` | Retrieve a full result by `log_id` when a tool returns `truncated: true`.       |
| `check_config` | Show active connection state — host, container, config file, write tool status. |

---

## Generation

| Tool                  | What it does                                                                  |
| --------------------- | ----------------------------------------------------------------------------- |
| `iris_generate`       | Build a context-rich prompt for generating ObjectScript. No API key required. |
| `iris_generate_class` | Generate and compile a class from a description (requires LLM API key).       |
| `iris_generate_test`  | Generate `%UnitTest` scaffolding for an existing class.                       |

---

## Interoperability

| Tool                        | What it does                                                                                                      |
| --------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| `iris_production` ✦         | Start, stop, update, check, or recover a production.                                                              |
| `iris_interop_query` ✦      | Query production logs, queue depths, or message archive.                                                          |
| `iris_production_item` 🔒   | Enable, disable, or get/set settings on an individual production config item. Works via HTTP, no Docker required. |
| `iris_production_diff`      | Diff the running production config against the last source-controlled version.                                    |
| `iris_message_body`         | Read a message body by ID (plain-text or stream-backed). Gated — see [Data safety gates](#data-safety-gates).     |
| `iris_business_rule_info`   | List or inspect Ensemble business rules (`Ens.Rule.RuleSet`).                                                     |
| `iris_credential_list`      | List Ensemble credentials (IDs/usernames only — passwords never returned).                                        |
| `iris_credential_manage` 🔒 | Create, update, or delete an Ensemble credential.                                                                 |
| `iris_lookup_manage`        | Read, write, delete, or list Ensemble lookup table entries (write actions gated).                                 |
| `iris_lookup_transfer`      | Export or import an Ensemble lookup table as XML (import gated).                                                  |

---

## Administration

| Tool                | What it does                                                                                              |
| ------------------- | --------------------------------------------------------------------------------------------------------- |
| `iris_admin`        | List namespaces, databases, users, roles, web apps; create/delete users (requires `IRIS_ADMIN_TOOLS=1`).  |
| `iris_containers` ✦ | List, select, or start IRIS Docker containers. Hot-swaps the active connection without a session restart. |

---

## Learning agent, skills, and knowledge base

| Tool                     | What it does                                                                                                                |
| ------------------------ | --------------------------------------------------------------------------------------------------------------------------- |
| `agent_history`          | Recent tool-call history for the current session (tool, success, duration, timestamp).                                      |
| `agent_stats`            | Learning agent status: skill count, pattern count, KB size.                                                                 |
| `telemetry_query`        | Query the durable telemetry record beyond the in-memory session — by tool name, session id, or time range.                  |
| `telemetry_export_trace` | Export recorded tool calls as `{from, to, via, count, ts}` dispatch-trace records, aggregated.                              |
| `skill`                  | Manage the learning agent skill registry: list, describe, search, forget, or propose (mines recent calls into a new skill). |
| `skill_community`        | Browse or install community skills published to subscribed GitHub repos.                                                    |
| `kb`                     | Index markdown/text into the IRIS knowledge base, or recall content by keyword.                                             |

---

## Coverage

`iris_coverage` measures which executable lines of your ObjectScript classes were hit
during a `%UnitTest` test run, using `%Monitor.System.LineByLine`.

**Requires** `gmheap ≥ 256 MB` — run `mode=check` first to verify. If `BBSIZ_NOT_CONFIGURED`
is returned, increase `gmheap` in Management Portal → System Administration →
Configuration → Additional Settings → Advanced Memory, then restart IRIS.

| Mode     | What it does                                                                  |
| -------- | ----------------------------------------------------------------------------- |
| `check`  | Pre-flight: verify monitor available; includes `testcoverage_available` field |
| `run`    | All-in-one: start → RunTest → stop → collect results                          |
| `start`  | Start monitoring the given class list                                         |
| `stop`   | Stop monitoring                                                               |
| `report` | Collect results from a previously stopped monitor run                         |

**Quick reference:**

```text
iris_coverage(mode="run", classes=["MyApp.MyClass"], test_path="MyApp.Tests", target_pct=80)
iris_coverage(mode="run", package="MyApp", test_path="MyApp.Tests")
iris_test(pattern="MyApp.Tests", coverage=true, coverage_target_pct=80)
```

Every response includes `testcoverage_available`. When the
[TestCoverage](https://github.com/intersystems/TestCoverage) IPM package is installed,
`cobertura_path` writes Cobertura XML output.

**VS Code:** The [InterSystems Testing Manager](https://marketplace.visualstudio.com/items?itemName=intersystems-community.intersystems-testingmanager)
extension surfaces the same `%UnitTest` classes in the Test Explorer view. Use `iris_test`
to run and fix tests from Copilot or Claude; use Testing Manager to browse results and
coverage in the IDE. They share the same server connection — no extra configuration needed.

---

## Data safety gates

PHI is Protected Health Information — the patient-identifying data HIPAA governs.

Some tools can reach PHI, and some can reach the globals IRIS uses to store its own code
and configuration. Those calls are checked before they run and refused by default. Four
checks run in order; the first one that refuses wins.

**1. Environment template.** A connection declares what kind of instance it points at via
`mcpTemplate` in `.iris-agentic-dev.toml` — `dev` (the default) permits everything, `test`
blocks code execution and compiles, and `live` blocks those plus source control. Writes
count as execution here: `iris_global` with `action=set` or `kill`, and `iris_query` with
`mode=write`, are treated as execution even though reads from the same tools are not.
Error code: `ENV_GATE_BLOCKED`.

**2. Bulk-PHI tools.** `journal_search` and `view_message_body` return whole records, so
there is no field to inspect and no safe subset to return. They are refused outright
unless the connection sets `dataPolicy = "allow"`, with no per-call override. Error code:
`DATA_POLICY_BLOCKED`.

**3. System globals.** IRIS keeps compiled classes, routines, roles, users, and
interoperability config in globals such as `^oddDEF`, `^ROUTINE`, `^%Dictionary*`,
`^ROLE`, and `^Ens.Config*`. Writing to them can leave the instance unable to compile or
start. This blocklist is hardcoded and cannot be switched off — `globalBlocklist` in your
config adds to it, never replaces it. The one exception is `dataPolicyKillAllowlist`,
which exempts the patterns you name from this check on kill operations only. Error code:
`SYSTEM_BLOCKLIST`.

**4. Globals whose names suggest PHI.** Reading `^PAPMI*`, `^PAADM*`, `^MRADM*`,
`^ORDER*`, and similar requires `acknowledgePhi: true` on the call. This is a speed bump,
not a lock — it exists so nobody pulls a patient record into a chat transcript by
accident. Error code: `PHI_GATE_BLOCKED`.

`iris_message_body` is gated separately, by `dataPolicy` alone: `block` refuses the call
(`PHI_POLICY_BLOCKED`), `allow` requires `acknowledgePhi: true` (`PHI_ACK_REQUIRED`), and
`redact` returns the body with the standard HL7 v2 patient fields replaced by
`[REDACTED]` (PID-3, 5, 7, 8, 11, 18 and MSH-3). Redaction only recognizes HL7 v2 —
anything else comes back as-is, so `redact` is not a safe default for XML or custom
message bodies.

Both the name patterns and the system blocklist are taken from InterSystems Server
Manager, so they match the lists that already shipped there.

---

## Common error codes

| Code                    | Meaning                                                                                 |
| ----------------------- | --------------------------------------------------------------------------------------- |
| `POLICY_GATE`           | Call blocked by per-connection policy — see `allow` in `.iris-agentic-dev.toml`         |
| `ENV_GATE_BLOCKED`      | Tool not permitted by this connection's `mcpTemplate` — see [gates](#data-safety-gates) |
| `DATA_POLICY_BLOCKED`   | Bulk-PHI tool called without `dataPolicy = "allow"`                                     |
| `SYSTEM_BLOCKLIST`      | Global is on the system blocklist — not bypassable                                      |
| `PHI_GATE_BLOCKED`      | Global name matches a PHI pattern — pass `acknowledgePhi: true`                         |
| `SCOPE_REQUIRED`        | `iris_search` called without a document scope — pass a `documents` wildcard list        |
| `STALE_CONTENT`         | `iris_doc` insert/delete_lines `expected` field didn't match stored content             |
| `CODE_EDIT_BLOCKED`     | `iris_execute` call matched a code-editing pattern — use `iris_doc` + `iris_compile`    |
| `CHECKIN_BLOCKED`       | SCM CheckIn called without `IRIS_SCM_ALLOW_CHECKIN=1`                                   |
| `HTTP_EXECUTION_FAILED` | Atelier HTTP call failed — check host, port, credentials                                |
| `IRIS_UNREACHABLE`      | No IRIS connection discoverable — run `check_config`                                    |
