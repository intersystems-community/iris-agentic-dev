# Quickstart: working on 113

How to run each test layer, and how to reproduce the measurements this plan rests on.

## Prerequisites

```bash
docker ps --filter name=iris-dev-iris        # must be running for the live layer
cargo build                                  # binary tests need target/debug/iris-agentic-dev
```

`iris-dev-iris` is the only container this feature may touch: TCP 11975, web 52780,
`iris-community:2026.2`, Atelier REST on 52780.

## The three layers

```bash
# Layer 1 — unit: schema generation, no binary, no IRIS
cargo test -p iris-agentic-dev-core --test unit

# Layer 2 — binary: spawn the server, assert on what tools/list actually emits
cargo build && cargo test --test '*' -- --test-threads=1 --include-ignored

# Layer 3 — live IRIS: per-parameter round-trips
cargo test --test '*' -- --test-threads=1 --include-ignored
```

`--test-threads=1` is mandatory for anything past layer 1: the binary and live tests set
environment variables, and parallel test binaries race on them.

Layer 2 tests locate the binary through `testing::iad_binary_path()`, which honours
`IAD_BINARY` (relative paths resolve against the workspace root, which is what CI passes) and
otherwise tries `target/debug` then `target/release`. Use `require_iad_binary()`, which
**panics** when the binary is absent rather than skipping — a binary test with no binary has
verified nothing. `IAD_ALLOW_SKIP=1` opts out deliberately.

Spawn the server with `testing::clean_mcp_command(&bin)`, never `Command::new` — it strips the
behaviour-affecting environment listed in `BEHAVIOR_ENV_VARS` so a developer's shell cannot make
a test pass or fail (Principle XII).

## Gates and coverage

```bash
python3 scripts/gates/antipatterns.py          # must report the new undeclared-params class
cargo clippy -- -D warnings
cargo fmt --all -- --check                     # CI enforces; run before every commit
cargo llvm-cov clean && ./scripts/coverage.sh  # clean first, or stale objects skew the number
./scripts/check-coverage-floors.sh
```

## Reproducing the measurements

Every number in `plan.md`, `research.md`, and `data-model.md` came from one of these two.

**The tool listing** — 81 tools, 37 with empty `properties`, 0 with
`additionalProperties: false`, `nextCursor: null`:

```bash
python3 - <<'PY'
import json, os, subprocess
env = dict(os.environ) | {
    "IRIS_HOST": "localhost", "IRIS_WEB_PORT": "52780",
    "IRIS_USERNAME": "_SYSTEM", "IRIS_PASSWORD": "SYS",
    "IRIS_NAMESPACE": "USER", "IRIS_CONTAINER": "iris-dev-iris",
}
msgs = [
    {"jsonrpc": "2.0", "id": 1, "method": "initialize",
     "params": {"protocolVersion": "2024-11-05", "capabilities": {},
                "clientInfo": {"name": "probe", "version": "0"}}},
    {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}},
]
p = subprocess.run(["./target/debug/iris-agentic-dev", "mcp"],
                   input="\n".join(json.dumps(m) for m in msgs) + "\n",
                   capture_output=True, text=True, env=env, timeout=120)
res = next(json.loads(l)["result"] for l in p.stdout.splitlines()
           if l.startswith("{") and json.loads(l).get("id") == 2)
tools = res["tools"]
print("tools", len(tools), "nextCursor", res.get("nextCursor"))
print("empty properties",
      sum(1 for t in tools if not t["inputSchema"].get("properties")))
print("additionalProperties:false",
      sum(1 for t in tools if t["inputSchema"].get("additionalProperties") is False))
PY
```

Swap the second message for a `tools/call` to reproduce the behaviour probes — the four that
matter are recorded in `research.md`'s baseline table, including the `namesapce` silent-discard
and the bare-text deserialization failure.

**The parameter inventory** — 31 tools, 132 slots, 71 distinct names. Pull every `p.get("…")`
out of each `Parameters<AnyParams>` handler body in `src/tools/mod.rs`; the per-tool result is
tabulated in `data-model.md`. Re-run it after each batch: a converted tool drops out of the
`Parameters<AnyParams>` scan, so the remaining count is the progress bar, and it must reach zero
before `AnyParams` is deleted.

## Writing the tests

Two hazards specific to this feature, both Principle XI:

1. **Looping over `properties` passes when the map is empty.** Assert non-emptiness before
   iterating, and assert the tool was actually found in the listing.
2. **An enum on an optional parameter is not at the property's top level** — `list_tools` runs
   `normalize_schema_openapi3`, which moves `enum` into `anyOf[0]`. A test reading
   `properties.mode.enum` finds nothing; written as "compare if present", it passes vacuously.
   Resolve through `anyOf` and assert a non-empty list came back. See
   `contracts/parameter-contract.md`.

## Order of work

Conversion first, enforcement last. A tool that advertises no properties would have _every_
argument rejected by the FR-015 validation site, so turning that site on before the last batch
lands breaks the tools it exists to protect. Batches 1–8, then `AnyParams` deletion, then the
validation site, then the guards.
