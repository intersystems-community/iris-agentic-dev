# Tasks 078 — AI Hub Skill Wrappers

**Prerequisite:** Spec 076 Phase 3 complete (`contrib/aihub/IAD.ToolSet.xml` exists).

## Phase 1: Tests (write first)

- [X] T1.1 Create `contrib/aihub/test/test_skills.py`
- [X] T1.2 Write T-078-01: import smoke test — all four skill classes compile in aihub-iris-116
- [X] T1.3 Write T-078-02: skill discovery — each skill's SUMMARY XData has name and description fields
- [X] T1.4 Write T-078-03: ObjectScriptRepair round-trip — agent identifies known mistake (mark manual/optional — requires ANTHROPIC_API_KEY)
- [X] T1.5 Write T-078-04: IrisNavigation read-only — TOOLS parameter is IrisAgenticDevReadOnly
- [X] T1.6 Write T-078-05: declarative agent compile — `IAD.Agent.ObjectScriptDev` has PROVIDER and SKILLS parameters
- [ ] T1.7 Confirm tests fail (skill classes don't exist yet — requires live aihub-iris-116)

## Phase 2: Skill Classes

- [X] T2.1 Read `skills/skills/objectscript-review/SKILL.md` — extracted rules
- [X] T2.2 Wrote `IAD.Skill.ObjectScriptRepair` in IAD.ToolSet.xml (TOOLS=full, SUMMARY, INSTRUCTIONS)
- [X] T2.3 Read `skills/skills/objectscript-guardrails/SKILL.md` — extracted rules
- [X] T2.4 Wrote `IAD.Skill.ObjectScriptGuardrails` in IAD.ToolSet.xml (TOOLS=full, standalone-safe)
- [X] T2.5 Read `skills/skills/ensemble-production/SKILL.md` — extracted rules
- [X] T2.6 Wrote `IAD.Skill.InteropDebugging` in IAD.ToolSet.xml (TOOLS=full, condensed)
- [X] T2.7 Read `skills/skills/objectscript-navigation/SKILL.md` — extracted rules
- [X] T2.8 Wrote `IAD.Skill.IrisNavigation` in IAD.ToolSet.xml (TOOLS=read-only)
- [ ] T2.9 Import all four classes into aihub-iris-116 via `iris_doc(mode=put)` and verify compile

## Phase 3: Example Agent

- [X] T3.1 Wrote `IAD.Agent.ObjectScriptDev` declarative agent class in IAD.ToolSet.xml
- [ ] T3.2 Import into aihub-iris-116 and verify compile

## Phase 4: Run Tests

- [ ] T4.1 Run T-078-01, T-078-02, T-078-04, T-078-05 — all must pass (requires aihub-iris-116)
- [ ] T4.2 T-078-03 is manual — document result in this tasks.md

## Phase 5: Expand XML Export

- [X] T5.1 IAD.ToolSet.xml includes both ToolSet classes + four Skill classes + example agent (7 classes total)
- [X] T5.2 contrib/aihub/IAD.ToolSet.xml contains all 7 classes
- [ ] T5.3 Verify round-trip import into aihub-iris-116 — all seven classes compile

## Phase 6: Documentation

- [X] T6.1 `contrib/aihub/README.md` covers skill class descriptions and declarative agent example
- [X] T6.2 `markdownlint-cli2 --fix contrib/aihub/README.md && prettier --write contrib/aihub/README.md` — clean

## Phase 7: Commit

- [ ] T7.1 `git add contrib/ specs/078-aihub-skills/`
- [ ] T7.2 Commit: `feat(aihub): add IAD.Skill ObjectScript wrappers for AI Hub progressive-disclosure skills`
