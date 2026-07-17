# ObjectScript AI — Light Skills

**Benchmark-validated AI coding support for ObjectScript / IRIS.**

**Primary path**: Use the [iris-dev VS Code extension](https://github.com/intersystems-community/iris-agentic-dev) — it wires compile, test, introspect, and Interoperability tools into Copilot agent mode with zero extra configuration, reading your existing `objectscript.conn`.

**This repo** (`skills/`) provides the skill files and AGENTS.md that make any AI agent — with or without the MCP extension — write better ObjectScript.

---

## The numbers

Tested with Claude Sonnet 4.6 on **41 tasks** across three benchmarks (real patterns from ISC codebases):

| Suite               | Tasks | Baseline | Best skill | Lift     |
| ------------------- | ----- | -------- | ---------- | -------- |
| ObjectScript repair | 22    | 73%      | **100%**   | **+27%** |
| Multi-file repair   | 5     | 80%      | **100%**   | **+20%** |
| IRIS SQL quirks     | 14    | 93%      | **100%**   | **+7%**  |

The top-scoring skill on the repair benchmark is **`objectscript-review`** by [Timothy Leavitt](https://github.com/tleavitt-isc) — a 205-word hard-gate checklist that catches the 10 most common ObjectScript mistakes before the AI shows you any code.

---

## 🏆 Skill Leaderboard

_Ranked by pass rate on the 22-task ObjectScript repair benchmark. Baseline (no skill) = 73%._

| Rank | Skill                                                            | Author              | Words | Score    | Lift     | Suite                       |
| ---- | ---------------------------------------------------------------- | ------------------- | ----- | -------- | -------- | --------------------------- |
| 🥇 1 | **[objectscript-review](skills/objectscript-review/)**           | **Timothy Leavitt** | 205   | **100%** | **+29%** | Repair                      |
| 🥈 2 | [objectscript-list-patterns](skills/objectscript-list-patterns/) | Tom Dyar            | 472   | 91%      | —        | Repair                      |
| 🥈 2 | [objectscript-unit-test](skills/objectscript-unit-test/)         | Timothy Leavitt     | 340   | 86%      | —        | Repair                      |
| 🥈 2 | [objectscript-guardrails](skills/objectscript-guardrails/)       | Tom Dyar            | 268   | 86%      | +14%     | Repair (no MCP)             |
| 4    | [objectscript-navigation](skills/objectscript-navigation/)       | Timothy Leavitt     | 231   | 82%      | —        | Repair                      |
| 5    | [objectscript-tdd](skills/objectscript-tdd/)                     | Timothy Leavitt     | 256   | 55%      | —        | Repair                      |
| —    | [iris-sql](skills/iris-sql/)                                     | Tom Dyar            | 2445  | 100%     | +7%      | **SQL**                     |
| —    | [iris-light](skills/iris-light/)                                 | Tom Dyar            | 5170  | 21%      | —        | Repair                      |
| —    | [iris-vector-ai](skills/iris-vector-ai/) ⚡                      | Tom Dyar            | 434   | —        | domain   | **Vector/AI**               |
| —    | [iris-connectivity](skills/iris-connectivity/) ⚡                | Tom Dyar            | 490   | —        | domain   | **Connectivity**            |
| —    | [iris-product-features](skills/iris-product-features/) ⚡        | Tom Dyar            | 679   | —        | domain   | **Product caps**            |
| —    | [iris-agentic-dev](skills/iris-agentic-dev/) ⚡                  | Tom Dyar            | —     | —        | domain   | **MCP config/troubleshoot** |
| —    | [iris-vector-graph](skills/iris-vector-graph/) ⚡                | Tom Dyar            | —     | —        | domain   | **Vector/graph ecosystem**  |
| —    | [iris-embedded-python](skills/iris-embedded-python/) ⚡          | Tom Dyar            | —     | —        | domain   | **Embedded Python**         |
| —    | [iris-vector-rag](skills/iris-vector-rag/) ⚡                    | Tom Dyar            | —     | —        | domain   | **RAG pipeline**            |

> **Note**: Negative results matter too. `objectscript-loop-patterns` (572 words) measured **-19% lift** when loaded globally. The ⚡ product knowledge skills (`iris-vector-ai`, `iris-connectivity`, `iris-product-features`) are **load-on-demand** only — they correct specific failure modes when you're working in those domains but hurt if loaded globally. See [BENCHMARKING.md](BENCHMARKING.md).

**Want your skill on the leaderboard?** See [Contributing a skill](#contributing-a-skill) below.

---

## 60-second setup

### Step 1: Copy AGENTS.md to your repo

```bash
curl -sL https://raw.githubusercontent.com/intersystems-community/iris-agentic-dev/master/skills/AGENTS.md \
  > AGENTS.md
```

That's it for the baseline (+14%). Your AI agent now knows the top 10 ObjectScript gotchas.

### Step 2: Add the #1 skill for 100% (without the MCP extension)

**Claude Code:**

```bash
mkdir -p .claude/skills/objectscript-review
curl -sL https://raw.githubusercontent.com/intersystems-community/iris-agentic-dev/master/skills/skills/objectscript-review/SKILL.md \
  > .claude/skills/objectscript-review/SKILL.md
```

**opencode:**

```bash
mkdir -p ~/.config/opencode/skills/objectscript-review
curl -sL https://raw.githubusercontent.com/intersystems-community/iris-agentic-dev/master/skills/skills/objectscript-review/SKILL.md \
  > ~/.config/opencode/skills/objectscript-review/SKILL.md
```

### Step 3 (optional): Install the full validated set

```bash
SKILLS_DIR=~/.config/opencode/skills   # or ~/.claude/skills for Claude Code
BASE=https://raw.githubusercontent.com/intersystems-community/iris-agentic-dev/master/skills/skills

for skill in objectscript-review objectscript-list-patterns objectscript-sql-patterns \
             objectscript-loop-patterns objectscript-unit-test objectscript-navigation \
             objectscript-guardrails; do
  mkdir -p "$SKILLS_DIR/$skill"
  curl -sL "$BASE/$skill/SKILL.md" > "$SKILLS_DIR/$skill/SKILL.md"
done
echo "Done. $(ls $SKILLS_DIR | wc -l) skills installed."
```

---

## What's in this directory

| File/Directory                    | Purpose                                                        |
| --------------------------------- | -------------------------------------------------------------- |
| `AGENTS.md`                       | ObjectScript rules — drop in your repo root                    |
| `BENCHMARKING.md`                 | **How to run the benchmarks yourself and submit results**      |
| `compile.md`                      | Skill: compile via Atelier REST, structured errors             |
| `introspect.md`                   | Skill: fetch any class definition from IRIS                    |
| `skills/objectscript-review/`     | **🥇 Start here** — hard-gate review, 100% on repair benchmark |
| `skills/objectscript-*`           | Validated pattern skills (see leaderboard above)               |
| `skills/objectscript-guardrails/` | 268-word all-in-one hard gate (alternative to review)          |
| `skills/iris-sql/`                | IRIS SQL quirks: reserved words, SQLCODE, table naming         |
| `kb/`                             | Reference knowledge: error codes, idioms, IPM authoring        |
| `iris-dev.toml`                   | Package manifest for `iris-dev` CLI install                    |

---

## For ISC SEs and developers — dogfood instructions

**5 minutes to set up, measurable improvement immediately.**

### If you use Claude Code

```bash
# 1. Copy AGENTS.md to your project
curl -sL https://raw.githubusercontent.com/intersystems-community/iris-agentic-dev/master/skills/AGENTS.md > AGENTS.md

# 2. Install top skills globally
mkdir -p ~/.claude/skills
for skill in objectscript-review objectscript-sql-patterns objectscript-guardrails; do
  mkdir -p ~/.claude/skills/$skill
  curl -sL https://raw.githubusercontent.com/intersystems-community/iris-agentic-dev/master/skills/skills/$skill/SKILL.md \
    > ~/.claude/skills/$skill/SKILL.md
done
ls ~/.claude/skills/
```

### If you use opencode

```bash
curl -sL https://raw.githubusercontent.com/intersystems-community/iris-agentic-dev/master/skills/AGENTS.md > AGENTS.md

mkdir -p ~/.config/opencode/skills
for skill in objectscript-review objectscript-sql-patterns objectscript-guardrails; do
  mkdir -p ~/.config/opencode/skills/$skill
  curl -sL https://raw.githubusercontent.com/intersystems-community/iris-agentic-dev/master/skills/skills/$skill/SKILL.md \
    > ~/.config/opencode/skills/$skill/SKILL.md
done
```

### If you use VS Code Copilot

The [iris-dev VS Code extension](https://github.com/intersystems-community/iris-agentic-dev) wires skills into Copilot agent mode automatically.

### Connect to IRIS (for compile/introspect skills)

```bash
export IRIS_HOST=localhost
export IRIS_WEB_PORT=52773     # Atelier REST port — NOT 1972
export IRIS_USER=_SYSTEM
export IRIS_PASS=SYS
export IRIS_NS=USER
```

> **Docker?** `docker port <container> 52773` gives you the mapped port.

---

## What to try first

1. **Open an existing ObjectScript class** in your editor
2. Ask your AI: _"Review this method for ObjectScript mistakes"_
3. With `objectscript-review` loaded, the AI runs the 10-item checklist and corrects issues before showing you anything
4. Ask it to _"write a unit test for this class"_ — with `objectscript-unit-test`, it reads the actual IRIS class definition first

**Tell us what you find.** File issues at [intersystems-community/iris-agentic-dev](https://github.com/intersystems-community/iris-agentic-dev) or ping `@tdyar` / `@tleavitt` on Teams.

---

## Contributing a skill

**Your skill could be on the leaderboard.** The bar is clear: write a `SKILL.md`, run the benchmark, report your score.

### 1. Write your skill

Use the `objectscript-review` skill as the reference design — 205 words, a hard gate, a checklist, an output format. See [BENCHMARKING.md](BENCHMARKING.md) for the RED-GREEN methodology.

Frontmatter required:

```yaml
---
name: "yourgithub/your-skill-name"
description: "Use when ..." # triggers only — no workflow summary
iris_version: ">=2024.1"
tags: [objectscript]
author: yourgithub
state: draft
---
```

### 2. Run the benchmark

```bash
# See BENCHMARKING.md for full instructions — takes ~5 minutes
git clone https://github.com/intersystems-community/iris-dev
cd iris-dev/skills
./bench/run_benchmark.sh --skill path/to/your/SKILL.md --baseline
```

### 3. Submit a PR

Open a PR to this repo with:

- Your `skills/yourgithub/your-skill/SKILL.md`
- Benchmark results in the PR description (pass rate, baseline, lift, IRIS version)
- A one-line description of what your skill catches that others don't

Skills that improve on the leaderboard get merged. Skills with negative lift on the repair suite get labeled "domain-specific" and noted as load-on-demand only.

---

## Want the full stack?

The `objectscript-mcp` server adds live IRIS integration — automatic introspection, symbol search, and a learning agent that synthesizes new skills from your session patterns.

```bash
pip install objectscript-mcp
objectscript-mcp  # starts MCP server on stdio
```

See [MCP_SETUP_GUIDE.md](../docs/MCP_SETUP_GUIDE.md) for configuration.

---

## Benchmark methodology

Three suites, 41 tasks total, all on IRIS 2025.1 Community Edition in Docker with Claude Sonnet 4.6 via AWS Bedrock:

| Suite               | Tasks | What it tests                                                 |
| ------------------- | ----- | ------------------------------------------------------------- |
| ObjectScript repair | 22    | Single-function bugs: null checks, loops, SQL, error handling |
| Multi-file repair   | 5     | Method renames, signature changes, SQL renames across files   |
| IRIS SQL quirks     | 14    | SQLCODE, reserved words, %INLIST, IN limits, streams, DDL     |

Each task: buggy `.cls` file + test that fails + oracle that verifies the fix. Lift = skill pass rate − baseline (no skill).

**Run it yourself → [BENCHMARKING.md](BENCHMARKING.md)**
