# Specification Analysis Report: 098-server-probe

**Analyzed**: 2026-09-02
**Artifacts**: spec.md, plan.md, tasks.md, data-model.md, contracts/, research.md, constitution.md v1.3.2
**Status**: CRITICAL issues found — resolve before `/speckit.implement`

---

## Findings Table

| ID  | Category           | Severity | Location(s)                                                                             | Summary                                                                                                                                                                                                                                                                                                                                                                       | Recommendation                                                                                                                                                                                                                              |
| --- | ------------------ | -------- | --------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C1  | Constitution       | CRITICAL | plan.md Phase 3 step 2                                                                  | Coverage floor stated as "≥ 85% baseline maintained" but Constitution VIII requires ≥ 90%; T031 correctly targets 90% — plan.md is inconsistent with both the constitution and the tasks                                                                                                                                                                                      | Update plan.md Phase 3 step 2 to read "confirm ≥ 90%"                                                                                                                                                                                       |
| C2  | Constitution       | CRITICAL | tasks.md (no task), spec.md SC-002                                                      | SC-002 ("discover-then-add workflow end-to-end") has no corresponding test task — direct violation of Constitution IV: "SC items in the spec MUST map to at least one test"                                                                                                                                                                                                   | Add a live IRIS integration test task (e.g., T032) that calls `iris_test_server` ad-hoc → `iris_add_server` → asserts server appears in pool; gate on Phase 3                                                                               |
| C3  | Constitution       | CRITICAL | contracts/iris_test_server.md, tasks.md T013, data-model.md                             | Error code `MISSING_PARAMS` introduced by this feature is referenced in the contract and T013 but is NOT documented in data-model.md's error code section; Constitution requires "New error codes MUST be documented in data-model.md" and SCREAMING_SNAKE_CASE; `MISSING_PARAMS` does not appear in the standard registry                                                    | Add `MISSING_PARAMS` to data-model.md error codes section; confirm it does not conflict with existing `INVALID_PARAMS` (or use `INVALID_PARAMS` if semantically equivalent)                                                                 |
| I1  | Inconsistency      | HIGH     | tasks.md L4, spec.md L3                                                                 | Branch name mismatch: tasks.md header states `Branch: 102-server-probe`; spec.md states `Feature Branch: 098-server-probe`                                                                                                                                                                                                                                                    | Fix tasks.md L4 to read `098-server-probe`                                                                                                                                                                                                  |
| I2  | Inconsistency      | HIGH     | spec.md FR-005, data-model.md ProbeResult, tasks.md T003, contracts/iris_test_server.md | `auth` field is in data-model.md `ProbeResult`, in T003's field list, and in the contract output schema, but is NOT listed in FR-005's enumerated response shape (`reachable, iris_version, namespace, atelier_version, latency_ms, error`). The 401 edge case requires `auth` — FR-005 is incomplete                                                                         | Add `auth: bool` to FR-005's field list; update spec.md                                                                                                                                                                                     |
| I3  | Inconsistency      | HIGH     | contracts/iris_servers.md, data-model.md ProbeResult, tasks.md T023                     | `ProbeResult` includes `auth: bool`; T023 says "merge ProbeResult fields into each server entry"; but iris_servers.md probe=true contract does NOT include an `auth` field per entry — the merge behavior for `auth` in fleet probe output is unspecified                                                                                                                     | Explicitly decide: (a) include `auth` in iris_servers probe=true entries and add it to the contract, or (b) strip `auth` before merging; document the decision in the contract                                                              |
| I4  | Inconsistency      | MEDIUM   | plan.md risk notes (L109), spec.md US1 scenario 4, data-model.md TestServerParams       | The JSON key name for the existing pool-lookup parameter is ambiguous: spec acceptance scenarios write `iris_test_server(server="existing-server")` (using `server`), data-model.md names the field `name`, and plan.md risk notes explicitly flag this without resolution. No task resolves the ambiguity                                                                    | Add a task to verify the actual JSON parameter name in the existing `iris_test_server` dispatch (mod.rs ~L10483) and reconcile with `TestServerParams.name` field; update contract accordingly                                              |
| I5  | Inconsistency      | MEDIUM   | spec.md FR-007 + FR-009                                                                 | FR-007 specifies parallel probing with 5s per-server timeout; FR-009 specifies total response bounded by one 5s period — both state the same constraint from different angles, no explicit statement of what happens when all probes time out simultaneously (does the handler itself have a ceiling, or just the per-probe timeout?)                                         | Merge or cross-reference FR-007/FR-009; add an explicit statement that the handler has no additional outer timeout beyond the per-probe timeouts                                                                                            |
| U1  | Underspecification | HIGH     | spec.md edge cases, tasks.md                                                            | Timeout behavior — when a probe exceeds 5s — is described in spec edge case and US2 acceptance scenario 3, but no test task specifically targets a probe that times out via tokio::time::timeout (a live test against a deliberately closed/firewalled port differs from an actual timeout). T019 covers "down via a closed port" (fast refusal) but not a genuine 5s timeout | Add a unit or binary test that uses a non-routable address (192.0.2.x) and a short timeout override to verify the timeout path; or document that T004's `probe_result_unreachable_host_no_panic` covers this via the 100ms timeout override |
| U2  | Underspecification | MEDIUM   | spec.md FR-004                                                                          | FR-004 specifies the error message string but does not specify the error code (`MISSING_PARAMS`). Spec and task (T008) reference the message but not the code; the contract defines the code. This creates a gap between the spec requirement and the contract/implementation                                                                                                 | Add the error code to FR-004: `"MUST return error code MISSING_PARAMS with message 'Provide either a server name or host/web_port parameters.'"`                                                                                            |
| U3  | Underspecification | LOW      | spec.md edge cases                                                                      | Edge case: "`host` is provided but `web_port` is omitted → default to 52773" — specified in spec and data-model.md, but no unit test task explicitly tests this default; T006 uses `web_port: 52780` explicitly and T007/T008/T009 do not cover the default fallback                                                                                                          | Add a unit test assertion to T006 or a new test that deserializes `{"host":"localhost"}` and asserts `web_port == None` resolves to 52773 in the handler                                                                                    |
| U4  | Underspecification | LOW      | tasks.md T023                                                                           | T023 says "use `futures::future::join_all` (or `tokio::task::JoinSet` if `futures` not in workspace)" — but T001 is supposed to resolve this choice first. T023 still carries the conditional, suggesting the decision may not be captured in T001's output. If T001 finds `futures` is absent, T023's implementation changes materially                                      | T001 should produce a documented decision (code comment + tasks.md note) that T023 can reference; T023 description should reference that decision rather than re-carrying the conditional                                                   |
| D1  | Duplication        | LOW      | spec.md FR-007 + FR-009                                                                 | See I5 above — FR-007 and FR-009 express the same parallelism constraint from two angles                                                                                                                                                                                                                                                                                      | Cross-reference or merge; see I5 recommendation                                                                                                                                                                                             |
| D2  | Duplication        | LOW      | tasks.md T022 + T023 vs. T021                                                           | T022 and T023 re-describe the probe=false / probe=true logic already summarized in T021's handler signature change description; minor prose overlap                                                                                                                                                                                                                           | Acceptable; T021 sets up the handler, T022/T023 implement the two paths — keep as-is but ensure T021 is clearly a signature-only task                                                                                                       |
| A1  | Ambiguity          | MEDIUM   | spec.md FR-005                                                                          | "response shape MUST be identical" is unverifiable without an explicit field list. The field lists in FR-005, data-model.md, and the contract partially disagree (the `auth` omission in FR-005). "Identical" needs to be anchored to a canonical source                                                                                                                      | Anchor FR-005's "identical shape" to the contract file (`contracts/iris_test_server.md`) or the data-model.md `ProbeResult`; make one of these the normative shape reference                                                                |
| E1  | Coverage Gap       | MEDIUM   | spec.md SC-002, tasks.md                                                                | SC-002: no test task (also flagged as C2)                                                                                                                                                                                                                                                                                                                                     | See C2                                                                                                                                                                                                                                      |
| E2  | Coverage Gap       | LOW      | plan.md risk notes (L107)                                                               | Plan risk note: "`iris_servers` currently has no params — adding `IrisServersParams` changes handler signature; must verify dispatch macro accepts the change without breaking `tools/list` schema for existing callers" — no task specifically runs `tools/list` and validates the schema before and after the change                                                        | Add a binary invocation test assertion that `tools/list` output for `iris_servers` is valid JSON-Schema after the change; or fold into T018                                                                                                 |
| X1  | Phantom Reference  | LOW      | Task brief                                                                              | The invoking context referenced "FR-011" as covering shared probe logic; spec.md has no FR-011 (FR-001 through FR-010 only); FR-010 covers the shared function requirement                                                                                                                                                                                                    | No change needed in spec; note for awareness: if any downstream doc references FR-011, update to FR-010                                                                                                                                     |

---

## Coverage Summary Table

| Requirement Key | Has Task? | Task IDs         | Notes                                     |
| --------------- | --------- | ---------------- | ----------------------------------------- |
| FR-001          | Yes       | T006, T007, T012 | Complete                                  |
| FR-002          | Yes       | T010, T011, T013 | Complete                                  |
| FR-003          | Yes       | T009, T013       | Complete                                  |
| FR-004          | Yes       | T008, T013       | Error code gap (see U2)                   |
| FR-005          | Yes       | T010, T011, T013 | `auth` field omission (see I2)            |
| FR-006          | Yes       | T016, T017, T022 | Complete                                  |
| FR-007          | Yes       | T019, T023       | Timeout path underspecified (see U1)      |
| FR-008          | Yes       | T019, T023       | Complete                                  |
| FR-009          | Yes       | T019, T023       | Redundant with FR-007 (see I5)            |
| FR-010          | Yes       | T005             | Complete                                  |
| SC-001          | Yes       | T011, T013       | Complete                                  |
| SC-002          | **No**    | —                | CRITICAL — no test task (Constitution IV) |
| SC-003          | Yes       | T019, T023       | Complete                                  |
| SC-004          | Yes       | T018, T022       | Complete                                  |
| SC-005          | Yes       | T007, T013       | Complete                                  |

---

## Constitution Alignment Issues

| Principle                      | Status               | Detail                                                                                                         |
| ------------------------------ | -------------------- | -------------------------------------------------------------------------------------------------------------- |
| I. Zero-Install Binary         | PASS                 | No new runtime deps; `futures` or tokio::task::JoinSet both already present or in-workspace                    |
| II. ObjectScript Sanity Gate   | N/A                  | Pure HTTP probe only                                                                                           |
| III. HTTP-First Execution      | PASS                 | Atelier REST probe; no docker exec in probe path                                                               |
| IV. Test-First, Fixture-Driven | **FAIL**             | SC-002 has no test task (C2); per constitution "SC items MUST map to at least one test"                        |
| V. Output Shape Parity         | **WARN**             | `auth` field missing from FR-005 enumeration (I2); `auth` handling in fleet probe output undefined (I3)        |
| VI. Environment Guard          | N/A                  | Read-only probe; no write capability introduced                                                                |
| VII. Dependency Minimalism     | PASS                 | T001 defers decision; research.md documents `futures` rationale                                                |
| VIII. 90% Coverage Gate        | **FAIL**             | plan.md Phase 3 step 2 contradicts constitution by citing 85% baseline (C1); T031 correctly cites 90%          |
| IX. Tool Lift Requirement      | N/A (marked in plan) | Enhancement to existing tools, not new tools; plan.md N/A designation is defensible but undocumented           |
| X. ObjectScript Coverage Gate  | N/A                  | No ObjectScript introduced                                                                                     |
| Error Code Registry            | **FAIL**             | `MISSING_PARAMS` not documented in data-model.md; potentially conflicts with established `INVALID_PARAMS` (C3) |

---

## Unmapped Tasks

All tasks map to at least one requirement or user story. No orphan tasks detected.

---

## Metrics

| Metric                                 | Value                        |
| -------------------------------------- | ---------------------------- |
| Total Functional Requirements          | 10 (FR-001 – FR-010)         |
| Total Success Criteria                 | 5 (SC-001 – SC-005)          |
| Total Tasks                            | 31 (T001 – T031)             |
| Requirement Coverage (FR with ≥1 task) | 10/10 = 100%                 |
| SC Coverage (SC with ≥1 task)          | 4/5 = 80% (SC-002 uncovered) |
| Ambiguity Findings                     | 1 (A1)                       |
| Duplication Findings                   | 2 (D1, D2)                   |
| Inconsistency Findings                 | 5 (I1–I5)                    |
| Underspecification Findings            | 4 (U1–U4)                    |
| Constitution Violations                | 3 CRITICAL (C1, C2, C3)      |
| Coverage Gap Findings                  | 2 (E1, E2)                   |
| Total Findings                         | 17                           |
| Critical Issues                        | 3                            |

---

## Next Actions

### CRITICAL — Resolve Before `/speckit.implement`

1. **C1 — Plan coverage floor**: Edit plan.md Phase 3 step 2 to say "≥ 90%" (not 85%).
   Constitution VIII is non-negotiable.

2. **C2 — SC-002 missing test task**: Add T032 (live IRIS integration test) for the
   discover-then-add workflow: `iris_test_server` ad-hoc → `iris_add_server` → assert pool entry.
   Place in Phase 3, depends on T011.

3. **C3 — MISSING_PARAMS error code**: Add `MISSING_PARAMS` to data-model.md error codes section.
   Decide whether to reuse `INVALID_PARAMS` (already in registry) or introduce `MISSING_PARAMS`
   as a distinct code; document the decision. If introducing a new code, add it before coding.

### HIGH — Address Before or During Implementation

4. **I1 — Branch name in tasks.md**: Fix the `Branch:` header in tasks.md from `102-server-probe`
   to `098-server-probe`. One-line fix.

5. **I2 — `auth` field missing from FR-005**: Add `auth: bool` to FR-005's field enumeration.
   This is required for the 401 path and is already in the data model, contract, and T003.

6. **I3 — `auth` in fleet probe output**: Explicitly decide whether `auth` appears in
   `iris_servers` probe=true entries and update the contract accordingly. Without this decision,
   T023 implementation will be ambiguous.

7. **I4 — `server` vs. `name` param name**: Add a task (or extend T001) to verify the actual
   JSON parameter name used by the existing `iris_test_server` dispatch in mod.rs, confirm it
   matches `TestServerParams`'s `name` field, and document the finding.

### MEDIUM — Recommended Improvements

8. **U2 — FR-004 error code gap**: Add `MISSING_PARAMS` code to FR-004 text to match contract
   and implementation intent.

9. **I5 / D1 — FR-007 / FR-009 overlap**: Add a cross-reference note in FR-007 or FR-009
   clarifying that they express the same constraint and that no outer handler timeout exists
   beyond individual per-probe timeouts.

10. **A1 — FR-005 shape reference**: Anchor "identical shape" in FR-005 to the normative source
    (recommend `contracts/iris_test_server.md` as the single source of truth for response shape).

### LOW — Optional Polish

- U1: Clarify that T004's 100ms timeout override covers the genuine timeout path; if not,
  add a note to T019 about how timeout behavior is exercised.
- U3: Extend T006 or add a micro-test for the `web_port` omitted → default 52773 path.
- U4: After T001 resolves the parallel primitive, update T023 to reference the decision
  rather than carrying the conditional.
- E2: Fold a `tools/list` schema validation assertion into T018 or add as T033.

---

## Remediation

Would you like concrete remediation edits for the top issues (C1, C2, C3, I1, I2, I3)?
These are the six blockers. I can produce exact text changes to spec.md, plan.md, tasks.md,
and data-model.md for each.
