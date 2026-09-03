# Specification Analysis Report: 093-toml-hot-reload

**Date**: 2026-09-02
**Artifacts analyzed**: `spec.md`, `plan.md`, `tasks.md`, `data-model.md`, `contracts/iris_reload_pool.md`
**Constitution version**: 1.3.2

---

## Findings Table

| ID  | Category       | Severity | Location(s)                                      | Summary                                                                                                                                                                                                                                                                                                                                                                                                | Recommendation                                                                                                                                                                                                                                                                      |
| --- | -------------- | -------- | ------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| I1  | Inconsistency  | CRITICAL | tasks.md T003, T006 vs plan.md §R-004, §Phase 1  | `Arc<Mutex<...>>` in tasks vs `Arc<RwLock<...>>` in plan and data-model. T003/T006 use `Mutex` and `.lock()`. Plan §R-004, data-model, and contract all specify `RwLock` and `.read()`/`.write()`. One type must be chosen before implementation.                                                                                                                                                      | Decide on `RwLock` (aligns with plan rationale: concurrent readers, single writer) and make tasks.md T003, T006 consistent. Document the choice in data-model.md.                                                                                                                   |
| I2  | Inconsistency  | HIGH     | tasks.md T003 line 29 vs data-model.md           | T003 says call sites use `self.pool.lock().unwrap()` (Mutex API). Data-model specifies `.read().unwrap()` (RwLock API). Whichever primitive is chosen, all descriptions must agree.                                                                                                                                                                                                                    | Align T003's call-site migration text to match the chosen primitive.                                                                                                                                                                                                                |
| I3  | Inconsistency  | HIGH     | tasks.md T003 ("six call sites") vs mod.rs       | T003 says "Update all six call sites … that call `self.pool.get(...)` or `self.pool.source_of(...)`". Actual count in `mod.rs` is 20 occurrences of `self.pool.`. Under-counting callsites risks an incomplete migration and compile errors.                                                                                                                                                           | Replace "six call sites" with "all call sites" (or enumerate precisely). Add a post-T003 checkpoint that `grep -c 'self\.pool\.'` returns 0 against the unwrapped type.                                                                                                             |
| I4  | Inconsistency  | HIGH     | tasks.md §Phase 5 T025 vs constitution §VIII     | T025 uses `IRIS_WEB_PORT=52780`. Constitution §VIII canonical coverage command uses `IRIS_PORT=52780`. These are different env vars; if the binary reads `IRIS_PORT`, T025's command will silently fail to connect.                                                                                                                                                                                    | Verify which env var the binary actually reads and align T025 and constitution §VIII to the same name. The CLAUDE.md project table (port 52780) is not affected, but the var name must be consistent.                                                                               |
| C1  | Coverage Gap   | HIGH     | spec.md FR-001 — `servers_loaded` field          | FR-001 specifies the success response includes `servers_loaded: N`. No task explicitly asserts this field in any test. T007 asserts the server name appears in `servers`, but does not assert `servers_loaded` is correct.                                                                                                                                                                             | Add an assertion for `servers_loaded` to T007 (or T005) so the count field is tested.                                                                                                                                                                                               |
| C2  | Coverage Gap   | HIGH     | spec.md SC-002 (auto-reload within one call)     | SC-002 says "a manual toml edit is reflected within one subsequent tool call." T013 writes to toml and bumps mtime then calls `check_reload` directly — but does not assert timing (i.e., that the swap happens _before_ the tool response completes). T014 is the binary E2E but it writes the entry and immediately calls `iris_servers` with no mtime bump step.                                    | T014 must include an explicit mtime bump step (or `touch`) between writing the toml and calling `iris_servers`. Clarify whether bumping mtime or waiting for mtime change is the correct mechanism.                                                                                 |
| U1  | Underspecified | HIGH     | tasks.md T001 + T002                             | T001 adds `auto_reload: bool` to `WorkspaceConfig`. T002 tests it. Neither spec.md nor plan.md define this field, its TOML key name, or any functional requirement for it. There is no FR-_ or SC-_ that references `auto_reload`. The field appears to be a placeholder that was not fully designed.                                                                                                  | Either remove T001/T002 (and the field) or add an FR to spec.md describing the `auto_reload` flag, its effect, and when it should default to `true` vs `false`. If `auto_reload` enables the background ConfigWatcher path, that must be specified.                                 |
| U2  | Underspecified | HIGH     | tasks.md T022 + data-model.md §Error codes       | T022 says "Add `RELOAD_PARSE_ERROR` (or confirm `TOML_PARSE_ERROR` if that name is chosen)". The error code name is unresolved. Constitution §Error Code Registry requires all error codes to be documented in `data-model.md`; the current data-model entry explicitly says "(no error_code on parse failure)". The contract omits an `error_code` field entirely on parse failure.                   | Decide on a name before implementation. Given no `error_code` is in the contract today, either add it (with a chosen name) to data-model and contract, or explicitly document that parse failures omit `error_code` (and remove T022's ambiguity). Either way, resolve before T009. |
| U3  | Underspecified | MEDIUM   | spec.md US2 acceptance scenario 1                | "Within one tool call cycle — the next time any iad tool is called — the pool reflects the change automatically." This says "any iad tool" but `check_reload` is called only at specific call sites (mod.rs lines 2708, 4617, 4716, 4820, 8500 per plan §R-002). If an agent calls a tool that does NOT invoke `check_reload`, the pool is not refreshed. The spec overstates the guarantee.           | Clarify to "tools that invoke `check_reload`" or change the implementation to call `check_reload` on every tool dispatch.                                                                                                                                                           |
| A1  | Ambiguity      | MEDIUM   | spec.md Edge Case 1 ("Arc swap is atomic")       | "Arc swap is atomic; one reload wins, the other sees the updated pool." This describes the desired behavior but the current `Arc<ConnectionPool>` field has no interior mutability — the swap itself cannot happen from `&self` without either `Mutex` or `RwLock`. The edge case description presupposes the refactor (T003) is done, but reads as if the current field already supports atomic swap. | Add a note that this edge case relies on the T003 field change. No spec text change is critical here, but a reader could be confused.                                                                                                                                               |
| A2  | Ambiguity      | MEDIUM   | tasks.md T009 — "determine the config file path" | T009 says "determine the config file path (from `self.config_watcher` if set, else `None`)". It is not specified what `load_pool(None)` does — plan §R-003 says "No async, no panics on missing file (returns empty pool)", which means passing `None` silently empties the pool on a `None` watcher. This could wipe a valid pool.                                                                    | T009 must explicitly guard: if `config_watcher` is `None`, return an error or a note rather than calling `load_pool(None)` and wiping the pool. Add this guard to spec FR-002 or T009.                                                                                              |
| A3  | Ambiguity      | MEDIUM   | tasks.md Phase 1 (branch header)                 | The branch field in tasks.md reads: `093-toml-hot-reload (working on 102-server-probe)`. This is a stale copy-paste artifact from another spec. It does not block implementation but introduces confusion about which branch this tasks.md belongs to.                                                                                                                                                 | Remove the `(working on 102-server-probe)` parenthetical.                                                                                                                                                                                                                           |
| D1  | Duplication    | LOW      | plan.md §Phase 1 data-model + data-model.md      | The `iris_reload_pool` response JSON shapes are defined in three places: plan.md §Phase 1, data-model.md, and contracts/iris_reload_pool.md. The plan and data-model are identical; the contract adds invariants but repeats the JSON.                                                                                                                                                                 | Keep contracts/ as canonical; reduce plan.md to a prose reference ("see data-model.md and contracts/"). No functional impact.                                                                                                                                                       |
| D2  | Duplication    | LOW      | spec.md FR-007 + contract invariant 3            | FR-007 states "`note` field always present". Contract invariant 3 states "`note` field always present". Same rule in two places.                                                                                                                                                                                                                                                                       | Keep contract as authoritative; FR-007 is fine as the spec-level statement — this duplication is acceptable and consistent. No action needed.                                                                                                                                       |

---

## Coverage Summary Table

| Requirement Key                   | Has Task? | Task IDs                  | Notes                                                                     |
| --------------------------------- | --------- | ------------------------- | ------------------------------------------------------------------------- |
| fr-001-iris-reload-pool-tool      | YES       | T009, T010                | Success response shape fully covered; `servers_loaded` assertion gap (C1) |
| fr-002-arc-swap-pool-rebuild      | YES       | T003, T009                | Mutex vs RwLock inconsistency (I1) must resolve first                     |
| fr-003-config-watcher-pool-extend | YES       | T016                      | Only one task; gap around `check_reload` call site coverage (U3)          |
| fr-004-fail-safe-parse-error      | YES       | T006, T015                | T006 uses Mutex (inconsistency I1); otherwise well covered                |
| fr-005-read-only-gate             | YES       | T009 (inline), T010, T026 | Covered                                                                   |
| fr-006-iris-servers-reflects-pool | YES       | T007, T008                | Covered                                                                   |
| fr-007-note-field-mcp-constraint  | YES       | T009                      | Covered inline in T009 implementation description                         |
| tr-001-unit-toml-round-trip       | YES       | T005                      | Covered                                                                   |
| tr-002-binary-invocation          | YES       | T007                      | Covered; `servers_loaded` assertion gap (C1)                              |
| tr-003-live-iris                  | YES       | T008                      | Covered                                                                   |
| sc-001-same-session-workflow      | YES       | T007, T008                | Covered                                                                   |
| sc-002-auto-reload-one-call       | YES       | T013, T014                | mtime bump gap in T014 (C2)                                               |
| sc-003-parse-error-no-wipe        | YES       | T006                      | Covered (subject to I1 resolution)                                        |
| sc-004-binary-invocation-e2e      | YES       | T007                      | Covered                                                                   |
| us1-agent-adds-server-immediately | YES       | T005–T012                 | Well covered; I1 must resolve                                             |
| us2-manual-edit-auto-reflect      | YES       | T013–T016                 | mtime timing gap (C2); US2 scope ambiguity (U3)                           |

**Unmapped tasks**:

- T001 (`auto_reload` field) — no FR or SC in spec maps to this. (U1)
- T002 (test for `auto_reload`) — same, no spec-level requirement. (U1)

---

## Constitution Alignment Issues

| Principle                        | Status  | Detail                                                                                                                                                                                                                                                                                                                                                                                                                             |
| -------------------------------- | ------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| IV. Test-First                   | PASS    | Tests appear before implementation within each phase. ✓                                                                                                                                                                                                                                                                                                                                                                            |
| VI. Environment Guard            | PASS    | `iris_reload_pool` classified read-only in write gate. ✓                                                                                                                                                                                                                                                                                                                                                                           |
| VII. Dependency Minimalism       | PASS    | No new crates. `RwLock`/`Mutex` from std. ✓                                                                                                                                                                                                                                                                                                                                                                                        |
| VIII. 90% Coverage Gate          | PASS    | T025 covers coverage run; IRIS_WEB_PORT vs IRIS_PORT inconsistency (I4) may cause silent connection failure. **Verify env var name.**                                                                                                                                                                                                                                                                                              |
| IX. Tool Lift Requirement        | PASS    | Phase 4 (T017–T019) precedes Polish (Phase 5). lift-results.md required. ✓                                                                                                                                                                                                                                                                                                                                                         |
| X. ObjectScript Coverage Gate    | N/A     | Pure Rust feature. Constitution explicitly exempts. ✓                                                                                                                                                                                                                                                                                                                                                                              |
| §Error Code Registry             | PARTIAL | T022 has unresolved error code name (`RELOAD_PARSE_ERROR` vs `TOML_PARSE_ERROR`); data-model currently omits `error_code` on parse failure entirely. Resolve before T009. (U2)                                                                                                                                                                                                                                                     |
| Constitution Check table in plan | WARN    | Plan §R-004 is marked "NEEDS CLARIFICATION" for the pool swap mechanism. Constitution §Compliance Review states: "A plan that has 'NEEDS CLARIFICATION' in any Constitution Check gate MUST NOT proceed to implementation." R-004 is _not_ in the Constitution Check table (it was resolved in the design phase), so this is not a formal block — but the research note should be updated to "RESOLVED" with the chosen primitive. |

---

## Metrics

| Metric                                            | Value                                                   |
| ------------------------------------------------- | ------------------------------------------------------- |
| Total functional requirements (FR + TR + SC + US) | 16                                                      |
| Total tasks                                       | 27                                                      |
| Requirements with ≥ 1 task (coverage %)           | 14/16 = 87.5% (T001/T002 unmapped to spec requirements) |
| Ambiguity findings                                | 3                                                       |
| Inconsistency findings                            | 4                                                       |
| Duplication findings                              | 2                                                       |
| Underspecification findings                       | 3                                                       |
| Coverage gap findings                             | 2                                                       |
| **CRITICAL issues**                               | **1**                                                   |
| HIGH issues                                       | 5                                                       |
| MEDIUM issues                                     | 3                                                       |
| LOW issues                                        | 2                                                       |

---

## Next Actions

### CRITICAL — resolve before `/speckit.implement`

**I1 (Mutex vs RwLock)** is the blocking issue. Every downstream task in Phase 1–3 compiles against the pool field type. Choosing `Mutex` or `RwLock` now avoids a mid-implementation rework. The plan recommendation is `RwLock` (read-heavy workload with infrequent swaps); accept that and update tasks.md T003 and T006 to use `RwLock` + `.read()`/`.write()`.

### HIGH — strongly recommended before implementation

1. **I3** (callsite count): Replace "six call sites" in T003 with "all call sites" and add a post-merge grep assertion. The actual count is 20.
2. **I4** (IRIS_WEB_PORT vs IRIS_PORT): Confirm which env var the binary reads for the web port. If the binary reads `IRIS_WEB\_PORT`, update the constitution §VIII canonical command. If it reads `IRIS_PORT`, fix T025.
3. **U1** (`auto_reload` field): Either add an FR to spec.md for this field or remove T001/T002. The field has no spec-level justification today.
4. **U2** (error code name): Pick a name (`RELOAD_PARSE_ERROR` is clearer) and update data-model.md and the contract before T009 runs.
5. **C1** (`servers_loaded` assertion): Add an assertion for the count field to T007 (binary E2E).

### MEDIUM — address before Polish

- **U3**: Narrow the US2 acceptance scenario 1 claim to "tools that invoke `check_reload`" or widen the implementation.
- **A2**: Add a guard in T009 for `config_watcher = None` to prevent silent pool wipe.
- **A3**: Remove the stale branch parenthetical from tasks.md header.

### Commands

- To tighten the spec: manually edit `specs/093-toml-hot-reload/spec.md` to add an FR for `auto_reload` (or confirm its removal).
- To resolve I1: manually edit `tasks.md` T003 and T006, changing `Mutex` → `RwLock` and `.lock()` → `.read()`/`.write()`.
- To fix I4: run `grep -rn 'IRIS_WEB_PORT\|IRIS_PORT' crates/iris-agentic-dev-core/src/` to determine which env var the binary actually reads.
- After fixes, re-run `/speckit.analyze` to confirm zero CRITICAL findings before proceeding to `/speckit.implement`.

---

Would you like me to suggest concrete remediation edits for the top findings (I1, I3, I4, U1, U2)?
