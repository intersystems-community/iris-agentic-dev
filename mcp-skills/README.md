# ObjectScript Skills Marketplace

Skills are reusable, documented workflows for common ObjectScript development tasks. They're organized in this directory using the [superpowers](https://github.com/obra/superpowers) model.

## What is a Skill?

A **skill** is:

- A standalone directory with a `SKILL.md` file (YAML frontmatter + markdown)
- Self-contained instructions for a multi-step workflow
- Discoverable and loadable by Claude plugins and AI assistants
- Validated and tested before publication

## Available Skills

### Core Compilation & Testing

- **`compile`** — Upload and compile ObjectScript .cls files via Atelier REST, parse errors
- **`iris-objectscript-eval`** — Load, compile, run, and test ObjectScript code in IRIS Docker
- **`objectscript-tdd`** — Compile-test-fix loop for ObjectScript development

### Code Analysis & Repair

- **`objectscript-repair`** — Coordinated fixes across multiple ObjectScript files
- **`objectscript-review`** — Review ObjectScript code for common AI mistakes
- **`objectscript-debugging`** — Capture IRIS diagnostic packets, map .INT offsets to source

### Pattern Reference

- **`objectscript-list-patterns`** — %List, %ListOfDataTypes, $LISTBUILD, $LISTTOSTRING patterns
- **`objectscript-loop-patterns`** — For/While loops, $Order iteration, postfix Quit
- **`objectscript-sql-patterns`** — Embedded SQL, %SQL.Statement, date filtering, NULL handling
- **`objectscript-guardrails`** — 10-item hardgate checklist for common AI mistakes
- **`iris-sql`** — SQL query optimization, table naming, reserved words, date handling
- **`iris-product-features`** — IRIS capabilities, MCP, HL7/Interoperability, mirroring

### Domain-Specific

- **`iris-connectivity`** — Connecting to IRIS from Python, Java, JDBC, ODBC
- **`iris-vector-ai`** — Vector search, HNSW index, similarity search, AI features
- **`objectscript-navigation`** — Deep codebase discovery using MCP and text tools
- **`ensemble-production`** — Manage and observe IRIS Interoperability productions
- **`iris-product-features`** — IRIS capabilities (MCP, full-text search, mirroring, etc.)
- **`introspect`** — Introspect IRIS class definitions from %Dictionary

### Management & Learning

- **`opencode-introspect`** — Read and search OpenCode session logs from SQLite DB
- **`objectscript-unit-test`** — Generate %UnitTest.TestCase subclasses with live introspection

## Skill Structure

Each skill is a directory with this layout:

```
skills/
├── compile/
│   ├── SKILL.md           # Skill definition (YAML frontmatter + markdown)
│   └── [optional files]   # Examples, code snippets, test cases
├── objectscript-repair/
│   ├── SKILL.md
│   └── examples/
└── ...
```

## SKILL.md Format

Every skill has a `SKILL.md` file with YAML frontmatter:

```yaml
---
name: compile
description: Upload and compile ObjectScript .cls files via Atelier REST
trigger: "compile|upload|check"
iris_version: ">=2024.1"
tags: [objectscript, compilation]
author: "tim.leavitt@intersystems.com"
state: "reviewed"
pass_rate: 0.95
---
# /compile — Skill Title

Skill description and usage instructions in markdown...
```

**Frontmatter keys:**

- `name` — Unique skill identifier (used as `/name` in chat)
- `description` — One-line summary
- `trigger` — Activation keywords (optional, for auto-discovery)
- `iris_version` — Minimum IRIS version requirement (semver range)
- `tags` — Keywords for discovery
- `author` — Skill author
- `state` — `draft | reviewed | published`
- `pass_rate` — Test pass rate (set by `skill benchmark`)

## Using Skills

### In Claude Code / Claude Desktop

Once installed as a plugin, skills appear as slash commands:

```
/compile MyApp.Order        # Invoke by name
/objectscript-repair ...    # All skills auto-discoverable
```

Or ask naturally:

```
"Compile my Order class and show errors"
"Use the repair skill to fix cross-references"
```

### In Other Contexts

Skills can be discovered and loaded programmatically:

```bash
skill list                        # List all available skills
skill describe compile            # Show full skill definition
skill search "cross-reference"    # Find relevant skills
```

## Creating a New Skill

1. **Create a directory** under `/skills`:

   ```bash
   mkdir skills/my-new-skill
   ```

2. **Write `SKILL.md`** with YAML frontmatter and markdown body:

   ```markdown
   ---
   name: my-new-skill
   description: What this skill does
   trigger: "keywords to trigger it"
   author: "your-name"
   state: "draft"
   ---

   # /my-new-skill — Skill Title

   Full description...
   ```

3. **Add examples** (optional):
   - `examples/example1.md` — Worked example
   - `test/test_my_skill.py` — Unit tests

4. **Test locally**:

   ```bash
   skill describe my-new-skill
   ```

5. **Benchmark before sharing**:

   ```bash
   skill benchmark my-new-skill
   ```

6. **Publish** (requires PR approval):

   ```bash
   skill share my-new-skill
   ```

## Repository Structure

```
objectscript-mcp/
├── skills/
│   ├── compile/
│   ├── objectscript-repair/
│   ├── iris-connectivity/
│   └── ... (~20 skills)
├── skills/                # Skill pack files (npx skills add compatible)
├── docs/
│   ├── CLAUDE-PLUGIN-SETUP.md
│   └── ...
├── src/                   # VS Code extension source
├── mcp_server/            # MCP server implementation
└── package.json
```

## Discovery & Auto-Loading

Skills are discovered and loaded based on:

1. **Directory presence** — `/skills/**/SKILL.md`
2. **YAML frontmatter** — name, trigger, tags
3. **Registration** — Plugin system auto-registers available skills

When you type `/` in chat, all skills appear as slash commands.

## Contributing Skills

To contribute a skill:

1. **Create and test** locally (state: `draft`)
2. **Benchmark** — Run `skill benchmark` to validate pass_rate
3. **Transition to reviewed** — Once pass_rate >= 0.80
4. **Submit PR** — Skills are published via pull request to this repo
5. **Community** — Published skills appear in the marketplace

See [`docs/CONTRIBUTING.md`](../docs/CONTRIBUTING.md) for full guidelines.

## Examples in This Directory

Browse skills for patterns:

- **Structured error parsing** → see `compile/SKILL.md`
- **Multi-file coordination** → see `objectscript-repair/SKILL.md`
- **Reference/checklists** → see `objectscript-guardrails/SKILL.md`
- **Domain expertise** → see `iris-connectivity/SKILL.md`, `iris-vector-ai/SKILL.md`

---

**Questions?** See the parent repository [README](../README.md) or [TROUBLESHOOTING.md](../docs/TROUBLESHOOTING.md).
