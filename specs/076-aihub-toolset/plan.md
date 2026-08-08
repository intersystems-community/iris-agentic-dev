# Plan 076 — AI Hub ToolSet

## Tech stack

- ObjectScript (IRIS AI Hub 2026.2 build 162+)
- IRIS export format (`.xml` via `$system.OBJ.Export`)
- Markdown (README, skill update)
- Test runner: `iris-agentic-dev tool iris_execute` against aihub-iris-116 (port 21972)

## Architecture

All deliverables are ObjectScript class definitions exported to a single XML file.
No changes to the Rust codebase.

```text
contrib/aihub/
  IAD.ToolSet.xml          ← importable IRIS export (both ToolSet classes + example agent)
  README.md                ← setup guide

skills/skills/aihub-eap/SKILL.md  ← updated with contrib/aihub/ reference
```

### IAD.ToolSet.IrisAgenticDev

Extends `%AI.ToolSet`. Single `XData Definition` block:

```xml
<ToolSet>
  <MCP Name="IrisAgenticDev">
    <Stdio Platform="macos.*aarch64" Executable="/opt/homebrew/bin/iris-agentic-dev"/>
    <Stdio Platform="macos"          Executable="/usr/local/bin/iris-agentic-dev"/>
    <Stdio                           Executable="iris-agentic-dev"/>
    <Env Name="IRIS_HOST"      Value="@{env:IRIS_HOST}"/>
    <Env Name="IRIS_WEB_PORT"  Value="@{env:IRIS_WEB_PORT}"/>
    <Env Name="IRIS_USERNAME"  Value="@{env:IRIS_USERNAME}"/>
    <Env Name="IRIS_PASSWORD"  Value="@{env:IRIS_PASSWORD}"/>
    <Env Name="IRIS_NAMESPACE" Value="@{env:IRIS_NAMESPACE}"/>
  </MCP>
</ToolSet>
```

`IRIS_AGENTIC_DEV_BIN` override: a separate class method `GetExecutable()` returns
the env var value if set, falling back to the platform chain. The XData `Value` for
Executable references are the literal platform-specific paths; the env override is
implemented at the ObjectScript level via `%OnBeforeDiscover()` hook if available,
otherwise documented as a manual customisation step.

### IAD.ToolSet.IrisAgenticDevReadOnly

Extends `IAD.ToolSet.IrisAgenticDev` (inherits the MCP definition). Adds:

```xml
<Exclude Match="^iris_compile$|^iris_execute$|^iris_source_control$"/>
<Exclude Match="^iris_admin$"/>  <!-- write actions only — documented in README -->
```

Because `iris_admin` handles both read and write actions via a single tool, the
README documents how to further restrict by prompting the agent with a policy rule.

## File structure

```text
contrib/
  aihub/
    IAD.ToolSet.xml
    README.md
specs/076-aihub-toolset/
  spec.md
  plan.md
  tasks.md
skills/skills/aihub-eap/SKILL.md   (modified)
```

## Phases

### Phase 1 — Tests first (T-076-01 to T-076-03)

Write a test script `contrib/aihub/test/test_toolset.py` that:

1. Imports the XML into aihub-iris-116 via Atelier REST
2. Calls `$system.OBJ.IsCompiled("IAD.ToolSet.IrisAgenticDev")` — asserts true
3. Calls `##class(IAD.ToolSet.IrisAgenticDev).%Discover()` via `iris_execute` — asserts ≥20 tools
4. Calls `##class(IAD.ToolSet.IrisAgenticDevReadOnly).%Discover()` — asserts write tools absent

Tests are written and expected to fail before the XML exists.

### Phase 2 — ObjectScript classes

Write the two class definitions. Validate locally by importing into aihub-iris-116
via `iris_doc(mode=put)`. Run Phase 1 tests — must pass.

### Phase 3 — Export XML

Export both classes via `$system.OBJ.Export(["IAD.ToolSet.IrisAgenticDev",
"IAD.ToolSet.IrisAgenticDevReadOnly"], "/tmp/IAD.ToolSet.xml")`. Copy to
`contrib/aihub/IAD.ToolSet.xml`. Verify round-trip import into a clean namespace.

### Phase 4 — T-076-04 agent round-trip

Write and run the agent round-trip test via `iris_execute`. Confirm `check_config`
response via the agent.

### Phase 5 — Documentation

Write `contrib/aihub/README.md`. Update `skills/skills/aihub-eap/SKILL.md`.
Run markdownlint on both.

## Key decisions

- **Single XML export** rather than separate files per class — one import step is simpler
- **Env var credential injection** rather than wallet-only — wallet setup is an optional
  step documented in README; env vars work out of the box
- **Exclude by tool name** for read-only variant rather than a separate MCP block —
  simpler, and the full toolset is already defined in the parent class
- **No IRIS_AGENTIC_DEV_BIN in XData** — IRIS XData is static XML, cannot read env at
  definition time. Platform chain covers the common cases; custom path is a README footnote
