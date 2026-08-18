# Research Spike: rmcp 1.6.0 → 3.0.1 Upgrade

**Spec**: 076-interface-modernization, User Story 5 (FR-006, SC-006)
**Status**: Complete — reviewed, explicit go/no-go below
**Scope**: A written recommendation, not code. No dependency version, `Cargo.toml`, or transport code changes in this document or the commit that adds it.

## Question being answered

Should this project upgrade `rmcp` from 1.6.0 (currently resolved) to 3.0.1, to move toward
2026-07-28 MCP spec compliance — and specifically, does the SCM-checkout elicitation flow
(`elicitation.rs`, used by `iris_doc`/`iris_source_control`) need to be re-architected around
the 2026-07-28 MRTR (`InputRequiredResult`/`resultType`) pattern to make that upgrade possible?

## Methodology

Two sources, kept separate below so a reader can weight them differently:

1. **Direct reading of this codebase** — every file that imports `rmcp::` was read, not
   sampled. This is the authoritative half of the spike: what actually depends on rmcp
   internals, today, in this project.
2. **rmcp SDK research** — `github.com`, `crates.io`, and `docs.rs` were reachable this
   session (unlike the network-egress caveat noted in this spec's own Research Findings
   section for several other domains), but direct fetches of the rust-sdk repo's specific
   pages (`CHANGELOG.md`, the `discussions/969` migration thread, GitHub releases) returned
   404s or empty shells rather than usable content — the same shallow-fetch failure mode as
   before, just on different domains this time. Findings below sourced from rmcp SDK
   behavior are therefore via **WebSearch's synthesized snippets**, which themselves quote
   and cite `github.com/modelcontextprotocol/rust-sdk` — same caveat as the rest of this
   spec's Research Findings: verify against the primary source before treating specific
   wording as verbatim, especially the exact rmcp point-release that first ships a
   `ProtocolVersion::V_2026_07_28` variant (see Finding 5).

## Findings

### 1. The upgrade's actual blast radius in this codebase is four files, not "every transport-facing code path"

A full-text search for `rmcp::` outside `tests/` turns up exactly:

| File | What it touches | Upgrade-sensitive? |
|---|---|---|
| `tools/mod.rs` | The `impl ServerHandler for IrisTools` block — `get_info`, `call_tool`, `list_tools` (the only three methods manually overridden; no `get_prompt`/`read_resource`, since this server declares no prompts/resources capability) | Yes — see Finding 3 |
| `iris/ws_session.rs` | `rmcp::ErrorData` as an error-type alias only | No — generic type, stable across versions |
| `iris/connection_pool.rs` | `rmcp::ErrorData` as an error-type alias only | No — same as above |
| `cmd/mcp.rs` (bin crate) | `rmcp::transport::stdio`, `rmcp::ServiceExt::serve(stdio())` | Low — no manual protocol-version pinning, no manual handshake code; whatever the linked rmcp version negotiates is what runs, by construction |

`get_info()` declares exactly one capability — `ServerCapabilities::builder().enable_tools().build()`.
No prompts, resources, logging, or sampling capability is declared anywhere in this codebase.

**This means the 2026-07-28 changelog's "Roots, Sampling, and Logging scheduled for removal"
concern, and the `ping`/`logging/setLevel` RPC removal, do not apply to this project at all** —
not because of some mitigation, but because this server never used any of them. The general
framing in this spec's Research Findings ("the general blast radius of a stateless redesign
touching every transport-facing code path") turns out to describe the *ecosystem*, not *this*
codebase's actual exposure, once read directly.

### 2. The elicitation-flow migration risk was overstated — this project's elicitation is not rmcp's elicitation

This is the most important finding in this spike, and it reverses this spec's own prior
assumption.

`elicitation.rs` (472 lines, read in full) implements `ElicitationStore`: a plain
`Arc<Mutex<HashMap<String, PendingElicitation>>>`, keyed by a locally-generated UUID
(`Uuid::new_v4()`), with a 5-minute TTL swept on access. `insert`/`lookup`/`clear`/`sweep` are
its entire public API. **It contains zero references to any rmcp type.** A repo-wide search
for `CreateElicitation`, `create_elicitation`, `ElicitationCapability`, or any use of rmcp's
actual `elicitation/create` protocol RPC (confirmed present in rmcp 1.6.0's `model.rs` as
`ElicitationCreateRequestMethod`, `ElicitationResponseNotificationMethod`,
`ElicitationCompletionNotificationMethod`) returns nothing in this project's own source.

What `iris_doc`/`iris_source_control` actually do: a write that needs SCM checkout returns an
**ordinary, complete `CallToolResult`** whose JSON body happens to contain
`{"elicitation_required": true, "elicitation_id": "...", "message": "...", "options": [...]}`.
The caller resumes by calling the **same tool again** with `elicitation_id` + an answer
parameter. This is a hand-rolled, tool-parameter-level convention this project designed
itself — not an invocation of MCP's protocol-level elicitation feature. This spec's own
Research Findings already noted this in passing ("this project does not use rmcp's actual
protocol-level elicitation capability... That's a deliberate, sound design for MCP clients")
without following the observation to its conclusion for the upgrade question specifically:
**a protocol revision reshaping a feature this project never called cannot break this
project's use of that feature, because there is no use to break.**

The 2026-07-28 MRTR pattern (`InputRequiredResult` with `resultType: "input_required"` instead
of a separate follow-up notification) is, structurally, the *same shape* this project already
built for its own reasons (a CLI invocation has no persistent session to hold protocol-level
elicitation state in) — call, get told more input is needed, call again with the answer. This
project arrived at that shape independently, years earlier, for a different reason (statelessness
of a CLI process, not statelessness of HTTP). Upgrading rmcp does not force a migration onto
this shape; this project is already living in it.

**Answering FR-006's specific acceptance criterion directly: no, the SCM-checkout elicitation
flow does not need to be migrated to `InputRequiredResult`/`resultType` to unblock the
upgrade, and zero lines of `elicitation.rs` need to change for the upgrade itself to succeed.**
Adopting the formal `resultType`/`InputRequiredResult` pattern *in addition* to (or instead of)
the current hand-rolled one remains available as a future, purely optional enhancement — not
a migration this upgrade requires — see the itemized estimate in "If the optional MRTR
adoption is later wanted" below for what that would look like if a maintainer chooses to do it
anyway, for its own benefits (a standard client can render an in-protocol prompt UI instead of
parsing a tool's JSON body for a magic field).

### 3. One concrete, mechanical Rust-API break: `call_tool`'s manual override

Per the migration guidance found (`discussions/969`, via WebSearch synthesis): manual
`ServerHandler` implementations must change `call_tool`/`get_prompt`/`read_resource` return
types to `CallToolResponse`/`GetPromptResponse`/`ReadResourceResponse` respectively, wrapping
the existing result with `.into()` — explicitly **not required for macro-generated dispatch**,
only hand-written trait method bodies.

This project's `call_tool` override (`tools/mod.rs`) is hand-written (it wraps the
macro-generated `tool_router.call()` dispatch in a `CALL_START` task-local for latency
tracking — see its own doc comment) and currently returns `Result<CallToolResult, McpError>`.
This is affected: the fix is a signature change to whatever the 3.x equivalent return type is,
plus wrapping the tool-router's result with `.into()` before returning it. Small, mechanical,
and — critically — testable immediately by the existing full test suite (nothing about
*what* `call_tool` does changes, only its return type's name).

`list_tools`'s override was not specifically named in the migration guidance found. Given the
same SDK's general pattern of renaming paginated result types, treat this as an open item to
verify directly against the 3.x source at upgrade time, not a confirmed non-issue — the fix,
if one is needed, is very likely the same mechanical shape as `call_tool`'s.

Two other potential breaking changes were checked directly against this codebase's source and
found **absent, not merely unlikely**:
- Singular `*RequestParam` aliases (deprecated, removed in a 3.x beta per the migration
  guidance) — this project already uses only the plural `*RequestParams` forms throughout
  (confirmed: `CallToolRequestParams`, `PaginatedRequestParams`).
- Matches on the deprecated `ServerInitializeError`/`LocalSessionWorkerError` variants
  (removed the same way) — zero occurrences in this codebase.
- `StreamableHttpService`'s trait-bound change (`S: ServerHandler` instead of
  `S: Service<RoleServer>`) — irrelevant; this project only ever calls `.serve(stdio())`, no
  HTTP transport exists in this codebase at all.

### 4. The `rust-version = "1.88"` requirement is already satisfied

rmcp 3.x's workspace declares `rust-version = "1.88"` (via the same migration guidance). This
project's toolchain (`rust-toolchain.toml`) pins `1.92.0`. Not a blocker, no toolchain bump
needed alongside the dependency bump.

### 5. Upgrading to 3.0.1 does not, by itself, reach 2026-07-28 — this corrects an assumption in this spec's own Research Findings

This spec's Research Findings section states that because this project never calls
`.with_protocol_version(...)` and lets `get_info()` default to `ProtocolVersion::LATEST`, "a
bump would move the negotiated default forward automatically, no version-matrix code to
maintain on our side." Two independent search results surfaced during this spike complicate
that claim:

- One synthesis: *"As of 3.0.1, `ProtocolVersion::LATEST` is still `2025-11-25`, not the newer
  2026-07-28 standard."*
- Another synthesis: *"rmcp 3.0 adds explicit 2026-07-28 support"* via a
  `ProtocolVersion::V_2026_07_28` variant.

Read together (and this is inference, not a directly confirmed fact — flagged per this spike's
own methodology caveat above), the coherent explanation is that rmcp 3.x makes
`V_2026_07_28` an available, explicitly-selectable variant without making it the *default*
`LATEST` — a conservative SDK stance (don't auto-negotiate a three-week-old revision as the
default) rather than a contradiction. **If that reading is correct, reaching actual
2026-07-28 compliance requires this project to add one explicit line —
`.with_protocol_version(ProtocolVersion::V_2026_07_28)` in `get_info()` — on top of the
dependency bump, not the automatic default-forward this spec originally assumed.** That's a
smaller gap than "not automatic at all," but it is a real correction: **verify directly
against the rmcp 3.0.1 source (or whichever point release is actually targeted) which
`ProtocolVersion` variants it defines and what `LATEST` resolves to, before writing the
follow-up spec** — don't carry either search snippet forward as settled fact.

## Go / No-Go Recommendation

**Conditional GO.** Recommend scheduling the rmcp 1.6.0 → 3.x upgrade as its own small,
scoped piece of work — separate from any elicitation redesign, because none is required —
gated on three pre-flight steps a follow-up spec should treat as its actual first tasks, not
assumptions to inherit from this spike:

1. **Pin the exact target version directly from the rmcp source**, not from this spike's
   WebSearch-synthesized findings — confirm which `ProtocolVersion` variants exist and what
   `LATEST` resolves to in that exact version (Finding 5).
2. **Fix `call_tool`'s (and, if needed, `list_tools`'s) manual return type** per Finding 3 —
   mechanical, and the existing test suite (including `tests/mcp_handshake.rs`'s e2e
   assertions) is sufficient regression coverage for this change specifically.
3. **Decide, as an explicit product decision, whether to opt into `V_2026_07_28` via
   `.with_protocol_version(...)` or stay on whatever `LATEST` negotiates** — this is not a
   technical fait accompli the upgrade forces; it's a choice the upgrade merely makes
   *available*, and it deserves the maintainer's explicit sign-off, not a default inherited
   silently from picking a dependency version.

**What tips this to GO rather than NO-GO or DEFER**: every concrete risk this spec originally
flagged, checked directly against this codebase's actual source rather than assumed from
general SDK-upgrade experience, turned out smaller than expected — the elicitation flow needs
no change at all (Finding 2), the capability surface this server declares is minimal enough
that most of the changelog's removals don't apply (Finding 1), and the one confirmed
Rust-API break is a single mechanical signature fix with existing regression coverage
(Finding 3). The upgrade's cost, once actually investigated rather than estimated, is much
closer to "a scoped, low-risk dependency bump plus one small fix" than "a stateless redesign
touching every transport-facing code path."

**What would have tipped this to NO-GO**, for the record (none of these held up under direct
inspection): the elicitation flow genuinely depending on `elicitation/create`/
`notifications/elicitation/complete` (it doesn't); this project actively using
Roots/Sampling/Logging capabilities (it doesn't); a `call_tool`/`list_tools` break serious
enough to require redesigning telemetry timing or tool dispatch, not just a return-type
rename (it isn't — the `CALL_START` task-local wrapper is unaffected structurally); an
unsatisfiable toolchain requirement (satisfied, with margin).

## If the optional MRTR adoption is later wanted (not required by this upgrade)

Not part of this upgrade's scope, but since Acceptance Scenario 1 asks how much of
`elicitation.rs` a migration would touch, here's the honest estimate if a maintainer later
chooses to adopt `InputRequiredResult`/`resultType` for its own sake (a standard MCP client
rendering a native prompt UI instead of a tool parsing JSON for `elicitation_required`):

- `ElicitationStore`/`CheckoutCache`/`PendingElicitation` (the in-memory state itself): **no
  change** — MRTR changes how the *pending* state is signaled to the client, not how it's
  held server-side.
- The three call sites that currently build the `{"elicitation_required": true, ...}` JSON
  by hand (`tools/doc.rs`'s `write_with_scm`, `tools/scm.rs`'s `checkout`/`execute` action
  branches): each would change to return an `InputRequiredResult`-shaped response instead of
  embedding the signal in an ordinary `CallToolResult`'s JSON body — a handful of call sites,
  not a rearchitecture.
- The resume path (checking `elicitation_id`+answer at the top of `handle_iris_doc`/
  `handle_iris_source_control` before normal mode dispatch): the *lookup* logic is unchanged;
  only how the *original* call signaled "waiting for you" changes.
- Rough sizing: **low tens of lines across 2–3 files**, not the file-spanning rewrite "a
  stateless redesign touching every transport-facing code path" implied before this was
  actually read. Optional, not urgent, and cleanly separable from the upgrade itself.

## Fallback if a future maintainer disagrees with this GO

This spike's own recommendation could still be overridden (e.g., competing priorities, or a
maintainer weighing the "verify against primary source" caveats in Finding 5 more
conservatively than this document does). If the upgrade is deferred rather than scheduled:
this project's own architecture is not otherwise at risk of falling further behind in a way
that compounds — Finding 1 shows the actual rmcp-touching surface is small and contained
(four files), so deferring costs mostly "stay on 1.6.0's negotiated 2025-11-25 default a while
longer," not an accruing migration debt. The MCP deprecation notices for Roots/Sampling/Logging
don't threaten this project either way, since none are used. Revisit this spike's findings
(not necessarily redo the research) whenever CLI/MCP client feedback specifically asks for a
2026-07-28-only feature this project doesn't yet have — that's a concrete trigger to
re-open scheduling, rather than a calendar date.
