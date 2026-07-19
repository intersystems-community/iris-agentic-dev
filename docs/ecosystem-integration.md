# Ecosystem Integration Guide

This guide explains how an IRIS ecosystem package integrates with
`iris-agentic-dev` (iad). Three patterns cover the common cases.

---

## Pattern A: Connection handoff (idt-style)

A package that manages an IRIS container or connection (like
[iris-devtester](https://github.com/intersystems-community/iris-devtester)) can
hand off its connection details so iad auto-detects the right instance without
manual config.

**Mechanism**: write `.iris-agentic-dev.toml` into the project directory that
your test runner or build tool already sets as the working directory.

```toml
# Written by your package at test-setup time
host = "localhost"
web_port = 52780
namespace = "USER"
username = "_SYSTEM"
password = "SYS"
```

iad watches `$CWD/.iris-agentic-dev.toml` and hot-reloads when it changes. No
MCP server restart required.

**IRISConnectionInfo contract**:

| Field         | Required | Notes                                       |
| ------------- | -------- | ------------------------------------------- |
| `host`        | yes      | hostname or IP                              |
| `web_port`    | yes      | Atelier REST port (52773, 52780, 80, etc.)  |
| `namespace`   | yes      | target namespace                            |
| `username`    | yes      |                                             |
| `password`    | yes      |                                             |
| `container`   | no       | Docker container name; enables docker_exec  |
| `docker_only` | no       | `true` → skip Atelier REST, use docker exec |

**When to set `docker_only = true`**: NoPWS builds (IRIS 2026.2.0AI) have no
private web server. Set this flag so iad uses `$SYSTEM.OBJ.Compile` via
`docker exec` instead of probing port 52773.

---

## Pattern B: Skills contributor

A package ships its own `skills/<name>/SKILL.md` and registers with iad so
`iris_skill` and `iris_skill_community` can surface it.

**Step 1** — Add a skill file to your repo:

```text
your-repo/
  skills/
    skills/
      your-package/
        SKILL.md
```

The `SKILL.md` frontmatter must include:

```yaml
---
name: your-package
description: One sentence — when should an agent load this skill?
---
```

**Step 2** — Submit a PR to
[iris-agentic-dev](https://github.com/intersystems-community/iris-agentic-dev)
adding your package to `skills.sh.json` under the `"Ecosystem Projects"`
grouping:

```json
{
  "title": "Ecosystem Projects",
  "skills": ["iris-embedded-python", "your-package"]
}
```

And add a corresponding entry to `skills-lock.json`:

```json
"your-package": {
  "source": "intersystems-community/your-repo",
  "sourceType": "github",
  "skillPath": "skills/skills/your-package/SKILL.md"
}
```

iad resolves `sourceType: "github"` entries by fetching the `skillPath` from the
default branch of that repo. The lock entry can be registered before the
`SKILL.md` exists — `iris_skill_community` will return `not_found` gracefully
until the repo ships the file.

---

## Pattern C: Downstream consumer

A Python package that wraps IRIS functionality can list iad as an optional
development dependency so users get MCP tooling when they opt in.

**`pyproject.toml`**:

```toml
[project.optional-dependencies]
ai = ["iris-agentic-dev>=0.9"]
```

Users install it with:

```bash
pip install your-package[ai]
```

This installs the `iris-dev` binary. They then wire it into their MCP client
(Claude Code, VS Code Copilot, OpenCode) following the
[getting-started guide](https://github.com/intersystems-community/iris-agentic-dev#getting-started).

---

## Integration checklist

Run through this before shipping or opening the iad PR.

### Your repo

- [ ] `AGENTS.md` exists at repo root — tells Claude Code and other agents
      what this repo is for and how to work in it
- [ ] `skills/skills/<name>/SKILL.md` exists with correct frontmatter
      (`name`, `description` required; `managed_by: "iris-agentic-dev"`
      recommended so iad knows to keep it in sync)
- [ ] README cross-references iad — link to the MCP server and mention
      `iris_skill install <name>` as the install path

### iad PR (Pattern B)

- [ ] Entry added to `skills.sh.json` under `"Ecosystem Projects"`
- [ ] Entry added to `skills-lock.json` with correct `source` and
      `sourceType: "github"`
- [ ] `cargo test` passes (no live IRIS required for skill registry changes)
- [ ] `markdownlint-cli2 --fix` + `prettier --write` run on any `.md` touched

### Connection handoff (Pattern A)

- [ ] `.iris-agentic-dev.toml` written before any tool call that needs IRIS
- [ ] `docker_only = true` set when the build has no private web server
- [ ] `container` field populated when docker exec fallback is needed
