# 084 · Skill-tool dependency: industry state and iris-agentic-dev gap

_Draft for discussion with Keshav — not a spec yet._

---

## TL;DR

Splitting bundled skills into their own repo is the right call and matches what the rest
of the industry is doing. The missing piece before doing it: `requires_tools` frontmatter
in skill YAML plus load-time validation, so a subscribed skill can't silently describe
tools that don't exist in the user's version of the server. Do the dependency mechanism
first, then the split.

---

## The pattern that's winning: skills ship with the product

NordicDB, Supabase, and several others have settled on the same model: skills (structured
prompts, usage docs, coding patterns) live in the same repo as the tool they describe,
maintained by the same team, installed as a unit. The analogy is OpenAPI: you ship the
spec with the service because a spec in a separate repo drifts from the service the moment
someone forgets to update it.

`--subscribe <owner/repo>` already implements this. A tool author publishes a repo with
both the MCP server and a `skills/` directory. Users subscribe once; the skill stays in
sync with the tool version.

---

## MCP: no formal dependency mechanism yet

The MCP spec has no way for a server to declare "here is a skill that explains how to use
me." Some servers bundle ad-hoc `CLAUDE.md` hints. The objectscript-mcp `plugin.json` is
the closest local example: one installable artifact that brings both the tool surface and
the usage guidance together. It's just a convention, though.

Nothing in the MCP ecosystem handles "this skill requires tool X at version Y" in a portable, enforceable way. Version pinning for skill-to-tool dependencies is unsolved everywhere.

---

## Cline / RooCline: modes = skills + tool access control

Their "modes" concept is the most complete thinking on this. A mode is a role definition
(system prompt / persona) plus a tool subset (which tools the model can call). A `code`
mode gets file-edit tools; a `chat` mode gets none. Skills as capability gating, not just
documentation.

iris-agentic-dev doesn't have this. `IRIS_ENABLED_TOOLS` / `IRIS_DISABLED_TOOLS` are
config-level gates, not skill-level gates. A skill can't say "when this skill is active,
suppress these tools."

---

## Semantic Kernel: skills were first-class, then folded into plugins

SK v1 had named "Skills" as the primary abstraction. v2 renamed them "Plugins" and merged
tool and skill into one artifact. The lesson: the packaging boundary matters more than the
name. When the tool and its usage guidance are separate artifacts, they drift. When they're
one artifact, they stay coherent.

---

## What's missing everywhere

Nobody has shipped a portable "skill depends on tool at version X" mechanism:

1. **No frontmatter standard** for declaring tool dependencies in a skill YAML.
2. **No load-time validation** — skills load regardless of whether the tools they describe
   are present or at the right version.
3. **No client-side enforcement** — even if a skill declares a dependency, nothing in any
   MCP client currently acts on it.

---

## How this maps to iris-agentic-dev

| Capability                                          | State                                |
| --------------------------------------------------- | ------------------------------------ |
| Multi-repo skills via `--subscribe`                 | ✓ ships today                        |
| Skills colocated with tool source                   | ✓ bundled skills in `skills/skills/` |
| `requires_tools` frontmatter in skill YAML          | ✗ missing                            |
| Load-time validation against `requires_tools`       | ✗ missing                            |
| Skill-level tool subset gating (Cline modes analog) | ✗ missing                            |
| Version pinning for skill-to-tool dependency        | ✗ missing (unsolved industry-wide)   |

---

## Framing for the Keshav conversation

Keshav's proposal to split skills into their own repo is correct and industry-aligned. It
matches the NordicDB/Supabase model and the MCP convention that's emerging. The repo
separation is not the missing piece.

The missing piece is the dependency declaration. A skill in a separate repo needs a way
to say which tools it requires and at what minimum version, so the installer can validate
before loading. Without that, a subscribed skill can silently describe tools that don't
exist in the user's version of the server.

The right sequence:

1. Define `requires_tools` frontmatter in the skill YAML schema.
2. Add load-time validation: warn (or error) if a required tool is absent.
3. Split bundled skills into their own subscribable repo. At that point the dependency
   mechanism makes the split safe.

Splitting first makes silent failures harder to diagnose, not easier.

---

## Open questions for Keshav

- Should `requires_tools` be a minimum version, an exact version, or a capability flag?
- What's the right failure mode at load time: warn and load anyway, or refuse to load?
- Is there appetite in the MCP spec to standardize this, or should it be an
  iris-agentic-dev convention first?
