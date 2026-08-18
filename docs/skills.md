# Skills

Skills are concise instruction files that teach your AI assistant ObjectScript-specific
patterns and common mistakes. They work with or without the MCP server.

Skills and the MCP server are independent. Installing the `iris-agentic-dev` binary
installs no skills. Skills are opt-in, managed separately, and can live in any repo.
Domain-specific skill packs (like Keshav Iyer's
[IKO skill](https://gitlab.iscinternal.com/fschwich/isc-iko-skill)) are the intended
pattern for opinionated or team-specific skills.

---

## Benchmark results

Tested with Claude Sonnet 4.6 on the ObjectScript repair suite (22 tasks):

| Benchmark suite                | Baseline | With top skill | Lift |
| ------------------------------ | -------- | -------------- | ---- |
| ObjectScript repair (22 tasks) | 73%      | **100%**       | +27% |

The top skill is **`objectscript-review`** — a 205-word checklist that catches the 10 most
common ObjectScript mistakes before the AI writes any code.

The multi-file and SQL-quirks suites referenced in earlier versions of this table are not
yet ported to the current native benchmark harness (`iris-agentic-dev benchmark`) — only
the repair suite above is runnable today. See
[skills/BENCHMARKING.md](../skills/BENCHMARKING.md) to run it yourself, including a
[Limitations](../skills/BENCHMARKING.md#limitations) section covering contamination
risk, single-run variance, and single-model validation caveats.

---

## Installing skills

Install the full official pack to Claude Code and OpenCode:

```bash
iris-agentic-dev skill install
```

Install specific skills, target an agent, or preview first:

```bash
iris-agentic-dev skill install objectscript-review objectscript-guardrails
iris-agentic-dev skill install --agent claude-code
iris-agentic-dev skill install --agent opencode
iris-agentic-dev skill install --agent copilot   # repo-scoped; run from a git repo
iris-agentic-dev skill install --dry-run         # preview without writing
iris-agentic-dev skill install --force           # overwrite user-authored files
```

Check what's installed:

```bash
iris-agentic-dev skill list
iris-agentic-dev skill list --agent claude-code
iris-agentic-dev skill status                    # managed vs user-authored
```

**VS Code Copilot**: The extension installs the binary, not the skills. Run
`iris-agentic-dev skill install --agent copilot` from a git repo root to install skills
into `.github/instructions/` — commit that directory to share with your team.

**Manual fallback** — if you prefer not to use the CLI:

**Claude Code:**

```bash
mkdir -p ~/.claude/skills
for skill in objectscript-review objectscript-guardrails objectscript-sql-patterns; do
  mkdir -p ~/.claude/skills/$skill
  curl -sL https://raw.githubusercontent.com/intersystems-community/iris-agentic-dev/master/skills/skills/$skill/SKILL.md \
    > ~/.claude/skills/$skill/SKILL.md
done
```

**OpenCode:**

```bash
mkdir -p ~/.config/opencode/skills
for skill in objectscript-review objectscript-guardrails objectscript-sql-patterns; do
  mkdir -p ~/.config/opencode/skills/$skill
  curl -sL https://raw.githubusercontent.com/intersystems-community/iris-agentic-dev/master/skills/skills/$skill/SKILL.md \
    > ~/.config/opencode/skills/$skill/SKILL.md
done
```

---

## Skill inventory

| Skill                        | What it does                                                                                    | Benchmark   |
| ---------------------------- | ----------------------------------------------------------------------------------------------- | ----------- |
| `objectscript-review`        | Hard-gate checklist: 10 most common AI mistakes in ObjectScript                                 | 100% repair |
| `objectscript-guardrails`    | All-in-one hard gate, works without MCP                                                         | 86% repair  |
| `objectscript-sql-patterns`  | IRIS SQL quirks: reserved words, SQLCODE, table naming, NULL handling                           | 100% SQL    |
| `objectscript-unit-test`     | Generates `%UnitTest` scaffolding from live class introspection                                 | 86% repair  |
| `objectscript-list-patterns` | `%List`, `$LISTBUILD`, `$LISTNEXT`, `$LISTTOSTRING` patterns                                    | 91% repair  |
| `objectscript-navigation`    | Codebase discovery using MCP introspection tools                                                | 82% repair  |
| `objectscript-tdd`           | Compile-test-fix loop for iterative development                                                 |             |
| `objectscript-debugging`     | Maps `.INT` offsets to `.CLS` source lines, reads error logs                                    |             |
| `objectscript-repair`        | Coordinated fixes across multiple dependent classes                                             |             |
| `iris-docs`                  | Fetches live IRIS class reference before implementing any API — eliminates hallucinated methods |             |
| `iris-vector-ai`             | IRIS vector search syntax (HNSW, `VECTOR_COSINE`, `TO_VECTOR`)                                  | domain      |
| `iris-connectivity`          | IRIS connection APIs from Python, Java, JDBC, ODBC                                              | domain      |
| `ensemble-production`        | Interoperability production lifecycle, logs, queues                                             | domain      |
| `iris-devtester`             | `IRISContainer` factory methods and test fixture patterns                                       | domain      |

"repair" scores are reproducible today via `iris-agentic-dev benchmark --suite jira`.
"SQL" and "domain" scores predate the current native harness and are not yet
re-verifiable — see [BENCHMARKING.md](../skills/BENCHMARKING.md#additional-suites-not-yet-ported).

---

## Skill loading caution

Some skills hurt if loaded globally:

- `objectscript-loop-patterns` measured **−19% lift** when loaded for all tasks.
- Domain skills (`iris-vector-ai`, `iris-connectivity`, `ensemble-production`) should only
  be loaded when working in those areas — loading them for general ObjectScript work adds
  noise without benefit.

See [BENCHMARKING.md](../skills/BENCHMARKING.md) for detailed per-skill results.

---

## MCP-backed skill registry

When the MCP server is running, the learning agent can mine your session history to propose
new skills and optimize existing ones. Use the `skill` tool:

| Tool                   | What it does                                   |
| ---------------------- | ---------------------------------------------- |
| `skill` with `list`    | Show all skills in the registry                |
| `skill` with `propose` | Mine recent tool calls to propose a new skill  |
| `skill` with `search`  | Find skills relevant to a topic                |
| `skill` with `forget`  | Remove a skill from the registry               |
| `skill_community`      | Browse or install community skills from GitHub |

---

## Contributing a skill

Write a `SKILL.md`, run the benchmark, submit a PR with your results.

See [`skills/`](../skills/) for the full skill list, benchmark results, and
contribution guide.
