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

### User Story 1 - Declare tool output schemas (Priority: P1) — 🔶 In Progress (86/90 tools)

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

**Remaining after batch 1**: 75 of 90 tools still needed their shape read, modeled, and declared. No shortcut across the remaining batches — same one-tool-at-a-time process.

**Batch 2 delivered — 15 more tools (30/90 total).** Grouped by delegate module rather than picked at random, since several share one impl file (`admin_tools.rs`) and reading it once yields multiple shapes cheaply: `debug_capture_packet`, `debug_get_error_logs`, `iris_add_server`, `iris_remove_server`, `iris_test_server`, `iris_import_servers`, `global_kill`, `iris_namespace_list`, `iris_database_list`, `iris_namespace_create`, `iris_database_stats`, `my_access`, `capability_matrix`, `hl7_schema_list`, `journal_search`.

New findings from this batch:

- **Not every tool follows the shared `ToolError` convention.** `iris_add_server`, `iris_remove_server`, and `iris_import_servers` predate it — their error branches are a bespoke `{error_code, message}` shape (`iris_remove_server`'s `REMOVE_NOT_ALLOWED` case adds a `source` field on top), never a `success` key at all. Modeled as a separate `ServerMutationError` type rather than forcing these into `ToolError` and getting the schema wrong.
- **`iris_test_server` never calls `err_json` at all** — every outcome (network failure, non-2xx, JSON parse failure, success) goes through `ok_json` with `reachable` as the discriminant. No `Ok | Err` union needed here; it's one flat struct with most fields `Option<T>`, which is the accurate shape, not a simplification.
- **`journal_search` was one of this session's earlier field-report bug fixes** (the `TypeName="SetKillRecord"` gate that never matched) — declaring its output schema now is a second, independent confirmation that the fixed code path's real shape is `{success, entries: [{timestamp, type, job_id, global}], returned}`, not a coincidence with the fix.

`debug_capture_packet` and `debug_get_error_logs` join `debug_map_int_to_cls`/`debug_source_map` as Merged-toolset-absent (all four replaced by `iris_debug` there) — `test_output_schema.rs`'s `MERGED_REMOVED` list now covers all four, plus a new `test_merged_removed_tools_are_absent_from_merged_router` test confirming that exclusion is a real toolset fact, not papering over a bug.

**No new `test_output_schema_shapes.rs` coverage this batch, and that's a deliberate, disclosed gap, not an oversight.** Every one of these 15 tools needs a live IRIS connection to produce a real response (`resolve_server`/`get_iris_reloaded`), *or* — for `iris_add_server`/`iris_remove_server`/`iris_import_servers` — would mutate the real, non-test-isolated `iad-native` server config file and OS keychain on the host running the test, which is not something a unit test should do regardless of IRIS. Neither is a batch-1-style "genuinely needs no IRIS and no side effects" case, so schema-declaration coverage (`test_output_schema.rs`) is what this batch gets; the response-shape half of Acceptance Scenario 2 for these 15 belongs in a future `--include-ignored` live test.

**Remaining after batch 2**: 60 of 90 tools.

**Batch 3 delivered — 15 more tools (45/90 total, halfway).** `compare_document`, `compare_namespace`, `global_preview`, `query_audit_log`, `stream_inspect`, `hl7_schema_inspect`, `mermaid_class`, `mermaid_production`, `skill_propose`, `skill_optimize`, `skill_share`, `skill_community_install`, `telemetry_query`, `telemetry_export_trace`, `iris_credential_list`.

New findings from this batch:

- **A real modeling mistake caught before it shipped.** `global_preview`'s one fallible step propagates via `?` as a protocol-level `McpError` (no embedded-JSON error path at all), so its response type is one flat struct, not an `Ok | Err` union — this pattern was already established by `iris_ws_exec`/`iris_ws_close`. First draft of this batch mis-modeled `mermaid_production` the same way by analogy, assuming it also had no error path; rereading its body showed it *does* call `err_json("IRIS_UNREACHABLE", ...)` on a query failure, same as almost everything else in `admin_tools.rs`. Fixed to the correct `Ok | Err(ToolError)` union before wiring the `#[tool(...)]` attribute — a reminder that "looks like the last one" isn't a substitute for reading each body; this is a fully manual, per-tool process by design (see Batch 1's opening paragraph), and skipping that step here would have shipped a schema that silently didn't cover the tool's own error path.
- **`hl7_schema_inspect` has two distinct success shapes, not one** — segment-level lookup (`{success, schema, segment, fields}`) and whole-schema structure listing (`{success, schema, structures}`) never appear in the same response, driven by whether the caller passed a `segment` param. Modeled as a 3-variant untagged enum (`Segment | Structures | Err`), proving `oneof_output_schema`'s approach generalizes past the 2-variant `Ok | Err` case it was built for.
- **The four `NOT_IMPLEMENTED` stub tools got a schema too** (`skill_propose`, `skill_optimize`, `skill_share`, `skill_community_install`) — each unconditionally returns `err_json("NOT_IMPLEMENTED", ...)`, so `ToolError` alone (no union) is the complete, accurate shape. Declaring it now means a future real implementation that changes the response shape without updating `output_schemas.rs` gets caught by schema-declaration coverage instead of drifting silently. These four are also pruned from every non-Baseline toolset (`stubs_to_remove`) — added to `test_output_schema.rs`'s `MERGED_REMOVED` list alongside the debug_* quartet, and its doc comment broadened to explain both exclusion reasons (iris_debug consolidation vs. stub pruning) since it's no longer only about one.

**`test_output_schema_shapes.rs` gained real coverage this batch, unlike batch 2.** The four stub tools are always callable with no live IRIS and no side effects — their response is deterministic regardless of connection state — so all four got genuine `call_for_test` assertions (not mocks), the same standard as batch 1's no-IRIS-needed tools.

**Remaining after batch 3**: 45 of 90 tools — exactly halfway.

**Batch 4 delivered — 11 more tools (56/90 total).** Smaller than the previous three batches on purpose: `resolve_dynamic_dispatch`, `find_subclass_implementations`, `skill_describe`, `skill_search`, `iris_get_log`, `agent_info`, `kb`, `kb_index`, `iris_credential_manage`, `iris_lookup_manage`, `iris_lookup_transfer` needed genuinely more careful, multi-action modeling than a straight 15-tool batch would allow without cutting corners.

New findings from this batch:

- **Action-multiplexed tools can have several genuinely distinct success shapes, not just two.** `iris_lookup_manage` dispatches on `action` (`list_tables`/`get`/`set`/`delete`/`list_keys`) to five shapes that share no fields; `iris_get_log` has three (list / paginated-get / full-get, depending on whether `id` and `limit` are present); `agent_info` and `kb` each have two (`what`/`action`-driven). All modeled as N-variant untagged enums — `oneof_output_schema` generalizes cleanly past the 2- and 3-variant cases from batches 1 and 3.
- **`kb_index` and `kb` are a genuinely interesting pair.** Both call the same underlying `handle_kb`, but `kb_index` always passes `action="index"` hardcoded — so it gets its own single-shape `KbIndexResponse` (`Ok(KbIndexOk) | Err`), while `kb` is the action-multiplexed one (`KbResponse`, both `Index` and `Recall` variants). Two different declared schemas for two different call sites into the same handler, both accurate.
- **`extract_message_map_routing` was scoped out of this batch, deliberately.** Reading its body showed a third code path beyond MessageMap success/failure: BPL/DTL classes get detected early and return an entirely different, differently-shaped value (`detect_bpl_dtl_routing`) before the MessageMap logic even runs. Modeling that accurately needs tracing `xdata_flow::parse_bpl`/`parse_dtl`'s own return shapes, which this batch's time budget didn't cover — deferred to a future batch rather than declaring a schema that would silently misdescribe that third path. This is the Edge Cases section's "per-tool judgment call" being exercised for real, not a shortcut.
- **A second toolset-exclusion direction, not just the first.** Every prior batch's `MERGED_REMOVED` handled tools absent *from* Merged. `iris_get_log` is the opposite: it's `merged_only` (present *only* in Merged, per `with_registry_and_toolset`), so it would have failed `test_declared_tools_advertise_output_schema_in_baseline` if left unhandled. Added a mirror `BASELINE_REMOVED` list and a symmetric `test_baseline_removed_tools_are_absent_from_baseline_router` test — the same principle as batch 3's `MERGED_REMOVED` exclusion, just running the other way.

`test_output_schema_shapes.rs` gained 6 more real, no-live-IRIS-needed tests this batch: `skill_describe` (NOT_FOUND path — the bundled-skill lookup needs no connection), `skill_search` (same reasoning as batch 1's `skill_list`), `iris_get_log`'s list path (backed entirely by the in-process `LogStore`), and `iris_credential_manage`/`iris_lookup_manage`/`iris_lookup_transfer` — all three take `Option<&IrisConnection>` and return a real, deterministic `IRIS_UNREACHABLE` `ToolError` with no connection, not a mock. Also caught two `note` fields (`SkillListResponse`, `SkillSearchResponse`) that were typed as loose `serde_json::Value` when `bundled::searched_note` always returns a plain `String` — tightened both while writing this batch's `SkillNotFoundError` type, which needed the same field modeled correctly for the first time.

**Remaining after batch 4**: 34 of 90 tools.

**Batch 5 delivered — 12 more tools (68/90 total).** `iris_list_containers`, `iris_select_container`, `iris_start_sandbox`, `iris_generate_class`, `iris_generate_test`, `resolve_storage`, `iris_info`, `iris_table_info`, `iris_doc_search`, `iris_message_body`, `iris_business_rule_info`, `iris_production_diff`.

New findings from this batch:

- **A third bespoke error convention, distinct from both `ToolError` and `ServerMutationError`.** `iris_select_container`'s two failure shapes put the error CODE directly in an `error` field (`"error": "CONTAINER_NOT_FOUND"`) rather than a separate `error_code`+`error` pair, and each carries different extra context (`requested`/`available` vs `container`/`port_web`/`message`). Modeled as two distinct structs, not shoehorned into either existing error type.
- **Not every failure path is even JSON-shaped the same as a "normal" error.** `iris_doc_search` (a live Algolia network call, not IRIS) returns `{error, hits: []}` on failure — no `success` or `error_code` field at all. `iris_table_info`'s `NOT_FOUND` case has `error` but no `error_code`. Both got their own bespoke error struct rather than being forced to match `ToolError`.
- **LLM-backed tools (`iris_generate_class`/`iris_generate_test`) have no embedded-JSON error path for their main failure mode.** `LLM_UNAVAILABLE`/`LLM_TIMEOUT` propagate via `?` as protocol-level `McpError`s; the only embedded-JSON failure is a shared `INVALID_OUTPUT` shape (`{success: false, error_code, raw_llm_output}`, no `error` field) when the LLM's response fails `validate_cls_syntax` — same struct reused for both tools since it's identical.
- **Nested type-dependent variance was deliberately left dynamic rather than double-nested.** `iris_table_info`'s success shape wraps a `result` object whose *internal* fields differ by `type` (`class_projection` vs `ddl_table`) — modeling that fully would mean a second nested untagged enum inside the first. Left as `serde_json::Value` with a comment explaining why, rather than over-engineering a schema this batch's scope didn't call for.
- **A live external network call (not IRIS) is still not something to unit-test as if it were free.** `iris_doc_search` calls Algolia's real search API — no live-IRIS policy applies here, but hitting a real third-party endpoint from a unit test is still flaky/slow/rate-limit-prone by nature, so it wasn't added to `test_output_schema_shapes.rs` despite being technically callable with no connection. Same reasoning kept `iris_list_containers`/`iris_select_container`/`iris_start_sandbox` out too — all three shell out to `docker`/`idt` subprocesses that aren't available in this sandbox and would behave differently across environments.

`test_output_schema_shapes.rs` gained 3 more real tests: `iris_message_body`, `iris_business_rule_info`, `iris_production_diff` — all three resolve their connection via `self.iris_arc()` (never `resolve_server`/`get_iris_reloaded`, which fail via `?` instead) when no `server` param is given, so with no connection each hits its own `Option<&IrisConnection>` match and returns a real, deterministic `IRIS_UNREACHABLE` — not a mock, same pattern as batch 4's credential/lookup tools.

**Remaining after batch 5**: 22 of 90 tools.

**Batch 6 delivered — 6 more tools (74/90 total), plus a retroactive correctness fix to 3 already-shipped schemas.** `iris_execute_method`, `iris_macro`, `iris_debug`, `iris_generate`, `skill`, `skill_community`.

**The correctness fix, found while modeling this batch, not after:** while reading `iris_execute_method`'s wrapper to model its schema, its call to the shared cross-tool policy gate (`crate::policy::gate::dispatch_gate`) stood out — the same gate `iris_message_body`, `iris_business_rule_info`, and `iris_production_diff` (all shipped in batch 5) also call, *before* the impl function this file had modeled even runs. Batch 5's schemas for those three never accounted for the gate's own blocked-response shape, which the wrapper returns via `ok_json(gate)` ahead of the tool's real logic — a real gap in already-merged work, not a hypothetical one. Fixed by adding a `GateBlocked(serde_json::Value)` variant to all three response enums (`IrisMessageBodyResponse`, `IrisBusinessRuleInfoResponse`, `IrisProductionDiffResponse`) plus the new `IrisExecuteMethodResponse`. The gate's blocked-response shape genuinely varies by which of its four internal checks fired (env-template, bulk-PHI, global blocklist, PHI-name pattern) — left as free-form JSON rather than a fourth nested union on top of each tool's own variants, consistent with this spec's existing "genuinely dynamic" carve-outs.

**This same gate is called by at least 6 tools total** (`iris_compile`, `iris_execute`, `iris_query`, `iris_source_control`, `iris_global`, `iris_execute_method`) — a note is now in `IrisExecuteMethodResponse`'s doc comment so a future batch declaring schemas for the still-undeclared four doesn't repeat batch 5's mistake.

Other findings from this batch:

- **`iris_debug` is a separate implementation from batch 1's individual debug tools, not a thin dispatcher to them.** Despite the near-identical name and four matching actions (`map_int`/`error_logs`/`capture`/`source_map`), `iris_debug` lives in `info.rs` with its own SQL/exec logic — it needed its own four response structs, not a reuse of `debug_map_int_to_cls`'s/`debug_source_map`'s types. Its `DOCKER_REQUIRED` failure path happens to already match `ToolError`'s exact shape, so no bespoke error type was needed there, unlike `skill_forget`'s superficially-similar case in batch 1.
- **`iris_generate` (the context-provider tool) has no embedded-JSON error path at all** — it always returns one of two prompt-context shapes (`gen_type=class` vs `gen_type=test`), with HTTP failures propagating via `?`. Genuinely distinct from the LLM-backed `iris_generate_class`/`iris_generate_test` from batch 5, despite the similar name.
- **`skill` and `skill_community` are yet another instance of "looks like the individual tools, isn't."** `skill`'s five actions (`list`/`describe`/`search`/`forget`/`propose`) read `^SKILLS` directly via their own ObjectScript, completely independent from `skill_list`/`skill_describe`/`skill_search`/`skill_forget`'s bundled-skill logic from batches 1 and 4 — same naming collision risk as `iris_debug`, same fix (separate response types, not reused ones).

No new `test_output_schema_shapes.rs` coverage this batch — all six new tools need `resolve_server`/`get_iris_reloaded` (which fail via `?` with no connection, unlike the `Option<&IrisConnection>` tools from batches 4-5), so there's no genuine no-IRIS-needed path to test for real.

**Remaining after batch 6**: 16 of 90 tools — `iris_compile`, `iris_test`, `iris_execute`, `iris_doc`, `iris_query`, `check_config`, `iris_search`, `extract_message_map_routing`, `iris_source_control`, `iris_global`, `iris_production`, `iris_interop_query`, `iris_containers`, `iris_production_item`, `iris_admin`. These are the tools this spec's batches have been implicitly triaging away from throughout: the five core execution tools (large, multi-mode, already CLI-delegated in User Story 2), `check_config` (intentionally uncategorized elsewhere in this codebase for the same reason — genuinely heterogeneous, conditionally-appended fields), `iris_search` (an async-polling implementation with a sync/async fallback path), `extract_message_map_routing` (deferred in batch 4 — a third BPL/DTL response path beyond success/failure), and the Merged-tier action-multiplexed dispatchers (`iris_production`, `iris_interop_query`, `iris_containers`, `iris_production_item`, `iris_admin` — `iris_admin` alone is ~200 lines of action-dispatch). Each remaining tool needs meaningfully more reading than a normal batch's per-tool budget without repeating the mermaid_production-style mistake batch 3 caught.

---

### Batch 7 — `iris_query` (75/90 total)

Confirmed the previous paragraph's assessment by fully reading `iris_query` — one tool, taken on its own this time rather than bundled into a 15-tool sweep, because it turned out to justify that on its own: **four modes** (default/select, `explain`, `count`, `write`), **three independent gate mechanisms** ahead of any mode-specific logic (`dispatch_gate`, `crate::iris::server_manager::policy_gate` — which carries its own `allowed_categories` field — and `workspace_config::check_role_gate`, gating SELECT-vs-write SQL separately from either), and **per-mode bespoke error shapes** beyond `ToolError` (`SQL_WRITE_BLOCKED` in select mode carries `blocked_keyword`+optional `force_ignored`; write mode's DDL-keyword case carries `blocked_keyword` with no `force_ignored`; write mode's rows-affected pre-check carries `actual_count`+`limit`). Nine variants total in `IrisQueryResponse` — the largest single-tool schema in this file so far, and unlike `iris_table_info`/`extract_message_map_routing`'s dynamic carve-outs, every branch was actually modeled here rather than left as `serde_json::Value`, because the complexity was already fully read while confirming the "these are hard" assessment above.

The three gate mechanisms' own blocked-response shapes are the one deliberately dynamic part (`GateBlocked(serde_json::Value)`) — same reasoning as `IrisMessageBodyResponse`'s single-gate case from batch 6, just three gates instead of one.

No `test_output_schema_shapes.rs` coverage — every mode needs `resolve_server`/`get_iris_for_exec_with_client`, both of which fail via `?` with no connection.

**Remaining**: 15 of 90 tools — the same list minus `iris_query`. Given how much one tool from that list just took, the other four core execution tools (`iris_compile`, `iris_test`, `iris_execute`, `iris_doc`) should be assumed to be comparably sized rather than batched together casually.

---

### Batch 8 — `iris_compile` (76/90 total)

Confirmed. `iris_compile` has three sub-paths (docker-exec when Atelier REST is unavailable, local-file upload+compile, and the normal Atelier `/action/compile` path), the same three gate mechanisms `iris_query` has ahead of any of them, and its own progressive-disclosure truncation (`log_store::apply_truncation`, same helper `debug_get_error_logs`/`iris_info` use) on top of that.

**A second, distinct error convention surfaced — and a real accuracy gap in `iris_query`'s already-shipped schema was found and fixed in the same pass.** `err_json_with_url` (used by every HTTP-calling branch of both `iris_compile` and `iris_query`) adds `attempted_url` and a fixed `hint` string on top of `ToolError`'s three fields — genuinely not `ToolError`, the same category of finding as `ServerMutationError`/`IrisSelectContainerNotFound`/`IrisDocSearchError` from earlier batches. Batch 7's `IrisQueryResponse` used plain `Err(ToolError)` for its `IRIS_UNREACHABLE` case, missing this — caught while modeling the same helper for `iris_compile`. Added a shared `IrisUnreachableWithUrlError` type and a new variant to both `IrisCompileResponse` and (retroactively) `IrisQueryResponse`. Doesn't change what actually validates today — schemars only emits `additionalProperties: false` under `#[serde(deny_unknown_fields)]`, which nothing in this file uses, so the extra real fields were already accepted by the permissive `ToolError` variant — but it fixes the schema's accuracy as documentation, which is the entire point of this user story.

No `test_output_schema_shapes.rs` coverage — every sub-path needs a live connection.

**Remaining**: 14 of 90 tools — `iris_test`, `iris_execute`, `iris_doc`, `check_config`, `iris_search`, `extract_message_map_routing`, `iris_source_control`, `iris_global`, `iris_coverage`, `iris_production`, `iris_interop_query`, `iris_containers`, `iris_production_item`, `iris_admin`. (`iris_coverage` was in the original remaining-tools count all along — batches 6 and 7's prose lists above dropped it by mistake; the tool counts themselves were always right.)

---

### Batch 9 — `iris_test` (77/90 total)

The third core execution tool. Simpler than `iris_query`/`iris_compile` in one specific way — **no policy gate at all**: `iris_test` never calls `dispatch_gate`/`policy_gate`/`check_role_gate`, so `IrisTestResponse` has no `GateBlocked` variant, the first of the five core tools without one. Its complexity is elsewhere: parsing free-text `%UnitTest.Manager` RunTest stdout into structured pass/fail results (IRIS's own output format, not a JSON API), an optional coverage sub-run that wraps the whole test run, and a `NO_TESTS_FOUND` case IRIS itself doesn't distinguish from "ran zero test methods" at the protocol level — a synthetic 1-failure suite gets created at the path-separator level instead, which this code's stdout parser has to notice and re-report as its own explicit error.

`coverage` (present only when `coverage: true` is passed) stays `serde_json::Value` — `iris_coverage`'s own output schema isn't declared yet (still on the remaining list below), so referencing a type that doesn't exist would be backwards; this field gets tightened for real once that tool's own batch lands.

No `test_output_schema_shapes.rs` coverage — every path needs a live connection. The "known-undeclared" example in `test_a_tool_without_a_declared_schema_reports_false_not_a_panic` moved to `iris_execute` (its third home, after `iris_compile` then `iris_test`, as batches 8 and 9 gave each of them a real schema in turn).

**Remaining**: 13 of 90 tools — `iris_execute`, `iris_doc`, `check_config`, `iris_search`, `extract_message_map_routing`, `iris_source_control`, `iris_global`, `iris_coverage`, `iris_production`, `iris_interop_query`, `iris_containers`, `iris_production_item`, `iris_admin`.

---

### Batch 10 — `iris_execute` (78/90 total)

The fourth core execution tool. Same three gate mechanisms as `iris_query`/`iris_compile` ahead of the real work, plus a `SESSION_INVALID` early-return that matches `ToolError` exactly. Past the gates, its complexity comes from two structurally different execution paths rather than one: the normal Atelier HTTP path (`method: "http"`) carries `auth_user`/`service_account_env`/an optional `session_state` token that the docker-exec fallback path (`method: "docker"`) has no equivalent for at all — reusing one struct with everything `Option` would have hidden which fields actually co-occur, so this became two distinct success structs (`IrisExecuteHttpOk`/`IrisExecuteDockerOk`) rather than one loosely-typed one.

Two more error shapes, both bespoke rather than plain `ToolError`: `IrisExecuteSessionError` (a session-fatal failure that still needs to report which namespace/method/auth_user/service_account_env it was attempting, so the caller can tell *which* session died) and `IrisExecuteHttpExecutionFailedError` (the case where the docker fallback's own `DOCKER_REQUIRED` path surfaces the *original* HTTP error rather than a docker-specific one, carrying it in an `http_error` field ToolError has no room for). `IrisExecuteResponse` ends up with six variants: `Http`, `Docker`, `SessionError`, `HttpExecutionFailed`, `Err(ToolError)`, `GateBlocked(serde_json::Value)`.

No `test_output_schema_shapes.rs` coverage — every path needs a live connection or service-account routing; there's no side-effect-free way to exercise this one without a real IRIS instance. The "known-undeclared" example in `test_a_tool_without_a_declared_schema_reports_false_not_a_panic` moved off the execution-tool rotation entirely this time, to `check_config` — deliberately picked as unlikely to need a schema soon (genuinely heterogeneous, conditionally-appended fields, already carved out elsewhere in this codebase as intentionally uncategorized) rather than another tool that would just need swapping out again next batch.

**Remaining**: 12 of 90 tools — `iris_doc`, `check_config`, `iris_search`, `extract_message_map_routing`, `iris_source_control`, `iris_global`, `iris_coverage`, `iris_production`, `iris_interop_query`, `iris_containers`, `iris_production_item`, `iris_admin`.

---

### Batch 11 — `iris_doc` (79/90 total)

The fifth and last core execution tool, and the largest by mode count: get, put, delete, head, fragment, compiled, list, insert, delete_lines, plus a top-level elicitation-resume path handled before mode dispatch so it works uniformly for put and both surgical-edit modes. No gate calls at all — like `iris_test`, not like `iris_query`/`iris_compile`/`iris_execute` — so no `GateBlocked` variant.

The real complexity is that `do_write`'s core success JSON gets progressively more fields merged onto it depending on which caller invoked it, never fewer: plain mode=put, mode=put with `compile: true`, an elicitation-resume write, and a successful mode=insert/mode=delete_lines edit all share one underlying shape with a growing set of optional annotations (`compiled`/`compile_errors`/`compile_console` for the compile case; `resumed` for the elicitation-resume case; `edit`/`inserted_at`/`deleted_start`/`deleted_end`/`lines_added`/`lines_removed`/`diff`/`total_lines`/`content` for the surgical-edit case). Modeled as one `IrisDocWriteOk` struct with all of these as `Option` fields rather than four separate structs — that's what the code itself does (`annotate_edit` merging onto `do_write`'s base JSON), not an approximation of it. The SCM checkout dialog (`elicitation_required`) gets the same treatment for the same reason: mode=put's plain dialog and mode=insert/mode=delete_lines' edit-annotated dialog are the same JSON shape with the edit fields present or absent.

Two more bespoke error shapes beyond plain `ToolError`: `IrisDocStaleContentError` (mode=insert/mode=delete_lines' `STALE_CONTENT` refusal, carrying the exact line/expected/actual divergence) and `IrisDocEditFailedError` (the rare case where a surgical edit's write itself fails — `SCM_REJECTED`, an HTTP error, or a requested compile failing — after the diff was already computed, so the edit-annotation fields get merged onto the failure the same way they do onto a success).

The two batch paths (`names` set on get/delete) stay `Vec<serde_json::Value>` for their per-item entries rather than a nested oneof — each entry's shape (`{name, content}` vs `{name, error}` for get; `{name, error}` for delete's failures) is decided per-document, and a oneof-of-oneof here would cost more schema complexity than it documents. `IrisDocResponse` ends up with 13 variants: `Get`, `GetBatch`, `ElicitationRequired`, `WriteOk`, `Delete`, `DeleteBatch`, `Head`, `Fragment`, `Compiled`, `List`, `StaleContent`, `EditFailed`, `Err(ToolError)`.

No `test_output_schema_shapes.rs` coverage — every mode needs a live connection (`resolve_server`).

This closes the five core execution tools (`iris_query`, `iris_compile`, `iris_test`, `iris_execute`, `iris_doc`) — all now schema'd.

**Remaining**: 11 of 90 tools — `check_config`, `iris_search`, `extract_message_map_routing`, `iris_source_control`, `iris_global`, `iris_coverage`, `iris_production`, `iris_interop_query`, `iris_containers`, `iris_production_item`, `iris_admin`.

---

### Batch 12 — `iris_coverage` (80/90 total)

Line coverage via `%Monitor.System.LineByLine`, with five modes (check/run/start/stop/report) instead of gates or session state as its source of complexity. No gate calls. Two more error-shape conventions surfaced on top of the four already known: this tool's own local `err_json` uses `message`, not `error`, as the free-text field name (`{success, error_code, message}`) — distinct from `ToolError` in the same way `ServerMutationError`/`IrisTableInfoNotFound`/`IrisDocSearchError` were — and mode=check's success case has no `success` field at all, using `ok: true` instead (mirrors `IrisDocSearchError`'s precedent of a tool inventing its own top-level marker).

mode=run and mode=check both merge extra fields onto their parsed result unconditionally — including onto an *error* result, not just a success — so `IrisCoverageError` carries all of those mode-specific extras (`fix`, `meets_target`/`target_pct`/`cobertura_skipped`, `testcoverage_available`/`testcoverage_hint`) as optional fields on one shared struct, documented per-field with which mode populates it, rather than proliferating near-duplicate per-mode error types. mode=report calls the exact same coverage-result parser as mode=run but skips the merge step entirely, which is why `IrisCoverageRunOk`'s run-specific fields are optional: report's success is the identical shape with them simply absent.

Also resolved a deferred TODO from batch 9: `iris_test`'s `coverage` field (present only when `coverage: true` is passed — `iris_test` runs `iris_coverage`'s own `mode=report` internally around the test run) was left as `serde_json::Value` because this tool's schema didn't exist yet. Tightened to a new `IrisCoverageReportResult` — deliberately narrower than the full 5-variant `IrisCoverageResponse`, since only `mode=report`'s two outcomes (`RunOk` | `Err`) can ever appear there.

Both `parse_check_output`/`parse_coverage_output`'s JSON-passthrough branches (`trimmed.starts_with('{')`) are explicitly for feeding test fixtures directly — real IRIS output is pipe-delimited text, never JSON-shaped — so neither is modeled as a schema variant.

No `test_output_schema_shapes.rs` coverage — every mode needs a live connection.

**Remaining**: 10 of 90 tools — `check_config`, `iris_search`, `extract_message_map_routing`, `iris_source_control`, `iris_global`, `iris_production`, `iris_interop_query`, `iris_containers`, `iris_production_item`, `iris_admin`.

---

### Batch 13 — `iris_global` (81/90 total)

Read/write/kill/list on raw IRIS globals. Fires `dispatch_gate` (PHI + system-blocklist checks) before any IRIS call, so has a `GateBlocked` variant — and turned up a toolset-membership fact worth flagging: `iris_global` is itself Merged-only (`with_registry_and_toolset`'s `merged_only` list, 052-iris-global), the same category as `iris_get_log`/`iris_message_body`/`iris_business_rule_info`/`iris_production_diff`/`iris_execute_method`/`iris_debug` before it — added to `BASELINE_REMOVED` rather than the plain declared-tools list.

Same `message`-not-`error` local error convention as `iris_coverage`, but not the same type — `IrisGlobalError`'s fields don't overlap with `IrisCoverageError`'s mode-specific extras, so reusing one for the other would document fields that can never appear. `INVALID_SUBSCRIPT` (a requested subscript failing the allowlist regex) gets its own extended error type carrying which subscript failed and the pattern checked, rather than folding into the general error — this is a rejected-input error a caller should be able to tell apart from an IRIS-side failure without parsing `message`.

Each action's success shape is a genuinely different struct, not a shared one with options: `get` (single value) returns `{success, defined, value}`; `get` with `subtree: true` returns a node list; `set`/`kill` return only `{success: true}` — nothing else to report; `list` returns a subscript list. Four structs, not four flavors of one.

No `test_output_schema_shapes.rs` coverage — `iris_global` routes through `resolve_server`/`get_iris_for_exec_with_client`, both of which need a live connection (unlike the `self.iris_arc()`-based tools that return `None` gracefully).

**Remaining**: 9 of 90 tools — `check_config`, `iris_search`, `extract_message_map_routing`, `iris_source_control`, `iris_production`, `iris_interop_query`, `iris_containers`, `iris_production_item`, `iris_admin`.

---

### Batch 14 — `iris_source_control` (82/90 total)

SCM status/menu/checkout/execute via `%Studio.SourceControl.Interface`, plus a top-level elicitation-resume path mirroring `iris_doc`'s. Fires all three gate mechanisms (`dispatch_gate`, `server_manager::policy_gate`, `check_role_gate` for checkout/execute) — back on the same footing as the five core execution tools, unlike `iris_test`/`iris_doc`/`iris_coverage`/`iris_global`. Also back on `ToolError`'s own `error` convention, not the `message` variant the last two tools used — conventions vary per tool, not by chronology.

action=execute's confirmation dialog has two distinct follow-up mechanisms depending on what the SCM provider's `UserAction` asked for: a yes/no confirmation (`options`) or a free-text prompt (`input_type: "text"`) — modeled as one `IrisSourceControlElicitationRequired` struct with both optional, since they're the same envelope with a different resume mechanism, not two different outcomes. action=menu never fails outright — a transport error or empty response degrades to an empty `actions` list rather than an error result, so it has no error variant of its own.

One more single-purpose error type: action=status's specific `SCM_UNAVAILABLE` (no `SCMSTATUS` sentinel found, and the native-provider-notice fallback didn't match either) extends `ToolError` with the raw truncated IRIS output, so the real cause — a `<PROTECT>`, an auth banner, an empty body — stays diagnosable instead of collapsing into an opaque message.

No `test_output_schema_shapes.rs` coverage — routes through `resolve_server`, needing a live connection.

**Remaining**: 8 of 90 tools — `check_config`, `iris_search`, `extract_message_map_routing`, `iris_production`, `iris_interop_query`, `iris_containers`, `iris_production_item`, `iris_admin`.

---

### Batch 15 — `iris_containers` (83/90 total)

A pure dispatcher (Merged toolset only — added to `BASELINE_REMOVED`, same category as `iris_global` before it): `action: list|select|start` calls straight through to `iris_list_containers`/`iris_select_container`/`iris_start_sandbox` respectively and returns exactly what they return. No new struct needed — `IrisContainersResponse` just composes those three tools' own already-declared response types plus this dispatcher's own `INVALID_ACTION` `ToolError`, since reusing the real shapes is more accurate than re-describing them.

No `test_output_schema_shapes.rs` coverage — inherits the same exclusion as the three tools it dispatches to (all shell out to `docker`/`idt` subprocesses).

**Remaining**: 7 of 90 tools — `check_config`, `iris_search`, `extract_message_map_routing`, `iris_production`, `iris_interop_query`, `iris_production_item`, `iris_admin`.

---

### Batch 16 — `iris_interop_query` (84/90 total)

A `what: logs|queues|messages` dispatcher over three SQL-backed lookups (`Ens_Util.Log`, `Ens.Queue_Enumerate()`, `Ens.MessageHeader`). Each success shape carries its row data as raw `serde_json::Value` (the SQL query result's `result.content` array, passed through verbatim) — the same carve-out used for other tools' raw query-result fields, since arbitrary SQL row shapes can't be modeled precisely. All three sub-actions share one error convention (plain `ToolError`), including the no-connection case.

This tool resolves its connection via `self.iris_arc()`, not `resolve_server`/`get_iris_reloaded` — same pattern as batch 5's trio (`iris_message_body`/`iris_business_rule_info`/`iris_production_diff`) — so it gets real `test_output_schema_shapes.rs` coverage: one test per `what` value, each hitting the deterministic `IRIS_UNREACHABLE` response with no live IRIS needed.

**Remaining**: 6 of 90 tools — `check_config`, `iris_search`, `extract_message_map_routing`, `iris_production`, `iris_production_item`, `iris_admin`.

---

### Batch 17 — `iris_production_item` (85/90 total)

An `action: enable|disable|get_settings|set_settings` dispatcher against a single production config item. Three genuinely different success shapes, one per action group (enable/disable share one, get_settings and set_settings each have their own) — all errors funnel through the shared `ToolError` convention (`ITEM_NOT_FOUND`, `NO_PRODUCTION`, `UPDATE_FAILED`, `INTEROP_ERROR`, `IRIS_UNREACHABLE`, `INVALID_PARAMS`, `INVALID_ACTION`).

Resolves its connection via `self.iris_arc()` like `iris_interop_query` before it, so it gets a real `test_output_schema_shapes.rs` test hitting the deterministic `IRIS_UNREACHABLE` response with no live IRIS needed.

**Remaining**: 5 of 90 tools — `check_config`, `iris_search`, `extract_message_map_routing`, `iris_production`, `iris_admin`.

---

### Batch 18 — `iris_production` (86/90 total)

An `action: status|start|stop|update|check|recover|get_autostart|set_autostart` dispatcher over the whole production lifecycle. Several actions share one success shape exactly rather than needing their own struct: start/stop/recover all report only `{success, state}` (just a different state string), and get_autostart/set_autostart both report `{success, namespace, autostart_enabled, production}`. All errors funnel through `ToolError` (`NO_PRODUCTION`, `INTEROP_ERROR`, `IRIS_UNREACHABLE`, `INVALID_ACTION`).

Resolves its connection via `self.iris_arc()` like the other interop dispatchers before it, so it gets a real `test_output_schema_shapes.rs` test hitting the deterministic `IRIS_UNREACHABLE` response with no live IRIS needed.

**Remaining**: 4 of 90 tools — `check_config`, `iris_search`, `extract_message_map_routing`, `iris_admin`.

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
