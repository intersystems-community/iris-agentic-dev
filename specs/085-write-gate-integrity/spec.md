# Feature Specification: Write-Gate Integrity

**Feature Branch**: `085-write-gate-integrity`
**Created**: 2026-08-25
**Status**: Draft
**Input**: Close the gap between what the write-protection docs promise and what the binary
enforces, and make that gap impossible to reopen.

## Why this spec exists

This is the fourth round of issue #110. The first three rounds each fixed what the reporter
had measured and left the class of defect in place. The forensic trail matters because
several requirements below exist only to stop a specific repeat.

### Timeline

| Date       | Event                                                                                                                                |
| ---------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| 2026-07-31 | `d3bc028` 23:58:43 — MCP tool annotations on all tools. Real, shipped, and with no tests of its own                                  |
| 2026-08-01 | `07f8007` 00:00:16 — 93 seconds later, specs 073 and 074, describing enforcement built on those annotations. Spec text only          |
| 2026-08-01 | `ab79fe2` 07:44 — one 141-line docs commit covering annotations, destructive gate, and allowlist together, in present tense          |
| 2026-08-01 | `64d70e9` 10:18 — the same claims propagated into a shipped skill, which invents a third tier that exists in no spec                 |
| 2026-08-01 | `8ea6931` 10:28 — v1.0.0 release notes written, and they correctly omit 073 and 074                                                  |
| 2026-08-02 | v1.0.0 ships the docs and the skill. Neither spec has ever been implemented                                                          |
| 2026-08-19 | Claudio Devecchi Junior files #110: the documented keys load, the server starts, writes go through. His root-cause analysis is right |
| 2026-08-19 | `98e0531` binds the config keys and `bb4917f` surfaces them in `check_config` — **reporting only**                                   |
| 2026-08-19 | Issue closed, before any enforcement exists                                                                                          |
| 2026-08-20 | `6381193` gates four tools. Its title names exactly the four tools the reporter had probed                                           |
| 2026-08-20 | Reporter's re-test proves 1.2.1 still unenforced. Arrives on a closed issue; answered "a fix is committed"; never reopened           |
| 2026-08-25 | Reporter re-probes 1.2.6: still unenforced, and now `check_config` lies about it too                                                 |

### Four process failures this spec must close

1. **Docs described unimplemented specs.** `git log --all -S"DESTRUCTIVE_TOOLS_DISABLED" -- crates/`
   returns nothing. Same for `write_allowed_servers`. Three error codes and a six-step check
   order have been documented as working since v1.0.0 and exist only in prose. #110 did not
   begin as a code bug — it began as a documentation commit that made promises, and the
   reporter set the keys because `docs/connecting.md` recommended them.

   The mechanism was bundling. One docs commit covered three subjects — tool annotations, the
   destructive gate, the allowlist — and only the first existed. All three shared a vocabulary,
   so the diff read as one coherent write-up of one body of work. The annotations commit had no
   tests either, so there was no verifiable subject sitting next to the unverifiable ones to
   make the difference visible. The docs text is traceably derived from the spec text: the
   check-order steps are the spec's ordering, the rationale paragraph is the spec's rationale,
   the error messages match the specs' envelopes verbatim. Specs in this repo are written in
   flat declarative present tense, which is also documentation voice, so transcribing one into
   the other required no change of mood and nothing signalled the crossing.

   Two facts narrow the blame and widen the scope. The release notes written 2h44m later
   correctly omit both features, so this was not a belief that they had shipped. And eight other
   spec-only specs existed at the time and none leaked into the docs — 073 and 074 were
   documented because they were written minutes after the commit they extend, in one continuous
   session where spec-writing, docs-writing, and releasing had no boundary between them.

   **Following the full process would not have prevented it.** Spec 072 has a plan, tasks, lift
   results, and a complete implementation, and it also leaked four phantom identifiers into the
   docs: two error codes the binary never emits (it emits differently-named ones), an
   environment variable that is hardcoded in the source instead, and a tool parameter the
   handler ignores. The common factor is not skipped process. It is that `docs/` and `skills/`
   are the only shipped artifacts in this repository that no automated check reads. This is the
   same defect class as bundled skills naming tools that do not exist (`ensemble-production`
   shipped seven phantom `interop_*` names for months).

2. **Fixes were scoped to the reported instances, not the class.** Four handler guards were
   added for the four tools named in a comment. The question "what is the complete set of
   write-capable tools?" was never asked. `iris_ws_exec` — arbitrary ObjectScript, a complete
   bypass of the `iris_execute` gate — is still ungated five releases later.

3. **The issue was closed on the reporting half.** After that there was no open tracker for
   the destructive tier, the allowlist, or the remaining ungated tools, so nothing prompted a
   completeness check. Two subsequent re-tests landed on a closed issue.

4. **A known deviation shipped with a comment explaining it, and a test that locked it in.**
   The invalid-config path logs its error and returns, with an inline comment deferring the
   promised hard failure to "callers that need to surface this" — callers that were never
   written. Its test asserts only that the function returned nothing, never the resulting gate
   state. That is why the configuration documented as refused instead starts with writes
   **enabled**.

### Evidence

All defects below were reproduced against the **released** 1.2.6 and 1.2.1 macOS arm64 assets
(checksums verified against the GitHub release) driving live IRIS at `iris-dev-iris`
(localhost:52780) over stdio. Probe artifacts were removed afterward.

Two findings correct the reporter's write-up and belong in the reply to him:

- **Nothing regressed between 1.2.1 and 1.2.6.** `git diff v1.2.1 v1.2.6` over the two
  relevant source files is empty, and the 1.2.1 binary shows identical behavior. The
  difference between his two probes is cold start versus edit-in-place. His 1.2.1 result was
  ordering luck, not a stronger guarantee.
- **The stale-reporting bug was introduced by the #110 fix itself**, in the same commit that
  bound the config key.

## User Scenarios & Testing _(mandatory)_

### User Story 1 - Declared gate actually blocks every write (Priority: P1)

An operator hardening a shared IRIS instance sets `write_tools_enabled = false`. Every tool
capable of changing state on that instance refuses, and nothing reaches IRIS.

**Why this priority**: This is the promise the documentation has made since v1.0.0 and the
substance of #110. Without it the other stories are cosmetics. `iris_ws_exec` alone makes the
gate meaningless today — it runs arbitrary ObjectScript with no check.

**Independent Test**: With the gate off, call every write-capable tool against live IRIS.
Each must return the write-gate error, **and** the global, class, lookup entry, or namespace
it would have created must not exist afterward.

**Acceptance Scenarios**:

1. **Given** a config declaring `write_tools_enabled = false`, **When** `iris_ws_open` then
   `iris_ws_exec` is called with code that sets a global, **Then** the call is refused and the
   global does not exist in IRIS.
2. **Given** the same config, **When** `iris_global` set, `iris_lookup_manage` set,
   `iris_execute_method`, or any other write-capable tool is called, **Then** the call is
   refused and no state changed.
3. **Given** the same config, **When** a read-only tool is called, **Then** it succeeds
   normally.
4. **Given** `write_tools_enabled = true`, **When** the same write calls are made, **Then**
   they proceed and the expected state change is observable.

---

### User Story 2 - Editing the config changes the gate, and reporting tells the truth (Priority: P1)

An operator edits `.iris-agentic-dev.toml` while the server is running. The new value takes
effect in both directions, and `check_config` reports both the effective gate and what decided
it.

**Why this priority**: Equal to US1 because it is the failure the reporter is looking at right
now. A gate that silently keeps a stale value is worse than no gate — the operator gets a
green light that is not real. Reporting the deciding source is what makes a future mismatch
diagnosable instead of another four-round issue.

**Independent Test**: One server process, one config file rewritten twice. Assert the gate and
the reported value follow the file each time, and that an actual write attempt agrees with the
report.

**Acceptance Scenarios**:

1. **Given** a running server started with `write_tools_enabled = true`, **When** the config is
   rewritten to `false` and reload occurs, **Then** `check_config` reports `false` and a write
   call is refused.
2. **Given** a running server started with `write_tools_enabled = false`, **When** the config is
   rewritten to `true` and reload occurs, **Then** `check_config` reports `true` and a write
   call succeeds.
3. **Given** any resolved gate, **When** `check_config` is called, **Then** the response names
   the source that decided it — config file, environment variable, or inferred default.
4. **Given** an operator has set the gate environment variable explicitly, **When** a config
   file declares the opposite, **Then** the environment variable wins and `check_config` says
   so.

---

### User Story 3 - A new write-capable tool cannot ship ungated (Priority: P2)

A contributor adds a tool that changes state. CI fails until that tool is declared in the gate
list.

**Why this priority**: This is what converts the fix from a one-time sweep into a property.
Every previous round of #110 added per-handler guards, which is exactly the shape that lets the
next tool slip through.

**Independent Test**: Add a fake write-capable tool without declaring it; the test suite must go
red naming that tool.

**Acceptance Scenarios**:

1. **Given** the write-capable tool list, **When** the suite runs, **Then** every registered
   tool is classified as either write-capable or read-only, with no tool unclassified.
2. **Given** a tool declared write-capable, **When** the gate is off, **Then** a table-driven
   test asserts it returns the write-gate error — with no per-tool test to forget.
3. **Given** a tool whose declaration disagrees with its advertised read-only annotation,
   **When** the suite runs, **Then** the mismatch fails the build.

---

### User Story 4 - Invalid gate configuration fails closed (Priority: P2)

An operator writes a contradictory combination of gate keys. The server refuses to start, with
a non-zero exit, rather than starting with writes enabled.

**Why this priority**: Today this configuration produces the opposite of what was asked for. It
is a small change with the worst possible failure direction, and both `docs/tools.md` and spec
073 already promise it.

**Independent Test**: Start the binary with the contradictory config; assert a non-zero exit
code and that no server session is established.

**Acceptance Scenarios**:

1. **Given** a config combining the destructive gate on with the write gate off, **When** the
   server is started, **Then** it exits non-zero with the documented error code.
2. **Given** any configuration error that prevents resolving the gate, **When** the server
   starts, **Then** writes are off — never inferred on.

---

### User Story 5 - Documented controls exist (Priority: P3)

Someone reading the shipped documentation or a bundled skill can act on every key, error code,
environment variable, and parameter it names.

**Why this priority**: The root cause of #110, and it is not confined to this feature — a fully
processed spec leaked four phantom identifiers into the same docs. Lower priority only because it
protects against recurrence rather than fixing live behavior. Without it, the next
spec-before-code cycle reproduces this issue exactly.

**Independent Test**: A test extracts every configuration key, error code, environment variable,
and documented tool parameter from the shipped documentation and from bundled skills, and asserts
each resolves to something real. Documented-but-planned items must carry an explicit marker
naming their spec.

**Acceptance Scenarios**:

1. **Given** the shipped docs and bundled skills, **When** the suite runs, **Then** every error
   code named in them is one the binary can actually emit, or is explicitly marked as planned
   with its spec number.
2. **Given** the same surfaces, **When** the suite runs, **Then** every configuration key named
   deserializes into the config structure **and** has a reader that acts on it — a key the system
   only writes and never reads fails the test.
3. **Given** the same surfaces, **When** the suite runs, **Then** every environment variable and
   every documented tool parameter named is read by the code that the documentation attributes it
   to.
4. **Given** a documented multi-step check order, **When** the suite runs, **Then** every step in
   it corresponds to a real check.
5. **Given** documentation that states a count of tools carrying some property, **When** the
   suite runs, **Then** the stated count equals the actual count.

---

### User Story 6 - Official releases report an honest version (Priority: P3)

An operator reads the server version and can tell an official build from a local one.

**Why this priority**: Cosmetic for behavior but not harmless — the version string is
advertised as the way to identify an official build, and every 1.2.x release has carried a
dirty-build suffix.

**Independent Test**: Build from a clean checkout of a tag; assert the reported version has no
dirty suffix. Assert dependency-lock drift fails the build rather than mutating the tree.

**Acceptance Scenarios**:

1. **Given** a clean checkout at a release tag, **When** the release build runs, **Then** the
   reported version equals the tag with no suffix.
2. **Given** a dependency lockfile out of sync with the manifests, **When** CI runs, **Then**
   the build fails with a clear message instead of silently rewriting the lockfile.

---

### User Story 7 - Irreversible operations need a second key (Priority: P2)

An operator wants ordinary development writes but not irreversible ones. Turning the write gate
on leaves the destructive tier off until it is declared separately.

**Why this priority**: The destructive key has been documented and accepted by the config loader
since v1.0.0 and has never had a reader, so an operator who set it believes they narrowed their
exposure and did not. Below US1 only because it is a subset of it — US1 blocks these tools too
when writes are off.

**Independent Test**: With writes on and the destructive key absent or off, call each of the
seven destructive tools; each is refused and the data it would have destroyed still exists.
Declare the key on and the same calls succeed.

**Acceptance Scenarios**:

1. **Given** writes on and the destructive tier not declared, **When** a destructive tool is
   called, **Then** it is refused and the target data still exists in IRIS.
2. **Given** writes on and the destructive tier declared on, **When** the same tool is called,
   **Then** it proceeds.
3. **Given** writes off, **When** a destructive tool is called, **Then** it is refused by the
   write gate regardless of the destructive key.
4. **Given** any resolved destructive gate, **When** `check_config` is called, **Then** it
   reports the tier's effective value and deciding source alongside the write gate's.

---

### Edge Cases

- Config file declares the gate, and the operator has also exported the gate environment
  variable: documented precedence is environment over file. The system must distinguish an
  environment variable the **operator** set from one the system set itself while loading a
  config — conflating the two is the current stale-value defect.
- Config file is deleted while the server runs: the gate must fall back to the documented
  default, not retain the last file value.
- Config file becomes unparseable on reload: keep the last known-good gate, report the parse
  failure, and never widen access on a parse error.
- A tool takes an action parameter where only some actions write (get versus set, read versus
  put). Classification is per action, not per tool, and the read-only actions must stay usable
  with the gate off.
- A write-capable tool is called while no IRIS connection exists: the gate answer must not
  depend on connectivity, since an unreachable server previously produced a permissive answer.
- Tools removed from the router at startup cannot be restored by a later config reload, because
  the tool list is fixed once a session is established. Reload must therefore enforce at call
  time, not by re-shaping the tool list.
- Two tools named in the destructive tier are currently stripped from the router at startup
  rather than gated. Classification must still cover them, so the completeness check does not
  quietly pass because a tool is absent, and so that whichever mechanism ends up governing them
  is a deliberate choice rather than an artifact.
- Some destructive-tier tools act on local state rather than on IRIS — removing a saved server,
  forgetting a skill. The tier covers them, and their enforcement tests assert the local artifact
  survives, since there is no IRIS side effect to observe.
- A documented key that the system parses, re-exports, and never reads would pass any check that
  only greps for the identifier. The destructive key is in exactly that state today. The
  documentation check must therefore assert a reader, not a mention.
- Documentation can be wrong while every identifier in it is real: a stated tool count that has
  since drifted, or prose promising a hard failure where the code logs and continues. Both exist
  today. The documentation check raises the floor; only behavioral tests catch these, which is
  why the enforcement requirements above do not delegate to it.

## Requirements _(mandatory)_

### Functional Requirements

#### Gate resolution

- **FR-001**: The system MUST resolve the write gate from declared configuration and MUST NOT
  derive it from process-global state that an earlier load may have set.
- **FR-002**: The system MUST apply a newly declared gate value on every configuration load,
  in both directions, for the lifetime of the process.
- **FR-003**: The system MUST honor documented precedence — an operator-set environment
  variable overrides a configuration file — and MUST distinguish an operator-set environment
  variable from one the system set while loading configuration.
- **FR-004**: The system MUST report, alongside the effective gate value, which source decided
  it.
- **FR-005**: When configuration cannot be resolved, the system MUST fail closed: writes off.
- **FR-006**: The system MUST reject a configuration that enables the destructive tier while
  the write gate is off, exiting non-zero with the documented error code, and MUST NOT start a
  session on that configuration.

#### Gate enforcement

- **FR-007**: The system MUST classify every registered tool as write-capable or read-only, in
  one declarative place, with no tool left unclassified.
- **FR-008**: The system MUST check the gate for write-capable tools at a single dispatch point,
  not in each tool's handler.
- **FR-009**: For tools whose write behavior depends on an action or mode parameter, the system
  MUST classify per action and MUST leave read-only actions available with the gate off.
- **FR-010**: When a write-capable call is refused, the system MUST return the documented error
  code and MUST NOT reach IRIS.
- **FR-011**: The system MUST enforce the gate at call time so that a configuration reload takes
  effect within an established session.
- **FR-012**: Gate enforcement MUST NOT depend on IRIS connectivity.
- **FR-013**: The write-capable classification MUST cover, at minimum, the tools verified
  ungated today: interactive session execution, global set and kill, lookup-table write and
  delete, and class-method execution — plus source control, production control, lookup
  transfer, test execution, code generation, knowledge-base indexing, and skill mutation.

#### Documented controls

- **FR-014**: Every configuration key named in the shipped documentation or in a bundled skill
  MUST have a reader that acts on its value, or carry an explicit marker naming it as planned and
  citing its spec. A key the system parses and re-exports but never reads does not satisfy this —
  that is the current state of the destructive key and the reason a presence check is not enough.
- **FR-015**: Every error code and every environment variable named in the shipped documentation
  or in a bundled skill MUST either be one the system can actually emit or read, or carry the same
  planned marker.
- **FR-016**: Every step of a documented check order MUST correspond to a real check, and every
  documented count of tools carrying a property MUST equal the actual count.
- **FR-016a**: Bundled skills are a shipped documentation surface and MUST be covered by the same
  checks. The destructive-gate claims were propagated into a skill within hours of the docs
  commit, including a third tier that appears in no spec.
- **FR-016b**: Every documented tool parameter MUST be read by that tool's handler. A parameter
  the documentation describes and the handler ignores MUST fail the same check.
- **FR-016c**: Documentation items that the new checks reveal as unbacked MUST be corrected in
  this feature. The known set outside the write-gate scope is four identifiers from spec 072 —
  two error codes whose real names differ, one environment variable that is hardcoded in source,
  one ignored tool parameter — plus one stale count. Correcting the documentation is in scope;
  implementing the described behavior instead is not.
- **FR-017**: The per-server write allowlist MUST be removed from shipped documentation,
  including its error code and the check-order steps that reference it. Spec 074 stays open as
  the design of record. The classification and dispatch point required by FR-007 and FR-008
  MUST leave room for a per-server predicate at the same call site, so implementing 074 later
  does not require reopening this work.
- **FR-018**: The system MUST implement the destructive tier as a second gate over the seven
  tools named in spec 073, controlled by its own declared key, and MUST require the write gate
  to be on for the destructive gate to have any effect. Destructive classification MUST live in
  the same declarative place as write classification (FR-007) and MUST be checked at the same
  dispatch point (FR-008).
- **FR-019**: When neither configuration nor an operator environment variable declares the gate,
  the system MUST keep today's inferred default, and MUST report that inference as the deciding
  source per FR-004. Changing the default itself is out of scope.

#### Build integrity

- **FR-020**: Release and CI builds MUST fail on dependency-lock drift rather than rewriting
  the lockfile during the build.
- **FR-021**: A build from a clean checkout at a release tag MUST report a version string
  identical to that tag, with no dirty-build suffix.

### Test requirements

These are requirements, not implementation notes: every previous round of this issue shipped
with passing tests.

- **FR-022**: Configuration tests MUST parse a configuration **string**, not construct the
  configuration structure directly, so that a silently dropped or renamed key fails.
- **FR-023**: At least one test MUST rewrite the configuration file **twice within one server
  process** and assert the gate follows the file each time. No existing test does this; that
  single omission is why three defects shipped together.
- **FR-024**: Gate-resolution tests MUST cover the case where the gate environment variable is
  already set, not only the cleared-environment case. The current defect exists solely in the
  branch every existing test clears away.
- **FR-025**: Enforcement tests MUST assert the **absence of the side effect** in IRIS — the
  global, class, lookup entry, or namespace does not exist afterward — not merely that an error
  code was returned.
- **FR-026**: A single table-driven test MUST assert every write-capable tool is refused with
  the gate off, and every destructive-tier tool is refused with writes on and the tier off, so
  that adding an ungated write or destructive tool fails the build.
- **FR-027**: Invalid-configuration tests MUST assert the resulting behavior — exit code and
  effective gate — not merely that a message was logged.
- **FR-028**: Tests asserting a reported gate value MUST assert the **value**, not its presence.
- **FR-029**: A test MUST assert the dependency lockfile is in sync with the workspace
  manifests.
- **FR-030**: Tests touching IRIS MUST run against the live development container and MUST NOT
  mock IRIS or its transport.

### Key Entities

- **Write gate**: The effective permission to change state on a connection. Has a value and a
  deciding source (configuration file, operator environment, inferred default).
- **Tool write classification**: The declarative mapping from tool — and where relevant, tool
  action — to write-capable or read-only. Single source of truth for enforcement and for the
  completeness test.
- **Destructive tier**: The seven tools named in spec 073 that irreversibly destroy data or
  configuration. A subset of write-capable, gated by its own key, meaningful only when the write
  gate is on.
- **Documented control surface**: The set of configuration keys, error codes, environment
  variables, tool parameters, counts, and check-order steps named in shipped documentation **and
  in bundled skills**. Must be a subset of what the system actually reads or emits, or explicitly
  marked planned.

## Success Criteria _(mandatory)_

### Measurable Outcomes

- **SC-001**: With writes declared off, 100% of write-capable tools refuse, and IRIS shows zero
  resulting state changes. Today at least five tools write anyway.
- **SC-002**: A configuration edit changes the effective gate in both directions within one
  running process, on the first attempt, with no restart. Today the first edit sticks forever.
- **SC-003**: The reported gate value matches actual enforcement in 100% of tested
  configurations, including edit-in-place, cold start, absent config, and the invalid
  combination. Today the reported value and enforcement disagree in at least two of those.
- **SC-004**: The configuration documented as refused produces a non-zero exit in 100% of
  attempts. Today it starts, with writes enabled.
- **SC-005**: Every configuration key, error code, environment variable, and tool parameter in
  shipped documentation and in bundled skills resolves to real behavior or is explicitly marked
  planned — verified by test, so the count cannot drift. Today at least eight identifiers across
  two surfaces do not, four of them from a fully processed spec. The per-server allowlist and its
  error code appear nowhere in shipped documentation.
- **SC-006**: Adding an unclassified write-capable tool fails the build, demonstrated by
  deliberately adding one.
- **SC-007**: An official release reports a version string equal to its tag, with no dirty
  suffix, for every artifact.
- **SC-008**: The original reporter can run all three of his published reproductions against a
  build and observe the documented behavior in each.
- **SC-009**: With writes on and the destructive tier off, all seven tools in the tier refuse and
  their targets survive. Today the key has no reader and all seven proceed.

## Assumptions

- The reporter's environment (macOS arm64, IRIS Community in Docker, Atelier over stdio) stays
  the reference configuration for verification.
- Existing users who rely on writes being allowed when nothing is declared keep working. The
  inferred default is unchanged; only its visibility improves.
- The destructive tier defaults to off. An operator who has never declared the key and relies on
  one of the seven tools will need to declare it. That break is the point of the tier, and it
  belongs in the release notes.
- Removing a documented control is acceptable when it was never implemented, provided the
  removal is called out in release notes and the spec stays open. Documentation promising
  enforcement that does not exist is worse than no documentation.
- Read-only tools are never gated. A gate that blocks inspection would push operators to
  disable it entirely.
- Specs 073 and 074 remain the design of record for their features; this spec does not redesign
  them, it decides whether to implement them now and fixes the integrity defects around them.

## Out of Scope

- Redesigning the per-server policy category system.
- The per-server write allowlist itself. Spec 074 stays open; this spec only removes its
  documentation and leaves the dispatch point able to host it.
- Per-tool granular gates beyond the write and destructive tiers.
- Approval prompts or interactive confirmation as an alternative to configuration.
- Audit logging of refused calls.
- Changing the production-namespace safety inference itself, beyond reporting when it is what
  decided the gate.
- Implementing the behavior behind the four unbacked identifiers inherited from spec 072. This
  spec corrects the documentation to match the code; whether the WebSocket timeout should be
  configurable and whether `stream_inspect` should honor a character limit are separate calls.
- Repository process controls — branch protection, tracking the quality-gate policy and
  constitution in git rather than leaving them untracked and machine-local, requiring a status
  field on every spec. The forensic review found all three missing and all three would have helped,
  but none is a change to this codebase's behavior and none belongs in a feature branch.
