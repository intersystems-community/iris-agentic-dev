# Research: Official InterSystems Skill Pack (061)

## 1. Agent Skill Directory Conventions

### Decision

Three distinct install targets. Claude Code and OpenCode share an identical convention;
Copilot uses a completely different, repo-scoped mechanism.

| Agent             | Install target                                                             | Scope       | Format                                          |
| ----------------- | -------------------------------------------------------------------------- | ----------- | ----------------------------------------------- |
| Claude Code       | `~/.claude/skills/<name>/SKILL.md`                                         | User-global | Markdown + optional YAML frontmatter            |
| OpenCode          | `~/.config/opencode/skills/<name>/SKILL.md`                                | User-global | Same format (by design)                         |
| Copilot (VS Code) | `.github/instructions/<name>.instructions.md`                              | Repo-level  | Markdown + frontmatter (see §3)                 |
| Cursor            | `~/.cursor/rules/<name>.mdc` (global) or `.cursor/rules/<name>.mdc` (repo) | Both        | `.mdc` with glob frontmatter — different format |

**Cursor excluded from P1**: Cursor uses `.mdc` with glob-based rather than
description-triggered selection — a different model. P2 at most.

### Frontmatter (Claude Code / OpenCode)

Only `name` and `description` are consumed by the agents; everything else is inert metadata:

```yaml
---
name: "pyprod"
description: "Use when creating or modifying pyprod interoperability components..."
---
```

### Platform paths (Rust)

- Unix/macOS: `std::env::var("HOME")` → `~/.claude/skills/` and `~/.config/opencode/skills/`
- Windows: Claude Code uses `%APPDATA%\Claude\skills\`; use `dirs::home_dir()` for Unix path, `dirs::config_dir()` for OpenCode
- Recommend: use the `dirs` crate (already in workspace? check) for cross-platform home resolution

### Rationale

Claude Code and OpenCode are the P1 targets. Same file format, different base paths — one
implementation handles both. Copilot is P2 with a distinct write path. Cursor is out of scope.

---

## 2. Content Distribution: Bundle vs Fetch

### Decision

**Fetch from `raw.githubusercontent.com` at install time.** Do not bundle via `include_str!`.

### Rationale

- FR-009 requires content updatable without a binary release — `include_str!` defeats this entirely
- `SkillRegistry::load_from_github` in `crates/iris-agentic-dev-core/src/skills/mod.rs` already
  does this: fetches from `raw.githubusercontent.com`, no auth required, no rate-limit concern,
  ~80–200ms per file from North America
- **Reuse the existing mechanism directly** — `skill install` calls `load_from_github` then writes
  `skill.content` to disk; no new HTTP logic needed
- Raw GitHub URLs are unauthenticated and have no rate-limit header dance; GitHub API is not needed

### Alternatives rejected

- `include_str!` bundle: defeats FR-009; rejected
- GitHub API: OAuth complexity, 60 req/hr unauthenticated cap; raw URLs are simpler and faster

---

## 3. Copilot Install Target

### Decision

**`.github/instructions/<name>.instructions.md`** — one file per skill, not the monolithic
`.github/copilot-instructions.md`.

### Rationale

- VS Code scans `.github/instructions/` recursively and auto-combines all `.instructions.md` files
- Writing one file per skill is **collision-free** — no merge conflict risk with user content
- Each file can have `applyTo: "**"` to apply globally or a glob to target specific file types
- Avoids the need for sentinel-section merge logic in the monolithic file
- Users can toggle skills via `chat.instructionsFilesLocations` VS Code setting

### Frontmatter format

```markdown
---
name: "pyprod"
description: "Use when creating or modifying pyprod interoperability components"
applyTo: "**"
---

[skill content here]
```

### Constraints

- Requires current directory to be a git repo with a `.github/` directory (or create it)
- File is repo-scoped — shared with team if checked into git; warn user of this
- No documented size limit; "2 pages" is guidance only

---

## 4. FR-006: Idempotent Update

### Decision

**Just overwrite.** No checksum check before write.

### Rationale

- `std::fs::write` is fast and local — the read round-trip for checksum adds complexity with no benefit
- Overwrite-on-reinstall is the standard pattern (`brew upgrade`, `npm install`, `cargo install`)
- FR-008 (collision protection) is handled separately via managed-by marker (see §5)

---

## 5. FR-008: Collision Detection (User-Authored Skills)

### Decision

**Managed-by marker in frontmatter.** Check for marker before overwriting; skip + warn if absent.

```yaml
---
name: "pyprod"
description: "..."
managed_by: "iris-agentic-dev" # marker written by skill install
---
```

### Rationale

- Self-describing: the file carries its own provenance — no side-channel hash database needed
- Hash databases get stale when official content changes; a marker never does
- Same pattern as Terraform (`# This file is maintained automatically by Terraform`) and Renovate
- On `skill install`: write marker on first install; on re-install, check for marker — if present,
  overwrite freely; if absent, skip and print warning

### Alternatives rejected

- Content hash database: requires a side file, gets stale; rejected
- No check (always overwrite): violates FR-008 explicitly; rejected

---

## 6. Dependency: `dirs` Crate

### Decision

**Add `dirs` crate** for cross-platform home directory resolution.

### Justification (Constitution VII)

- `std::env::var("HOME")` fails on Windows (Claude Code uses `%APPDATA%\Claude\` not `~\.claude\`)
- `dirs::home_dir()` handles Unix/macOS/Windows correctly in ~1 line vs ~10 lines inline
- `dirs` is a minimal, widely-used crate (no transitive deps beyond `std`); binary size impact negligible
- Verify: is `dirs` already in workspace? `grep -r "dirs" Cargo.toml` — if so, no new dep needed

---

## 7. Spec Additions Required Before Implementation

These gaps from the planning session must be added to `spec.md`:

1. **FR-001 clarification**: P1 targets are Claude Code and OpenCode (user-global);
   Copilot is P2 (repo-scoped, `.github/instructions/`); Cursor is out of scope.
2. **`--agent` flag**: `skill install [--agent claude-code|opencode|copilot|all]`; default = all
   user-global agents (claude-code + opencode).
3. **FR-007 clarification**: IRIS mirror = copy filesystem-installed content into `^SKILLS`
   via MCP `skill` tool or direct global write. Not automatic on filesystem install.
   The MCP `skill` tool and `~/.claude/skills/` are completely separate stores.
4. **Benchmark evidence**: Skill injection confirmed +1.83–2.33 lift on PYPR benchmark
   (baseline 0.17/3 → 2.00–2.50/3); validates the feature's core value claim.
5. **FR-008 mechanism**: Managed-by frontmatter marker; skip-not-overwrite on collision.

---

## 8. Constitution VII Gate Resolution

| Crate     | New?                                                                                 | Justification                                |
| --------- | ------------------------------------------------------------------------------------ | -------------------------------------------- |
| `reqwest` | No — already in workspace (used by `skills/mod.rs`)                                  | Reuse existing                               |
| `dirs`    | Check — likely already present; if not, justified for cross-platform home resolution | Minimal dep, no alt in ≤30 lines for Windows |

**Gate: PASS** pending `dirs` workspace check. If already present: no new deps at all.

---

## 9. Vercel Labs `skills` CLI — Verified Behavior (2026-07-17)

**Sources verified**: `vercel-labs/skills` (the CLI tool) and `vercel-labs/agent-skills` (the
official pack). Both are public GitHub repos.

### How `npx skills add owner/repo` actually works

The CLI **clones the repo via `git clone`** into a temp directory (uses `simple-git`), then
walks the filesystem. It does NOT use `raw.githubusercontent.com`. There is no HTTP manifest
fetch step in the `skills add` flow.

Discovery order inside the cloned repo:

1. Repo root — if `SKILL.md` exists at root, use it and stop (single-skill repos)
2. `skills/` subdirectory — scans one level deep, each subdir that has `SKILL.md` is a skill
3. `skills/.curated/`, `skills/.experimental/`, `skills/.system/` — same depth-1 scan
4. Plugin-manifest-declared dirs (`.claude-plugin/marketplace.json`, `plugin.json`)

**Our `skills/skills/<name>/SKILL.md` layout is correct.** The CLI will clone the repo,
find `skills/` at the repo root, scan one level deep, and discover each `<name>/SKILL.md`.

### Required SKILL.md frontmatter

Only `name` and `description` are required. A skill is skipped if either is missing.
`license` and `metadata.*` in frontmatter are optional. From `src/skills.ts`:

```typescript
if (!data.name || !data.description) {
  return null; // skill discarded
}
```

### `metadata.json`

Optional. Used by skills.sh web registry for display. Not read during `skills add` install.
Fields seen in `vercel-labs/agent-skills`: `version`, `organization`, `date`, `abstract`,
`references` (array of URLs). No required fields. Safe to add incrementally.

### `skills.sh.json`

Only affects the **skills.sh discovery web page** for the repo. Does NOT affect `npx skills add`
behavior. Required field: `groupings` (array of `{title, skills[]}`). Optional: `description`
per group, `notGrouped: "top"|"bottom"` (default `"bottom"`).

Verified schema: `https://skills.sh/schemas/skills.sh.schema.json`

### Impact on T020

T020 (fetch pack manifest) is for the **Rust `skill install` command**, not for `npx skills add`.
These are two separate install paths:

- `npx skills add` → clones repo via git, walks filesystem, fully handled by the `skills` CLI
- `iris-agentic-dev skill install` → Rust binary fetches manifest + skill content from GitHub raw URLs

T020 is correct for the Rust path. The `iris-agentic-dev.toml` → `[provides] skills = [...]`
approach works for the Rust binary. The `npx` path requires no changes to T020.

### Agent support count

At time of research: 74 named agent types in `src/types.ts` + `universal` = 75 supported.
