# Quickstart: Tool Telemetry and Benchmark Harness

This is the corrected version of `light-skills/BENCHMARKING.md`'s Quick Start (FR-008) —
runnable from a clean clone of the public `iris-agentic-dev` repository with no reference
to any private repository or separate Python MCP server.

## Run one skill in ~10 minutes

```bash
git clone https://github.com/intersystems-community/iris-agentic-dev.git
cd iris-agentic-dev
cargo build --release

# 1. Start the IRIS benchmark container (any name; harness auto-provisions via
#    the same discovery chain every other iris-agentic-dev tool uses)
docker run -d --name iris-bench \
  -p 1972:1972 -p 52773:52773 \
  intersystemsdc/iris-community:latest
sleep 30   # wait for IRIS to start

# 2. Run the benchmark with a skill from the repo's own light-skills/skills/
export IRIS_HOST=localhost
export IRIS_WEB_PORT=52773
export IRIS_GENERATE_CLASS_MODEL=claude-sonnet-4-6   # or any model generate.rs supports
export ANTHROPIC_API_KEY=sk-ant-...                   # or OPENAI_API_KEY for gpt-* models

./target/release/iris-agentic-dev benchmark \
  --skill light-skills/skills/objectscript-review/SKILL.md \
  --baseline \
  --output results.json

# 3. See your results
cat results.json | python3 -c "
import json,sys
d=json.load(sys.stdin)
print(f\"Pass rate: {d['pass_rate']:.0%} ({d['tasks_passed']}/{d['tasks_total']})\")
print(f\"Baseline: {d.get('baseline_pass_rate',0):.0%}\")
print(f\"Lift:     {d.get('lift',0):+.0%}\")
"
```

No `pip install`, no `git clone` of a second repository, no Python MCP server — the
harness is a subcommand of the same `iris-agentic-dev` binary already built.

## Verifying the durable telemetry record (US2)

```bash
# After the benchmark run above, restart is simulated by querying with an explicit
# session_id (or omit it to see the most recent session):
./target/release/iris-agentic-dev mcp --stdio &
# via any MCP client, call telemetry_query with {"tool_name": "iris_compile"}
# — records from the just-completed benchmark run are present, not just the live 5000-
# entry in-memory buffer, because the durable IRIS-global sink persisted them.
```

## Exporting trace records for 058-iris-graph (US3)

```bash
# via MCP client, call telemetry_export_trace with {}
# → {"traces": [{"from": "iris-agentic-dev", "to": "iris_compile", "via": "mcp", "count": 22, "ts": "..."}]}
```

## Independent test verification (maps to spec.md's Independent Test sections)

1. **US1**: `results.json` contains `pass_rate`, `baseline_pass_rate`, `lift` per
   `data-model.md`'s `BenchmarkResult` shape — verified by the JSON parse above.
2. **US2**: run >50 tool calls in one session (e.g. a benchmark run with 22 tasks × several
   calls each easily exceeds 50), restart the MCP server process, `telemetry_query` for the
   prior `session_id` — all calls present, not truncated to 50.
3. **US3**: run a session with repeated identical tool calls, `telemetry_export_trace` —
   verify one record per distinct `(from, to, via)` with `count` reflecting total
   occurrences, not one record per call.
