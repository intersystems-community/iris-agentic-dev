# Tasks: Agent Attribution and Audit

**Input**: Design documents from `/specs/086-agent-attribution-audit/`
**Prerequisites**: [plan.md](./plan.md), [spec.md](./spec.md), [research.md](./research.md),
[data-model.md](./data-model.md), [contracts/](./contracts/), [quickstart.md](./quickstart.md)

**Tests**: mandatory, and first within every phase. The project constitution allows no mocked
IRIS, no mocked Atelier HTTP client and no stubbed responses — a live `iris-dev-iris`
(`localhost:52780`) is the only valid test object. Every live run uses `--test-threads=1`.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel — different files, no dependency on an incomplete task
- **[Story]**: the user story this task serves (US1 … US4)

## Already shipped before this task list (do not redo)

Measured against the working tree, not assumed:

| Thing                                                                                    | Where                                                          |
| ---------------------------------------------------------------------------------------- | -------------------------------------------------------------- |
| `CallerMode`, `CALLER_MODE` `OnceLock`, `set_caller_mode`/`caller_mode`, `MAX_LABEL_LEN` | `crates/iris-agentic-dev-core/src/iris/connection.rs:12-45`    |
| `user_agent(mode)` and `sanitized_agent_label()`                                         | `crates/iris-agentic-dev-core/src/iris/connection.rs:54-90`    |
| Marker applied on `probe_client()` and `http_client()`                                   | `crates/iris-agentic-dev-core/src/iris/connection.rs:734, 757` |
| Caller mode wired from the subcommand                                                    | `crates/iris-agentic-dev-bin/src/main.rs:80-86`                |
| 7 unit tests, written before the code, wired as `[[test]] test_user_agent`               | `crates/iris-agentic-dev-core/tests/unit/test_user_agent.rs`   |
| Live read-back of the marker on the exec path                                            | `test_exec_live.rs::test_user_agent_visible_to_iris`           |

Nine client-construction sites remain anonymous and are the subject of US1: `discovery.rs:134`,
`223`, `429`, `582`; `ws_session.rs:335` (CSP cookie) and `ws_session.rs:127` (the
`connect_async` handshake); `tools/search.rs:109`; `tools/doc.rs:359`. The three non-IRIS
builders (`generate.rs:128`, `230`; `manifest/resolve.rs:204`; `skill_install/mod.rs`;
`skills/mod.rs:65`) MUST stay anonymous per caller-marker invariant 6.

---

## Phase 1: Setup

- [x] T001 Confirm the baseline is green before touching anything: `cargo fmt --all -- --check`,
      `cargo clippy -- -D warnings`, `cargo test` from the repository root
- [x] T002 Confirm the live container is up and reachable: `docker ps --filter name=iris-dev-iris`
      and `curl -sf -u _SYSTEM:SYS http://localhost:52780/api/atelier/` — every `#[ignore]` task
      below depends on it
- [x] T003 Record the container's as-found audit configuration so Phase 5 can prove it was
      restored: run `##class(Security.Events).Get("%System","%System","RoutineChange",.p) Write p("Enabled")`
      and the equivalent for any custom `iris-agentic-dev` event, then save the output to
      `/tmp/iad086-audit-baseline.txt`

---

## Phase 2: Foundational (blocking prerequisites)

Both items below are read by US1 (the marker) and US3 (the audit record's `EventData`), so
neither story can be finished until they exist.

- [x] T004 [P] Unit test the MCP peer task-local: absent outside a tool call, present with
      name and version inside one, and `None` when the peer sent no `clientInfo` — new file
      `crates/iris-agentic-dev-core/tests/unit/test_mcp_peer_identity.rs`, wired as a `[[test]]`
      entry in `crates/iris-agentic-dev-core/Cargo.toml`
- [x] T005 Add an `MCP_PEER` `tokio::task_local!` beside the existing `CALL_START` at
      `crates/iris-agentic-dev-core/src/tools/mod.rs:60-64`, set it inside the same `scope(…)` in
      `call_tool` (`mod.rs:8319-8325`) from `context.peer.peer_info()`, and expose a
      `mcp_peer()` reader that returns `Option<(String, String)>` — task-scoped, not global,
      because the HTTP transport clones one `IrisTools` across sessions (research R3)
- [x] T006 Add a shared `iris_http_client()` constructor in
      `crates/iris-agentic-dev-core/src/iris/connection.rs` that applies
      `.user_agent(user_agent(caller_mode()))` in exactly one place, accepting the
      per-call-site timeout and TLS settings the existing builders pass, so a future
      client cannot be added anonymously by omission

**Checkpoint**: `cargo test` green; T004 passes; nothing user-visible has changed yet.

---

## Phase 3: User Story 1 — Agent traffic is identifiable at the IRIS boundary (P1)

**Goal**: every IRIS-bound request from this tool carries the marker, including discovery,
probing, scanning, search, batch document reads and the WebSocket handshake, and the marker names
the connected MCP client.

**Independent test**: read `%request.CgiEnvs("HTTP_USER_AGENT")` back out of live IRIS on each
request path and assert the marker; separately grep a Web Gateway access log for the product name
and see only agent traffic.

### Tests first (US1)

- [x] T007 [P] [US1] Extend `crates/iris-agentic-dev-core/tests/unit/test_user_agent.rs` with the
      cases the marker grammar added: MCP client part present (`(mcp; label; claude-code/2.1.0)`),
      client present with no label (`(mcp; claude-code/2.1.0)` — no dangling `"; "`), label at
      exactly `MAX_LABEL_LEN`, a multi-byte label truncated on a `char_indices` boundary, and a
      label that is entirely control characters (sanitizing leaves nothing → marker omits it and
      stays a valid `HeaderValue`)
- [x] T008 [P] [US1] Live test that discovery, probe and scan traffic carries the marker — new
      `crates/iris-agentic-dev-core/tests/integration/test_attribution_live.rs`, `#[ignore]`,
      asserting the marker on the request paths built at `discovery.rs:134`, `223`, `429`, `582`
- [x] T009 [P] [US1] Live test that the WebSocket handshake carries the marker: open a session
      through `iris_ws_open`, read `%request.CgiEnvs("HTTP_USER_AGENT")` inside it via
      `iris_ws_exec`, assert the prefix and the `mcp` mode — add to
      `crates/iris-agentic-dev-core/tests/integration/test_attribution_live.rs`
- [x] T010 [P] [US1] Subprocess test that the shipped binary in MCP mode emits the `mcp` marker
      and not `cli`: spawn with `IAD_BINARY`, send `initialize` with a `clientInfo` of
      `{"name":"test-client","version":"9.9.9"}` plus a `tools/call`, and assert the marker IRIS
      observed names that client — new
      `crates/iris-agentic-dev-bin/tests/integration/test_attribution_stdio.rs`, `#[ignore]`,
      modelled on `test_mcp_binary_config.rs`, wired as a `[[test]]` entry in
      `crates/iris-agentic-dev-bin/Cargo.toml`
- [x] T011 [P] [US1] Live test that a `docker_only = true` connection warns exactly once that
      attribution is unavailable on that transport, and that a second call on the same connection
      does not warn again — add to
      `crates/iris-agentic-dev-core/tests/integration/test_attribution_live.rs`
- [x] T012 [P] [US1] Guard test that non-IRIS clients stay anonymous (caller-marker invariant 6):
      assert no `user_agent(` call reaches `generate.rs`, `manifest/resolve.rs`,
      `skill_install/mod.rs` or `skills/mod.rs` — add to
      `crates/iris-agentic-dev-core/tests/unit/test_user_agent.rs` as a source-tree assertion

### Implementation (US1)

- [x] T013 [US1] Route the four discovery builders through `iris_http_client()` in
      `crates/iris-agentic-dev-core/src/iris/discovery.rs:134`, `223`, `429`, `582`
- [x] T014 [P] [US1] Route the CSP-cookie client through `iris_http_client()` in
      `crates/iris-agentic-dev-core/src/iris/ws_session.rs:335`
- [x] T015 [US1] Set `User-Agent` on the `connect_async` handshake request at
      `crates/iris-agentic-dev-core/src/iris/ws_session.rs:118-127` — `tokio-tungstenite` takes a
      `Request`, so the header goes on the request builder rather than on a reqwest client; this
      is the highest-privilege path and the worst one to miss
- [x] T016 [P] [US1] Route `sync_client` through `iris_http_client()` in
      `crates/iris-agentic-dev-core/src/tools/search.rs:109`
- [x] T017 [P] [US1] Route `batch_client` through `iris_http_client()` in
      `crates/iris-agentic-dev-core/src/tools/doc.rs:359`
- [x] T018 [US1] Have `user_agent()` append the MCP client part from `mcp_peer()` (T005) in
      `crates/iris-agentic-dev-core/src/iris/connection.rs:54-68`, assembling fresh at
      client-construction time and never caching, so the value always reflects the peer in scope
- [x] T019 [US1] Add the docker-exec warn-once in
      `crates/iris-agentic-dev-core/src/iris/connection.rs`: per-connection `bool`, `tracing::warn`
      naming the transport and the consequence, fired when a `docker_only` connection is used

### Phase gate (US1)

- [x] T020 [US1] E2E gate — all of T007 … T012 pass:
      `cargo test --test '*' -- --test-threads=1 --include-ignored` with
      `IAD_BINARY=./target/debug/iris-agentic-dev`. Do not start Phase 4 until this is green.

---

## Phase 4: User Story 2 — An honest attribution guide (P2)

**Goal**: `docs/agent-attribution.md` exists, states what IRIS can and cannot see, and every key
and recipe it names is enforced by the documentation-contract test.

**Independent test**: hand the document to someone uninvolved; they enable the audit events on a
clean container and filter an access log using only what it says, and end up with agent activity
records and no unexplained gaps.

### Tests first (US2)

- [x] T021 [P] [US2] Add `docs/agent-attribution.md` to the surfaces read by
      `crates/iris-agentic-dev-core/tests/unit/test_docs_contract.rs` so its error codes, config
      keys and `IRIS_*` env vars are extracted and required to have readers — `IRIS_AGENT_LABEL`
      must resolve to a read, and `irisAudit` carries `PLANNED(spec-086)` until Phase 5 lands it
- [x] T022 [P] [US2] Add a link-resolution assertion to
      `crates/iris-agentic-dev-core/tests/unit/test_docs_contract.rs`: every
      `docs/<file>.md` path referenced from `crates/*/src` must exist (FR-015) — this is the test
      that fails today, because `connection.rs:51` already cites the guide

### Implementation (US2)

- [x] T023 [US2] Write `docs/agent-attribution.md` from [quickstart.md](./quickstart.md) and
      [contracts/caller-marker.md](./contracts/caller-marker.md), covering: the measured
      `HTTP_USER_AGENT` empty / `CSPa24.so`-for-everyone / gateway-host facts; the marker grammar
      and `IRIS_AGENT_LABEL`; the `Security.Events.Modify` recipe for `RoutineChange` with the
      no-restart note (FR-017); the `%SQL/*` cost caveat and the selective-SQL-auditing
      alternative (research R5); the `IrisDevTmp.IrisDevRun<hash>` fingerprint (FR-019); the empty
      client fields on `RoutineChange` versus populated on user-defined records (FR-018); access
      log filtering for IIS, Apache and the Web Gateway with the PWS-has-no-access-log exception;
      the `%SYS.Audit` List-query read-back with the DP-449511 staleness warning; the native
      `%MCP` events from 2026.3.0 with the policy gate and the hosted-versus-external boundary
      (research R13); the trust asymmetry; and the plain statement that the marker is
      caller-asserted and not a security boundary
- [x] T024 [P] [US2] Fix the dangling reference at
      `crates/iris-agentic-dev-core/src/iris/connection.rs:51` so it resolves to the file T023
      created (FR-015)
- [x] T025 [P] [US2] Add `docs/agent-attribution.md` to the documentation index in `docs/` and to
      the `## Docs` list in `CLAUDE.md`, so the guide is discoverable rather than orphaned

### Phase gate (US2)

- [x] T026 [US2] E2E gate — `cargo test --test test_docs_contract` green, plus
      `markdownlint-cli2 --fix docs/agent-attribution.md` and
      `prettier --write docs/agent-attribution.md` both clean

---

## Phase 5: User Story 3 — A durable, attributable audit trail (P3)

**Goal**: opt-in `%SYS.Audit` emission per connection, off by default, with refuse-and-instruct
when the event definition is absent and a failure that never fails the tool call.

**Independent test**: on the live container, enable the event, run a tool call, read the record
back through the `%SYS.Audit` List query, assert `EventSource`/`EventType`/`Event`/`EventData`,
then restore the container's prior audit configuration.

### Tests first (US3)

- [x] T027 [P] [US3] TOML round-trip test for `irisAudit`, parsed from a config **string** through
      the real deserializer — all five assertions from
      [contracts/policy-config.md](./contracts/policy-config.md): `true` parses true, `false`
      parses false, absent defaults false, `irisaudit`/`iris_audit` do **not** silently enable it,
      and the key on one connection does not affect another. New file
      `crates/iris-agentic-dev-core/tests/unit/test_policy_audit_config.rs`, wired as a `[[test]]`
      entry in `crates/iris-agentic-dev-core/Cargo.toml`
- [x] T028 [P] [US3] Unit test the `EventData` format and the refuse-and-instruct text: exact
      shape `tool=<name> mode=<mcp|cli> ua=<marker> [client=<name>/<version>]`, ASCII-only
      (DP-446307), the `ua=` value byte-identical to the marker sent on the same call, no
      parameters included, and the remediation string containing the literal
      `Security.Events.Create("iris-agentic-dev","Tool","ToolCall",…)` call. New file
      `crates/iris-agentic-dev-core/tests/unit/test_iris_audit.rs`, wired as a `[[test]]` entry
- [x] T029 [P] [US3] Live positive test: create the event definition, enable `irisAudit`, run one
      tool call, read the record back through
      `##class(%ResultSet).%New("%SYS.Audit:List")` filtered on `EventSource='iris-agentic-dev'`,
      and assert the four identity fields plus a populated `ClientIPAddress` — new
      `crates/iris-agentic-dev-core/tests/integration/test_iris_audit_live.rs`, `#[ignore]`,
      wired as a `[[test]]` with `required-features = ["testing"]`. Read the `SessionID` column,
      not `CSPSessionID`; the table and the List query name it differently
- [x] T030 [P] [US3] Live negative test: with `irisAudit` absent and with it `false`, a tool call
      writes no `iris-agentic-dev` record — compare a **filtered** count before and after, never a
      total, because audit records are immutable (research R11)
- [x] T031 [P] [US3] Live refuse-and-instruct test: with `irisAudit = true` and **no** event
      definition, a tool call succeeds, no record is written, and the warning names the cause and
      carries the exact `Security.Events.Create` command — `$SYSTEM.Security.Audit` returns `0`
      for both missing and disabled, and IRIS supplies no error text, so the assertion is on the
      tool's own text
- [x] T032 [US3] Container restoration test: a test that runs after T029 … T031 and asserts the
      audit configuration matches `/tmp/iad086-audit-baseline.txt` from T003 — the event
      definition deleted, `RoutineChange` back to its prior state (SC-007)
- [x] T033 [P] [US3] Subprocess wiring test (the #111 pattern): start the binary with a config
      that sets `irisAudit = true`, call a tool over stdio, assert the emission path was taken;
      a matching run with the key absent asserts it was not — add to
      `crates/iris-agentic-dev-bin/tests/integration/test_attribution_stdio.rs`
- [x] T034 [P] [US3] Unit test the failure counter: first failure warns once with cause and
      remediation, subsequent failures increment rather than repeat, and the count is what
      `check_config` reports for that connection (FR-023) — add to
      `crates/iris-agentic-dev-core/tests/unit/test_iris_audit.rs`

### Implementation (US3)

- [x] T035 [US3] Add `#[serde(rename = "irisAudit", default)] pub iris_audit: bool` to
      `ConnectionPolicyRaw` at
      `crates/iris-agentic-dev-core/src/iris/workspace_config.rs:213-226` and surface it on
      `ConnectionPolicy` (`:234-242`) beside `mcp_template` and `data_policy`
- [x] T036 [US3] New module `crates/iris-agentic-dev-core/src/iris/iris_audit.rs`: build
      `EventData` from the tool name, caller mode, marker and `mcp_peer()`; call
      `$SYSTEM.Security.Audit("iris-agentic-dev","Tool","ToolCall",EventData,Description)`;
      map the `0` return to the tool's own cause-and-remediation text; never write `%SYS`
      security configuration (FR-024)
- [x] T037 [US3] Wire emission at the existing choke point in
      `crates/iris-agentic-dev-core/src/tools/mod.rs:3063` (`write_audit_entry`), so the local
      JSONL entry and the `%SYS.Audit` record are driven from one place and derive the marker from
      the same values (research R10) — gated on `ConnectionPolicy::iris_audit`, and a record is
      still written for a call a gate blocked
- [x] T038 [US3] Add the per-connection audit-failure counter and surface it in the connection
      status `check_config` reports (`crates/iris-agentic-dev-core/src/tools/mod.rs:182`), so an
      operator cannot hold a false belief that emission is recording (FR-023)
- [x] T039 [P] [US3] Enforce ASCII in `sanitized_agent_label()`
      (`crates/iris-agentic-dev-core/src/iris/connection.rs:71-90`) — strip non-ASCII rather than
      pass it through, because a single Unicode character anywhere in a record makes
      `%SYS.Audit::Export()` throw `<ILLEGAL VALUE>` (DP-446307, research R7a)
- [x] T040 [P] [US3] Document the `irisAudit` key in `docs/connecting.md`; also remove the
      `PLANNED(spec-086)` annotation from `crates/iris-agentic-dev-core/tests/unit/test_docs_contract.rs`
      that T021 put there, now that the key has a real reader

### Phase gate (US3)

- [x] T041 [US3] E2E gate — `cargo test --test '*' -- --test-threads=1 --include-ignored` green
      with `IAD_BINARY` set, and T032 confirms the container's audit configuration is as-found

---

## Phase 6: User Story 4 — Restricting agents on a given environment (P4)

**Goal**: correct guidance on what enforces "no agents here" and an explicit statement of what
this tool's own gates cannot do. No new enforcement mechanism.

**Independent test**: on the live container, deny an agent credential the privilege a tool needs
and confirm the call fails with an IRIS-side denial rather than a client-side one.

### Tests first (US4)

- [x] T042 [P] [US4] Live test that enforcement is IRIS-side: connect with a credential whose
      roles lack the needed privilege, call a tool, and assert the failure comes from IRIS
      (a `%Security`-sourced denial) and not from a local gate — new
      `crates/iris-agentic-dev-core/tests/integration/test_environment_restriction_live.rs`,
      `#[ignore]`, restoring any user or role it creates
- [x] T043 [P] [US4] Live test that the non-configurable code-edit refusal still holds and its
      reason is visible: `iris_execute` attempting a class edit returns `CODE_EDIT_BLOCKED` with
      both `message` and `remediation` populated (FR-025) — add to the same file
- [x] T044 [P] [US4] Documentation assertion that the guide leads with IRIS-side controls and
      explicitly disclaims the client-side gates — add to
      `crates/iris-agentic-dev-core/tests/unit/test_docs_contract.rs`, keyed on the required
      headings rather than on prose

### Implementation (US4)

- [x] T045 [US4] Add the "Limit agents on an environment" section to `docs/agent-attribution.md`
      from [quickstart.md](./quickstart.md) step 5: distinct credentials and roles first, gateway
      or reverse-proxy rules keyed on the marker second, the non-configurable code-edit refusal
      third, and then — plainly — that this tool's write and destructive gates run in the agent's
      own process against the agent's own config, so `curl`, Postman, another MCP server or a
      rebuilt binary ignores them entirely, and that their real value is per-tool granularity IRIS
      resources cannot express (FR-014)

### Phase gate (US4)

- [x] T046 [US4] E2E gate — T042 … T044 pass, and the container has no leftover test user or role

---

## Phase 7: Polish & Cross-Cutting Concerns

- [x] T047 [P] Coverage against the 90% gate:
      `cargo llvm-cov --features testing --workspace -- --test-threads=1 --include-ignored`,
      with each new branch (marker with and without label, with and without MCP client,
      docker-exec warn, emission on/off/failed) reachable from a test
- [x] T048 [P] Version-consistency check: if this feature touches any version-bearing file, add
      the explicit cross-file assertion the constitution requires
- [x] T049 [P] `cargo fmt --all -- --check` and `cargo clippy -- -D warnings` clean
- [x] T050 [P] Run `/no-ai-slop` on `docs/agent-attribution.md` and address every flagged item —
      this document is the field answer to a customer question and will be read externally
- [x] T051 [P] `markdownlint-cli2 --fix` then `prettier --write` on every `.md` this feature
      touched, including this file
- [x] T052 Walk [quickstart.md](./quickstart.md) end to end on the live container as an operator
      would, and correct anything that does not behave as written — SC-004 requires fewer than
      five commands and no restart

---

## Dependencies

```text
Setup (T001-T003)
   └─> Foundational (T004-T006)   ← blocks US1 and US3
          ├─> US1  (T007-T020)  P1  MVP
          ├─> US2  (T021-T026)  P2  needs US1's marker to describe it accurately
          ├─> US3  (T027-T041)  P3  needs T005 (peer identity) and T006
          └─> US4  (T042-T046)  P4  needs US2's document to add a section to
                 └─> Polish (T047-T052)
```

Story-level notes:

- **US1 is independently shippable.** The marker is useful with no server-side configuration —
  an operator greps one log file.
- **US2 depends on US1** only for accuracy: the guide describes the marker, so writing it first
  would document behavior that does not exist yet. T022 (the link-resolution test) can be written
  at any time and fails until T023 lands.
- **US3 depends on Foundational T005** for the MCP client part of `EventData`, and on US1's T018
  so the `ua=` value and the sent header are byte-identical (FR-021).
- **US4 depends on US2** because T045 adds a section to the file T023 creates. T042 and T043 are
  independent of everything and can run as soon as the container is up.

## Parallel execution examples

Within US1, after T006:

```text
T007  unit — marker grammar cases          (test_user_agent.rs)
T008  live — discovery/probe/scan          (test_attribution_live.rs)
T010  subprocess — mcp mode + clientInfo   (test_attribution_stdio.rs)
T012  guard — non-IRIS clients anonymous   (test_user_agent.rs)
```

T007 and T012 both touch `test_user_agent.rs`, so run them sequentially with respect to each
other; T008, T009 and T011 all touch `test_attribution_live.rs` and are likewise sequential
among themselves. The `[P]` markers above are honest about the file, not about the phase.

Within US3, after T035:

```text
T027  unit — TOML round-trip     (test_policy_audit_config.rs)
T028  unit — EventData format    (test_iris_audit.rs)
T029  live — positive read-back  (test_iris_audit_live.rs)
T033  subprocess — key wiring    (test_attribution_stdio.rs)
```

Implementation tasks T014, T016, T017 are genuinely parallel — three different files, one shared
constructor already in place.

## Implementation strategy

**MVP is US1 alone.** It answers the customer's literal question, it needs no server-side
configuration, and it is the piece that was missing. Ship it and the guide (US2) together if
possible, because a marker nobody knows to grep for is not much use.

**US3 is the trustworthy half but the optional one.** It is off by default, it requires an
operator to create a `%SYS` event definition, and the native events it documents already work
today with zero code change. Land it after the guide exists to explain the trust asymmetry.

**US4 adds no code.** It is guidance plus two live tests that assert the boundary is where the
document says it is.

## Format validation

Every task above: starts with `- [ ]`, carries a sequential `T0NN` ID, carries a `[US1]`…`[US4]`
label in the four story phases and none in Setup, Foundational or Polish, and names an exact file
path. `[P]` appears only where the task touches a file no concurrent task touches.
