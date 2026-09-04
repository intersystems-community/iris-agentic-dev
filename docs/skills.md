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

**Upgrading**: `brew upgrade iris-agentic-dev` (or replacing the binary directly)
updates the binary only — installed skills are not touched. After upgrading, run
`iris-agentic-dev skill install` to pick up new skills. Files that lack the
`managed_by: "iris-agentic-dev"` marker (installed before it was introduced, or
installed by other means) are skipped as unrecognized; pass `--force` once to
overwrite them and stamp them for automatic updates going forward.

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

All 34 skills below ship embedded in the binary — no download, no IRIS connection, no
filesystem lookup. This is the whole list. Agents read a short inventory as "these are
the skills that exist" and reimplement from scratch rather than asking for one that is
missing from the table, so any skill in the binary belongs here.

| Skill                              | What it does                                                                                    | Benchmark   |
| ---------------------------------- | ----------------------------------------------------------------------------------------------- | ----------- |
| `aihub-eap`                        | AI Hub Early Access API patterns: `%AI.Agent`, `%AI.Provider`, ConfigStore, per-build breakage  |             |
| `ensemble-production`              | Interoperability production lifecycle, logs, queues                                             | domain      |
| `iris-agentic-dev`                 | Configuring, connecting and troubleshooting this MCP server itself                              |             |
| `iris-ai-hub`                      | AI Hub production patterns: agents wrapped in business operations, BPL async, human-in-the-loop |             |
| `iris-connectivity`                | IRIS connection APIs from Python, Java, JDBC, ODBC                                              | domain      |
| `iris-container-graceful-shutdown` | Why `docker stop` leaves a dirty WIJ, and how to stop IRIS so data survives a restart           |             |
| `iris-cpf-merge`                   | Configuring containers via `ISC_CPF_MERGE_FILE` instead of `docker exec`                        |             |
| `iris-devtester`                   | `IRISContainer` factory methods and test fixture patterns                                       | domain      |
| `iris-docs`                        | Fetches live IRIS class reference before implementing any API — eliminates hallucinated methods |             |
| `iris-embedded-python`             | Running Python inside IRIS: the native API, calling Python from ObjectScript                    |             |
| `iris-linux-docker`                | The UID 51773 bind-mount permission failure that crashes IRIS containers on Linux               |             |
| `iris-objectscript-eval`           | Execute/compile/test loop over the MCP tools, with docker exec only as a fallback               |             |
| `iris-pgwire`                      | Connecting to IRIS over the PostgreSQL wire protocol (psycopg3 and other PG clients)            |             |
| `iris-product-features`            | What IRIS actually ships — the features and product boundaries models invent                    |             |
| `iris-sql`                         | Writing and debugging IRIS SQL: table naming, NULL semantics, `SQLCODE`, DDL quirks             |             |
| `iris-vector-ai`                   | IRIS vector search syntax (HNSW, `VECTOR_COSINE`, `TO_VECTOR`)                                  | domain      |
| `iris-vscode-objectscript`         | VS Code ObjectScript setup against a container, including the 52773-vs-1972 trap                |             |
| `iris-windows-iis-setup`           | IIS configuration for a native Windows IRIS so this server can reach Atelier                    |             |
| `irishealth-container`             | IRIS for Health and AI Hub containers: FHIR R4 without ZPM, the enterprise/community web split  |             |
| `irispython-connector`             | Python to IRIS over TCP: DB-API, SQLAlchemy, pandas, and the segfault that hits every newcomer  |             |
| `objectscript-coverage`            | Measuring ObjectScript line coverage with `iris_coverage`                                       |             |
| `objectscript-debugging`           | Maps `.INT` offsets to `.CLS` source lines, reads error logs                                    |             |
| `objectscript-fewshot-fixes`       | Worked Bug → Root Cause → Fix examples for the seven most common ObjectScript mistakes          |             |
| `objectscript-guardrails`          | All-in-one hard gate, works without MCP                                                         | 86% repair  |
| `objectscript-list-patterns`       | `%List`, `$LISTBUILD`, `$LISTNEXT`, `$LISTTOSTRING` patterns                                    | 91% repair  |
| `objectscript-loop-patterns`       | `For`/`While`, `$Order` iteration, postfix `Quit`, `Return` vs `Quit`                           | −19% lift   |
| `objectscript-mac-routines`        | MAC routine syntax: labels, `#include`, `$ZTRAP`, extrinsic functions                           |             |
| `objectscript-navigation`          | Codebase discovery using MCP introspection tools                                                | 82% repair  |
| `objectscript-repair`              | Coordinated fixes across multiple dependent classes                                             |             |
| `objectscript-review`              | Hard-gate checklist: 10 most common AI mistakes in ObjectScript                                 | 100% repair |
| `objectscript-sql-patterns`        | IRIS SQL quirks: reserved words, SQLCODE, table naming, NULL handling                           | 100% SQL    |
| `objectscript-tdd`                 | Compile-test-fix loop for iterative development                                                 |             |
| `objectscript-unit-test`           | Generates `%UnitTest` scaffolding from live class introspection                                 | 86% repair  |
| `opencode-introspect`              | Reading and searching opencode session logs out of its SQLite database                          |             |

`skills/skills/iris-agentic-dev/nopws-setup/SKILL.md` is a repo reference file, not a
bundled skill: discovery globs `<skills dir>/*/SKILL.md`, so a file one level deeper is
never loaded, and it is not in the embedded catalog. Read it in the repo.

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
