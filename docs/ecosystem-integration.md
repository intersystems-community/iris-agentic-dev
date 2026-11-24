# Ecosystem Integration Guide

This guide explains how an IRIS ecosystem package integrates with
`iris-agentic-dev` (iad). Four patterns cover the common cases.

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

The file is throwaway: do not commit a password. The connection details above need
no trust. A setting that loosens a limit, such as `mode`, `service_username` or
`tls_verify = false`, waits until the operator runs `iris-agentic-dev trust` on
the file ([connecting.md](connecting.md)). The config walk stops at the repository
root, so write the file inside the repository your tool runs from.

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
`skill` and `skill_community` can surface it.

> **What works in 1.7.1.** Ship `skills/<name>/SKILL.md` in your repo and list it in
> your repo's `iris-agentic-dev.toml`: `[provides] skills = ["skills/<name>"]`. A user
> adds it with `iris-agentic-dev mcp --subscribe owner/repo`, and `skill_community`
> lists and installs it. To ship a skill inside iad itself, open a PR that adds
> `skills/skills/<name>/SKILL.md` with a `tier:` line and an entry in `skills.sh.json`.
>
> **Not implemented: step 2's `skills-lock.json`.** No `skills-lock.json` exists in
> iad and no code reads `sourceType` or `skillPath`. The text below describes the
> planned design. Do not rely on it yet.

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

And add a corresponding entry to `skills-lock.json` (planned, not implemented):

```json
"your-package": {
  "source": "intersystems-community/your-repo",
  "sourceType": "github",
  "skillPath": "skills/skills/your-package/SKILL.md"
}
```

iad resolves `sourceType: "github"` entries by fetching the `skillPath` from the
default branch of that repo. The lock entry can be registered before the
`SKILL.md` exists — `skill_community` will return `not_found` gracefully
until the repo ships the file.

---

## Pattern C: Downstream consumer

A Python package that wraps IRIS functionality can list iad as an optional
development dependency so users get MCP tooling when they opt in.

> Nothing in the iad repository publishes an `iris-agentic-dev` package to PyPI. Confirm
> that one exists before you depend on it.

**`pyproject.toml`**:

```toml
[project.optional-dependencies]
ai = ["iris-agentic-dev>=0.9"]
```

Users install it with:

```bash
pip install your-package[ai]
```

This installs the `iris-agentic-dev` binary. They then wire it into their MCP client
(Claude Code, VS Code Copilot, OpenCode, or [Cursor](cursor-quickstart.md))
following the
[getting-started guide](getting-started.md).
For Cursor-specific MCP wiring and limitations vs Copilot, see
[cursor-quickstart.md](cursor-quickstart.md).

---

## Pattern D: Tool subset consumer

> **Planned, not implemented in 1.7.1.** `iris-agentic-dev install` resolves
> `[dependencies]` and writes `iris-agentic-dev.lock`. It does not read
> `[provides] tools`, validate the names or write `enabled_tools`; the code that
> would is a `TODO` in `manifest/resolve.rs`. What works today is setting
> `enabled_tools = ["iris_query", ...]` in `.iris-agentic-dev.toml` or
> `IRIS_ENABLED_TOOLS` in the environment by hand. The rest of this section
> describes the planned behavior.

A downstream project that only needs a specific set of iad tools can declare
that subset in its manifest. The install command validates the names and writes
them into the local config. The next server start enforces the subset.

**`iris-agentic-dev.toml`**:

```toml
[provides]
tools = ["iris_query", "iris_search", "iris_compile"]
```

When the install command resolves this manifest:

1. Each name is validated against the full tool registry. An unknown name is an
   error — not silently ignored. Fix the typo at declaration time, not when
   someone else runs it.
2. The validated list is written into `.iris-agentic-dev.toml` as `enabled_tools`.
3. The next MCP server start exposes exactly those tools.

This is the declarable counterpart to setting `IRIS_ENABLED_TOOLS` by hand. It
belongs in version control so every developer on the project gets the same tool
surface without copying environment variables.

See `docs/connecting.md` for `IRIS_ENABLED_TOOLS` and `enabled_tools` details.

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
      `iris-agentic-dev skill install <name>` as the install path

### iad PR (Pattern B)

- [ ] Entry added to `skills.sh.json` under `"Ecosystem Projects"`
- [ ] Entry added to `skills-lock.json` with correct `source` and
      `sourceType: "github"` (planned; the file does not exist yet)
- [ ] `cargo test --features testing` passes (no live IRIS required for skill registry changes)
- [ ] `markdownlint-cli2 --fix` + `prettier --write` run on any `.md` touched

### Connection handoff (Pattern A)

- [ ] `.iris-agentic-dev.toml` written before any tool call that needs IRIS
- [ ] `docker_only = true` set when the build has no private web server
- [ ] `container` field populated when docker exec fallback is needed
