# Coverage lift: 085 write-gate integrity

Baseline (T003) and post-implementation (T073) coverage for `iris-agentic-dev-core`, measured with
the same command against the same live container so the two numbers are comparable.

## Baseline — T003

Taken from a detached worktree at `c54ae58` (`fix: synthesized_skills() emits invalid JSON when
^SKILLS has entries (#119)`), the commit this feature branched from, so none of the 085 tests
inflate the starting point.

| Metric    | Total       | Missed | Coverage   |
| --------- | ----------- | ------ | ---------- |
| Lines     | 29288       | 3622   | **87.63%** |
| Regions   | 48942       | 6622   | 86.47%     |
| Functions | 3212        | 411    | 87.20%     |
| Tests     | 4322 passed | 0      | 0 ignored  |

Command (as run):

```bash
IRIS_HOST=localhost IRIS_WEB_PORT=52780 \
LLVM_COV=~/.rustup/toolchains/stable-aarch64-apple-darwin/lib/rustlib/aarch64-apple-darwin/bin/llvm-cov \
LLVM_PROFDATA=~/.rustup/toolchains/stable-aarch64-apple-darwin/lib/rustlib/aarch64-apple-darwin/bin/llvm-profdata \
cargo llvm-cov --summary-only -p iris-agentic-dev-core --features testing \
  -- --include-ignored --test-threads=1
```

Two deviations from the constitution's canonical command, both deliberate:

- `IRIS_WEB_PORT`, not `IRIS_PORT`. No code reads `IRIS_PORT`; with the wrong name,
  `discovery_tests::discover_iris_returns_none_when_nothing_found` fails on any machine with a
  running container, because its skip guard tests `IRIS_WEB_PORT`. Recorded in plan.md Complexity
  Tracking.
- `--features testing` and `--test-threads=1`. The former is required by the 085 test targets; the
  latter by the project's IRIS test-parallelism rule.

The e2e targets spawn `iris-agentic-dev` as a subprocess and resolve it from the workspace root, so
a fresh worktree needs `cargo build -p iris-agentic-dev --bin iris-agentic-dev` first — an
uninstrumented build at `target/debug/iris-agentic-dev`, which is the same candidate the main
worktree resolves. Without it, 11 targets abort with `spawn iris-dev mcp: NotFound` and the run
exits 101.

## Advertised tool list — T068

The 085 change that touches agent behaviour is T023: a write-capable tool stays in `tools/list`
when writes are off and refuses at call time, instead of vanishing from the catalog. Measured by
handshaking both binaries over stdio and diffing `tools/list`, `IRIS_TOOLSET=merged`:

| Build                       | `IRIS_WRITE_TOOLS_ENABLED=false` | `=true` | Hidden when writes off                           |
| --------------------------- | -------------------------------- | ------- | ------------------------------------------------ |
| v1.2.6 (clean clone of tag) | 76 tools                         | 78      | `iris_credential_manage`, `iris_production_item` |
| this branch                 | 78 tools                         | 78      | none                                             |

With the gate off, both formerly-hidden tools now answer:

```json
{"error":"iris_production_item is write-capable and write tools are disabled (source: operator_env).
Set write_tools_enabled = true in .iris-agentic-dev.toml to allow writes.","error_code":"WRITE_TOOLS_DISABLED"}
```

Two tools moving from absent to visible-but-refusing is the whole delta in what an agent sees. It
is strictly more information than before — the agent learns the tool exists and why it was refused,
rather than inferring absence — so no task can lose a capability it previously had.

## GEPA task scores — T068

Run on 2026-08-25 against the live `iris-dev-iris` container (localhost:52780), 10 tasks × both
paths × 2 builds, `--toolset merged`. There is no stored pre-085 GEN/MOD baseline to compare
against — the archived merged runs are DOC-only at v0.9.1 — so the reference arm was run rather
than cited. Both arms report `server_version 1.2.6`, so the binary identity is recorded as a path:

| Arm     | `iris_dev_path`                                                  | mean A | mean B |
| ------- | ---------------------------------------------------------------- | ------ | ------ |
| pre-085 | `/tmp/iad-clean085/target/debug/iris-agentic-dev` (317ea11)      | 2.1    | 2.7    |
| branch  | `/Users/tdyar/ws/iris-agentic-dev/target/debug/iris-agentic-dev` | 2.0    | 2.6    |

The harness spawns `iris-dev` from `PATH`, not from the workspace target dir, so each arm ran behind
a shell shim. Without that, both arms would have measured the `/Users/tdyar/.local/bin/iris-dev`
that happens to be installed (v0.9.10) and neither result would mean anything.

Per task, pre-085 → branch:

| Task   | A       | B       |
| ------ | ------- | ------- |
| GEN-01 | 2 → 2   | 3 → 3   |
| GEN-02 | 3 → 1 ▼ | 1 → 1   |
| GEN-03 | 2 → 2   | 3 → 3   |
| GEN-04 | 2 → 3 ▲ | 3 → 3   |
| GEN-05 | 2 → 2   | 3 → 3   |
| MOD-01 | 2 → 2   | 3 → 3   |
| MOD-02 | 2 → 2   | 3 → 3   |
| MOD-03 | 2 → 2   | 3 → 3   |
| MOD-04 | 2 → 2   | 2 → 1 ▼ |
| MOD-05 | 2 → 2   | 3 → 3   |

Two tasks scored lower on the branch. Neither is the gate:

- **GEN-02 path A (3 → 1).** `gate_refusal_count` is 0 on both arms — no tool was refused, so no
  gate was consulted. The judge marked the branch run down for not checking that `Bench.Validator`
  exists before writing the method that calls it (8 tool calls vs 4). Three more samples of that
  one task: branch 1, 2, 1 / pre-085 3, 1, 2. The ranges are the same, and the pre-085 arm scored a
  1 on its own. This task is unstable, not regressed.
- **MOD-04 path B (2 → 1).** Both arms produced the identical wrong output, `patients=,orders=`
  instead of `patients=0,orders=0`, and the judge scored the same artifact 2 on one arm and 1 on the
  other. Two more samples each: branch 2, 2 / pre-085 2, 2.

`gate_refusal_count` and the full 20 transcripts are persisted under
`benchmark/021/results/<run>/transcripts/` — added for this run, because a score alone cannot show
whether a refusal happened. Totals: 2 refusals across 20 branch tasks, 0 across 20 pre-085 tasks,
both in GEN-01 path A:

```json
{"error":"iris_namespace_create is a destructive tool and the destructive tier is disabled
(source: inferred_default). Set destructive_tools_enabled = true in .iris-agentic-dev.toml to allow
it.","error_code":"DESTRUCTIVE_TOOLS_DISABLED"}
```

The second was `iris_admin`. Both are the new destructive tier doing its job: the harness declares
no gate, so writes are inferred on from the namespace and destructive stays off. GEN-01 still scored
2, same as pre-085 — the agent used another route. This is the one behaviour change a task can see,
and it cost nothing on this set.

No task lost a capability. `iris_production_item` and `iris_credential_manage` are visible and
refusing with writes off rather than absent from `tools/list`, which is strictly more information
than the agent had before.

Caveat worth recording: the harness's `reset_benchmark_namespace` fails on this IRIS build
(`<METHOD DOES NOT EXIST> Delete,%SYS.Namespace`) and falls back to `ensure`, so BENCHMARK carries
state across runs. That is pre-existing and affects both arms equally.

## Post-implementation — T073

| Metric    | Total       | Missed | Coverage   | vs baseline |
| --------- | ----------- | ------ | ---------- | ----------- |
| Lines     | 29565       | 3618   | **87.76%** | +0.13       |
| Regions   | 49251       | 6656   | 86.49%     | +0.02       |
| Functions | 3246        | 413    | 87.28%     | +0.08       |
| Tests     | 4388 passed | 0      | 0 ignored  | +66 tests   |

121 test targets, 0 failures. `write_gate.rs`, the file the feature adds, on its own:

| Metric    | Total | Missed | Coverage |
| --------- | ----- | ------ | -------- |
| Lines     | 256   | 5      | 98.05%   |
| Regions   | 265   | 12     | 95.47%   |
| Functions | 30    | 1      | 96.67%   |

Command (as run):

```bash
IRIS_HOST=localhost IRIS_WEB_PORT=52780 \
LLVM_COV=~/.rustup/toolchains/stable-aarch64-apple-darwin/lib/rustlib/aarch64-apple-darwin/bin/llvm-cov \
LLVM_PROFDATA=~/.rustup/toolchains/stable-aarch64-apple-darwin/lib/rustlib/aarch64-apple-darwin/bin/llvm-profdata \
cargo llvm-cov -p iris-agentic-dev-core --features testing \
  -- --include-ignored --test-threads=1 \
  --skip install_manifest_covers_every_skill_on_disk \
  --skip registry_manifest_covers_every_skill_on_disk
```

Same two deviations as the baseline (`IRIS_WEB_PORT`, and `--features testing` with
`--test-threads=1`), plus one more that is not about 085: the working tree carries five bundled
skill directories (`iris-ai-hub`, `iris-global-archaeology`, `iris-interop-debug`, `iris-rest-api`,
`iris-sql-tuning`) that `bundled.rs` embeds but `iris-agentic-dev.toml` and `skills.sh.json` do not
list, so `test_skill_manifest_sync` fails two assertions and takes the whole run's report with it.
Skipping those two assertions is the only way to get a number today; the manifest gap is separate
work and is not caused by this feature.

### What the run exposed

Two things worth recording, because both were invisible while the numbers looked fine.

**The measured binary was stale.** `iris_dev_bin()` prefers
`target/llvm-cov-target/debug/iris-agentic-dev` over `target/debug/iris-agentic-dev`, and every
spawn-based e2e test resolves through it. That copy was left over from an earlier point in this
branch, so the e2e tests had been handshaking a binary without the destructive tier. Running
`cargo llvm-cov clean --workspace` removed it, the tests fell through to the current build, and
`interop_e2e_tests::test_lookup_crud` immediately failed:

```json
{"error":"iris_lookup_manage is a destructive tool and the destructive tier is disabled
(source: inferred_default). Set destructive_tools_enabled = true in .iris-agentic-dev.toml to allow
it.","error_code":"DESTRUCTIVE_TOOLS_DISABLED","success":false}
```

That is the gate working. `iris_lookup_manage` is one of the seven ☠ tools (docs/tools.md:1461) and
`set` is a destructive action, so a harness that writes lookup entries has to declare the tier. The
harness now sets `IRIS_WRITE_TOOLS_ENABLED` and `IRIS_DESTRUCTIVE_TOOLS_ENABLED` on the child, the
same way an operator would. It was the only casualty: a full `--no-fail-fast` pass over all 121
targets afterwards was clean.

**A 46% reading was a stale-profile artifact, not a regression.** Before the clean, the same command
reported 46.42% lines and 58.22% for `write_gate.rs`. Per-file line counts were roughly doubled
across files 085 never touched (`tools/mod.rs` 7090 → 23310, `iris/connection.rs` 1040 → 2070), which
is duplicated per-instantiation records from older builds in the merged profile inflating totals and
misses together. `cargo llvm-cov clean --workspace` alone moved the number to 87.63% with no source
change. Worth knowing: this profile directory silently accumulates, so a coverage figure taken
without a preceding clean can be off by half.

### `write_gate.rs` from 81% to 98%

The first clean measurement put the new file at 81.22% lines, below the 90% this task requires. 35 of
the 40 uncovered lines were the four `const fn` row constructors (`ro`/`wr`/`de`/`mixed`): private to
the module, called only from the `CLASSIFICATION` initialiser, evaluated at compile time, so no
integration test can reach them and no runtime path executes them. `GateSource::as_str` was missing
four of its seven arms and `init_operator_env_gates` was never called.

Covered by a `#[cfg(test)] mod tests` inside `write_gate.rs` — the only place with access — asserting
what each constructor builds, all seven wire values, and that the operator snapshot is captured at
most once. The four still-uncovered lines are the `if first` branch of that last test, which only
runs if it wins the race to seed the snapshot; the assertion that a second seed is refused runs
either way.
