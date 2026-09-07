# Feature Specification: Typed input schemas for every MCP tool

**Feature Branch**: `113-typed-tool-schemas`
**Created**: 2026-09-06
**Status**: Draft
**Input**: User description: "Typed input schemas for every MCP tool: eliminate AnyParams."

## Context

An MCP client learns how to call a tool from that tool's advertised input schema: the
parameter names, their types, which are required, and — where the value set is fixed —
the permitted values. 31 of this server's 81 tools advertise nothing at all. They present
an open object, and the parameters they actually read appear only in the English prose
of the tool description.

Measured against the shipped 1.3.2 binary:

| Tool listing fact                              | Count |
| ---------------------------------------------- | ----- |
| Tools advertised                               | 81    |
| Tools advertising no parameters                | 37    |
| …of those, tools that do read named parameters | 31    |
| Parameter slots read but never advertised      | 132   |
| Distinct parameter names among them            | 71    |

Not one of the 31 is a genuine no-argument tool. The largest are `iris_admin` (26
parameters), `iris_interop_query` (15), and `iris_admin`'s peers in the administration
group. Eight or more tools carry a fixed value set — `mode`, `action`, `profile` — as a
pipe-separated list inside a sentence rather than as a declared set of permitted values.

Four of the 31 (`compare_document`, `compare_namespace`, `global_preview`, `global_kill`)
already have a complete typed parameter definition in the codebase that the tool listing
does not use. The gap for those four is wiring, not authorship.

### Why the existing guard does not catch it

The suite already has a test asserting that every documented parameter can actually be
passed. It passes today because it treats an unadvertised parameter surface as a case it
cannot answer, and falls back to searching the handler body for the parameter name. The
fallback's own comment records the reasoning: an open object "cannot answer the question."
So the suite enforces agreement between the documentation and the handler, and accepts the
missing advertisement as a given. Nothing enforces agreement between what a tool
advertises and what it reads.

That fallback caught one real bug and is the reason it is known: `stream_inspect` was
documented with a `max_chars` parameter that no code read, so a caller asking for 10,000
characters silently received the entire stream. The missing advertisement is the root
cause of that class of defect; the fallback is a partial net under it.

## Clarifications

### Session 2026-09-06

- Q: A caller passes a parameter no tool reads — today it is silently discarded. What should
  ship? → A: Reject with an error naming the unknown parameter and listing the ones the tool
  accepts. The silent discard is the failure mode this feature exists to fix, so preserving it
  would leave the defect half-fixed for any client that does not read the schema. Accepted
  cost: a caller passing a stray key that "works" today starts failing.

## User Scenarios & Testing _(mandatory)_

### User Story 1 - An agent can discover a tool's parameters without reading prose (Priority: P1)

An agent connects to the server, retrieves the tool listing, and needs to call
`iris_admin` or `iris_system_performance`. Today it must parse an English sentence to
learn that `mode` exists, guess that `run_id` is only meaningful for one mode, and hope it
spelled everything correctly. After this change the listing states each parameter, its
type, and whether it is required, for all 81 tools.

**Why this priority**: It is the whole defect. Every other story in this spec is either a
refinement of this one or a guard on it. Delivered alone, it takes the share of the tool
surface that is structurally self-describing from 62% to 100%.

**Independent Test**: Retrieve the tool listing from a freshly started server and assert
that no tool advertises an empty parameter set, and that each of the 31 tools advertises
exactly the parameter names its handler reads. Requires no IRIS instance.

**Acceptance Scenarios**:

1. **Given** a running server, **When** a client retrieves the tool listing, **Then** every
   tool advertises a non-empty parameter set, or takes no parameters at all.
2. **Given** a tool that reads a parameter, **When** its advertised parameters are compared
   against the names its handler reads, **Then** the two sets match exactly, with no name
   advertised but unread and none read but unadvertised.
3. **Given** any parameter that was accepted before this change, **When** a caller passes it
   after the change, **Then** it is still honored and the response is unchanged.
4. **Given** a tool whose parameter is optional, **When** a caller omits it, **Then** the
   call succeeds with the same default the tool applied before.
5. **Given** a caller misspells a parameter, **When** the call is made, **Then** it is
   rejected with the misspelled name and the list of names the tool accepts, and no part of
   the tool's work has run.

---

### User Story 2 - Fixed value sets are stated, not described (Priority: P2)

Eight or more tools accept a parameter with a closed set of valid values. A caller
choosing among them today reads a pipe-separated list out of a sentence. After this change
the permitted values are part of the tool's advertised contract, so a client can offer
them as choices and reject a wrong one before a call is made.

**Why this priority**: It is the difference between a discoverable tool and a guessable
one, and it is where agents most often fail — inventing a plausible-sounding mode. It
depends on Story 1 being done for the tool in question, so it follows rather than leads.

**Independent Test**: For each tool with a closed value set, assert the advertised
permitted values match the values the handler actually branches on. Comparing against the
description prose is not sufficient and does not satisfy this test.

**Acceptance Scenarios**:

1. **Given** a tool with a closed value set, **When** a client reads its advertised
   contract, **Then** the permitted values are enumerated there.
2. **Given** a value the handler does not branch on, **When** a caller passes it, **Then**
   the response names the parameter and lists the permitted values.
3. **Given** the handler gains or loses a branch, **When** the suite runs, **Then** a test
   fails unless the advertised value set was updated to match.

---

### User Story 3 - A documented parameter that nothing reads cannot survive (Priority: P2)

Converting each tool forces a comparison between three things that have drifted: what the
docs promise, what the tool advertises, and what the handler reads. Every disagreement
found is resolved rather than carried forward — the parameter is either implemented or
removed from the documentation.

**Why this priority**: This is the `max_chars` class of defect, and it is the one that
silently returns wrong results to a caller who did everything right. It is P2 rather than
P1 only because the count of surviving instances is unknown until Story 1 begins.

**Independent Test**: Enumerate every disagreement between documented, advertised, and
read parameters; assert the list is empty at completion. The audit output is a deliverable
in its own right.

**Acceptance Scenarios**:

1. **Given** a parameter promised in the docs, **When** the suite runs, **Then** it is
   advertised by the tool and read by the handler, or the promise is gone from the docs.
2. **Given** a parameter the handler reads, **When** the docs are checked, **Then** the
   parameter appears in the tool's documented reference table.
3. **Given** a disagreement is found during conversion, **When** it is resolved, **Then**
   the resolution is recorded with which of the three sources was wrong.

---

### User Story 4 - The next tool cannot reintroduce the gap (Priority: P3)

Tool 82 is added six months from now. Nothing in the current suite or gates would notice
if it advertised an open object, because that is how 31 tools behave today and the suite
accommodates them.

**Why this priority**: It protects the work rather than delivering it, so it lands last —
but if it is skipped, the gap returns and this spec gets rewritten in a year.

**Independent Test**: Add a tool advertising an open parameter set on a scratch branch and
confirm the suite and the gate both fail. Remove it and confirm both pass.

**Acceptance Scenarios**:

1. **Given** a new tool advertising no parameters while reading some, **When** the suite
   runs, **Then** it fails and names the tool.
2. **Given** the same tool, **When** the antipattern gate runs, **Then** it reports the
   occurrence and exits non-zero.
3. **Given** the last tool with an open parameter set is converted, **When** the suite
   runs, **Then** the fallback path in the documentation contract test is gone and its
   absence is asserted, so it cannot be reintroduced as a permanent exemption.

---

### Edge Cases

- **A parameter is read but was never documented.** The reverse of `max_chars`: the handler
  honors something no caller knows exists. Advertising it makes it discoverable; the
  alternative is deleting a capability someone may depend on. Advertise and document it.
- **Required parameters depend on another parameter's value.** `iris_admin` reads 26
  parameters, and which are meaningful depends entirely on `action`; `iris_system_performance`
  needs `run_id` only for one mode. A flat contract cannot express "required when action is
  X" and must not mark those parameters unconditionally required, or existing valid calls
  start failing.
- **A caller sends a value of the wrong type.** Parameters read as text today may be receiving
  numbers, or numbers arriving as text, from callers that work fine. Declaring a type must not
  turn a call that currently works into a failure. Nine integer parameters are the stated
  exception: `{"limit": "100"}` succeeds today and silently uses the default, because the
  string fails the integer accessor. Declaring them numeric rejects it. The two parameters that
  genuinely parse both forms — `iris_interop_query.session_id` and `.since_id` — keep accepting
  both and advertise both.
- **A caller sends a parameter no tool reads.** Today it is silently discarded, so a typo'd
  or renamed parameter produces a plausible-looking wrong answer. It is now rejected with the
  offending name and the accepted names (FR-015). This is the one place the feature changes
  runtime behavior deliberately, and it is the change that makes the schema worth having:
  without it, a client that ignores the schema keeps getting silent wrong answers.
- **A caller sends an extra parameter that used to be tolerated.** The direct cost of
  rejecting unknown parameters. A call that "worked" while quietly dropping a key now fails —
  which is the correct outcome, because the dropped key means the caller and the tool
  disagreed about what was being asked. The error must say enough to fix the call in one
  attempt.
- **A parameter is rejected on a destructive tool.** Rejecting must happen before any work
  starts, or a mis-specified `global_kill` could act on the part it understood.
- **A tool genuinely takes no parameters.** None of the 31 qualify, but future tools might.
  Advertising an empty parameter set must remain expressible and distinguishable from
  advertising nothing because no one got around to it.
- **The universal `server` parameter.** Nearly every tool accepts it to route to a named
  instance. It must be advertised consistently rather than described 31 different ways.

## Requirements _(mandatory)_

### Functional Requirements

- **FR-001**: Every tool MUST advertise each parameter it reads, with that parameter's type
  and whether it is required.
- **FR-002**: No tool MAY advertise an open or unspecified parameter surface. A tool taking
  no parameters MUST advertise an empty set explicitly.
- **FR-003**: The set of parameters a tool advertises MUST equal the set its handler reads —
  no advertised-but-unread names, no read-but-unadvertised names.
- **FR-004**: Every parameter accepted before this change MUST still be accepted, with the
  same effect and the same default when omitted.
- **FR-005**: Where a parameter's valid values form a closed set, the tool MUST advertise
  that set, and the advertised set MUST equal the values the handler branches on.
- **FR-006**: A rejected value for such a parameter MUST produce a response naming the
  parameter and listing the permitted values.
- **FR-007**: Parameters whose necessity depends on another parameter's value MUST NOT be
  advertised as unconditionally required.
- **FR-008**: Type declarations MUST NOT reject values the tool **honors** today. A value the
  tool currently accepts and then discards is not honored, and declaring its type correctly
  turns that silent discard into a rejection.
- **FR-009**: Every disagreement among documented, advertised, and read parameters MUST be
  resolved before completion, and the resolution recorded.
- **FR-010**: The suite MUST fail if any tool advertises no parameters while reading some.
- **FR-011**: The gate suite MUST report any newly introduced open parameter surface and
  exit non-zero.
- **FR-012**: The documentation contract test MUST stop accepting an open parameter surface
  as an unanswerable case, and MUST assert that its former fallback path is gone.
- **FR-013**: Per tool, the advertised contract MUST be verified against the listing a
  running server actually emits, not against a definition inspected in isolation.
- **FR-014**: Per tool, every advertised parameter MUST be exercised through a real call
  proving it is still honored at runtime; tools that reach IRIS MUST be verified against a
  live instance.
- **FR-015**: A call carrying a parameter the tool does not read MUST be rejected rather than
  partially honored, and the rejection MUST name the offending parameter and list the
  parameters the tool does accept.
- **FR-016**: A rejection under FR-015 MUST NOT execute any part of the tool's work, so a
  write or destructive tool cannot half-apply a call that was mis-specified.
- **FR-017**: The rejection MUST be distinguishable from a value-level rejection (FR-006), so
  a caller can tell "you sent a name I do not know" from "you sent a value I do not permit."

### Key Entities

- **Tool**: A named capability the server advertises. Has a description, a parameter
  contract, and a handler. 81 exist; 31 have an empty parameter contract.
- **Parameter contract**: The advertised set of parameter names, types, optionality, and
  permitted values for one tool. The artifact this feature creates where it is missing.
- **Parameter**: One named input. Has a name, a type, an optionality, sometimes a closed
  value set, and sometimes a dependency on another parameter's value.
- **Parameter audit**: The three-way comparison of documented, advertised, and read
  parameters for one tool. Produces the disagreement list that FR-009 requires be emptied.

## Success Criteria _(mandatory)_

### Measurable Outcomes

- **SC-001**: 100% of advertised tools state their parameters — up from 62% (50 of 81).
- **SC-002**: All 132 currently-unadvertised parameter slots — 71 distinct names — are
  advertised by the tool that reads them.
- **SC-003**: Zero disagreements remain among documented, advertised, and read parameters,
  measured across all 81 tools.
- **SC-004**: Every tool with a closed value set advertises it; the count of value sets
  living only in prose reaches zero.
- **SC-005**: Every call that uses only advertised parameters with permitted values behaves
  exactly as before — same result, same defaults when a parameter is omitted — demonstrated by a
  runtime round-trip per advertised parameter. A call carrying an unadvertised key is the one
  deliberate exception (FR-015).
- **SC-006**: A tool introduced with an unstated parameter surface is rejected by both the
  suite and the gates, demonstrated by adding one and observing two failures.
- **SC-007**: The advertised contract alone is sufficient to construct a valid call to each of
  the 31 converted tools: for every converted tool, a call assembled from nothing but its
  advertised parameter names, types, and permitted values — with the description prose
  withheld — is accepted.
- **SC-008**: A misspelled parameter is caught on the first call rather than producing a
  wrong-but-plausible result, and the error alone is enough to correct the call without
  consulting the docs.

## Assumptions

- **Behavior is preserved, not redesigned, with one deliberate exception.** This changes what
  tools say about themselves, not what they do. Parameter names, defaults, and semantics stay
  as they are; renaming or removing a parameter is out of scope even where the current name is
  poor. Two behavior changes are intended, both deliberate: FR-015, where unknown parameters
  stop being silently discarded, and the nine integer parameters below, where a numeric string
  stops being discarded in favor of the default and is rejected instead.
- **Conditional requirements are expressed as optional plus a documented rule.** For
  `iris_admin` and its peers, every action-dependent parameter is advertised optional and
  the per-action requirements stay in the documented reference tables. Modelling each action
  as a separate variant would change the tool's shape and is out of scope.
- **The four tools with existing unused definitions go first.** `compare_document`,
  `compare_namespace`, `global_preview`, and `global_kill` need wiring only, so they
  establish the pattern and the test shape at the lowest risk.
- **Numbers may arrive as text only where they already do.** Two parameters accept both an
  integer and a numeric string today; those keep accepting both and advertise both. The rest
  already discard a string where a number is expected, so declaring them numeric changes a
  silent discard into a rejection rather than breaking a working call.
- **The universal `server` parameter is advertised identically everywhere**, including its
  meaning and its optionality, so 31 tools do not produce 31 phrasings.
- **Tool count is 81 as of version 1.3.2.** Tools added while this work is in flight are
  subject to the same requirements; the counts in Success Criteria are floors, not fixed
  targets.

## Dependencies

- **112-antipattern-gates** — `scripts/gates/antipatterns.py` and its never-baselined class
  list must exist before the `undeclared-params` and `prose-only-enum` detectors can be added.
- **085-write-gate-integrity** — puts the gate check on raw arguments in `call_tool` ahead of
  the router. FR-016's ordering guarantee and the gate-wins-over-parameter-rejection rule are
  both properties of that placement, not of this feature.

Neither is blocking: both are shipped. Listed because the tests this feature adds fail if
either is reverted.

## Out of Scope

- Output schema changes. This feature is about inputs only.
- Renaming or removing any existing parameter, or changing any default.
- Adding tool capability of any kind.
- The SystemPerformance profile-management gap — listing profiles, creating one, and
  retrieving a finished report. That is a separate follow-on spec, kept separate so this one
  stays a pure contract change.
- Rewriting tool description prose for style. Descriptions change only where they promised a
  parameter that does not exist, or described a value set now advertised structurally.
