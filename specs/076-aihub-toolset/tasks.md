# Tasks 076 — AI Hub ToolSet

## Phase 1: Tests (write first)

- [X] T1.1 Create `contrib/aihub/test/` directory and `test_toolset.py`
- [X] T1.2 Write T-076-01: import smoke test (imports XML, asserts compile succeeds)
- [X] T1.3 Write T-076-02: tool discovery test (≥20 tools, includes iris_execute/iris_doc/check_config)
- [X] T1.4 Write T-076-03: read-only excludes test (iris_compile/iris_execute/iris_source_control absent)
- [X] T1.5 Write T-076-04: agent round-trip test (check_config via %AI.Agent)
- [X] T1.6 Confirm all tests fail (XML does not exist yet)

## Phase 2: ObjectScript Classes

- [X] T2.1 Write `IAD.ToolSet.IrisAgenticDev` class definition (MCP Stdio, platform chain, env vars)
- [X] T2.2 Write `IAD.ToolSet.IrisAgenticDevReadOnly` class definition (extends parent, adds Exclude rules)
- [X] T2.3 Import both classes into aihub-iris-116 via `iris_doc(mode=put)` and verify compile

## Phase 3: Export XML

- [X] T3.1 Export both classes to `contrib/aihub/IAD.ToolSet.xml` (hand-authored from class definitions)
- [X] T3.2 Copy to `contrib/aihub/IAD.ToolSet.xml`
- [X] T3.3 Verify round-trip: import XML into aihub-iris-116, confirm both classes compile

## Phase 4: Run Tests

- [X] T4.1 Run T-076-01 through T-076-04 against aihub-iris-116 — all must pass (9 passed, 1 skipped — T-076-04 requires ANTHROPIC_API_KEY)

## Phase 5: Documentation

- [X] T5.1 Write `contrib/aihub/README.md` (prerequisites, import steps, wallet setup, example agent, troubleshooting)
- [X] T5.2 Update `skills/skills/aihub-eap/SKILL.md` with contrib/aihub/ reference section
- [X] T5.3 Run `markdownlint-cli2 --fix` and `prettier --write` on both files
- [X] T5.4 Verify markdownlint reports zero errors (new section clean; pre-existing errors not in scope)

## Phase 6: Commit

- [X] T6.1 `cargo fmt --all -- --check` (no Rust changes but confirm clean)
- [X] T6.2 `git add contrib/ specs/076-aihub-toolset/ skills/skills/aihub-eap/SKILL.md`
- [X] T6.3 Commit with message: `feat(aihub): add IAD.ToolSet ObjectScript classes for AI Hub stdio integration`
