# Data Model: Deep E2E Skills Harness (039)

## Entities

### IsolatedEnv
Temporary per-run environment; torn down after run (retained on failure if `--keep-on-failure`).

```
IsolatedEnv
├── skills_dir: Path          # temp dir where SKILL.md files are curl-installed
├── db_path: Path             # OPENCODE_DB for this run
├── run_id: str               # ISO timestamp, used in paths and result JSON
└── config_json: dict         # assembled OPENCODE_CONFIG_CONTENT value
```

### HarnessTask
Versioned fixture defining one test scenario. Stored in `tests/e2e/tasks/*.yaml`.

```
HarnessTask
├── id: str                          # e.g. "SKILL-01"
├── category: str                    # "skills_quality" | "mcp_tools" | "full_stack"
├── scenario: str                    # "us1_skills_only" | "us2_mcp" | "us3_full_stack"
├── description: str
├── prompt: str                      # exact message passed to `opencode run`
├── fixtures: List[Fixture]          # IRIS classes/globals to load before run
├── skills_to_install: List[str]     # skill names to curl from GitHub ([] = baseline)
├── assertions: List[Assertion]
└── expected_tool_calls: List[str]   # MCP tool names expected in event stream
```

### Fixture (nested in HarnessTask)
Reuses existing benchmark/021 fixture schema.

```
Fixture
├── type: "cls" | "global" | "routine"
├── name: str
└── content: str
```

### Assertion (nested in HarnessTask)

```
Assertion
├── type: "code_absent_pattern"      # anti-pattern must NOT appear in output code blocks
         | "tool_called"             # MCP tool must appear in event stream
         | "tool_output_contains"    # tool result must contain substring
├── pattern: str                     # regex (for code_absent_pattern) or tool name
├── description: str                 # human-readable explanation
└── required: bool                   # if false, logged but not a hard failure
```

### RunResult
Written to `tests/e2e/results/{run_id}.json` after each run.

```
RunResult
├── run_id: str
├── harness: "e2e-opencode"
├── opencode_version: str
├── iris_agentic_dev_version: str
├── model: str
├── tasks: List[TaskResult]
└── summary: Summary
```

### TaskResult (nested in RunResult)

```
TaskResult
├── task_id: str
├── scenario: str
├── condition: "baseline" | skill-name
├── pass: bool
├── skill_loaded: bool               # skill name appeared in event stream skill tool calls
├── tool_calls: List[str]            # all tool names observed
├── assertion_results: List[AssertionResult]
├── llm_output_excerpt: str          # first 500 chars of text events
└── duration_seconds: float
```

### AssertionResult (nested in TaskResult)

```
AssertionResult
├── assertion_type: str
├── description: str
├── pass: bool
└── detail: str                      # what was found/not found
```

### Summary (nested in RunResult)

```
Summary
├── pass_rate: float
├── skill_lift: float | None         # baseline vs skill pass rate delta (if both ran)
├── tool_calls_observed: List[str]   # unique tool names across all tasks
└── by_scenario: dict[str, float]    # pass rate per scenario
```

## State Transitions

```
IsolatedEnv lifecycle:
  created → skills_installed → opencode_run → assertions_evaluated → torn_down
                                                                   ↘ retained (on failure + --keep-on-failure)

HarnessTask execution:
  pending → fixtures_loaded (if IRIS available) → opencode_spawned → events_parsed → assertions_run → complete
```
