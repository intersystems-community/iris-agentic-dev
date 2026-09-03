# Specification Analysis Report: 095-password-persistence

**Analyzed**: 2026-09-02 | **Artifacts**: spec.md, plan.md, tasks.md, data-model.md,
contracts/iris_add_server.md, constitution.md v1.3.2

---

## Findings Table

| ID  | Category           | Severity | Location(s)                                        | Summary                                                                                                                                                                    | Recommendation                                                                                                                                                                                                                                                                                          |
| --- | ------------------ | -------- | -------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| A1  | Ambiguity          | HIGH     | spec.md:L95                                        | FR-006 says "Optionally add `has_plaintext_credential`" — the word "optionally" makes this a SHOULD, not a MUST                                                            | Remove "optionally". The contracts/, data-model.md, and T026/T027 all treat it as definite. Align spec to match.                                                                                                                                                                                        |
| A2  | Ambiguity          | MEDIUM   | spec.md:L45–48                                     | US1 test hook mentions `IAD_MOCK_KEYCHAIN_UNAVAILABLE=1` as a possibility, then plan.md discards it — residual text misleads                                               | Delete the parenthetical "(or similar test hook)" from spec.md:L46 to match the resolved plan decision.                                                                                                                                                                                                 |
| A3  | Ambiguity          | MEDIUM   | tasks.md:T019                                      | T019 says "stub `resolve_credential` to return Err" — `resolve_credential` is a plain free function; no mock seam exists                                                   | Replace primary approach with the temp-file path: write a temp servers.json with `"password": "SYS"`, call `load_from_path`, inspect `conn.password`. Drop stub option.                                                                                                                                 |
| I1  | Inconsistency      | HIGH     | spec.md:L29 vs. spec.md:L138–139, L161–162         | Overview says "server is usable immediately"; Success Criteria and Assumptions correctly qualify "after pool reload"                                                       | Fix the Overview sentence (L29) to match: "server is usable after iad restarts or the pool reloads (spec 093)."                                                                                                                                                                                         |
| I2  | Inconsistency      | MEDIUM   | spec.md:L90–91 vs. tasks.md:T024                   | FR-005 says iris_remove_server "clears the password field (set to `None`)" implying an explicit zero-out; T024 notes "no explicit action needed if entry is fully removed" | Clarify in both artifacts which behavior is required: full-entry removal (current) already satisfies the invariant. FR-005 should say "entry is fully removed, leaving no password in servers.json" to match the implementation.                                                                        |
| I3  | Inconsistency      | LOW      | plan.md:L35 vs. constitution.md Release Discipline | Plan claims "≥ 90% check"; constitution Release Discipline (L316) says `scripts/coverage.sh ≥ 88%`. These thresholds differ.                                               | Note is informational — Principle VIII (the MUST) sets 90% as the gate. T033 must enforce 90%, not 88%. No change needed in this spec; the constitution inconsistency is a separate concern.                                                                                                            |
| I4  | Inconsistency      | LOW      | tasks.md:T033                                      | T033 says "assert ≥ 90% (or document gap)" — the "or document gap" clause weakens a constitution MUST                                                                      | Remove the escape clause. The constitution is explicit: if coverage drops below 90%, the merge is blocked. T033 should read "assert ≥ 90%; if not, add tests — do not merge."                                                                                                                           |
| C1  | Coverage Gap       | HIGH     | spec.md SC items vs. tasks.md                      | SC2 (keychain available → no behavior change) has no dedicated test task                                                                                                   | Add a task in Phase 3: binary invocation test verifying that when keychain succeeds, response has no `stored_plaintext` field. T012 gestures at this but marks it "skip on Linux" without a positive assertion.                                                                                         |
| C2  | Coverage Gap       | MEDIUM   | spec.md:US1 only vs. tasks.md Phase 4–5            | tasks.md introduces "User Story 2" (pool credential resolution) and "User Story 3" (remove/list) with no backing user story text in spec.md                                | Add US2 and US3 story blocks to spec.md, or rename Phase 4–5 headings in tasks.md to "FR-004 implementation" and "FR-005/FR-006 implementation" to avoid implying story backing that doesn't exist.                                                                                                     |
| C3  | Coverage Gap       | MEDIUM   | spec.md:L117–132 (Test Layers) vs. tasks.md        | Layer 3 ("No live IRIS integration needed") has no coverage-check task verifying that existing connection tests exercise the fallback credential path                      | Constitution Principle VIII MUST: "Every new tool action MUST have at least one integration test that exercises the happy path." Even for config-only changes, a live integration test that adds a server via plaintext and connects should be present. Add a task or justify the exception explicitly. |
| C4  | Coverage Gap       | LOW      | spec.md:L135–143 (Success Criteria)                | SC5 (no password + no keychain → added without credential, response notes no credential stored) — the "response notes" part is not covered by a test task                  | Add assertion to T015 verifying the no-password edge-case response shape (no `stored_plaintext`, no `warning`, `added: true`).                                                                                                                                                                          |
| D1  | Duplication        | LOW      | plan.md:L165–197 vs. data-model.md + contracts/    | plan.md Phase 1 inline reproduces the full data model and contract shapes already captured in data-model.md and contracts/iris_add_server.md                               | Plan's inline content is not harmful, but becomes a maintenance hazard. Consider replacing with cross-references to the dedicated files.                                                                                                                                                                |
| U1  | Underspecification | MEDIUM   | spec.md:FR-004                                     | "Locate the resolution point in connection_pool.rs or discovery.rs" — ambiguous; plan.md resolves this (connection_pool.rs:L199–218) but the spec text remains unresolved  | Update FR-004 to name the confirmed location: `connection_pool.rs:~199–218, load_pool function`.                                                                                                                                                                                                        |
| U2  | Underspecification | MEDIUM   | tasks.md:T011                                      | T011 asserts "servers.json written to temp dir contains `password` key" but does not specify how the binary invocation test controls the servers.json write path           | Add: the test must set `IAD_SERVERS_JSON` or equivalent env var (or a `--config-dir` override) so the binary writes to a temp dir, not `~/.config/iris-agentic-dev/servers.json`. If no such override exists, a task to add it must precede T011.                                                       |
| U3  | Underspecification | LOW      | tasks.md:T012                                      | T012 says "skip on Linux" for the keychain-success path test, but gives no mechanism (platform `#[cfg]`, runtime skip, or `#[ignore]` with note)                           | Specify: use `#[cfg_attr(target_os = "linux", ignore)]` or a runtime `if cfg!(target_os = "linux") { return; }` skip pattern. Document it explicitly.                                                                                                                                                   |

---

## Coverage Summary Table

| Requirement Key                        | Has Task? | Task IDs   | Notes                                                |
| -------------------------------------- | --------- | ---------- | ---------------------------------------------------- |
| `server-entry-password-field`          | Yes       | T004–T009  | Full TDD cycle; JSON round-trip test included        |
| `iris-add-server-plaintext-fallback`   | Yes       | T010–T017  | Binary invocation test present; SC2 gap flagged (C1) |
| `plaintext-fallback-success-response`  | Yes       | T011, T014 | Response shape tested via binary invocation          |
| `credential-resolution-pool-fallback`  | Yes       | T018–T023  | Test approach partially underspecified (A3, U2)      |
| `remove-server-clears-password`        | Yes       | T024–T028  | FR-005 vs T024 semantics mismatch flagged (I2)       |
| `servers-listing-no-password-exposure` | Yes       | T026–T027  | "Optionally" ambiguity in spec (A1)                  |
| `docs-update-connecting`               | Yes       | T030, T035 | Both content and lint tasks present                  |

All 7 functional requirements have at least one associated task. Coverage = **100%**.

---

## Constitution Alignment Issues

| Principle               | Status  | Finding                                                                                                                                                                                                                                                                  |
| ----------------------- | ------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| IV. Test-First          | PARTIAL | SC2 (keychain success path) has no dedicated positive-assertion test. Constitution requires every SC item to map to at least one test. See C1.                                                                                                                           |
| VIII. 90% Coverage Gate | PARTIAL | T033 includes an "or document gap" escape clause that violates the constitution MUST. See I4. The spec also claims "No live IRIS integration needed" (Test Layer 3) which conflicts with VIII's "every new tool action MUST have at least one integration test." See C3. |

No CRITICAL constitution violations found — but two PARTIAL alignment issues (C1, C3, I4) must be resolved before `/speckit.implement`.

---

## Unmapped Tasks

| Task      | Maps To                   | Notes                                   |
| --------- | ------------------------- | --------------------------------------- |
| T001      | Setup / container verify  | Infrastructure task; no FR required     |
| T002      | Setup / baseline test     | Infrastructure task; no FR required     |
| T003      | Setup / baseline coverage | Infrastructure task; no FR required     |
| T009      | FR-001 (fmt gate)         | Quality gate; maps implicitly to FR-001 |
| T017      | FR-002 (clippy gate)      | Quality gate; maps implicitly to FR-002 |
| T031–T036 | Polish / all FRs          | Polish gates; no single FR required     |

All infrastructure and polish tasks are intentional; none represent orphaned implementation work.

---

## Metrics

| Metric                        | Value |
| ----------------------------- | ----- |
| Total Functional Requirements | 7     |
| Total Tasks                   | 36    |
| Coverage % (FRs with ≥1 task) | 100%  |
| Ambiguity Findings            | 3     |
| Inconsistency Findings        | 4     |
| Coverage Gap Findings         | 4     |
| Underspecification Findings   | 3     |
| Duplication Findings          | 1     |
| CRITICAL Issues               | 0     |
| HIGH Issues                   | 3     |
| MEDIUM Issues                 | 6     |
| LOW Issues                    | 5     |

---

## Next Actions

No CRITICAL issues block implementation. Three HIGH issues should be resolved first:

1. **A1** — Remove "Optionally" from FR-006 in spec.md. The field is treated as definite
   in all downstream artifacts.

2. **C1** — SC2 (keychain success → no `stored_plaintext`) needs a dedicated positive-
   assertion test. T012 partially covers this but lacks a clear assertion and has an
   unspecified skip mechanism. Add or strengthen T012 before coding Phase 3.

3. **I1** — Fix the Overview sentence "server is usable immediately" to accurately reflect
   the restart/pool-reload requirement that the rest of the spec correctly describes.

Then, before implementation begins, address MEDIUM issues:

- **A3** / **U2**: Confirm the temp-file test approach for T019 is viable (it is — `load_from_path` accepts a path), and verify that a test env var or `--config-dir` flag exists (or plan a task to add one) so T011's binary invocation test writes to a temp dir.
- **C2**: Add US2 and US3 story text to spec.md, or rename the phase headings in tasks.md.
- **C3**: Either add a live IRIS integration test task for the happy path, or explicitly document the constitution VIII exception justification ("config-only, no IRIS call path exercised by this feature") in the spec.
- **I2**: Align FR-005 wording with the actual implementation (full entry removal, not field zero-out).

No changes are needed to `/speckit.specify`, `/speckit.plan`, or `/speckit.tasks` unless
the above issues are addressed — these can be resolved with targeted edits directly in
spec.md, plan.md, and tasks.md.

---

## Remediation Offer

Would you like concrete remediation edits for the top issues (A1, C1, I1, I2, A3, U2)?
