# Phase 0 Research: Agent Attribution and Audit

**Feature**: 086-agent-attribution-audit | **Date**: 2026-08-27
**Spec**: [spec.md](./spec.md) | **Plan**: [plan.md](./plan.md)

Every IRIS claim below was produced by running the stated code against the live
`iris-dev-iris` container (IRIS Community 2026.2, Atelier REST on `localhost:52780`) on
2026-08-26 and 2026-08-27, through `./target/debug/iris-agentic-dev exec --host localhost
--web-port 52780`. Nothing here is inferred from documentation.

Audit configuration is global `%SYS` state shared with every other test in this container,
so each probe restored what it changed. End-of-research state was re-read and matches
as-found: 75 events, 38 enabled, sources `%SYSTEM` and `%ENSEMBLE` only,
`%System/%System/RoutineChange` disabled.

---

## R1. Transport for the caller marker

**Decision**: The `User-Agent` HTTP header, formatted
`iris-agentic-dev/<version> (<mcp|cli>; <label>)`.

**Rationale**: Every HTTP caller reaches IRIS through the Web Gateway, which flattens
caller identity. Verified by reading `$System`/`%request` from inside an `iris_execute`
call: `$System.Process.ClientExecutableName()` returns `CSPa24.so` (the gateway module) for
every caller, and `client_node` is the gateway host. `%request.CgiEnvs("HTTP_USER_AGENT")`
is the only caller-controlled field IRIS records and web-server access logs capture. Before
this work that read returned `<none>` — the tool sent no `User-Agent` at all, which is why
the Postman-vs-Chrome technique the customer described could not distinguish anything.

**Alternatives rejected**:

- A custom header (`X-IAD-Caller`): not recorded by IRIS in `%request.CgiEnvs` without
  gateway configuration, and not in default access-log formats. The point is that an
  operator sees it with no server-side setup.
- Basic-auth username per caller kind: conflates identity with authorization and multiplies
  credentials the operator must manage. Distinct credentials remain the _enforcement_
  mechanism (R6), not the _attribution_ mechanism.
- `$SYSTEM.Process.SetClientApplication`-style process metadata: does not survive the
  gateway; `ClientExecutableName` is fixed at `CSPa24.so`.

## R2. Which clients must carry the marker

**Decision**: Route every IRIS-bound HTTP client through a single constructor that applies
the marker, and apply it to the WebSocket handshake explicitly.

**Measured inventory** — IRIS-bound clients constructed anonymously today (the spec's
"4 uncovered builders in discovery.rs" undercounts; the real count is 7 plus the handshake):

| Location                 | Purpose                           |
| ------------------------ | --------------------------------- |
| `iris/discovery.rs:134`  | instance discovery probe          |
| `iris/discovery.rs:223`  | instance discovery probe          |
| `iris/discovery.rs:429`  | instance scan                     |
| `iris/discovery.rs:582`  | instance scan                     |
| `tools/search.rs:109`    | `sync_client` for code search     |
| `tools/doc.rs:359`       | `batch_client` for document batch |
| `iris/ws_session.rs:335` | `get_csp_session_cookie`          |
| `iris/ws_session.rs:118` | `tokio_tungstenite` WS handshake  |

Already covered: `connection.rs::http_client()` and `connection.rs::probe_client()`.

Explicitly **not** in scope — these do not talk to IRIS and must not advertise an IRIS
caller marker: `tools/generate.rs:128`, `tools/generate.rs:230` (LLM provider APIs),
`manifest/resolve.rs:204`, `skill_install/mod.rs:199`, `skill_install/mod.rs:213`
(registry/package downloads).

**Rationale**: No struct in the codebase stores a `reqwest::Client`; every one of these is
built per operation and dropped. So a shared constructor is sufficient and there is no
long-lived client whose header would go stale.

**Alternatives rejected**: adding `.header("User-Agent", …)` at each of the 8+ `.send()`
call sites — more edit sites, more places for the next new call site to miss it, and no
behavioral gain given R3.

## R3. Carrying the connected MCP client's name and version

**Decision**: A `tokio::task_local!` holding the peer's `Implementation` (name, version),
set in `call_tool` from `context.peer.peer_info()` and read when the HTTP client is
constructed.

**Verified**: `rmcp` 3.1.3 `service.rs:1197` defines
`RequestContext<R> { ct, id, meta, extensions, peer: Peer<R> }`; `service.rs:1018` defines
`Peer::peer_info() -> Option<Arc<R::PeerInfo>>`; `model.rs:1396` defines
`Implementation { name, title, version, description, icons, website_url }`. `call_tool`
(`tools/mod.rs:~8319`) already receives `context` and passes it straight to
`ToolCallContext` without reading `peer_info()`.

**Rationale**: The pattern already exists in this file — `tools/mod.rs:60` declares
`tokio::task_local! { static CALL_START: std::time::Instant; }` and `call_tool` wraps its
whole dispatch in `CALL_START.scope(start, …)`. Adding one more task-local to that same
scope costs no dependency (constitution VII) and is correct under concurrent sessions.

This supersedes the rationale clause attached to FR-012 in the spec, which asserts that a
per-request header is required because connections are constructed before any tool call
runs. That is true of _connections_, but not of _HTTP clients_: per R2 the clients are built
inside the call. Building the marker inside the call's task achieves the same result without
touching request sites. The requirement itself (the marker carries the MCP client's name and
version) is unchanged.

**Alternatives rejected**:

- A process-global `OnceLock`, mirroring `CALLER_MODE`. Wrong for the HTTP transport:
  `mcp.rs` hands `StreamableHttpService::new` a factory that clones one `IrisTools` across
  every session, so two concurrent MCP clients would overwrite each other's identity.
  `CALLER_MODE` is safe as a global only because it is a property of the process invocation,
  not of a peer.
- Rebuilding the connection's clients on `initialize`: mutates shared state for a
  connection that other sessions share, for a value that is only needed per call.

## R4. Transports that cannot carry the marker

**Decision**: Warn once per connection, naming the transport and the consequence.

**Verified**: `workspace_config.rs:802` exposes `docker_only`; `connection.rs:587` executes
through `docker exec … iris session` with the script on stdin. There is no HTTP request and
therefore no header. This path is mandatory for Enterprise 2026.2.0AI builds, which have no
Private Web Server (DPP-1192).

**Rationale**: Silent loss of attribution is the failure this whole feature exists to fix.
Warning once per connection rather than per call keeps it visible without becoming noise.

**Alternatives rejected**: refusing to connect (breaks the only transport those builds
have); passing the label as an environment variable to `docker exec` (IRIS does not record
it in `%SYS.Audit` or any log, so nothing is gained).

## R5. Native audit events — what to enable and what they prove

**Decision**: Documentation-first. The guide names the events to enable; the tool never
writes `%SYS` security configuration.

**Verified API** (`Security.Events`, introspected and exercised):
`Create(Source,Type,Name,Description="",Enabled=1,Flags=0)`, `Delete(Source,Type,Name)`,
`Exists(Source,Type,Name,&Event,&Status)`, `Get(Source,Type,Name,&Properties)`,
`Modify(Source,Type,Name,&Properties)`.

**Verified baseline**: `Security.System.Get` returns `AuditEnabled=1` out of the box, yet 24
hours of heavy agent traffic produced zero records of agent work — only
`%System/%Login/LoginFailure` and `%System/%Security/*` config changes. The events that
would capture agent work are off by default, including
`%System/%System/RoutineChange` and all 20 `%System/%SQL/*` events.

**Verified enablement**: one
`##class(Security.Events).Modify("%System","%System","RoutineChange",.p)` with
`p("Enabled")=1`, no restart, immediately captured agent work:

```text
14:25:52.448 RoutineChange u=_SYSTEM ns=USER ^|^^/usr/irissys/mgr/user/|AuditProbe.Demo.cls
14:25:38.193 RoutineChange u=_SYSTEM ns=USER ^|^^/usr/irissys/mgr/user/|IrisDevTmp.IrisDevRunf420fb30d940.cls
```

That second line is the temp routine every `iris_execute` compiles
(`IrisDevTmp.IrisDevRun<hash>`), so this tool's activity already has a distinctive
fingerprint in the native audit log with zero code change (FR-019).

**Verified limitation**: `RoutineChange` records carry `Username`, `Roles`, `Namespace`,
`RoutineSpec` and `Description`, but `ClientIPAddress`, `ClientExecutableName` and
`CSPSessionID` come back **empty**. Native code-change audit tells an operator which _user_,
never which _client_. This is why distinct per-environment credentials are the mechanism
that makes the native trail attributable, not a workaround for a missing feature (FR-014,
FR-018).

**Rationale for never writing the config**: creating or enabling an audit event is a `%SYS`
security write. An agent performing it is a privilege escalation, and an agent that can
enable its own auditing can disable it.

**`%SQL/*` carries a real cost — recommend it with the caveat, not blankly.** DP-430959 (CPG037,
Selective SQL Auditing) records that with SQL auditing on, global-reference counts rise enough
that IRIS's own performance unit tests fail; the redesign also left
`%System/%SQL/DynamicStatement*` and `%System/%SQL/XDBCStatement*` enabled in those tests, "which
is NOT a default system setting." DP-453173 is the customer request behind it — "Customer needs
SQL Audit for the selected user. We Audit nothing or everything" — resolved by the selective SQL
auditing added in DP-429579/CPG037, so on a version that has it, scope the events to a user rather
than enabling them instance-wide. One more sharp edge worth documenting: DP-418348 (Open) has
audit events written with **empty** `EventData` for JDBC connections that were already open when
`%SYSTEM:%SQL:XDBCStatement` was enabled, so enabling mid-session silently produces useless
records until clients reconnect. The guide should keep `RoutineChange` as the unconditional
recommendation and present `%SQL/*` as a deliberate, scoped choice.

## R6. Self-reported emission — feasibility and failure modes

**Decision**: Opt-in per connection, off by default, one fixed event definition, and
refuse-and-instruct when the event is absent.

**Verified signature**: `$SYSTEM.Security.Audit(Source,Type,Name,EventData,Description)` —
5 arguments, confirmed by `%Dictionary` introspection; `%SYSTEM.Security` exposes exactly
two audit methods, `Audit` and `AuditID`. There is no flush API.

**Verified round trip**: after
`##class(Security.Events).Create("iris-agentic-dev","Tool","<name>","…",1)`, a record read
back from `%SYS.Audit` carried `EventSource=iris-agentic-dev`, `EventType=Tool`,
`Event=<name>`, `Username=_SYSTEM`, `Roles=%All`, `Authentication=32`,
`ClientIPAddress=192.168.215.1`, `ClientExecutableName=CSPa24.so`,
`CSPSessionID=eFnNTL6kBm`, `OSUsername=irisowner`, `Pid`, `Namespace`, `RoutineSpec`,
`Description`, and arbitrary `EventData`. Unlike `RoutineChange`, self-reported records
**do** carry the client fields.

**Verified return-code semantics** — this is the part the implementation depends on:

| Condition                                    | Return | Record written |
| -------------------------------------------- | ------ | -------------- |
| Event exists and `Enabled=1`                 | `1`    | yes            |
| Event exists but `Enabled=0`                 | `0`    | no             |
| Event source/type/name does not exist at all | `0`    | no             |

`$SYSTEM.Status.GetErrorText` on the failure path returns
`ERROR #00: (no error description)` — an empty diagnostic. So the tool must author its own
cause and remediation text (FR-023); it cannot forward an IRIS error string.

**Verified cost**: 10 consecutive writes completed in under `$ZHOROLOG` resolution
(`0.0000s`). The write is in-process and effectively free, so a per-tool-call emission
carries no latency argument against it.

**Verified namespace scope**: the write itself succeeds from `USER`. Only the _configuration_
requires `%SYS` — `%Dictionary.CompiledClass.%ExistsId("Security.Events")` returns 0 in
`USER`, so the tool cannot even see the class from a working namespace. That asymmetry is
what makes "emit from anywhere, configure only in `%SYS`" workable.

**Decision on event granularity**: one event definition, `iris-agentic-dev` / `Tool` /
`ToolCall`, with the tool name in `EventData`. `Event` is a single field and `EventData` is
free text (both confirmed above), so per-tool event names would force the operator to create
and maintain ~100 definitions. One definition means one operator command.

**Trust asymmetry to encode (FR-018, guide)**: `RoutineChange` and `%SQL/*` records are
written by IRIS and cannot be skipped by the caller. Records this tool writes about itself
are self-reported: a non-`iad` caller, or a rebuilt binary, writes none. Both belong in the
answer; they are not equal evidence.

## R7. Audit-record visibility timing (drives the live-test shape)

**Decision**: Live tests read audit records through the `%SYS.Audit` **List query**, not a
`SELECT`, and poll with a bounded timeout.

**Measured**: same-process write-then-read via SQL became visible in 0.06s. Cross-process
`SELECT COUNT(*)` 1–2 seconds after a write returned `0`, and the same records were present
minutes later — confirmed by re-querying and finding the exact `EventData` values from those
writes. Four later trials against the same event were visible at 0s.

**Cause — this is a known IRIS bug, not our timing**: DP-449511 (Open, "Internal feedback for
`%SYS.Audit` — using SQL Selects could return stale data, but List updates the indices
first"). An SQL `SELECT` against `%SYS.Audit` reads indices that have not been refreshed; the
class's List query refreshes them first. So the zeros were stale index reads, not unflushed
records. This corrects the earlier reading of the same measurements as post-creation lag.

Related open/closed work worth knowing about: DP-433826 (closed, "Make Audit queries faster
and fix a bunch of bugs"), DP-450153 (closed, "Audit default queries need to sort ascending on
audit index"), DP-249933 (open, "Audit Report fails if all items are purged").

**Rationale**: Reading through the List query removes the flakiness at its source. Polling
stays as a belt-and-braces bound, but a test built on a bare `SELECT` would be intermittently
green against a real, currently-unfixed platform bug — and would have sent us chasing our own
code.

**Consequence for the guide**: any read-back recipe we publish must use the List query, or
warn that `SELECT` can lag. A customer who follows a `SELECT`-based recipe, sees nothing, and
concludes auditing is broken is a support call we would have caused.

## R7a. Audit record character set (`EventData` interacts with an open export bug)

**Decision**: Restrict `IRIS_AGENT_LABEL` to ASCII in the sanitizer, on top of the existing
control-character and length rules.

**Cause**: DP-446307 (Open, "`%SYS.Audit::Export()` throws `<ILLEGAL VALUE>` if AUDIT record
contains Unicode character"). Our `EventData` carries the caller marker verbatim, and the
marker carries an operator-supplied label. A label with any non-ASCII character would produce
an audit record that breaks `Export()` — an operator action we actively recommend for getting
records off the box.

**Rationale**: We control the one field in that record that can carry arbitrary text. Widening
the sanitizer is a two-line change; the alternative is shipping a feature that can poison an
operator's audit export and waiting for a platform fix. The existing unit test that asserts
injected text survives sanitizing rather than being dropped extends naturally to this case.

## R8. Configuration surface

**Decision**: One new key on the existing per-connection policy block,
`[policy.<server-name>]`.

**Rationale**: The precedent is `audit_log.rs::should_write(policy)`, which keys the existing
local JSONL audit log off the presence of a policy block; `write_audit_entry`
(`tools/mod.rs:3061`) is already the choke point where a policy-gated call is recorded. The
customer's requirement is per-environment behavior, and `[policy.<server-name>]` is exactly
the per-environment scope (`ConnectionPolicyRaw`, `workspace_config.rs:213`, with existing
camelCase keys `mcpTemplate`, `dataPolicy`, `globalBlocklist`,
`dataPolicyKillAllowlist`).

**Test consequence** (constitution IV, the #110 pattern): the key must be exercised by
parsing a TOML _string_ through `ConnectionPolicyRaw`, never by constructing the struct
literal — a struct literal cannot catch a missing `#[serde(rename)]`.

**Alternatives rejected**: a top-level `[audit]` table (wrong scope — it would apply to
every environment at once); an environment variable (not per connection, and invisible to
the config that documents the environment).

## R9. Dependencies

**Decision**: No new crates. `tokio::task_local!` (already used), `reqwest` (already used),
`serde`/`toml` (already used), `tracing` (already used). Constitution VII satisfied with
nothing to justify.

## R10. Emission placement

**Decision**: Emit from the same choke point that writes the local audit entry, so the two
records describe the same event and cannot drift.

**Rationale**: `call_tool` already resolves gates as data and calls `write_audit_entry`
once (085). Emitting there means every tool is covered by construction, including tools
added later, and the caller-mode plus MCP-client identity from R3 are in scope at that
point. FR-021 requires the access log and `%SYS.Audit` to agree; that only holds if both
are derived from the same values in the same place.

**Alternatives rejected**: per-tool emission calls (every new tool is a chance to forget);
emitting inside the HTTP layer (no tool name there, and read-only calls that never reach
IRIS would be invisible).

## R11. Container-state restoration in tests

**Decision**: Any test that creates a user-defined event or enables a native event restores
the prior value, read before the change rather than assumed.

**Measured caveat to state plainly in the guide and the test docs**: audit _records_ are
immutable history. Deleting the event definition does not remove records already written —
verified: after `Security.Events.Delete` returned OK and `Exists` returned 0, the records
remained queryable. This research left 19 rows with `EventSource='iris-agentic-dev'` in the
container's audit log. That is by design and harmless, but a test must not assert on a total
row count.

## R12. Documentation contract

**Decision**: `docs/agent-attribution.md` joins the docs-contract test, and the dangling
reference in the `user_agent()` doc comment in `connection.rs` is fixed in the same change.

**Rationale**: The guide will name a config key and an exact `Security.Events.Create`
recipe. The failure mode this project has already shipped twice — a documented security key
with no reader (073/074) and a TOML key silently dropped by serde (#110) — is exactly a
guide that describes behavior nothing implements. The contract test is what makes the guide
falsifiable.

## R13. IRIS ships native MCP auditing in 2026.3.0 — what it covers and what it does not

**Decision**: Keep our event under source `iris-agentic-dev`, type `Tool`, name `ToolCall`, and
never write to `%System`/`%MCP`. Document the native events as the first answer for
IRIS-**hosted** MCP servers and ours as the answer for **external** MCP servers.

**Found in Jira**: DP-452957 (Closed, fix version **IRIS 2026.3.0**) — "Add %MCP type audit type
and ToolCall and Tool Discovery audit events … `%System/%MCP/ToolCall` and
`%System/%MCP/ToolDiscovery` is enabled by default." Its parent feature is DP-445295 "MCP Server
auditing" (High, still in Implementation), and DP-453297 gives MCP login its own audit type.
The requirement is Jama IRIS-TechReq-634 "MCP auditing": "MCP Servers, Tool Discovery, Tool
Invocation, and Login Attempts are audited through the IRIS Auditing subsystem."

**"Enabled by default" is narrower than it sounds.** DP-452957 says the _event type_ ships
enabled. The parent DP-445295 says recording is gated twice, and its original
"(disabled by default)" wording is struck through in favour of: the system audit event type must
be enabled **and** an audit policy must be applied to the MCP server or tool set —
"Enabling the system audit event type alone does not automatically audit all MCP tool calls."
Two further limits from the same description: "MCP auditing only covers activity processed
through the configured MCP path. REST requests and other access paths continue to use their
existing auditing and logging mechanisms," and "OpenTelemetry should be used for detailed or
high-volume MCP observability rather than treating the audit database as a record of every MCP
interaction." Dedicated audit events for outbound LLM calls and external MCP calls are explicitly
out of scope. So an operator who enables nothing gets no `%MCP` records, and the guide must say
so rather than promising records appear on upgrade.

**Scope boundary, established from the Jama chain**: item 29185 sits under component
**IRIS-CMP-322 "MCP Server definitions"** — the feature where IRIS _hosts_ an MCP server. Those
events fire for tools IRIS itself serves. iris-agentic-dev is an external MCP server that reaches
IRIS over Atelier REST, so nothing in that path produces a `%MCP` record: to IRIS we are an HTTP
client, not a hosted MCP server. DP-445295 states the same boundary in ISC's own words: "REST
requests and other access paths continue to use their existing auditing and logging mechanisms."
The gap this spec closes therefore survives 2026.3.0.

**Consequences**:

1. **Naming stays disambiguated.** Writing `%System`/`%MCP`/`ToolCall` ourselves would collide
   with a platform-owned event, would be indistinguishable from IRIS-hosted tool calls in a
   report, and — since the platform ships that event type enabled from 2026.3.0 — would be
   emitting under a definition we do not own. `iris-agentic-dev`/`Tool`/`ToolCall` keeps the two
   trails filterable by `EventSource`.
2. **The answer to the customer changes.** From 2026.3.0 the honest answer is "yes, natively, for
   MCP servers IRIS hosts, once an audit policy is applied to the server or tool set", plus the
   marker and opt-in emission for external agents. Before 2026.3.0 the marker plus native
   `RoutineChange`/`%SQL/*` is the whole answer.
3. **The guide must state the version boundary and the policy gate.** An operator on 2026.3.0+
   who sees `%System/%MCP/ToolCall` records should not conclude that external agent traffic is
   covered — and one who sees none should not conclude no MCP activity occurred, because without
   an applied audit policy the events do not record.

**Alternative rejected**: waiting for the platform. DP-445295 covers inbound hosted MCP only, and
no ticket in that chain addresses attribution for an external caller.

---

## Open items carried into Phase 1

None. No `NEEDS CLARIFICATION` markers remain in the spec, and every IRIS API this feature
calls has been exercised against IRIS 2026.2 above.

## Defects found while researching, filed separately (out of scope per spec)

1. `crates/iris-agentic-dev-bin/src/cmd/exec.rs:120-131` prints only `body["output"]` or
   `body["error"]`, so a `CODE_EDIT_BLOCKED` refusal — which returns `message` plus
   `remediation` — exits 1 with empty stdout and empty stderr. An invisible security block.
2. The code-edit guard matches on substring, so the read-only
   `##class(%Dictionary.ClassDefinition).%ExistsId(...)` is refused as an edit.
