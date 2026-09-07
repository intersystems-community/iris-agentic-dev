# Feature Specification: CLI tool discovery and a detector that sees every value set

**Feature Branch**: `114-tool-surface-cost`
**Created**: 2026-09-07
**Status**: Draft
**Input**: Benchmark thread — "I am having a hard time justifying iris-agentic-dev with these results" /
"how would you address the tool cost?"

## Why this exists

A model-comparison harness scores agents on ObjectScript tasks. The MCP tool surface costs
context on every turn before a single call happens, and on the authoring tasks that make up most
of the suite it buys nothing, because there is no live system state to introspect. That makes the
tool surface a pure cost regression on those tasks and a fair criticism.

Measured on this tree today, `tools/list` for the Merged toolset:

|                     | 1.3.2                 | after 113              |
| ------------------- | --------------------- | ---------------------- |
| whole payload       | 77,078 B (~21.4K tok) | 104,282 B (~29.0K tok) |
| `inputSchema` bytes | 35,195 B              | 62,399 B               |
| `description` bytes | 35,062 B              | 35,062 B               |
| names alone         | 1,387 B               | 1,387 B                |

Two facts follow. First, the surface can be discovered for 1.4 KB instead of 104 KB, so the
question is not how to shrink the payload but how to avoid sending all of it. Claude Code already
defers schemas client-side; a bash-only harness has no such mechanism and no way to ask. Second,
113 added 27 KB of structural parameter documentation and removed none of the prose that was
carrying the same facts — but the prose cannot be cut yet, because for eleven parameters it is
still the only copy of the contract.

`iris-agentic-dev tool <name> --args '{...}'` already dispatches all 82 tools at 40–60 ms per
call including process spawn, which is a tool surface at zero context cost. It has no discovery:
an agent must already know that `iris_query` exists and guess its argument names.

## User Scenarios & Testing _(mandatory)_

### User Story 1 - A bash-only agent finds and calls a tool (Priority: P1)

An agent with nothing but a shell asks the binary what tools exist, gets names and one-line
summaries for about 1.4 KB, picks one, asks for that tool's schema, and calls it. Nothing is
loaded into context that the agent did not ask for, and no IRIS connection is needed to discover.

**Why this priority**: it is the arm the benchmark is missing. Without it the harness can only
compare "no tools" against "all 82 tool schemas in every request", which is the comparison that
makes the tool surface look like pure cost. It is also the smallest rung on the ladder.

**Independent Test**: run `iris-agentic-dev tool --list` with no IRIS container running and no
config file; assert every registered tool name appears, that the output is under 8 KB, and that
the exit code is 0. Then run `tool <name> --schema` for a tool and assert the JSON matches what
`tools/list` serves over MCP for the same tool.

**Acceptance Scenarios**:

1. **Given** no IRIS connection is reachable, **When** `tool --list` runs, **Then** it prints
   every tool name with a one-line summary and exits 0.
2. **Given** no IRIS connection is reachable, **When** `tool iris_query --schema` runs, **Then**
   it prints that tool's description and `inputSchema` and exits 0.
3. **Given** `--json`, **When** either command runs, **Then** stdout is a single JSON document and
   nothing else, so a harness can parse it without stripping prose.
4. **Given** a misspelled tool name, **When** `tool iris_quer --schema` runs, **Then** the error
   names the closest registered tool and exits non-zero.
5. **Given** the MCP server and the CLI are the same binary, **When** `tool <name> --schema` and
   an MCP `tools/list` are compared for that tool, **Then** the two schemas are byte-identical.

---

### User Story 2 - Every fixed value set is advertised, and the gate proves it (Priority: P2)

Eleven dispatcher parameters spell out their value set in English and declare no `enum`:
`iris_info.what`, `iris_doc.mode`, `iris_admin.type`, `iris_debug.action`, `iris_global.action`,
`iris_macro.action`, `iris_production.action`, `iris_source_control.action`, `kb.action`,
`skill.action`, `skill_community.action`. The `prose-only-enum` detector reports zero. It reports
zero because it recognises only two prose shapes — `mode: a | b | c`, and a comma list introduced
by "one of" — and none of the eleven is written either way.

**Why this priority**: the constitution says this detector MUST stay at zero, and it is at zero
because it does not fire, which is the `self-referential-gates` class already in the Bug Class
Registry. It is also the precondition for the next feature: prose that duplicates a declared enum
can be deleted, prose that is the only copy of the contract cannot.

**Independent Test**: run the detector against a fixture holding each of the three prose shapes
and a handler that branches on a parameter with no declared set; assert one finding per case. Then
run it against the tree and assert zero findings only after the eleven are declared.

**Acceptance Scenarios**:

1. **Given** a tool description written as `what=documents lists all docs, what=modified lists…`,
   **When** the detector runs and the parameter declares no `enum`, **Then** it reports a finding.
2. **Given** a description written as `mode: get, put, delete` with no "one of" lead-in, **When**
   the detector runs, **Then** it reports a finding.
3. **Given** a handler that branches on a declared parameter with three or more string literal
   match arms and an error default, **When** the parameter advertises no `enum`, **Then** the
   detector reports a finding whatever the prose looks like.
4. **Given** a parameter whose value set is genuinely open, **When** its doc comment says why it is
   `not an enum`, **Then** the detector stays silent.
5. **Given** the eleven parameters now advertise their sets, **When** a value outside the set is
   sent, **Then** the runtime answer is unchanged from before this feature — declaring an `enum`
   in the schema does not move validation out of the handler.

---

### Edge Cases

- `tool --list` and `tool --schema` in the same invocation: `--list` wins and the name is ignored,
  or the parse is rejected. It must not silently discover nothing.
- A tool registered in the router but missing from `TOOL_NAMES` (the CLI's hand-maintained
  dispatch list): `--list` must not advertise a name the CLI cannot dispatch.
- Toolset selection: `--list` must reflect `IRIS_TOOLSET`, because a harness that scopes the
  toolset and then reads a Merged listing will call tools that are not there.
- A description that names a value set for a parameter the struct does not declare at all: that is
  a different bug with an existing test, and the detector must keep passing it through rather than
  double-reporting.
- An `enum` declared on a parameter whose handler accepts more than the declared set: advertising
  less than the handler accepts is a silent narrowing, so the declared set must come from the match
  arms, not from the prose.

## Requirements _(mandatory)_

### Functional Requirements

- **FR-001**: `iris-agentic-dev tool --list` MUST print every tool registered for the active
  toolset, one per line, with a one-line summary derived from the tool's description.
- **FR-002**: `tool --list` and `tool <name> --schema` MUST NOT require or attempt an IRIS
  connection. Discovery is a property of the binary, not of a server.
- **FR-003**: `iris-agentic-dev tool <name> --schema` MUST print the tool's description and its
  `inputSchema` exactly as `tools/list` serves them.
- **FR-004**: Both MUST support `--json`, emitting a single JSON document on stdout and nothing
  else.
- **FR-005**: `tool --list` output MUST stay under 8 KB for the Merged toolset, so a harness can
  hold the whole catalogue for roughly 2K tokens.
- **FR-006**: An unknown name passed to `--schema` MUST produce the nearest registered name, reusing
  the suggestion logic already behind `UNKNOWN_PARAMETER`.
- **FR-007**: `tool --list` MUST list only names the CLI can actually dispatch, and a test MUST fail
  if the router and the CLI dispatch list disagree.
- **FR-008**: The `prose-only-enum` detector MUST fire on repeated `param=value` prose, on a
  `param: a, b, c` comma list with no lead-in, and on a handler that branches on a declared
  parameter with three or more string literal arms — in addition to the two shapes it already reads.
- **FR-009**: Each of the eleven undeclared dispatcher parameters MUST either advertise its value
  set with `#[schemars(extend("enum" = [...]))]` or carry a doc comment saying why it is `not an
enum`.
- **FR-010**: Declaring these sets MUST NOT change any runtime answer. Value validation stays in
  the handler, which is where the current error messages come from.
- **FR-011**: The declared set for a parameter MUST match the values its handler branches on, and a
  test MUST compare the two rather than comparing the schema to the prose.

### Key Entities

- **Tool catalogue**: name, one-line summary, and `inputSchema` per registered tool, derived from
  the same `tool_router` the MCP transport serves so the two cannot disagree.
- **Value set**: a parameter's fixed list of accepted values, held in three places today — the
  handler's match arms (authoritative), the schema's `enum` (what clients read), and the
  description (what models read). This feature makes the first two agree and the third redundant.

## Success Criteria _(mandatory)_

### Measurable Outcomes

- **SC-001**: A bash-only agent can discover the whole tool surface for under 8 KB and one tool's
  full contract for under 2 KB, against a baseline of 104,282 B for the MCP listing.
- **SC-002**: `tool --list` and `tool <name> --schema` both succeed with no IRIS container running.
- **SC-003**: For every registered tool, the schema `--schema` prints is identical to the schema
  MCP `tools/list` serves.
- **SC-004**: The `prose-only-enum` detector reports at least eleven findings on this tree before
  the declarations land, and zero after — with a canary test per prose shape so that zero is
  evidence rather than an absence of evidence.
- **SC-005**: Every runtime answer for the eleven parameters is unchanged, shown by a before/after
  comparison on an invalid value for each.

## Assumptions

- Enum declaration is schema-only in this codebase (`#[schemars(extend(...))]` on a `String`
  field), so it cannot change deserialization behaviour. Confirmed today: `iris_interop_query`
  advertises `what` as an enum and an invalid value still returns the handler's `INVALID_ACTION`.
- The benchmark harness can shell out. If it cannot, US1 is still useful to human callers but does
  not unblock the eval arm.
- Cutting the 35 KB of tool-description prose is a separate feature. This one only removes the
  reason it cannot be cut.

## Out of scope

- The mechanical schema diet (dropping `$schema`, flattening `anyOf`/null unions): measured at
  −8.9% and worth doing, but it changes what every client receives and belongs in its own change.
- Task-scoped toolset profiles.
- A `search_tools` RAG endpoint.
- Editing tool descriptions.
