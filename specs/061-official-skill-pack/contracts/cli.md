# CLI Contract: `iris-agentic-dev skill`

## Subcommands

### `iris-agentic-dev skill install`

Install the official InterSystems skill pack into AI agent instruction directories.

```
iris-agentic-dev skill install [OPTIONS] [SKILL...]

Arguments:
  [SKILL...]   One or more skill names to install. Omit to install the full pack.

Options:
  --agent <AGENT>   Target agent(s): claude-code, opencode, copilot, all
                    Default: all user-global agents (claude-code + opencode)
  --dry-run         Show what would be installed without writing any files
  --force           Overwrite user-authored skills (bypasses FR-008 collision check)
```

**Output (stdout)**:

```
Installing pyprod → ~/.claude/skills/pyprod/SKILL.md ... written
Installing pyprod → ~/.config/opencode/skills/pyprod/SKILL.md ... written
Installing objectscript-review → ~/.claude/skills/objectscript-review/SKILL.md ... updated
Skipped: ~/.claude/skills/my-custom/SKILL.md (user-authored — use --force to overwrite)

3 written, 1 updated, 1 skipped.
```

**Exit codes**:

- `0` — all installs succeeded (skips do not count as failures)
- `1` — one or more installs failed (fetch error, write error)

---

### `iris-agentic-dev skill list`

List skills in the official pack and their install status.

```
iris-agentic-dev skill list [--agent <AGENT>]
```

**Output**:

```
SKILL                     CLAUDE CODE    OPENCODE    COPILOT
pyprod                    installed      installed   not installed
objectscript-review       installed      not found   n/a
objectscript-guardrails   not installed  not found   n/a
```

---

### `iris-agentic-dev skill status`

Show install paths and managed-by status for all local skill files.

```
iris-agentic-dev skill status
```

---

## Copilot-Specific Behavior

When `--agent copilot` is specified:

1. Check that cwd is a git repo or contains a `.github/` directory. If not, error with `COPILOT_NO_REPO`.
2. Write to `.github/instructions/<skill-name>.instructions.md` in cwd.
3. Warn: "Note: .github/instructions/ is repo-scoped. Commit this directory to share with your team."

### Copilot output file format

Each skill written to `.github/instructions/` has this layout:

```markdown
---
name: "objectscript-review"
description: "Use when writing or reviewing ObjectScript code..."
applyTo: "**"
managed_by: "iris-agentic-dev"
---

[full SKILL.md body content here, verbatim]
```

- `applyTo: "**"` applies the skill globally across all file types in the repo
- `managed_by: "iris-agentic-dev"` is the FR-008 collision marker (same as other agents)
- VS Code reads all `.github/instructions/*.instructions.md` files and auto-combines them

---

## Managed-By Marker

Every file written by `skill install` includes this frontmatter field:

```yaml
managed_by: "iris-agentic-dev"
```

On re-install, presence of this marker = safe to overwrite.
Absence = user-authored = skip unless `--force`.
