# Feature Specification: Modular Tool & Skill Installation

**Feature Branch**: `075-modular-tool-install`
**Created**: 2026-08-16
**Status**: In Progress — User Story 1 (P1) delivered; User Stories 2–3 (P2–P3) not started
**Input**: User description: "A user in the global masters community wants skills and tools to be separately installable/usable, rather than bundled all-or-nothing with the full iris-agentic-dev binary. Skills already have a mostly-solved path (the 061 official skill pack via `npx skills`); scope what's needed on the tools side — let a developer or a downstream project expose, or depend on, a chosen subset of the 90 MCP tools without pulling in the whole tool surface, and evaluate whether a genuinely smaller compiled artifact is warranted or whether interface-level subsetting is enough."

## User Scenarios & Testing _(mandatory)_

### User Story 1 - Expose only a chosen subset of tools to an agent (Priority: P1) — ✅ Delivered

A developer configuring their AI coding agent's MCP connection to iris-agentic-dev wants their agent to see only a specific, named set of tools — for example, only the SQL and search tools, to reduce the number of tools their agent has to reason over and to shrink the blast radius of anything that could touch a live instance. Today they can only choose between three fixed presets (`baseline`, `nostub`, `merged`) or remove individually-named tools from whichever preset they picked; there is no way to start from "nothing" and add exactly the tools they want.

**Why this priority**: This is the cheapest, highest-leverage slice of the request. The runtime mechanism that would serve it (pruning routes from an already-built `ToolRouter` before the MCP server starts) already exists and is already exercised by the disabled-tools blocklist and the `Toolset` presets — this closes the interface gap without touching how the binary is built, tested, or distributed.

**Independent Test**: Start the MCP server with a chosen allowlist of N named tools and no live IRIS connection. Send an MCP `list_tools` request and verify the response contains exactly those N tools and no others — including tools that would normally be present in whichever `Toolset` preset is otherwise active.

**Acceptance Scenarios**:

1. **Given** an allowlist naming 3 real tools, **When** the MCP server starts and a client calls `list_tools`, **Then** the response contains exactly those 3 tools, regardless of the active `Toolset` preset.
2. **Given** an allowlist naming a tool that does not exist, **When** the MCP server starts, **Then** startup does not fail and the nonexistent name is silently ignored (matching the existing behavior of `IRIS_DISABLED_TOOLS` for unknown names).
3. **Given** both an allowlist and the existing `IRIS_DISABLED_TOOLS` blocklist name the same tool, **When** the MCP server starts, **Then** that tool is absent — the blocklist takes precedence over the allowlist for any name present in both.
4. **Given** an allowlist is set, **When** the CLI (`iris-agentic-dev tool <name>`), the MCP stdio transport, and the VS Code extension's toolset setting are all used against the same running configuration, **Then** all three observe the identical effective tool set — no consumer has a separate enforcement path that could disagree with the others.

**Delivered**: `IRIS_ENABLED_TOOLS` env var + `enabled_tools` field in `.iris-agentic-dev.toml`
(`workspace_config.rs`), enforced by pruning `self.tool_router` with the same
`remove_route()` primitive the `Toolset` presets and `IRIS_DISABLED_TOOLS` already use
(`with_registry_and_toolset` in `tools/mod.rs`) — no second enforcement path, satisfying
FR-003/AS-4 by construction rather than by convention. All four acceptance scenarios
covered by tests in `test_toolset.rs` (`test_enabled_tools_env_restricts_to_named_subset`,
`test_enabled_tools_env_unknown_name_is_ignored`, `test_disabled_tools_wins_over_enabled_tools_for_same_name`,
`test_enabled_tools_env_applies_on_top_of_toolset_pruning`) plus the toml/env-export path
in `test_workspace_config.rs`. AS-4's CLI/VS-Code-setting claim holds by construction
(one router, one enforcement point) rather than by a dedicated cross-consumer test — no
new test exercises the VS Code extension or the CLI specifically against an active
allowlist, so treat that half of AS-4 as architecturally true, not independently verified.

---

### User Story 2 - A downstream project declares a reusable tool subset (Priority: P2)

A team maintaining a downstream project (for example, an internal tool that only ever needs SQL introspection and code search) wants to declare, once, exactly which iris-agentic-dev tools their project depends on — the same way they already can declare a dependency on specific skills or knowledge-base items — rather than every developer on the team hand-copying an `IRIS_ENABLED_TOOLS` environment variable into their own shell profile or `.iris-agentic-dev.toml`.

**Why this priority**: This turns User Story 1's mechanism into something shareable and versionable, using infrastructure (`manifest::Provides`, the `Resolve`/lockfile pipeline) that already exists for skills, kb_items, and plugin binaries today. It depends on User Story 1 — there has to be an enforcement mechanism to resolve *into* before a manifest can declare a subset.

**Independent Test**: Author a manifest (`iris-agentic-dev.toml`) declaring `provides.tools = [...]` naming a subset. Run the existing install/resolve command against it and verify the resulting local configuration exposes exactly that subset when the MCP server next starts, with no other manual step.

**Acceptance Scenarios**:

1. **Given** a manifest declaring 5 named tools under `provides.tools`, **When** the install/resolve command runs, **Then** the local configuration is updated so that the next MCP server start exposes exactly those 5 tools.
2. **Given** a manifest declares a tool name that does not exist, **When** the install/resolve command runs, **Then** it reports the invalid name clearly rather than silently producing a configuration that omits it without explanation (this is stricter than User Story 1's silent-ignore, because a manifest is authored once and shared — a typo should be caught at declaration time, not discovered later by everyone who depends on it).
3. **Given** a manifest-declared subset and a locally-set `IRIS_ENABLED_TOOLS`/`IRIS_DISABLED_TOOLS` both apply, **When** the MCP server starts, **Then** the precedence between manifest-declared and locally-set values is well-defined and documented (not merely "whichever happens to run last").

---

### User Story 3 - Ship a smaller compiled artifact for a tool subset (Priority: P3)

A developer or downstream packager wants an iris-agentic-dev *binary* — not just a filtered MCP interface — that contains only the code for a chosen tool subset, for a smaller footprint or a narrower dependency tree.

**Why this priority**: This is the only user story that touches binary size or compile-time dependencies, and it is the most expensive by a wide margin: today all 90 `#[tool]`-annotated methods live in one `impl` block behind one `#[tool_router]` macro invocation, sharing `IrisTools`'s connection pool, config watcher, elicitation store, and telemetry session as common state. Splitting that apart into feature-gated modules is a real architectural refactor, not a configuration change. This story is scoped as an *evaluation*, not a committed deliverable — see Requirements below.

**Why this priority is last**: User Stories 1 and 2 deliver the "usable" and "declarable" halves of "separately installable/usable" almost entirely with infrastructure that already exists. Nothing about them requires committing to this story, and this story should not be started until there is evidence (from real usage of Stories 1–2) that interface-level subsetting is insufficient for whoever is asking.

**Independent Test**: Not a test — a written design spike evaluating feasibility, options, and cost, reviewed before any implementation work is scheduled.

**Acceptance Scenarios**:

1. **Given** the design spike is complete, **When** it is reviewed, **Then** it states explicitly whether a smaller binary is achievable without destabilizing `IrisTools`'s shared state, and if so, what the smallest viable refactor looks like.
2. **Given** the spike recommends proceeding, **When** a follow-up spec is written, **Then** it is a new, separate spec — this spec does not authorize implementation work for User Story 3 on its own.

---

### Edge Cases

- ~~What happens when an allowlist (User Story 1) is set to an empty list~~ — **Resolved**: empty means "no allowlist" (falls back to the active `Toolset` preset), not "expose zero tools." Documented on the `enabled_tools` field and covered by `test_enabled_tools_env_empty_string_means_no_allowlist`.
- What happens when a tool named in an allowlist exists in the tool registry but is currently write-gated or destructive-gated and those gates are not satisfied by the active connection — does the allowlist name it as present-but-blocked, or does it disappear entirely the way `iris_production_item`/`iris_credential_manage` already do today when write tools are disabled?
- What happens when a manifest-declared subset (User Story 2) is resolved on a machine that also has a `Toolset` preset explicitly set — which one is the "outer" constraint and which is the "inner" one?
- What happens when the CLI's own tool-name validation (`iris-agentic-dev tool <name>`) is asked to run a tool that exists in the registry but has been excluded by an active allowlist — is that the same "unknown tool" error as a genuinely nonexistent name, or a distinguishable "excluded by policy" error? (These must not be conflated — see the Dependencies section on why that distinction matters here specifically.)
- What happens to `docs/tools.md`'s tool-by-tool documentation when a subset is in effect — does documentation for excluded tools need to be hidden, or is "the tool exists in the project but isn't in your configured subset" an acceptable state for reference docs to leave undistinguished?

## Requirements _(mandatory)_

### Functional Requirements

- **FR-001**: ✅ Done. System MUST support an explicit tool allowlist — a named, comma-separated set of tool names — configurable via an `IRIS_ENABLED_TOOLS` environment variable and an `enabled_tools` field in `.iris-agentic-dev.toml`, symmetric to the existing `IRIS_DISABLED_TOOLS` blocklist mechanism.
- **FR-002**: ✅ Done. When both an allowlist and the blocklist name the same tool, the blocklist MUST win — that tool MUST be absent regardless of allowlist membership.
- **FR-003**: ✅ Done. The allowlist MUST be enforced by pruning routes from the same already-built `ToolRouter` the `Toolset` presets and `IRIS_DISABLED_TOOLS` already prune — no second, independent enforcement path may be introduced. (This project has already paid once for a second enforcement path disagreeing with the first — see Dependencies below — and must not do it again.)
- **FR-004**: `manifest::Provides` MUST gain an optional `tools: Vec<String>` field, resolved through the existing `Resolve`/lockfile pipeline used today for `skills`, `kb_items`, and `plugins`, and MUST write its resolved result through the same enforcement mechanism as FR-001 (env var or toml field) rather than a parallel code path.
- **FR-005**: A manifest-declared tool name (FR-004) that does not exist in the tool registry MUST be reported as an error at resolve time, not silently dropped — this is intentionally stricter than the allowlist's own silent-ignore behavior (FR-001 mirrors `IRIS_DISABLED_TOOLS`'s existing unknown-name tolerance; a manifest is authored once and consumed by everyone who depends on it, so a typo deserves a loud failure at the point of authorship).
- **FR-006**: System MUST produce a written design evaluation of whether a genuinely smaller compiled artifact (User Story 3) is achievable, before any implementation work toward it is scheduled. The evaluation MUST address `IrisTools`'s shared mutable state (connection pool, config watcher, elicitation store, telemetry session) explicitly, since that state is what makes today's single-binary architecture what it is.
- **FR-007**: The two skill-manifest files that currently disagree with each other (`iris-agentic-dev.toml` and `skills/iris-dev.toml`) MUST be reconciled to one authoritative file.
- **FR-008**: `docs/skills.md`'s claim that VS Code Copilot receives skills "automatically" via the extension MUST either be backed by an implementation, or corrected to describe actual behavior — the current text does not correspond to any code path in `vscode-iris-agentic-dev/`.
- **FR-009**: *(Completed ahead of this spec — see Assumptions & Dependencies.)* Every real tool MUST resolve to a `ToolCategory` or appear in an explicit, documented exemption list — no tool may silently bypass `check_env_gate` and `policy_gate` by having no category at all. This was a precondition for FR-001–002 to mean what they say: an allowlist mechanism built on top of a category taxonomy that half the tools don't participate in would have inherited the same silent-bypass problem it exists to close.

### Key Entities

- **Tool Allowlist**: A named, deny-list-symmetric set of tool names that, when set, restricts the effective tool surface to exactly those names (minus anything additionally blocked). Enforced at the same route-pruning step as the existing `Toolset` presets and blocklist.
- **Manifest Tool Subset**: A `provides.tools` declaration in a package manifest, resolved through the existing dependency/lockfile pipeline into a local allowlist — the tool-side analog of the `provides.skills` mechanism the 061 skill pack already uses.
- **Modular Build (exploratory)**: A hypothetical compiled artifact containing only the code for a chosen tool subset. Not committed by this spec — gated behind FR-006's design evaluation.

## Assumptions & Dependencies

- **Skills are substantially solved already.** Spec 061 (`specs/061-official-skill-pack`) shipped a file-based, binary-free, IRIS-free skill install path (`npx skills add intersystems-community/iris-agentic-dev`), which already satisfies the core of "skills separately installable/usable." This spec's skills-side requirements (FR-007, FR-008) are cleanup of loose ends discovered while scoping this request, not new mechanism.
- **The tool registry's single-source-of-truth problem has already been fixed as prerequisite work**, discovered and resolved in the course of scoping this spec: `IrisTools::registered_tool_names()` used to be an independently hand-maintained ~170-line mirror of the constructor's real route-pruning logic, and had already silently drifted from it — four real, callable tools (`agent_info`, `iris_list_containers`, `iris_select_container`, `iris_start_sandbox`) were absent from every toolset's reported name set, and two more (`iris_coverage`, `iris_doc_search`) were reported as Merged-only when the real router never removed them from Baseline/Nostub. It now derives directly from the live `ToolRouter`, which cannot drift from itself. Fixing it also surfaced and closed a second, independent gap: the CLI's own `TOOL_NAMES` validation list was missing 10 further real, dispatchable tool names. Both fixes, plus pinned-count regression tests (`test_baseline_tool_count`, `test_merged_tool_count`) and a `plugin.json`-vs-workspace version consistency test, landed in this branch ahead of this spec. Any FR above that reads or writes "the tool registry" is assumed to mean this now-authoritative source.
- **`ToolCategory`'s security taxonomy has since been completed** (FR-009, done ahead of this spec, not deferred as originally planned below). What changed the calculus: this was flagged as a security-policy judgment call best left to the tools' owners rather than a mechanical backfill — but on reflection, and once asked directly, categorizing was the right call to make immediately rather than defer, because the gap was not merely incomplete metadata. All 55 uncategorized tools (not 54 — `iris_get_log` was also missed in the original count) silently bypassed both `check_env_gate` (the `mcpTemplate=live/test` guarantee) and `policy_gate` (the `policy.<server>.allow` category allowlist `docs/connecting.md` documents as a way to lock a connection to read-only categories). Two were live gaps in a stated safety guarantee: `iris_ws_exec` (arbitrary code execution over a WebSocket terminal) and `iris_test`/`iris_coverage` (test execution) were invisible to the documented "live/test blocks Execute" rule. All 55 now have a category or an explicit, commented exemption (`check_config`, the one tool that makes no IRIS call at all), enforced going forward by `test_tool_category_coverage.rs` — a new tool with neither fails immediately, no live IRIS required.
- **`docs/tools.md` cross-checking against the live registry was evaluated and deferred.** Its `### \`name\`` heading convention is shared with two non-tool config-key sections (`write_tools_enabled`, `write_allowed_servers`), so a naive automated check produces false positives; a reliable check would need either a docs-structure change (a distinct marker for tool headings) or a maintained exception list. Not blocking for this spec, but worth doing before docs are relied upon as a subsetting reference surface.

## Success Criteria _(mandatory)_

### Measurable Outcomes

- **SC-001**: ✅ Done. A user can start the MCP server exposing only a caller-chosen subset of named tools and see exactly those tools — and no others — in a `list_tools` response, verified with no live IRIS container required. (`test_enabled_tools_env_restricts_to_named_subset` asserts this against `registered_tool_names()`, which is itself derived from the same router `list_tools` reads — see Dependencies.)
- **SC-002**: Not started (User Story 2 / FR-004–005).
- **SC-003**: ✅ Done. No regression in existing `Toolset` behavior (`baseline`/`nostub`/`merged`) — full `cargo test` suite (`--test-threads=1`) passes with zero failures after the allowlist change, plus `cargo clippy -- -D warnings` and `cargo fmt --all -- --check` clean.
- **SC-004**: The design evaluation for User Story 3 is written and reviewed, with an explicit go/no-go recommendation, before any code toward a modular binary is written.
- **SC-005**: The two previously-disagreeing skill manifest files are reconciled to one, and `docs/skills.md`'s VS Code Copilot claim matches actual behavior — verified by reading the file, not by re-asking whether it's true.
