# Spec 078 — AI Hub Skill Wrappers

## Overview

Ship ObjectScript `%AI.Agent.Skill` subclasses that bundle iris-agentic-dev tools
(via the ToolSet from Spec 076) with workflow-specific instructions drawn from the
existing SKILL.md files. An AI Hub agent gains deep ObjectScript/IRIS capability by
calling `agent.UseSkill("IAD.Skill.ObjectScriptRepair")` — one line, no boilerplate.

## Problem

Spec 076 gives AI Hub agents the full iris-agentic-dev tool surface. But a ToolSet
alone provides no instructions — the agent has tools but no guidance on when to use
them or how ObjectScript idioms differ from other languages. The existing SKILL.md
files contain exactly that guidance, but they're in a format designed for Claude Code
/ OpenCode, not for AI Hub's `%AI.Agent.Skill` XData convention. Bridging the two
requires wrapper classes that reference the correct ToolSets and translate the SKILL.md
content into `XData INSTRUCTIONS` blocks.

## Goals

- Four `%AI.Agent.Skill` subclasses covering the most-requested workflows
- Instructions sourced directly from the existing SKILL.md files (no duplication)
- Each skill references `IAD.ToolSet.IrisAgenticDev` (or the read-only variant)
- Ships alongside the ToolSet in `contrib/aihub/`
- Documented and tested against aihub-iris-116

## Non-goals

- Covering all 14 SKILL.md files in this spec (start with 4; extend later)
- Modifying the existing SKILL.md files
- Python AI Hub bindings (separate future spec)

## Skills to ship

| Class                              | Wraps SKILL.md            | ToolSet   | Description                                            |
| ---------------------------------- | ------------------------- | --------- | ------------------------------------------------------ |
| `IAD.Skill.ObjectScriptRepair`     | `objectscript-review`     | Full      | Hard-gate checklist for ObjectScript mistakes          |
| `IAD.Skill.ObjectScriptGuardrails` | `objectscript-guardrails` | Full      | All-in-one guard, works without MCP connected          |
| `IAD.Skill.InteropDebugging`       | `ensemble-production`     | Full      | Interoperability production lifecycle and log analysis |
| `IAD.Skill.IrisNavigation`         | `objectscript-navigation` | Read-only | Codebase discovery and namespace exploration           |

## Functional requirements

### FR-001 — Skill class structure

Each class:

- Extends `%AI.Agent.Skill`
- Sets `Parameter TOOLS` to the appropriate ToolSet class name(s)
- Has `XData SUMMARY` (YAML: name, description, tags)
- Has `XData INSTRUCTIONS` (Markdown: content drawn from the corresponding SKILL.md,
  adapted for the AI Hub context — references to "Claude Code" replaced with
  "this agent", "MCP tools" terminology preserved)

### FR-002 — SKILL.md fidelity

The `XData INSTRUCTIONS` content must preserve all numbered rules, code patterns,
and "never do X" constraints from the source SKILL.md. No summarisation that loses
enforcement rules.

### FR-003 — Declarative agent example

`contrib/aihub/README.md` (from Spec 076) gains an example declarative agent class
`IAD.Agent.ObjectScriptDev` with `Parameter SKILLS` listing all four skill classes,
showing a complete working agent in ~20 lines of ObjectScript.

### FR-004 — Export artifact

All four skill classes included in `contrib/aihub/IAD.ToolSet.xml` alongside the
ToolSet classes from Spec 076 (single import covers everything).

### FR-005 — Standalone usability

`IAD.Skill.ObjectScriptGuardrails` must work without the MCP server running — its
instructions alone (no tool calls) provide value. The `TOOLS` parameter may point
to the ToolSet class but the skill must not fail if the binary is absent.

## Test requirements

### T-078-01 — Import smoke test

All four skill classes import into aihub-iris-116 with zero compile errors.

### T-078-02 — Skill discovery

For each skill class, instantiate and call `%GetSummary()` (or equivalent). Verify
name and description fields are non-empty.

### T-078-03 — ObjectScriptRepair agent round-trip

Create an agent with `IAD.Skill.ObjectScriptRepair`. Submit a prompt containing a
known ObjectScript mistake (e.g. `Set x = $List(list, 1)` — wrong API). Verify the
agent response identifies the error.

### T-078-04 — IrisNavigation read-only toolset

Instantiate `IAD.Skill.IrisNavigation`, call `%Discover()`, verify write tools
(`iris_compile`, `iris_execute`) are absent.

### T-078-05 — Declarative agent example compiles

Import `IAD.Agent.ObjectScriptDev` (the example from the README). Verify it compiles
and `%Init()` succeeds (will fail to create a provider session without an API key,
but must not error on class load).

## Acceptance criteria

- T-078-01 through T-078-05 pass against aihub-iris-116
- All four skills present in the single `IAD.ToolSet.xml` export
- `contrib/aihub/README.md` contains the declarative agent example
- markdownlint clean on all changed `.md` files
