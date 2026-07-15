# iris-dev Path-Aware Benchmark (021)

Measures how well AI coding agents perform ObjectScript tasks on two development paths:

- **Path A** — Local Files + Atelier (blessed path): agent writes .cls files locally, uses `iris_compile`
- **Path B** — ISFS Only (legacy/remote-only): agent uses `iris_doc put/get`, no local files

## Prerequisites

- `iris-dev` binary on PATH (`iris-dev --version`)
- IRIS container running (iris-dev-iris or equivalent)
- `ANTHROPIC_API_KEY` set
- Python 3.11+ with deps: `pip install -r runner/requirements.txt`

## Running

```bash
cd /path/to/iris-dev

# Set connection env vars
export IRIS_HOST=localhost
export IRIS_WEB_PORT=52780
export IRIS_USERNAME=_SYSTEM
export IRIS_PASSWORD=SYS
export ANTHROPIC_API_KEY=sk-ant-...

# Dry run (no IRIS calls, no API calls) — verify tasks load
python -m benchmark.021.runner --dry-run

# Run all tasks, both paths, Claude Code harness
python -m benchmark.021.runner

# Run only GEN category, Path A
python -m benchmark.021.runner --path A --categories GEN

# Run a single task
python -m benchmark.021.runner --task GEN-01 --path A

# Generate report from existing results
python -m benchmark.021.runner --report-only results/2026-04-22T14-00-00Z/scores.json
```

## Task Categories

| Category | Description                                            | Path   |
| -------- | ------------------------------------------------------ | ------ |
| GEN      | Write new ObjectScript classes from scratch            | both   |
| MOD      | Read and modify existing classes                       | both   |
| DBG      | Diagnose and fix bugs                                  | both   |
| SCM      | Source control operations with elicitation             | B only |
| LEG      | MAC routines and globals, no classes (legacy)          | both   |
| PYPR     | Python interoperability production components (pyprod) | both   |

## Scoring (0-3, LLM-as-judge)

Scores are assigned by **Claude Haiku** acting as arbiter (consistent, cheap, fast).
The same model is used for all skill comparisons so scores are comparable across runs.

- **3** — Correct and efficient, minimal tool calls
- **2** — Correct but >2 unnecessary tool calls (agent confusion)
- **1** — Partial — compiled/loaded but incorrect behavior
- **0** — Failed or wrong output

## Adding New Tasks

Create a YAML file in `tasks/` following the schema in `../specs/021-path-aware-benchmark/contracts/task-schema.md`.

Required fields: `id`, `category`, `description`, `expected_behavior`, `path`

## Results

Results are written to `results/<timestamp>/`:

- `scores.json` — machine-readable per-task scores
- `report.html` — visual comparison report

See `results/` for published benchmark runs.
