# Feature Specification: Interface Modernization — Protocol Currency, CLI Parity, Progressive Disclosure

**Feature Branch**: `076-interface-modernization`
**Created**: 2026-08-16
**Status**: In Progress — User Stories 2 (P2) and 3 (P3) delivered; User Stories 1, 4, 5 not started
**Input**: User description: "Keep some things in view while we spec this: (1) MCP protocol has been updated, look into what benefits that could give us; (2) full CLI/MCP parity so iad could be used fully as just CLI tools — how much work would that be, and would it help with progressive disclosure given we now have ~90 tools; (3) research MCP alternatives — structured tool calling, and 'code mode' (models write code that calls tools instead of emitting JSON tool-call blocks)."

This spec is deliberately research-heavy: three of its four threads are genuinely new information (MCP shipped a major spec revision three weeks before this was written), and the right amount of committed work depends on getting the facts right before scoping effort. Every claim below is sourced; where a source was unreachable through this session's network egress, that's noted rather than guessed around.

## Background: what changed, and why it's all one spec

These four threads showed up together because they're the same underlying tension: **iris-agentic-dev's tool count (81 in Baseline, 90 total across tiers) has outgrown "send the whole catalog every time," and the wider ecosystem is mid-pivot on exactly that problem** — from three independent directions that turn out to reinforce each other:

1. **MCP itself just went stateless.** The 2026-07-28 spec revision (three weeks old at time of writing) removes the `initialize` handshake and protocol-level sessions entirely, in favor of self-contained requests and explicit server-minted handles for any cross-call state. That's the same shape as iris-agentic-dev's *existing* `session_state`/`session_token` design (see Dependencies) — the protocol moved toward this project's architecture, not away from it.
2. **Anthropic and Cloudflare both shipped "don't load 90 tool schemas up front" as a *client-side* pattern** (Tool Search Tool, Programmatic Tool Calling, Code Mode) — and confirmed, in the MCP maintainers' own discussion of the pattern, that it requires **no protocol-level server changes**. The leverage point for a server is making itself a good target for these patterns, not implementing them itself.
3. **A CLI that has full parity with the MCP tool surface *is* a code-mode-compatible substrate**, for free: any agent that can shell out can already "write code that calls tools" against iris-agentic-dev without anyone building a code-mode gateway for it specifically.

So: protocol currency, CLI parity, and progressive disclosure aren't three separate asks — CLI parity is the one piece of committed work that pays into all three.

## Research Findings

### 1. MCP protocol version history and what's actually new

| Revision | Date | Headline changes |
|---|---|---|
| Initial | 2024-11-05 | Baseline protocol |
| — | 2025-03-26 | OAuth 2.1 auth framework, tool annotations (`read_only_hint`/`destructive_hint` — **already used throughout this project**), audio content type |
| — | 2025-06-18 | Structured tool output (`outputSchema`), elicitation (server-initiated user input mid-call — **already used for SCM checkout dialogs**), removed JSON-RPC batching, `MCP-Protocol-Version` header required |
| — | 2025-11-25 | OpenID Connect Discovery, icons metadata, incremental scope consent, URL-mode elicitation, sampling tool-calling support, experimental Tasks (`SEP-1319`) |
| **Current** | **2026-07-28** | **Stateless redesign** — see below |

The 2026-07-28 revision is not incremental. Confirmed changes (sourced from the spec's own changelog):

- **Protocol-level sessions removed entirely** — no more `Mcp-Session-Id` header, no `initialize`/`notifications/initialized` handshake. Every request is a single, self-contained HTTP POST carrying its own protocol version and client capabilities in `_meta`.
- **New `server/discover` RPC** replaces the handshake for capability advertisement.
- **`ping` and `logging/setLevel` removed** — log level now set per-request via `_meta`.
- **Elicitation reshaped**: `notifications/elicitation/complete` removed. A new "Multi Round-Trip Requests" (MRTR) pattern replaces it — servers return `InputRequiredResult` (with a now-required `resultType` field: `"complete"` or `"input_required"`) instead of a separate follow-up request.
- **Tasks moved out of core** into an official extension (`io.modelcontextprotocol/tasks`) rather than being core-protocol.
- **Caching support added**: `CacheableResult` with `ttlMs`/`cacheScope`, and deterministic tool ordering is now required specifically *to make list-tools caching possible*.
- **Roots, Sampling, and Logging scheduled for removal** in a future revision.
- Resource-not-found error code changed (`-32002` → `-32602`); `inputSchema`/`outputSchema`/`structuredContent` validation loosened, not tightened.

**Why this matters here specifically**: the removed-session, explicit-handle model is *already* how `iris_execute`'s `%ctx` session carrier and the `iris_ws_open`/`iris_ws_exec`/`iris_ws_close` trio work — a server-minted opaque token, round-tripped by the caller as an ordinary argument, with no protocol-level session underneath. This project built that pattern for its own reasons (a CLI invocation has no persistent process to hold a session in) years before the protocol converged on the same shape for different reasons (stateless HTTP scaling). That's a real asset going into any future rmcp upgrade, not a liability.

**The elicitation reshape is the one piece of real migration risk.** The SCM-checkout elicitation flow (`elicitation.rs`, used by `iris_doc`/`iris_source_control`) is built on the 2025-06-18 elicitation model, and the 2026-07-28 changelog explicitly removes a notification that model depended on. Upgrading rmcp far enough to reach 2026-07-28 would very likely require re-architecting that flow around `InputRequiredResult`/`resultType`, not just a dependency bump.

### 2. rmcp (the Rust MCP SDK this project depends on) is two major versions behind

- `Cargo.toml` requires `rmcp = "1.4"`; `Cargo.lock` resolves that to **rmcp 1.6.0**. This project doesn't hard-pin an exact version — it accepts any compatible 1.x — but 1.x itself is now two majors behind.
- Current published version is **rmcp 3.0.1**, which targets 2026-07-28 while remaining compatible with 2025-11-25 and earlier.
- This project never calls `.with_protocol_version(...)` — `get_info()` builds `ServerInfo` and lets rmcp default to `ProtocolVersion::LATEST`, which in 1.6.0 resolves to `V_2025_11_25` (confirmed in the vendored source; the enum has no `2026-07-28` variant at all in this version). That's a real point in favor of eventually upgrading: this project isn't pinned to an old protocol version by explicit design, only by dependency version — a bump would move the negotiated default forward automatically, no version-matrix code to maintain on our side. (Every synthetic `initialize` request in this project's own e2e test fixtures declares `"protocolVersion":"2024-11-05"` — the *client* side of a test harness declaring the oldest revision it can, which says nothing about what the server actually negotiates back; don't confuse the two when reading those tests.)
- rmcp 1.6.0 **already supports declaring `output_schema` on a tool** (`Tool::with_output_schema<T: JsonSchema>()`) — confirmed in the vendored source. **This project uses it on zero of its 90 tools**, despite nearly every tool already returning a well-structured `serde_json::json!({...})` object. Declaring output schemas is available today, on the current dependency version, with no upgrade required — the gap is that nobody has done it, not that the SDK can't.
- Going from 1.6.0 → 3.0.1 is a two-major-version jump. Given the elicitation-flow risk above and the general blast radius of a stateless redesign touching every transport-facing code path, this deserves a dedicated research spike (see FR-006) before being scheduled as a real upgrade — not a "bump the version and see what breaks" change.

### 3. Progressive disclosure: this project already has one kind, and needs a different kind

Searching the codebase for "progressive disclosure" surfaces `027-progressive-disclosure` (folded into spec `032-iris-test-http`, implemented in `log_store.rs`) — but that's **result-size** progressive disclosure: large tool *outputs* get truncated with `truncated: true`, and `iris_get_log` fetches the full thing on demand. It says nothing about the *tool catalog* itself.

There is currently no mechanism for **catalog-size** progressive disclosure — every MCP client that connects gets the full Baseline (81) or Merged (78) tool list, with full schemas and descriptions, every time, with no caching (2026-07-28 supports `ttlMs`/`cacheScope` on results, but nothing in this codebase sets it) and no on-demand discovery. `crates/iris-agentic-dev-core/src/tools/mod.rs`'s `list_tools` override sets `next_cursor: None` unconditionally — the type supports MCP's pagination cursor, this server just never uses it.

A rough, self-measured cost estimate (not Anthropic's cross-vendor number, which mixes tools of unknown verbosity from five unrelated services): `docs/tools.md` — which documents this project's own tool surface at similar density to what ships in the real JSON schemas — is ~81KB, or roughly **15–25K tokens** by a rough 4-chars/token estimate. That's the order of magnitude paid on *every* new conversation with an MCP client that doesn't do its own tool-catalog pruning, before anything happens.

Two independent levers exist, and they're not mutually exclusive:

- **Server-side**: real cursor-based `list_tools` pagination (this project already has a `Toolset`-and-allowlist-pruned router to paginate *over* — the 075 work makes this strictly easier, not harder, since the effective tool set is already computed before `list_tools` runs).
- **Client-side, and requiring nothing from this server**: Anthropic's Tool Search Tool (`tool_search_tool_bm25_20251119`/`_regex_20251119`) lets a model discover tools on demand instead of front-loading the catalog — this already works against *any* MCP server, including this one, unmodified, the moment a client enables it. Anthropic's own example: five real-world MCP servers (58 tools total) cost ~55K tokens up front before Tool Search Tool; this project's 81–90 tools are in the same range.

**The practical near-term win is telling users Tool Search Tool exists and works today with zero changes here**, while treating server-side pagination as a real but lower-urgency improvement (Requirements below).

### 4. CLI/MCP parity — two distinct gaps, one much worse than expected

Precise counts, from a direct pass over `crates/iris-agentic-dev-bin/src/cmd/`: of the 90 real `#[tool]` methods, exactly **4 have a dedicated CLI subcommand** (`compile`, `exec`, `query`, `doc` — 4.4%). The generic `tool <name> --args '{...}'` fallback covers the other 78 that the Merged toolset exposes (`TOOL_NAMES` matches the Merged set 1:1, confirmed by diff); the remaining 12 (4 stub tools + 8 tools replaced by consolidated dispatchers in Merged) aren't reachable from the CLI at all, by the same toolset design that hides them from a Merged-tier MCP client too — that's consistent, not a CLI-specific hole.

**Gap 1 — the 4 "dedicated" subcommands are drifting reimplementations, not thin wrappers, and they've already lost real capability.** `compile.rs`, `exec.rs`, `query.rs`, and `doc.rs` don't call `iris_compile`/`iris_execute`/`iris_query`/`iris_doc`'s actual tool methods — each one re-implements the underlying Atelier HTTP calls directly. That means they've silently dropped everything the tool methods have grown since: `--server` multi-instance routing (none of the 4 have it — every dedicated subcommand can only ever talk to the default connection), policy/role gates, `iris_execute`'s `--timeout`/`--translate-sql`/session flags, `iris_query`'s `write`/`explain`/`count` modes, and `iris_doc`'s batch/fragment/diff modes and its whole elicitation path (`doc.rs` has *no* SCM-checkout handling at all — a write that needs checkout just fails, full stop). This is the same "parallel implementation drifts from the real one" pattern already found and fixed twice this session (`registered_tool_names()`'s hand-mirror; `TOOL_NAMES` vs `call_for_test` dispatch coverage) — a third instance, in a different subsystem. **The fix isn't "add more flags to `compile.rs`" — it's routing these four commands through the same `call_for_test` dispatcher `tool.rs` already uses**, which makes them inherit every tool-level feature for free and stops this specific drift permanently, the same way deriving `registered_tool_names()` from the real router stopped that one.

**Gap 2 — three genuinely different flavors of "stateful tool," only one of which a stateless CLI process can handle at all:**

| Tool(s) | Where state lives | Works across two separate CLI invocations? |
|---|---|---|
| `iris_execute` (`use_session`/`session_state`) | **Client-held.** The token is a Base64 blob of IRIS-serialized `%ctx` state; IRIS itself round-trips it via `%FromJSON`/`%ToJSON`. Nothing server-side to lose between processes. | **Yes, today, mechanically** — just not exposed as a flag on the dedicated `exec` subcommand (only reachable via generic `tool iris_execute`). |
| `iris_ws_open`/`iris_ws_exec`/`iris_ws_close` | **In-process only.** `WsSessionPool` holds the live WebSocket connection in memory, owned by one `IrisTools` instance. | **No — architecturally impossible without a persistent process.** Each CLI invocation constructs a fresh, empty pool; a token from invocation 1 can't resolve in invocation 2 (`SESSION_STALE`). |
| `iris_doc`/`iris_source_control`'s elicitation flow, and `iris_get_log` | **In-process only**, same shape as WS sessions — `ElicitationStore` and `LogStore` are both fresh-and-empty per `IrisTools` construction. | **No, for the same reason** — an `elicitation_id` or `log_id` minted in one CLI invocation is gone before a second invocation could resume it. This isn't merely "unergonomic," it's currently non-functional from the CLI: a `doc.rs` write needing SCM checkout has no path to complete at all today. |

Worth noting: this project does **not** use rmcp's actual protocol-level elicitation capability (`elicitation/create`) — the SCM-checkout flow is a hand-rolled, tool-parameter-based two-call pattern (return `elicitation_id`, resume by calling the *same tool* again with `elicitation_id`+`elicitation_answer`). That's a deliberate, sound design *for MCP clients* (a real MCP client keeps the same server connection alive across both calls) — it just happens to collide with the CLI's one-process-per-invocation model in a way genuine MCP usage never hits.

**What this means for scoping "full CLI parity":** it's not one uniform amount of work. `iris_execute` session support is a flag away. The WS-session/elicitation/log-retrieval trio needs either (a) an explicit CLI batch/script mode — one process, one `IrisTools` instance, running a short sequence of tool calls that share the same in-memory pools — or (b) accepting and clearly documenting that those three are MCP/persistent-connection-only. Option (a) is more interesting than it sounds: a batch mode that runs "a script of tool calls in one process" *is*, structurally, exactly the code-mode pattern from the research above — it would double as the most direct, least-effort way to make this project's tools usable the way Cloudflare's Code Mode and Anthropic's Programmatic Tool Calling expect an API surface to be usable, without building anything code-mode-specific.

## User Scenarios & Testing _(mandatory)_

### User Story 1 - Declare tool output schemas (Priority: P1) — 🔶 In Progress (15/90 tools)

A developer or tooling author consuming iris-agentic-dev's MCP tools programmatically (including any code-mode-style gateway that generates a typed SDK from tool schemas) wants to know the *shape* of a tool's response without parsing prose descriptions or guessing from examples.

**Why this priority**: Zero risk, zero dependency-version change required (rmcp 1.6.0 already supports it), and it's the one concrete piece of "structured tool calling" that's purely additive — declaring what's already true about every tool's return shape.

**Independent Test**: Call any tool via MCP `list_tools`; confirm its `Tool` definition includes a non-null `outputSchema`, and that calling the tool returns content matching that schema.

**Acceptance Scenarios**:

1. **Given** a tool that already returns a structured JSON object (the overwhelming majority of the 90), **When** its `#[tool(...)]` definition is inspected via `list_tools`, **Then** it declares an `output_schema` matching its actual return shape.
2. **Given** a tool's output schema is declared, **When** the tool is called, **Then** the actual response validates against that schema (this is a regression check, not new behavior — the shapes already exist, only the declaration is new).

**In progress — batch 1 of N delivered.** All 90 `#[tool(...)]` definitions live in one file (`crates/iris-agentic-dev-core/src/tools/mod.rs`, ~9,600 lines), each with its own hand-rolled `ok_json(serde_json::json!({...}))`/`err_json(...)` response shape — there is no shortcut that covers many tools at once; each one needs its actual body read and its real shape modeled. Batch 1 covers 15 tools, chosen for having a fully-understood, stable shape: `iris_servers`, `skill_list`, `skill_community_list`, `skill_forget`, `agent_stats`, `agent_history`, `kb_recall`, `iris_symbols`, `iris_symbols_local`, `docs_introspect`, `debug_map_int_to_cls`, `debug_source_map`, `iris_ws_open`, `iris_ws_exec`, `iris_ws_close`.

Response shapes live in a new file, `crates/iris-agentic-dev-core/src/tools/output_schemas.rs` — plain structs (`#[derive(Serialize, JsonSchema)]`) that exist purely to be handed to `schema_for_output::<T>()`; they're never constructed at runtime, so a tool's actual `ok_json(...)` body is completely untouched. Two design decisions, made once and reused across every tool in the batch:

- **Genuinely dynamic/heterogeneous fields stay `serde_json::Value`** rather than being force-fit into a struct that would misdescribe them (e.g. `iris_symbols`' raw SQL-query rows, `docs_introspect`'s BPL/DTL-dependent `xdata_flow`, `debug_source_map`'s `{method_name: int_line}` map with dynamic keys). This is the Edge Cases section's "per-tool judgment call" in practice, not a shortcut — each one is a deliberate call, documented inline in `output_schemas.rs`.
- **A real MCP constraint surfaced immediately and changed the implementation approach.** `rmcp::Tool::with_output_schema::<T>()` panics unless the root schema has a literal `"type": "object"` — and schemars renders a `#[serde(untagged)] enum { Ok(...), Err(ToolError) }` (the natural shape for a tool whose only two possible responses are its success object or this project's shared `{success: false, error_code, error}` embedded-error convention) as a bare `{"oneOf": [...]}` with no root type at all. Every tool in the batch with a real error path (`skill_forget`, `iris_symbols`, `iris_symbols_local`, `debug_map_int_to_cls`, `debug_source_map`, `iris_ws_open`) hit this. Fix: `output_schemas::oneof_output_schema::<T>()`, which calls the same underlying generator `schema_for_output` uses (`rmcp::handler::server::tool::schema_for_type`, which skips the root-type validation), adds the one key MCP requires, and hands the result to `with_raw_output_schema` — which does no validation itself, by design, as the escape hatch for exactly this. Still zero rmcp dependency-version change, still "the existing rmcp 1.6.0 API," per FR-001 — just not the one-line shorthand for tools with two possible shapes.

Verified two ways: `test_output_schema.rs` (no IRIS needed — inspects the live router's `Tool.output_schema` directly, confirming all 15 declare a schema in both Baseline and Merged, correctly excluding `debug_map_int_to_cls`/`debug_source_map` from the Merged check since both are consolidated into `iris_debug` there and legitimately absent, not "present but unscheduled"). `test_output_schema_shapes.rs` covers Acceptance Scenario 2 for real — actual `call_for_test` calls (not mocks) against the 6 tools in this batch that need no live IRIS connection at all (`skill_list`, `skill_community_list`, `agent_stats`, `agent_history`, `kb_recall`, `iris_symbols_local` — bundled/disk/in-memory data, `IrisTools::new(None)` is this project's own supported disconnected mode), asserting the real response carries every field the declared schema promises. The other 9 tools in this batch need a live IRIS connection to actually call — per this project's non-negotiable testing policy (no mocked IRIS), that half of Acceptance Scenario 2 isn't exercised in this pass; it belongs in an `--include-ignored` live test against a real container, not a substitute here.

**Remaining**: 75 of 90 tools still need their shape read, modeled, and declared. No shortcut across the remaining batches — same one-tool-at-a-time process.

---

### User Story 2 - Stop the CLI reimplementation drift; give `iris_execute` its session flags (Priority: P2) — ✅ Delivered

A developer using `compile`/`exec`/`query`/`doc` from the CLI wants the same capability the equivalent MCP tool call has — multi-instance `--server` routing, policy gates, and (for `exec` specifically) the session carrier — instead of a thinner, silently-diverging reimplementation.

**Why this priority**: This is the cheap, high-confidence half of "CLI parity." The fix is mechanical — route these four subcommands through `call_for_test`, the same dispatcher `tool.rs` already uses — not new design. It also closes a real, already-manifested drift risk: these four command files reimplement Atelier HTTP calls from scratch rather than calling the tool methods, so every feature the tool methods have gained since (multi-server routing, policy gates, `iris_doc`'s elicitation path, `iris_query`'s write/explain/count modes) is silently absent from the "equivalent" CLI command. This is the third instance this session of the same root cause — a hand-maintained parallel implementation drifting from the real one — after `registered_tool_names()`'s hand-mirror and the `TOOL_NAMES`/`call_for_test` dispatch gap, both fixed in 075.

**Independent Test**: Run `iris-agentic-dev exec --use-session` and confirm the printed `session_state` token, passed to a second `exec --session-state <token>` invocation, round-trips `%ctx` state correctly — and confirm `compile`/`query`/`doc` now accept `--server` and route to a named instance, which they cannot do today.

**Acceptance Scenarios**:

1. **Given** `compile`/`exec`/`query`/`doc` are rewritten to call `call_for_test` (or the tool methods directly) instead of re-implementing the Atelier calls, **When** any existing CLI test for these four commands runs, **Then** it still passes — this is a delegation refactor, not a behavior change for the paths that already work.
2. **Given** `iris-agentic-dev exec --use-session` completes, **When** its printed `session_state` token is passed to a second, separate `exec --session-state <token>` invocation, **Then** the second invocation sees the first's `%ctx` state (this works today only via the generic `tool iris_execute` fallback — User Story 2 is exposing it as a real flag on the dedicated command, not inventing new capability).
3. **Given** the delegation refactor lands, **When** `compile`/`query`/`doc` are run with `--server <name>`, **Then** they route to that named instance, which none of the four can do today.
4. **Given** `doc.rs` is rewritten to call the real `iris_doc` tool logic, **When** a write needs SCM checkout, **Then** the CLI surfaces the same `elicitation_required`/`elicitation_id` response the MCP path does, instead of failing with no path to resolution (closing part of the gap User Story 3 addresses for real interactive resumption).

**Delivered**, with one improvement on the original plan: `doc put` doesn't just surface the elicitation response and stop — it prompts interactively (stderr, `[y/N]`, defaults to declining on non-interactive/EOF stdin) and resumes immediately, all within the one CLI process. That's only possible because it reuses a single `IrisTools` instance (`dispatch::build_tools` + `dispatch::call`, not the one-shot `dispatch::dispatch_tool`) across both the initial write and the resume call, so the second call finds the `PendingElicitation` the first one stored — a fresh instance per call (which a real second CLI invocation would always get) could not do this, exactly per the Gap 2 table above. This means `doc put`'s elicitation path is functionally solved for the single-command case already; User Story 3's batch mode is still what's needed for the WS-terminal and `iris_get_log` cases, which don't have a single command that naturally owns both ends of the round trip the way `doc put`'s prompt-and-resume does.

Also found and preserved during the refactor, not merely carried over: `iris_execute`/`iris_doc`'s own role-gate (`check_role_gate`) only fires for a fleet "operate mode" `Subject` connection — it does not protect the default, single-instance case at all. The CLI's `is_write_allowed()` pre-check (SystemMode-based Live detection + `IRIS_ALLOW_PROD` override) was the only guard against running `exec`/`doc put` against a Live instance in the common case, and a naive delegation would have silently dropped it. Kept explicitly in both commands, ahead of the dispatch call, with a comment explaining why it can't just be assumed to live in the tool method.

`compile`/`query` output formats are unchanged (TSV via the same `tsv.rs` helpers, wrapping `iris_query`'s `{rows, count}` shape into a synthetic Atelier body so the extraction helpers didn't need duplicating); `compile`'s file-args success line now prints the `.cls`-suffixed document name `iris_compile` itself returns, a minor, disclosed formatting change (previously printed the bare class name) — not covered by any existing test, so not a regression by the tests' own definition, but worth knowing about if you're scripting against the old text.

---

### User Story 3 - A CLI batch/script mode for the tools a stateless process can't otherwise support (Priority: P3) — ✅ Delivered

A developer wants to use the WebSocket terminal, resume an SCM-checkout elicitation, or retrieve a truncated result via `iris_get_log` — all of which hold their state in an in-process pool that a fresh CLI invocation can never see again — from the CLI, without standing up a persistent daemon.

**Why this priority**: Genuinely new capability, not a flag addition — per the research above, `WsSessionPool`/`ElicitationStore`/`LogStore` are all constructed fresh and empty by every CLI invocation, so no combination of flags on separate one-shot invocations can make a WS session or an elicitation resume actually work. The only way to give these three real CLI support is to keep one `IrisTools` instance alive across multiple tool calls within a single process. Priority 3, not higher, because it's real design work and because — notably — a batch mode that runs "a short script of tool calls in one process, sharing state" *is*, structurally, this project's own version of the code-mode pattern from the research above: the same shape as Cloudflare's Code Mode and Anthropic's Programmatic Tool Calling, arrived at from a completely different motivation (CLI process-lifetime, not context-token cost).

**Independent Test**: Author a short batch script naming a sequence of tool calls (open a WS session, exec in it twice, close it) and run it through one CLI invocation; confirm the terminal state set in the first exec is visible in the second, within that one process.

**Acceptance Scenarios**:

1. **Given** a batch script opening a WS session, execing twice, and closing it, **When** it's run as a single CLI invocation, **Then** the second exec sees state the first one set (proving the pools survive across calls *within* the batch, which they cannot across separate invocations).
2. **Given** a batch script whose second call triggers an SCM-checkout elicitation and a third call answers it (`elicitation_id`/`elicitation_answer`), **When** the batch runs, **Then** the third call resolves against the same `ElicitationStore` the second call wrote to — something no pair of separate CLI invocations can do today.
3. **Given** a batch script calls a tool that returns a truncated result with a `log_id`, **When** a later call in the same batch requests `iris_get_log` with that ID, **Then** it retrieves the full content from the same in-process `LogStore`.

**Delivered** as `iris-agentic-dev batch [--file <path>]` (reads a JSON script from stdin if `--file` is omitted). The script is a JSON array of `{"tool": ..., "args": {...}}` steps (`args` defaults to `{}` if omitted); the command resolves one connection, builds exactly one `IrisTools` via `dispatch::build_tools`, and loops over the steps calling `dispatch::call` on that single shared instance — the same dispatch path `compile`/`exec`/`query`/`doc`/`tool` already use (FR-004's "thin loop, not a fourth parallel implementation" requirement), so `WsSessionPool`/`ElicitationStore`/`LogStore` state set by one step is visible to every later step in the same run, exactly as Acceptance Scenarios 1–3 require.

A later step frequently needs a value only a prior step's *response* produced at runtime (a WS `session` token, an `elicitation_id`, a `log_id`) — nobody authoring the script ahead of time can know these. Args support a placeholder `{{<step-index>.<field>}}`, recognized only as a whole string value (never embedded inside a larger string, so there's no ambiguity about what JSON type — string, number, bool — the resolved value should be), resolved against the accumulated history of already-parsed step responses immediately before that step dispatches. A step that fails — either the dispatch call itself errors, or its parsed response has `"success": false` — prints the error and stops the batch immediately (exit code 1) rather than running later steps against a known-bad state.

Example — open a WS session, exec in it twice using the session token the first step minted, then close it:
```json
[
  {"tool": "iris_ws_open", "args": {}},
  {"tool": "iris_ws_exec", "args": {"session": "{{0.session}}", "code": "Set x=1"}},
  {"tool": "iris_ws_exec", "args": {"session": "{{0.session}}", "code": "Write x"}},
  {"tool": "iris_ws_close", "args": {"session": "{{0.session}}"}}
]
```

Implementation: `crates/iris-agentic-dev-bin/src/cmd/batch.rs`, wired into `main.rs` as `Commands::Batch`. Covered by 8 inline unit tests (placeholder substitution across strings/arrays/objects/non-string field types, missing-index and missing-field error messages, `BatchStep` JSON parsing including the `args`-omitted default) — no live-IRIS integration test was added in this pass, since every step in the batch loop already dispatches through the same `call_for_test` path User Story 2's delegation work put live-IRIS test coverage behind; the batch loop itself is protocol-agnostic sequencing logic, not a new IRIS-facing code path.

---

### User Story 4 - Server-side tool-catalog pagination (Priority: P4)

An MCP client connecting to iris-agentic-dev with a large effective tool set (no `IRIS_ENABLED_TOOLS` allowlist configured) wants to avoid paying the full ~15–25K-token catalog cost on every connection when it only needs a handful of tools per turn.

**Why this priority**: Real, but lower urgency than User Stories 1–3, because the *client*-side lever (Tool Search Tool) already solves this today with zero server changes, for any client that enables it. This story is about serving clients that can't or don't use that — genuinely useful, but not blocking anything.

**Independent Test**: Send a `list_tools` request with a page-size hint; confirm the response is a strict subset with a valid `next_cursor`, and that a follow-up request with that cursor returns the remainder with no overlap or gap.

**Acceptance Scenarios**:

1. **Given** a `list_tools` request, **When** the effective tool count exceeds a configured page size, **Then** the response includes a `next_cursor` instead of the full list.
2. **Given** a `next_cursor` from a prior response, **When** it's passed to a follow-up `list_tools` call, **Then** the combined pages equal exactly the full effective tool set, each tool appearing exactly once.

---

### User Story 5 - rmcp upgrade research spike (Priority: P5 — spike, not implementation)

The maintainers want a clear, written answer to "should we upgrade rmcp to reach the 2026-07-28 spec, and what would break" before committing engineering time to it.

**Why this priority**: Lowest — this is explicitly a research deliverable (matching User Story 3 in spec 075's own precedent: "not a test — a written design spike"), not code, and it should happen *after* User Stories 1–3 land, so the spike evaluates a codebase that already declares output schemas, no longer has drifting CLI reimplementations, and has a working answer for the three in-process-state tools (both of which change what "upgrade impact" even means, and the elicitation-flow migration specifically depends on understanding how `ElicitationStore` is actually used post-User-Story-3).

**Independent Test**: Not a test — a written spike document, reviewed, with an explicit go/no-go and an itemized list of what the elicitation-flow migration would require.

**Acceptance Scenarios**:

1. **Given** the spike is complete, **When** it's reviewed, **Then** it states explicitly whether the SCM-checkout elicitation flow can be migrated to the `InputRequiredResult`/`resultType` MRTR pattern, and roughly how much of `elicitation.rs` that touches.
2. **Given** the spike recommends proceeding, **When** a follow-up spec is written, **Then** it is a new, separate spec — this spec does not authorize the rmcp upgrade itself.

---

### Edge Cases

- User Story 1: a tool whose return shape is genuinely dynamic/heterogeneous (varies by input in a way a single schema can't capture) — does it get a loose/permissive schema, or stay undeclared? Needs a per-tool judgment call, not a blanket rule.
- User Story 2: what happens when a `--session-state` token from `iris_execute` is passed to a CLI invocation with a different `namespace` than the one the session was created in? (The MCP tool's own behavior here should be the source of truth — the CLI must not invent different semantics.)
- User Story 3: what's the batch script's format (a JSON array of `{tool, args}` objects? a small DSL? literal shell-like syntax)? And critically: does a batch script's own execution model risk becoming a *fourth* parallel implementation of tool dispatch, or can it be built as a thin loop over the exact same `call_for_test` User Story 2 is already routing everything through? It must be the latter — anything else repeats the mistake User Story 2 exists to fix.
- User Story 4: does pagination state need to survive across separate CLI-driven `tool` calls (stateless, one process per call) the way it would for a long-lived MCP client connection? A CLI consumer of a paginated `list_tools` is a different usage pattern than an MCP client and may not need pagination at all — worth confirming before building it.
- User Story 5: if the spike recommends *against* upgrading (e.g., the elicitation migration cost outweighs the benefit right now), what's the fallback plan for eventually reaching 2026-07-28 compliance, given MCP's own deprecation notices (Roots/Sampling/Logging scheduled for removal) suggest standing still indefinitely isn't a real option?

## Requirements _(mandatory)_

### Functional Requirements

- **FR-001**: Every `#[tool(...)]` definition whose return shape is a fixed, well-structured object MUST declare an `output_schema` via `with_output_schema::<T>()`, using the existing rmcp 1.6.0 API — no dependency change required.
- **FR-002**: `compile`/`exec`/`query`/`doc` MUST be rewritten to dispatch through `call_for_test` (or call the tool methods directly) instead of re-implementing Atelier HTTP calls, so they inherit `--server` routing, policy gates, and every other tool-level feature by construction rather than by a second hand-maintained implementation.
- **FR-003**: `iris-agentic-dev exec` MUST expose `--use-session`/`--session-state` as real flags (the underlying mechanism already works across separate CLI invocations today — this is exposing it, not building it).
- **FR-004**: A CLI batch/script mode MUST exist that runs a sequence of tool calls within one process, one `IrisTools` instance, so `WsSessionPool`/`ElicitationStore`/`LogStore` state set by one call in the sequence is visible to a later call in the same sequence. It MUST be implemented as a thin loop over the same dispatch mechanism FR-002 uses — not a new, independent tool-invocation path.
- **FR-005**: `list_tools` MUST support real cursor-based pagination (respecting the incoming `PaginatedRequestParams`, returning a real `next_cursor` when the effective tool set exceeds a page size) instead of unconditionally returning everything with `next_cursor: None`.
- **FR-006**: A written research spike MUST evaluate the rmcp 1.6.0 → 3.0.1 upgrade path, explicitly addressing: the elicitation-flow migration (MRTR/`InputRequiredResult`/`resultType`), what other transport-facing code in this project touches session/handshake concepts the 2026-07-28 spec removes, and a go/no-go recommendation — before any upgrade work is scheduled.
- **FR-007**: Documentation (`docs/tools.md` or a new section) MUST tell users that Anthropic's Tool Search Tool already works against this server's MCP surface unmodified, as the immediately-available answer to "90 tools is a lot of context" for clients that support it.

### Key Entities

- **Output Schema**: A JSON Schema attached to a tool definition describing its response shape, distinct from the existing `input_schema` — supported by the current rmcp version, declared on zero tools today.
- **Session Handle (client-held)**: The `iris_execute` `session_state` pattern — an opaque Base64 blob the *caller* holds; nothing server-side to lose between processes. Works across separate CLI invocations today, just not exposed as a flag.
- **In-Process Pool (server-held)**: The `WsSessionPool`/`ElicitationStore`/`LogStore` pattern — a token that's a lookup key into state living only in one running `IrisTools` instance's memory. Cannot survive across separate CLI invocations under any flag design; requires either a batch mode (User Story 3) or a persistent process.
- **CLI Batch Mode**: A single CLI invocation running a sequence of tool calls against one shared `IrisTools` instance — this project's own instance of the code-mode shape (a script that calls tools, run once, sharing state), arrived at independently from the CLI's process-lifetime constraint rather than from token-cost motivations.
- **Tool-Catalog Pagination**: Cursor-based partial delivery of the `list_tools` response, distinct from the existing result-size truncation (`iris_get_log`) which already handles large tool *outputs*.
- **MRTR (Multi Round-Trip Requests)**: The 2026-07-28 replacement for the 2025-06-18 elicitation notification model — servers return `InputRequiredResult` with `resultType: "input_required"` instead of a separate follow-up request.

## Assumptions & Dependencies

- **This spec assumes spec 075's work as a foundation, not a prerequisite to redo.** The `Toolset`/`IRIS_ENABLED_TOOLS` machinery already computes an effective tool set before anything is listed — User Story 4's pagination paginates *over* that already-computed set, and User Story 2's delegation work sits on top of the now-single-source-of-truth `registered_tool_names()`/dispatch coverage from the same effort.
- **Network research caveat**: `modelcontextprotocol.io`, `modelcontextprotocol.info`, `anthropic.com`, `simonwillison.net`, `marktechpost.com`, `appwrite.io`, and `developers.cloudflare.com` were all unreachable from this session's network egress policy at research time. Findings sourced from those domains came through WebSearch's synthesized snippets (which quote and cite the underlying pages) or, for the 2026-07-28 changelog specifically, a GitHub-hosted copy of the changelog doc that *was* reachable. Verify against the primary spec site directly before treating any specific wording as a verbatim quote.
- **The elicitation-flow migration (FR-006) is the one item in this spec with real, non-cosmetic risk.** Everything else (output schemas, CLI delegation, session flags, batch mode, pagination) is additive and backward-compatible on the current dependency versions. Treat FR-006's spike as a genuine decision point, not a formality.
- **FR-002's delegation refactor touches code with existing test coverage** (`test_exec_args.rs`, `test_compile_args.rs`, `test_query_tsv.rs`, `test_doc_args.rs` per the bin crate's test registration) — those tests are the regression backstop for "delegation didn't change behavior for the paths that already worked," not something to route around.

## Success Criteria _(mandatory)_

### Measurable Outcomes

- **SC-001**: Every tool with a fixed return shape has a declared `output_schema`, verifiable via `list_tools`.
- **SC-002**: ✅ Done. `compile`/`query`/`doc` accept `--server` and route to a named instance; previously none could. Existing CLI argument-parsing tests for all four commands (`test_exec_args.rs`, `test_compile_args.rs`, `test_query_tsv.rs`, `test_doc_args.rs`) still pass after the delegation refactor.
- **SC-003**: ✅ Done. `iris-agentic-dev exec --use-session`/`--session-state` round-trips `%ctx` state across two separate invocations using only documented flags, no hand-constructed `--args` JSON.
- **SC-004**: ✅ Done. `iris-agentic-dev batch` runs a JSON script of `{tool, args}` steps within one CLI invocation, one shared `IrisTools` instance; a `{{<step-index>.<field>}}` placeholder lets a later step reference an earlier step's runtime response (e.g. a WS `session` token), proving state set by one step is visible to later steps in the same run.
- **SC-005**: `list_tools` pagination is exercised by at least one test that proves no tool is duplicated or omitted across pages.
- **SC-006**: The rmcp upgrade spike is written, reviewed, and has an explicit go/no-go — before any code changes toward the upgrade land.
- **SC-007**: `docs/tools.md` (or equivalent) documents that Tool Search Tool works today, unmodified.
