# Data Model: Official InterSystems Skill Pack (061)

## Entities

### SkillFile

A single skill's installable content. Fetched from GitHub, written to disk.

| Field | Type | Source | Notes |
|-------|------|--------|-------|
| `name` | `String` | SKILL.md frontmatter `name:` | Filesystem directory name |
| `description` | `String` | SKILL.md frontmatter `description:` | Displayed on `skill list` |
| `content` | `String` | Full SKILL.md text (post-frontmatter) | Written verbatim |
| `managed_by` | `"iris-agentic-dev"` | Injected on write | FR-008 collision marker |

### InstallTarget

Represents one agent's install destination.

| Field | Type | Notes |
|-------|------|-------|
| `agent` | `AgentKind` | `ClaudeCode` \| `OpenCode` \| `Copilot` |
| `base_path` | `PathBuf` | Resolved at runtime (platform-aware) |
| `skill_path_fn` | `fn(&str) -> PathBuf` | Given skill name, returns full write path |

### AgentKind

```rust
enum AgentKind {
    ClaudeCode,   // ~/.claude/skills/<name>/SKILL.md
    OpenCode,     // ~/.config/opencode/skills/<name>/SKILL.md
    Copilot,      // .github/instructions/<name>.instructions.md (repo-scoped)
}
```

### InstallResult

Per-skill outcome returned to the caller.

| Field | Type | Notes |
|-------|------|-------|
| `skill_name` | `String` | |
| `agent` | `AgentKind` | |
| `outcome` | `InstallOutcome` | See below |
| `path` | `PathBuf` | Where file was written (or would have been) |

### InstallOutcome

```rust
enum InstallOutcome {
    Written,          // New install — file created
    Updated,          // Re-install — managed file overwritten
    Skipped { reason: String },  // FR-008: user-authored, not touched
    Failed { error: String },    // I/O or fetch error
}
```

---

## State Transitions

```text
[skill fetch] → SkillFile

[install target resolution] → InstallTarget (platform path)

[per skill] →
  file exists?
    NO  → Write with managed_by marker → Written
    YES →
      has managed_by marker?
        YES → Overwrite → Updated
        NO  → Skip + warn → Skipped
```

---

## Platform Path Resolution

| Agent | Unix/macOS | Windows |
|-------|-----------|---------|
| Claude Code | `$HOME/.claude/skills/<name>/SKILL.md` | `%APPDATA%\Claude\skills\<name>\SKILL.md` |
| OpenCode | `$HOME/.config/opencode/skills/<name>/SKILL.md` | `%APPDATA%\opencode\skills\<name>\SKILL.md` |
| Copilot | `.github/instructions/<name>.instructions.md` (cwd) | Same |

Use `dirs::home_dir()` for Claude Code, `dirs::config_dir()` for OpenCode. Both are already
in workspace (`dirs = "5"` in `iris-agentic-dev-core/Cargo.toml`).

---

## Error Codes

| Code | When |
|------|------|
| `SKILL_FETCH_FAILED` | GitHub fetch returned non-200 or network error |
| `SKILL_WRITE_FAILED` | Filesystem write failed (permissions, disk full) |
| `SKILL_NOT_FOUND` | Named skill not in pack (for `skill install <name>`) |
| `COPILOT_NO_REPO` | `.github/` directory doesn't exist and cwd is not a git repo |
