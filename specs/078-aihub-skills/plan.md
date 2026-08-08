# Plan 078 — AI Hub Skill Wrappers

## Tech stack

- ObjectScript (IRIS AI Hub 2026.2 build 162+)
- IRIS export format
- Source SKILL.md files in `skills/skills/`
- Test runner: `iris_execute` against aihub-iris-116 (port 21972)

## Dependency

Spec 076 must be complete first — the skill classes reference
`IAD.ToolSet.IrisAgenticDev` and `IAD.ToolSet.IrisAgenticDevReadOnly`.

## Architecture

Four `%AI.Agent.Skill` subclasses. All go into `contrib/aihub/IAD.ToolSet.xml`
alongside the ToolSet classes from Spec 076 (single expanded export).

```text
IAD.Skill.ObjectScriptRepair      → objectscript-review SKILL.md
IAD.Skill.ObjectScriptGuardrails  → objectscript-guardrails SKILL.md
IAD.Skill.InteropDebugging        → ensemble-production SKILL.md
IAD.Skill.IrisNavigation          → objectscript-navigation SKILL.md
```

Each class:

```objectscript
Class IAD.Skill.ObjectScriptRepair Extends %AI.Agent.Skill
{
    Parameter TOOLS = "IAD.ToolSet.IrisAgenticDev";

    XData SUMMARY [ MimeType = "text/yaml" ]
    {
name: objectscript-repair
description: Hard-gate checklist for the 10 most common ObjectScript mistakes.
  Catches errors before the agent writes any code.
tags:
  - objectscript
  - repair
  - code-review
    }

    XData INSTRUCTIONS [ MimeType = "text/markdown" ]
    {
<content from objectscript-review SKILL.md, lightly adapted>
    }
}
```

### Instruction adaptation rules

1. Replace "Claude Code" → "this agent" or "you"
2. Remove any references to `~/.claude/skills/` paths
3. Keep all numbered rules, code examples, and "never" constraints verbatim
4. Preserve the tool call syntax (`iris_execute`, `iris_doc`, etc.) — these work
   in AI Hub agents via the MCP ToolSet

### IAD.Agent.ObjectScriptDev (example declarative agent)

Included in the XML export as an example. Uses `Parameter SKILLS` to list all four:

```objectscript
Class IAD.Agent.ObjectScriptDev Extends %AI.Agent
{
    Parameter PROVIDER = "anthropic";
    Parameter APIKEY   = "@{env:ANTHROPIC_API_KEY}";
    Parameter TOOLSETS = "IAD.ToolSet.IrisAgenticDev";
    Parameter SKILLS   = "IAD.Skill.ObjectScriptRepair,IAD.Skill.ObjectScriptGuardrails,IAD.Skill.InteropDebugging,IAD.Skill.IrisNavigation";

    XData INSTRUCTIONS [ MimeType = "text/markdown" ]
    {
You are an ObjectScript and InterSystems IRIS development assistant.
Use your skills to help with code review, debugging, and codebase navigation.
    }
}
```

## File changes

```text
contrib/aihub/IAD.ToolSet.xml   — expanded to include 4 skill classes + example agent
contrib/aihub/README.md         — declarative agent example + skill descriptions
```

No changes to Rust codebase or existing SKILL.md files.

## Phases

### Phase 1 — Tests first

Write `contrib/aihub/test/test_skills.py` with T-078-01 through T-078-05.
Tests import the XML and verify compile + behaviour. Expected to fail until
Phase 2 delivers the classes.

### Phase 2 — Skill classes

Write all four skill classes. Source instructions from the SKILL.md files.
Import into aihub-iris-116 via `iris_doc(mode=put)` and verify compile.

### Phase 3 — Example agent class

Write `IAD.Agent.ObjectScriptDev`. Import and verify compile.

### Phase 4 — Run tests

T-078-01 through T-078-04 (compile + discovery). T-078-03 (repair round-trip)
requires an ANTHROPIC_API_KEY in the test environment — mark `#[ignore]` if absent,
run manually to verify.

### Phase 5 — Expand XML export

Re-export `IAD.ToolSet.xml` to include all six classes (2 toolsets + 4 skills +
1 example agent). Verify round-trip import.

### Phase 6 — Documentation

Update `contrib/aihub/README.md` with skill descriptions and declarative agent
example. Run markdownlint + prettier.

## Key decisions

- **Single XML export** covers all 076 + 078 deliverables — one `Load()` call sets
  everything up
- **SKILL.md content copied into XData** rather than loaded from URI at runtime —
  avoids a GitHub dependency at agent startup; content can be updated by re-exporting
- **IAD.Skill.IrisNavigation uses read-only ToolSet** — navigation tasks never need
  write access; the restriction is a safety default
- **IAD.Skill.ObjectScriptGuardrails has TOOLS but works standalone** — if the MCP
  binary isn't installed, the instructions still fire and the agent gets the checklist
  even without tool access. The TOOLS parameter just extends capability when available.
- **Example agent not tested with live LLM in CI** — T-078-05 only checks compile;
  a live API call test is manual only to avoid burning tokens in CI
