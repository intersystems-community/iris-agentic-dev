# Implementation Plan: Official InterSystems Skill Pack

**Branch**: `061-official-skill-pack` | **Date**: 2026-07-16 | **Spec**: [spec.md](spec.md)

## Summary

Add `iris-agentic-dev skill install` — a subcommand that writes skill files from the
`skills/skills/` pack into AI agent instruction directories (`~/.claude/skills/`,
`.github/copilot-instructions.md`, etc.) without requiring a live IRIS connection. The pack
covers iris-agentic-dev itself plus designated InterSystems ecosystem projects. Content must
be updatable independently of a binary release (FR-009). User-authored skills must not be
silently overwritten (FR-008).

## Technical Context

**Language/Version**: Rust 2021 edition (existing workspace)
**Primary Dependencies**: existing workspace crates; `reqwest` (already present for GitHub fetches in `skills/mod.rs`); no new deps anticipated
**Storage**: Filesystem writes to `~/.claude/skills/`, `.github/copilot-instructions.md`; no IRIS, no database
**Testing**: `cargo test` (unit); no IRIS required for any skill install path
**Target Platform**: macOS arm64/x86_64, Linux x86_64, Windows x86_64 (Constitution I)
**Project Type**: CLI subcommand added to `iris-agentic-dev-bin`
**Performance Goals**: Single install completes in <5s on typical broadband (or instant if bundled)
**Constraints**: Zero IRIS dependency for install (spec FR-002); no new crates without justification (Constitution VII)
**Scale/Scope**: ~15 skills in pack; 3 agent targets (Claude Code, Copilot, OpenCode)

## Constitution Check

_GATE: Must pass before Phase 0 research. Re-check after Phase 1 design._

| Principle                      | Status | Notes                                                                                                       |
| ------------------------------ | ------ | ----------------------------------------------------------------------------------------------------------- |
| I. Zero-Install Binary         | PASS   | Install writes files, needs no IRIS, no runtime deps — pure filesystem                                      |
| II. ObjectScript Sanity        | N/A    | No ObjectScript APIs called by this feature                                                                 |
| III. HTTP-First Execution      | N/A    | No new MCP tools; CLI subcommand only                                                                       |
| IV. Test-First, Fixture-Driven | PASS   | Unit tests with no IRIS: path resolution, collision detection, idempotent write                             |
| V. Output Shape Parity         | N/A    | No new MCP tools with JSON response shapes                                                                  |
| VI. Environment Guard          | N/A    | No IRIS write operations                                                                                    |
| VII. Dependency Minimalism     | PASS   | `reqwest` already in workspace; `dirs` already in workspace (`dirs = "5"` in core Cargo.toml). No new deps. |
| VIII. 90% Coverage Gate        | PASS   | Polish phase will include coverage-check; skill install logic is pure Rust, highly testable                 |
| IX. Tool Lift Requirement      | N/A    | This is a CLI subcommand, not an MCP tool. No agent invokes `skill install` directly.                       |

_All gates PASS. No FAIL. No NEEDS CLARIFICATION remaining._

## Project Structure

### Documentation (this feature)

```text
specs/061-official-skill-pack/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/           # Phase 1 output
└── tasks.md             # Phase 2 output (/speckit.tasks)
```

### Source Code (repository root)

```text
crates/iris-agentic-dev-bin/src/cmd/
└── skill.rs             # new: SkillCommand with install/list/status subcommands

crates/iris-agentic-dev-core/src/
└── skill_install/
    ├── mod.rs           # SkillInstaller trait + platform path resolution
    ├── claude_code.rs   # ~/.claude/skills/<name>/SKILL.md writer
    ├── copilot.rs       # .github/copilot-instructions.md writer (merge strategy)
    └── opencode.rs      # ~/.config/opencode/skills/<name>/SKILL.md writer

skills/skills/           # existing — skill source content (renamed from light-skills/)
```

## Complexity Tracking

No constitution violations.

---

## Key Decisions (from research.md)

| Decision               | Choice                                                 | Rationale                                                                                |
| ---------------------- | ------------------------------------------------------ | ---------------------------------------------------------------------------------------- |
| Content distribution   | Fetch from `raw.githubusercontent.com` at install time | FR-009 requires updates without binary release; `SkillRegistry::load_from_github` reused |
| Copilot install target | `.github/instructions/<name>.instructions.md`          | Collision-free, composable, user can toggle per-skill                                    |
| Idempotent update      | Overwrite                                              | Simple, fast, standard pattern                                                           |
| Collision detection    | `managed_by: "iris-agentic-dev"` frontmatter marker    | Self-describing, no side-channel file needed                                             |
| P1 agents              | Claude Code + OpenCode                                 | Both user-global, same file format, same implementation                                  |
| Cursor                 | Out of scope                                           | `.mdc` format + glob-based selection model is incompatible                               |
| New deps               | None                                                   | `reqwest` and `dirs` already in workspace                                                |

## Spec Additions Required Before `/speckit.tasks`

These gaps must be added to `spec.md` (from benchmarking + research findings):

1. **Multi-target install**: `--agent` flag selects `claude-code | opencode | copilot | all`; default = `claude-code + opencode`
2. **Copilot target is `.github/instructions/`** not `.github/copilot-instructions.md`; requires cwd to be a git repo
3. **Managed-by marker** as the FR-008 collision mechanism
4. **FR-007 clarification**: IRIS mirror ≠ automatic; explicitly syncs filesystem → `^SKILLS`
5. **Benchmark validation**: Skill injection confirmed +1.83–2.33 lift on PYPR (0.17 → 2.00–2.50/3)
