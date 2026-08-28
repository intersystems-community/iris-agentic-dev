# Feature Specification: Agent Attribution and Audit

**Feature Branch**: `086-agent-attribution-audit`
**Created**: 2026-08-26
**Status**: Draft
**Input**: Customer question relayed by an ISC partner after a webinar: "Is there a way to identify
from IRIS when an agent takes an action and when a programmer does? For example, if an api call comes
from Postman vs Chrome one can see this in web gateway logs. The question is because one of our
clients would like to be able to limit agents on certain environments."

## Context

The partner's interim answer to the client was "there is no built in way to differentiate a
programmer from an agent other than to use different credentials," plus "it would be cool to have
good/better auditing." Both halves turned out to be roughly right, and the missing piece was in this
project rather than in IRIS.

Everything below was measured on 2026-08-26 against a live `iris-dev-iris` (community 2026.2,
localhost:52780, container up 24 hours). No inferred behavior.

**What IRIS could see.** Probing `%request` from inside an `iris_execute` call returned
`HTTP_USER_AGENT` empty, `ClientExecutableName` = `CSPa24.so`, `client_node` = the gateway host.
Every HTTP caller reaches IRIS through the Web Gateway, so the process-level fields are the gateway
for everyone — Chrome, Postman, VS Code, and this tool alike. The one caller-controlled field IRIS
records, and the one an IIS/Apache/Web Gateway access log captures, is `User-Agent`. This tool sent
none, which is why the Postman-vs-Chrome technique the customer described could not work. (PWS keeps
no access log, only `error.log`.)

**What IRIS auditing could see.** `AuditEnabled` is 1 out of the box and `Security.Events` holds 75
events with 38 enabled, yet 24 hours of heavy agent traffic produced zero audit records of agent
work — only `%System/%Login/LoginFailure` and `%System/%Security/*` config changes. The events that
would have captured it ship disabled: `%System/%System/RoutineChange`, all 20 `%System/%SQL/*`
events, `%System/%Login/Login`, `%System/%DirectMode/DirectMode`. Enabling `RoutineChange` took one
`Security.Events.Modify` call with no restart and immediately captured agent code changes, including
the `IrisDevTmp.IrisDevRun<hash>` temp routine that every `iris_execute` compiles — a fingerprint
this tool already leaves in the native audit log with zero code change.

**The limit that shapes the whole feature.** `RoutineChange` records carry Username, Roles,
Namespace and RoutineSpec, but `ClientIPAddress`, `ClientExecutableName` and `CSPSessionID` come
back _empty_. Native code-change audit tells an operator which **user** acted, never which
**client**. User-defined events written through `$SYSTEM.Security.Audit` do carry the client fields.
So distinct per-environment credentials are not a workaround for a missing feature — they are the
mechanism that makes the native audit trail attributable, and this spec says so rather than implying
a User-Agent header replaces them.

## Clarifications

### Session 2026-08-26

- Q: Does this tool write its own audit records, or only enable and document the native ones? → A:
  Both — native-first in the guide, with self-reported emission opt-in and off by default.
- Q: Does the tool auto-create the `Security.Events` entry that emission needs? → A: No — refuse and
  instruct; the tool never writes `%SYS` security configuration.
- Q: What happens on the Docker-exec transport, which carries no HTTP header? → A: Warn once per
  connection that attribution is unavailable there, and document it.
- Q: Should the marker carry the connected MCP client's name and version? → A: Yes — via a
  per-request header, and in emitted audit event data.
- Q: What happens when an audit write fails while emission is enabled? → A: Never fail the tool call;
  warn once per connection with cause and remediation, count the rest, and report the count in
  connection status.

## User Scenarios & Testing _(mandatory)_

### User Story 1 - Agent traffic is identifiable at the IRIS boundary (Priority: P1)

An operator running a shared IRIS instance wants to answer "was that change made by a person or by
an agent?" using the tools they already have: the web server access log, and an ObjectScript hook
that can read the request. Every request this tool sends to IRIS carries a marker naming the product,
its version, whether the caller is a long-lived agent session or a one-shot CLI dispatch, and an
operator-chosen label. The marker is present on every IRIS-bound HTTP request, not just some of them,
and an agent cannot suppress it by editing its own local config.

**Why this priority**: This is the customer's literal question, it is the piece that was missing, and
it is useful on its own with no server-side configuration at all — an operator greps one log file.

**Independent Test**: Send any tool call to a live IRIS, read
`%request.CgiEnvs("HTTP_USER_AGENT")` back out, and assert the marker. Separately, grep a Web
Gateway or IIS access log for the product name and see only agent traffic.

**Acceptance Scenarios**:

1. **Given** the MCP server is running, **When** it issues any IRIS request, **Then** IRIS observes a
   User-Agent naming the product, its version, and the `mcp` caller mode.
2. **Given** a one-shot CLI subcommand, **When** it issues any IRIS request, **Then** IRIS observes
   the same marker with the `cli` caller mode instead — an operator can separate an interactive agent
   session from a CI or hook dispatch.
3. **Given** an operator has set an agent label, **When** any request reaches IRIS, **Then** the
   label appears in the marker, so a fleet of agents is distinguishable from one another.
4. **Given** a label containing CR, LF, tab or other control characters, **When** the marker is
   built, **Then** the control characters are removed, the rest of the label survives, and no
   additional header can be injected.
5. **Given** a label longer than the cap, **When** the marker is built, **Then** it is truncated to a
   bounded length so one caller cannot flood every access-log line.
6. **Given** no label is set, **When** the marker is built, **Then** it contains no empty parentheses
   or dangling separator.
7. **Given** connection discovery, probing or scanning traffic, **When** those requests reach IRIS,
   **Then** they carry the same marker — no IRIS-bound request from this tool is anonymous.
8. **Given** an MCP client that identified itself at initialization, **When** a tool call reaches
   IRIS, **Then** the marker names that client and its version.
9. **Given** the Docker-exec transport, **When** a connection opens, **Then** the tool warns once
   that caller attribution is unavailable on that transport.

---

### User Story 2 - An honest attribution guide (Priority: P2)

An operator or an ISC field engineer answering this same customer question needs a single document
that states what IRIS can and cannot see, what the marker is, how to turn on the native audit events
that actually capture agent work, how to filter agent traffic in an access log, and — plainly —
that a User-Agent is caller-asserted and therefore an auditing and default-deny signal, not a
security boundary against a hostile caller.

**Why this priority**: Without the document the feature is a header nobody knows to grep for, and the
field answer to the customer stays wrong. The measured facts (empty User-Agent before this change,
`CSPa24.so` for everyone, empty client fields on `RoutineChange`) are the load-bearing content, and
they exist nowhere else.

**Independent Test**: Hand the document to someone who was not involved, have them enable the audit
events on a clean container and filter an access log using only what it says, and confirm they end up
with agent activity records and no unexplained gaps.

**Acceptance Scenarios**:

1. **Given** the document, **When** a reader follows the audit-configuration recipe verbatim,
   **Then** the named events are enabled and subsequent agent code changes appear in `%SYS.Audit`.
2. **Given** the document, **When** a reader looks for the limits, **Then** it states that the marker
   is caller-asserted, that native code-change records omit the client fields, and that distinct
   per-environment credentials and roles remain necessary for a hard boundary.
3. **Given** the document, **When** it names a configuration key or recipe, **Then** an automated
   contract check fails if that key has no implementation behind it.

---

### User Story 3 - A durable, attributable audit trail (Priority: P3)

An operator wants agent activity recorded by IRIS itself, in `%SYS.Audit`, so the evidence survives
the agent process and does not depend on the agent's cooperation. They configure the native events
that capture code changes and SQL, and they understand which records IRIS writes unconditionally
versus which ones this tool writes about itself.

**Why this priority**: This is the "better auditing" half of the ask, and it is the trustworthy half.
It ranks below the marker because it requires deliberate server-side configuration and because it
works today with zero code change — the value this feature adds is the recipe, the honesty about
trust, and (if in scope) richer self-reported records.

**Independent Test**: On a live container, enable the events, perform agent work, read the records
back out of `%SYS.Audit`, and restore the container to its prior audit configuration.

**Acceptance Scenarios**:

1. **Given** the documented events are enabled, **When** an agent changes code, **Then** a
   `RoutineChange` record exists naming the user, roles, namespace and routine.
2. **Given** an operator reads the guidance, **When** they weigh the two record sources, **Then**
   the trust asymmetry is explicit: IRIS-written records cannot be skipped by a caller, while
   records this tool writes about itself are absent for any caller that is not this tool.
3. **Given** self-reported audit emission is available and disabled, **When** tool calls run,
   **Then** no user-defined audit record is written.
4. **Given** self-reported audit emission is enabled, **When** a tool call runs, **Then** a record
   exists in `%SYS.Audit` whose event source, event type, event name and event data identify the
   product, the tool and the caller mode.

---

### User Story 4 - Restricting agents on a given environment (Priority: P4)

An operator wants agents allowed on dev and blocked on production. They need to know which
mechanisms actually enforce that and which only appear to.

**Why this priority**: It is the customer's stated motivation, but the enforcement lives in IRIS
security (credentials, roles, resources, web application configuration) and in the web server, not
in this tool. The deliverable here is correct guidance plus an explicit statement of what this
tool's own gates cannot do — not a new enforcement mechanism.

**Independent Test**: On a live container, deny an agent's credential the privilege the tool needs
and confirm the tool call fails with an IRIS-side denial rather than a client-side one.

**Acceptance Scenarios**:

1. **Given** guidance on restricting agents per environment, **When** an operator reads it, **Then**
   it leads with IRIS-side controls (distinct credentials, roles, resources, web application
   configuration) and with web-server-side filtering on the marker.
2. **Given** the same guidance, **When** it mentions this tool's write and destructive gates,
   **Then** it states that those gates run in the agent's own process against the agent's own local
   config, so `curl`, Postman, another MCP server, or a rebuilt binary ignores them entirely — and
   that their real value is per-tool granularity IRIS resources cannot express, such as denying
   arbitrary WebSocket execution while allowing document reads.
3. **Given** an agent attempts to edit class or routine code through arbitrary execution, **When**
   the call is dispatched, **Then** it is refused non-configurably and the operator-facing reason is
   visible, because forcing code changes down the document path is what makes a `RoutineChange`
   audit trail meaningful.

---

### Edge Cases

- **Transport with no HTTP layer.** When the connection runs in Docker-exec mode, requests reach
  IRIS over `iris session` standard input, so no HTTP header exists on that path at all and the
  marker cannot apply. This is not a rare path — Enterprise 2026.2.0AI builds ship without PWS
  (DPP-1192) and require it. The tool warns once per connection that attribution is unavailable
  there (FR-011); it does not attempt to carry the marker by another means.
- **WebSocket handshake.** The arbitrary-execution WebSocket path opens its connection through a
  different client than the HTTP builders, so it needs the marker applied separately or it stays
  anonymous — and it is the highest-privilege path, which makes it the worst one to miss.
- **A caller that lies.** Any caller can send this product's User-Agent. The marker is evidence for
  auditing and for default-deny policy, not proof against a hostile caller.
- **A caller that says nothing.** A modified build, `curl`, or another MCP client sends no marker and
  writes no self-reported audit record, while still appearing in IRIS-written audit records under
  whatever credential it used. Absence of the marker must not be read as absence of agent activity.
- **Audit configuration is global.** Enabling an event or creating a user-defined event mutates
  `%SYS` state shared by every other user of the instance, including every other test. Anything that
  changes it must restore it.
- **Audit volume.** Enabling `%SQL/*` events on a busy instance produces a large volume of records;
  guidance must say which events to enable for this purpose rather than "enable auditing."
- **Label at exactly the cap**, and a label that is entirely control characters (sanitizing leaves
  nothing) — the marker must remain a valid header value in both cases.

## Requirements _(mandatory)_

### Functional Requirements

#### Caller marker (backfilled from shipped, unreleased behavior)

- **FR-001**: Every request this tool sends to IRIS MUST carry a caller marker in the HTTP
  `User-Agent` header.
- **FR-002**: The marker MUST name the product and the exact version, so an operator can tie observed
  traffic to a release.
- **FR-003**: The marker MUST distinguish an MCP server session from a one-shot CLI dispatch.
- **FR-004**: The caller mode MUST be determined by how the process was invoked, not by
  agent-supplied configuration, so an agent cannot present itself as a different kind of caller.
- **FR-005**: The marker MUST carry an operator-supplied label when one is provided via the
  environment, so a fleet of agents is individually identifiable.
- **FR-006**: The label MUST be sanitized so the marker contains no CR, LF, or other control
  characters, and MUST NOT be silently discarded when sanitizing removes characters — the remaining
  label text survives.
- **FR-007**: The label MUST be length-capped, and the assembled marker MUST always be a valid HTTP
  header value.
- **FR-008**: When no label is set, the marker MUST NOT contain empty parentheses or a dangling
  separator.
- **FR-009**: Every HTTP client this tool constructs for IRIS traffic MUST carry the marker,
  including the clients used for connection discovery, probing and scanning. No IRIS-bound HTTP
  request may be anonymous.
- **FR-010**: The WebSocket handshake used for interactive execution MUST carry the marker.
- **FR-011**: On a transport that carries no HTTP request — the Docker-exec path, where requests
  reach IRIS over `iris session` standard input — the tool MUST warn once per connection that caller
  attribution is unavailable on that transport, rather than losing attribution silently.
- **FR-012**: When a connected MCP client identifies itself at initialization, the marker MUST carry
  that client's name and version, so an operator can tell which agent product acted and not merely
  that some agent did. This requires a per-request header rather than a connection-level default,
  because connections are constructed before any tool call runs.

#### Documentation

- **FR-013**: The project MUST ship an attribution and audit guide covering: what IRIS can and cannot
  see about an HTTP caller (including that the process-level client fields are the Web Gateway for
  every caller); the marker format and the label environment variable; the exact steps to enable the
  native audit events that capture agent work; how to filter agent traffic in IIS, Apache and Web
  Gateway access logs; the trust asymmetry between IRIS-written and self-reported records; and the
  explicit statement that the marker is caller-asserted and is not a security boundary.
- **FR-014**: The guide MUST state that distinct per-environment credentials and roles remain
  necessary for a hard boundary, and MUST NOT present this tool's client-side write or destructive
  gates as a way to restrict agents on an environment.
- **FR-015**: Every reference to the guide from source or documentation MUST resolve to a file that
  exists.
- **FR-016**: Any configuration key or recipe named in the guide MUST be covered by the
  documentation-contract check, so a documented key with no implementation behind it fails CI.

#### Native audit configuration

- **FR-017**: The guide MUST name the specific audit events to enable for agent attribution, and MUST
  state that enabling them requires no restart.
- **FR-018**: The guide MUST record that native code-change audit records omit the client IP,
  client executable and session ID, and that user-defined audit records carry them.
- **FR-019**: The guide MUST document the temp-routine naming fingerprint that identifies this tool's
  arbitrary-execution activity in native audit records without any code change.

#### Self-reported audit emission

In scope. The guide leads with the native events because those are the records a caller cannot skip;
emission adds what native records cannot carry and is off unless an operator turns it on.

- **FR-020**: Self-reported audit emission MUST be disabled by default and MUST be enabled only by
  explicit operator configuration.
- **FR-021**: When enabled, each emitted record MUST identify the product, the tool invoked, the
  resolved caller mode, and the connected MCP client's name and version when one is known — the same
  identity the marker carries, so the access log and `%SYS.Audit` agree.
- **FR-022**: When disabled, no user-defined audit record may be written.
- **FR-023**: A failure to write an audit record MUST NOT fail the tool call it describes. The first
  failure on a connection MUST be surfaced as a warning naming the cause and the remediation;
  subsequent failures MUST be counted rather than repeated, and the count MUST be reported in that
  connection's status output, so an operator cannot silently hold a false belief that emission is
  recording.
- **FR-024**: The tool MUST NOT create the user-defined audit event entry itself. When emission is
  enabled and the entry does not exist, emission MUST be inert and the tool MUST report the exact
  event-creation call for an operator to run. The tool MUST NOT write `%SYS` security configuration
  under any circumstance.

#### Code-edit path (already shipped; stated so it is not lost)

- **FR-025**: Editing class or routine code through arbitrary execution MUST remain refused and
  non-configurable, so code changes travel the document path where IRIS records them.

### Key Entities

- **Caller marker**: the User-Agent value. Attributes: product name, product version, caller mode,
  optional operator label, and the connected MCP client's name and version when known. Caller-asserted; observable in a web server access log and from
  ObjectScript.
- **Caller mode**: `mcp` (a long-lived agent session) or `cli` (a one-shot dispatch from a script,
  hook, or CI step). Derived from process invocation.
- **Operator label**: an operator-chosen string identifying which agent or fleet member is calling.
  Sanitized and length-capped.
- **Native audit event**: an IRIS-catalogued event with a source, type, name and enabled flag.
  Written by IRIS; cannot be skipped by a caller.
- **User-defined audit event**: an event this product registers and writes itself, carrying arbitrary
  event data plus the client fields IRIS fills in. Self-reported.
- **Audit record**: a row in `%SYS.Audit`. Carries timestamp, event identity, username, roles,
  namespace, and — depending on the event — client IP, client executable, session ID, and event data.

## Success Criteria _(mandatory)_

### Measurable Outcomes

- **SC-001**: An operator can separate agent requests from human requests in a web server access log
  with a single grep for the product name, with no false negatives across every IRIS-bound request
  path this tool uses (HTTP, discovery/probe, WebSocket).
- **SC-002**: An operator can tell an interactive agent session from a CI or hook dispatch, and one
  fleet member from another, using only the access log.
- **SC-003**: Reading the marker back from inside IRIS returns a non-empty value on every request
  path — the measured starting point was empty on all of them.
- **SC-004**: Following the guide's audit recipe on a clean instance takes fewer than five commands,
  requires no restart, and produces records for agent code changes where previously 24 hours of agent
  traffic produced none.
- **SC-005**: The guide answers the customer's question, including what does not work and why,
  without a reader needing to ask a follow-up about enforcement versus evidence.
- **SC-006**: Every claim in the guide about IRIS behavior is one that was measured against a live
  instance, and every configuration key it names has a test that fails if the implementation drops it.
- **SC-007**: No test leaves the shared container's audit configuration changed.

## Test Requirements

Three layers, per the project constitution. IRIS is the only valid test object — no mocked IRIS, no
mocked HTTP client, no stubbed responses.

1. **Unit**: assemble the marker without a network round trip and assert product, version, caller
   mode, label carriage, control-character sanitizing with label survival, the length cap, header
   validity, and no dangling separator. Any new configuration key is parsed from a config **string**,
   never constructed as a struct literal, so a serde silent-drop fails the test.
2. **Binary invocation**: spawn the binary as a subprocess, drive it over stdio, and assert on the
   JSON-RPC response — this is what catches a flag or field that exists but was never wired.
3. **Live IRIS**: against the project's dev container, `#[ignore]`, single-threaded. Read the marker
   back out of IRIS on every request path. For audit behavior, assert both sides: the record is
   present and its fields are correct when emission is on, and nothing is written when it is off. Any
   test that enables an audit event or creates a user-defined event restores the prior state.

Also required: the guide is covered by the documentation-contract test.

## Out of Scope

Two defects found while measuring, both real, both filed and fixed separately rather than folded in:

- The CLI prints only the success output or an error string, so a non-configurable security refusal —
  which returns a message and remediation instead — exits non-zero with empty stdout _and_ empty
  stderr. An invisible security block; it cost three iterations to diagnose during this
  investigation.
- The code-edit guard matches on substring, so a read-only existence check against a dictionary class
  is refused as an edit. Same false-positive class as the compile-package case noted in the benchmark
  writeup.

Also out of scope: any new client-side enforcement mechanism. Enforcement of "no agents on this
environment" belongs to IRIS security and the web server.
