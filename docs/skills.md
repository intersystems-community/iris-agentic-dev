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

**There is no measured effect yet, in either direction.** An earlier run reported the skills at
+0.098 [-0.034, +0.229] and the tools at +0.829 [+0.714, +0.944] over a bare model, on 41 graded
task-pairs. Both figures are void: that harness did not set `OPENCODE_DISABLE_CLAUDE_CODE=1`, so
opencode loaded the operator's own `~/.claude/CLAUDE.md` and personal skills into every session, in
every arm (the finding is in
[`specs/130-content-skills/research.md`](../specs/130-content-skills/research.md)).

On clean isolation, a 19-task ladder at three repeats passed 11 of 19 with the tools alone and 12 of
19 with the tools plus the matching skill (b=3, c=2). That is below the run's power floor, so it
supports no claim either way. Every task is scored by an ObjectScript check against live IRIS, not
by a model.

Older figures, including a +27% for `objectscript-review` on the 22-task repair suite, came from
`iris-agentic-dev benchmark`, which uses a model as the judge on a different corpus. They are not
comparable and should not be read as current. That suite is still useful as a fast signal while you
iterate on a skill you are writing.

The multi-file and SQL-quirks suites referenced in earlier versions of this table are not
yet ported to the current native benchmark harness (`iris-agentic-dev benchmark`) — only
the repair suite above is runnable today. See
[skills/BENCHMARKING.md](../skills/BENCHMARKING.md) to run it yourself, including a
[Limitations](../skills/BENCHMARKING.md#limitations) section covering contamination
risk, single-run variance, and single-model validation caveats.

---

## Installing skills

Every skill has a tier, set by `tier:` in its `SKILL.md` frontmatter:

- **core**: the ten skills most ObjectScript work needs. The Claude Code plugin ships these (and its own setup, coverage and pyprod skills), and a bare `skill install` installs these.
- **extra**: the other 24. Install them by name or with `--all`. The `iris-agentic-dev` skill lists each one with a line on when to load it, and `skill_describe` reads any of them over MCP without installing anything.
- **internal**: skills for working on iad itself (today only `opencode-introspect`). No list shows them. Install one by name if you want it.

Install the core skills to Claude Code and OpenCode:

```bash
iris-agentic-dev skill install
```

Install core and extra (what a bare install did before 1.5.0):

```bash
iris-agentic-dev skill install --all
```

Install specific skills at any tier, target an agent, or preview first:

```bash
iris-agentic-dev skill install iris-query-plans iris-vector-ai
iris-agentic-dev skill install --agent claude-code
iris-agentic-dev skill install --agent opencode
iris-agentic-dev skill install --agent copilot   # repo-scoped; run from a git repo
iris-agentic-dev skill install --dry-run         # preview without writing
iris-agentic-dev skill install --force           # overwrite user-authored files
```

Check what's installed:

```bash
iris-agentic-dev skill list                      # every core and extra skill, with its tier
iris-agentic-dev skill list --agent claude-code
iris-agentic-dev skill status                    # managed vs user-authored
```

**Upgrading**: `brew upgrade iris-agentic-dev` (or replacing the binary directly)
updates the binary only — installed skills are not touched. After upgrading, run
`iris-agentic-dev skill install` to pick up new skills. Files that lack the
`managed_by: "iris-agentic-dev"` marker (installed before it was introduced, or
installed by other means) are skipped as unrecognized; pass `--force` once to
overwrite them and stamp them for automatic updates going forward.

**Skills left by an older install**: before 1.5.0 a bare install wrote all 34 skills. A
bare install now writes the ten core skills and deletes nothing. If it finds managed
copies of extra or internal skills in `~/.claude/skills` or the OpenCode skills
directory, it names them and stops there. Then pick one:

```bash
iris-agentic-dev skill install --all                # keep them, and keep them up to date
iris-agentic-dev skill install --prune --dry-run    # show what --prune would remove
iris-agentic-dev skill install --prune              # remove them
```

`--prune` removes only files that carry the `managed_by: "iris-agentic-dev"` marker. A
`SKILL.md` you wrote yourself under the same name stays, and so does any other file in
that skill's directory. `--prune --all` removes only internal skills. `--prune` with
skill names is refused. Copilot's `.github/instructions/` is never pruned, since it is
committed to a repo.

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

All 35 skills below ship embedded in the binary, at every tier — no download, no IRIS connection, no
filesystem lookup. This is the whole list. Agents read a short inventory as "these are
the skills that exist" and reimplement from scratch rather than asking for one that is
missing from the table, so any skill in the binary belongs here.

| Skill                              | Tier     | What it does                                                                                                            | Benchmark   |
| ---------------------------------- | -------- | ----------------------------------------------------------------------------------------------------------------------- | ----------- |
| `ensemble-production`              | extra    | Interoperability production lifecycle, logs, queues                                                                     | domain      |
| `iris-agentic-dev`                 | core     | Configuring, connecting and troubleshooting this MCP server itself                                                      |             |
| `iris-ai-hub`                      | extra    | AI Hub (`%AI.*`): where the ai-hub-eap docs are, build-first workflow, facts measured on 2026.3.0AI build 139           |             |
| `iris-connectivity`                | core     | IRIS connection APIs from Python, Java, JDBC, ODBC                                                                      | domain      |
| `iris-container-graceful-shutdown` | extra    | Why `docker stop` leaves a dirty WIJ, and how to stop IRIS so data survives a restart                                   |             |
| `iris-cpf-merge`                   | extra    | Configuring containers via `ISC_CPF_MERGE_FILE` instead of `docker exec`                                                |             |
| `iris-devtester`                   | extra    | `IRISContainer` factory methods and test fixture patterns                                                               | domain      |
| `iris-docs`                        | extra    | Which documentation tool to use, the Algolia search recipe and the Documatic URL pattern                                |             |
| `iris-embedded-python`             | extra    | Running Python inside IRIS: the native API, calling Python from ObjectScript                                            |             |
| `iris-linux-docker`                | extra    | The UID 51773 bind-mount permission failure that crashes IRIS containers on Linux                                       |             |
| `iris-mdx`                         | extra    | MDX queries against IRIS BI cubes: discovery with `%DeepSee.Utils`, running through `iris_execute`, 2026.2 `%MDX()` bug |             |
| `iris-objectscript-eval`           | extra    | Execute/compile/test loop over the MCP tools, with docker exec only as a fallback                                       |             |
| `iris-pgwire`                      | extra    | Connecting to IRIS over the PostgreSQL wire protocol (psycopg3 and other PG clients)                                    |             |
| `iris-product-features`            | extra    | What IRIS actually ships — the features and product boundaries models invent                                            |             |
| `iris-query-plans`                 | extra    | Reading query plans; stale indexes, `%BuildIndices`, `TUNE TABLE`, outlier selectivity                                  |             |
| `iris-sql`                         | core     | Writing and debugging IRIS SQL: table naming, NULL semantics, `SQLCODE`, DDL quirks                                     |             |
| `iris-vector-ai`                   | extra    | IRIS vector search syntax (HNSW, `VECTOR_COSINE`, `TO_VECTOR`)                                                          | domain      |
| `iris-vscode-objectscript`         | extra    | VS Code ObjectScript setup against a container, including the 52773-vs-1972 trap                                        |             |
| `iris-windows-iis-setup`           | extra    | IIS configuration for a native Windows IRIS so this server can reach Atelier                                            |             |
| `irishealth-container`             | extra    | IRIS for Health and AI Hub containers: FHIR R4 without ZPM, the enterprise/community web split                          |             |
| `irispython-connector`             | extra    | Python to IRIS over TCP: DB-API, SQLAlchemy, pandas, and the segfault that hits every newcomer                          |             |
| `objectscript-coverage`            | extra    | Measuring ObjectScript line coverage with `iris_coverage`                                                               |             |
| `objectscript-debugging`           | core     | Maps `.INT` offsets to `.CLS` source lines, reads error logs                                                            |             |
| `objectscript-fewshot-fixes`       | extra    | Worked Bug → Root Cause → Fix examples for the seven most common ObjectScript mistakes                                  |             |
| `objectscript-guardrails`          | core     | All-in-one hard gate, works without MCP                                                                                 | 86% repair  |
| `objectscript-list-patterns`       | core     | `%List`, `$LISTBUILD`, `$LISTNEXT`, `$LISTTOSTRING` patterns                                                            | 91% repair  |
| `objectscript-loop-patterns`       | extra    | `For`/`While`, `$Order` iteration, postfix `Quit`, `Return` vs `Quit`                                                   | −19% lift   |
| `objectscript-mac-routines`        | extra    | MAC routine syntax: labels, `#include`, `$ZTRAP`, extrinsic functions                                                   |             |
| `objectscript-navigation`          | extra    | Codebase discovery using MCP introspection tools                                                                        | 82% repair  |
| `objectscript-repair`              | extra    | Coordinated fixes across multiple dependent classes                                                                     |             |
| `objectscript-review`              | core     | Hard-gate checklist: the 11 most common AI mistakes in ObjectScript                                                     | 100% repair |
| `objectscript-sql-patterns`        | core     | IRIS SQL quirks: reserved words, SQLCODE, table naming, NULL handling                                                   | 100% SQL    |
| `objectscript-tdd`                 | core     | Compile-test-fix loop for iterative development                                                                         |             |
| `objectscript-unit-test`           | core     | Generates `%UnitTest` scaffolding from live class introspection                                                         | 86% repair  |
| `opencode-introspect`              | internal | Reading and searching opencode session logs out of its SQLite database                                                  |             |

`skills/skills/iris-agentic-dev/nopws-setup/SKILL.md` is a repo reference file, not a
bundled skill: discovery globs `<skills dir>/*/SKILL.md`, so a file one level deeper is
never loaded by the binary, and it is not in the embedded catalog, so `skill_list` does not
show it. The Claude Code plugin names its path explicitly, so plugin users get it. Everyone
else can read it in the repo.

The Benchmark column is April 2026 data, scored by a model on earlier text of each skill. "repair" scores are reproducible today via `iris-agentic-dev benchmark --suite jira`.
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

When the MCP server is running, the learning agent can turn your last tool calls into a stub
skill and keeps skills in the `^SKILLS` global on the server. Use the `skill` tool:

| Tool                   | What it does                                                           |
| ---------------------- | ---------------------------------------------------------------------- |
| `skill` with `list`    | Show all skills in the registry                                        |
| `skill` with `propose` | Save a stub skill from the last tool calls (needs at least five calls) |
| `skill` with `search`  | Find skills relevant to a topic                                        |
| `skill` with `forget`  | Remove a skill from the registry                                       |
| `skill_community`      | List or install skills from repos named with `--subscribe owner/repo`  |

### Serving skills from a directory

The MCP server's `skill_list`, `skill_search` and `skill_describe` serve the skills embedded in the
binary. A copy on disk replaces the embedded one when it comes from:

1. `IRIS_AGENTIC_DEV_SKILLS_DIR`, set in the launch settings.
2. A `skills/skills` directory next to the executable or up to three levels above it, which is how
   a binary built in a checkout reads that checkout's edits.
3. `$OBJECTSCRIPT_WORKSPACE/skills/skills` and `<working directory>/skills/skills`, only when you
   opt in with `IAD_LOCAL_SKILLS=1` or `local_skills = true` in `~/.iris-agentic-dev.toml`. A
   project file cannot turn this on, and `check_config.skills.ignored_dir` names a directory that
   was left unread. A project's own text would otherwise arrive under this server's name, ahead of
   the guardrail skills the server tells the agent to follow.

A served disk copy is labelled `source: "override"` with its `path`, and keeps the embedded tier,
so no file can mark a bundled skill `internal` to hide it. A skill found only on disk is `local` and
keeps its own tier.

---

## Contributing a skill

Write a `SKILL.md`, run the benchmark, submit a PR with your results.

See [`skills/`](../skills/) for the full skill list, benchmark results, and
contribution guide.
