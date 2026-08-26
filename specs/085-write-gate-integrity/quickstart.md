# Quickstart: verifying write-gate integrity

**Feature**: 085-write-gate-integrity | **Date**: 2026-08-25

Every check below is a live reproduction, not a unit test. Each maps to a success criterion and
each is currently **failing** on 1.2.6. The point of writing them down is that the reporter can run
them (SC-008).

## Prerequisites

```bash
docker ps --filter name=iris-dev-iris        # must be running, web port 52780
cargo build --locked
export IAD=./target/debug/iris-agentic-dev
```

Do **not** export `IRIS_WRITE_TOOLS_ENABLED` in your shell — that is a legitimate operator
override and it will mask the config-file paths below. Check first: `env | grep IRIS_`.

Sections 1–4 are also scripted, so the reporter can run them instead of walking them (SC-008):

```bash
cargo test --test test_reporter_repro -- --include-ignored --test-threads=1
```

Three tests, one per published reproduction: stale reporting after a config edit, a write that
lands with the gate declared off, and the contradictory config starting with writes enabled. Each
asserts what was measured — whether the global exists, what the process exit code was — not the
returned error code.

## 1. Declared gate blocks every write (SC-001)

```bash
cd $(mktemp -d)
cat > .iris-agentic-dev.toml <<'TOML'
host = "localhost"
port = 52780
namespace = "USER"
username = "_SYSTEM"
password = "SYS"
write_tools_enabled = false
TOML
```

Start the server against this workspace, then in one session:

| Call                                                        | Expect                                                            |
| ----------------------------------------------------------- | ----------------------------------------------------------------- |
| `check_config`                                              | `write_tools_enabled: false`, `write_tools_source: "config_file"` |
| `iris_ws_open` then `iris_ws_exec` with `set ^IADGate085=1` | `WRITE_TOOLS_DISABLED`                                            |
| `iris_global` set `^IADGate085`                             | `WRITE_TOOLS_DISABLED`                                            |
| `iris_lookup_manage` set                                    | `WRITE_TOOLS_DISABLED`                                            |
| `iris_execute_method`                                       | `WRITE_TOOLS_DISABLED`                                            |
| `iris_doc` mode=put                                         | `WRITE_TOOLS_DISABLED` (already passes today)                     |
| `iris_query` mode=read                                      | succeeds — read-only tools are never gated                        |

Then the part that matters, and the part no existing test does:

```text
iris_global(mode="get", global="^IADGate085")   →  must report the global does not exist
```

An error code coming back is not the assertion. **The absence of the global is the assertion**
(FR-025). On 1.2.6 the first four rows above return success and the global exists.

## 2. Config edit changes the gate both ways, in one process (SC-002)

Same directory, same server process — do not restart.

```bash
sed -i '' 's/write_tools_enabled = false/write_tools_enabled = true/' .iris-agentic-dev.toml
```

Call any tool to trigger the reload, then `check_config`. Expect `true`, and expect a write to
succeed. Now edit it back to `false`, and expect `false` and a refusal.

On 1.2.6 the second edit reports `true` forever while `config_loaded_at` advances. That is the
whole of the reporter's current complaint, and it is why FR-023 requires the rewrite-twice test.

## 3. Reporting matches enforcement (SC-003)

For each of four configurations, compare what `check_config` says against what a write actually
does:

| Configuration                          | Expect reported                     | Expect a write to |
| -------------------------------------- | ----------------------------------- | ----------------- |
| `write_tools_enabled = false`          | `false`                             | refuse            |
| `write_tools_enabled = true`           | `true`                              | succeed           |
| no config file at all (USER namespace) | `true`, source `inferred_namespace` | succeed           |
| edited in place, `true` → `false`      | `false`                             | refuse            |

Row 4 is the one that fails today. Row 3 documents unchanged behavior — FR-019 keeps the inference
and only makes it legible.

## 4. Contradictory config fails closed (SC-004)

```bash
cat > .iris-agentic-dev.toml <<'TOML'
host = "localhost"
port = 52780
namespace = "USER"
write_tools_enabled = false
destructive_tools_enabled = true
TOML

$IAD mcp --workspace .
echo "exit=$?"
```

Expect `exit=2` and `DESTRUCTIVE_REQUIRES_WRITES` on stderr. On 1.2.6 this exits 0, serves
requests, and `check_config` reports `write_tools_enabled: true` — the exact inverse of the
declaration.

## 5. Destructive tier needs its own key (SC-009)

```toml
write_tools_enabled = true
# destructive_tools_enabled not declared
```

| Call                        | Expect                                                        |
| --------------------------- | ------------------------------------------------------------- |
| `iris_doc` mode=put         | succeeds — ordinary writes are on                             |
| `global_kill`               | `DESTRUCTIVE_TOOLS_DISABLED`, **and the global still exists** |
| `iris_lookup_manage` delete | `DESTRUCTIVE_TOOLS_DISABLED`, entry still present             |
| `iris_lookup_manage` get    | succeeds — read actions are not gated                         |
| `iris_remove_server`        | refused, **and the saved server is still listed**             |
| `skill(action="forget")`    | refused, and the skill is still installed                     |
| `check_config`              | `destructive_tools_enabled: false` with a source              |

Add `destructive_tools_enabled = true` and the same calls proceed. On 1.2.6 all seven proceed
regardless, because the key has no reader.

Three things about that table are easy to misread as defects:

- `iris_lookup_manage` `set` is in the destructive tier too, not just `delete` — spec 073 puts
  "write/delete" in the tier, and `set` overwrites an existing key. So creating the entry this
  section deletes needs the tier on; walk the rows in the order set-with-tier-on, tier off,
  delete-refused, get-still-present.
- `global_kill` with the tier on still answers `CONFIRM_REQUIRED` until `global_preview` hands out
  a `confirm_token`. The gate stops being the reason for the refusal; the confirm protocol is a
  separate control and this section is not testing it.
- `iris_remove_server` needs a server that is actually saved. Use one from `iris_servers`, and
  assert it is still in that list afterwards — that is the negative side effect for local state.

## 6. A new write tool cannot ship ungated (SC-006)

Three sabotages, each reverted afterwards. The messages below are the actual output, not a
paraphrase — the point of recording them is that the failure has to _name the thing to fix_.

### 6a. A router tool with no `CLASSIFICATION` entry

Add two throwaway `#[tool]` methods to `IrisTools` (`iad085_demo_write`, and `iad085_demo_ro`
carrying `annotations(destructive_hint = true)`) and classify neither:

```bash
cargo test --features testing --test test_gate_classification -- --test-threads=1
```

```text
2 tool(s) in the baseline toolset have no write_gate::CLASSIFICATION entry:
["iad085_demo_ro", "iad085_demo_write"]. Add each one to CLASSIFICATION in
crates/iris-agentic-dev-core/src/tools/write_gate.rs — ro() if it only reads, wr() if it can
mutate anything, de() for the destructive tier, mixed() if that depends on the action. Until
then gate_check fails them closed as Write.
```

### 6b. `ReadOnly` while advertising `destructive_hint = true`

Keep the demo tools, add `ro("iad085_demo_ro")` to `CLASSIFICATION`, and the forward-completeness
test goes green while the cross-check does not:

```text
the router's annotations and write_gate::CLASSIFICATION disagree on 1 tool(s). Fix whichever one
is wrong — do not derive one from the other, the point is that a mislabelled tool has to be
mislabelled twice:
  iad085_demo_ro: annotations say destructiveHint = true but CLASSIFICATION says Some(ReadOnly)
  — the destructive tier does not gate it
```

Two independent declarations is the whole design: one edit gets caught, and it takes two to lie.

### 6c. A gate that stops firing

Revert both demo tools, then early-return `None` from `gate_check` for `iris_ws_exec` — the
reporter's most severe finding, since a session plus `iris_ws_exec` runs arbitrary ObjectScript:

```bash
cargo test --test test_mcp_binary_config every_write_capable -- --include-ignored --test-threads=1
```

```text
2 write-capable call(s) were not refused by the write gate. Each one is reachable with writes off:
  [merged] iris_ws_exec {} is classified Write but answered {...}
  [baseline] iris_ws_exec {} is classified Write but answered {...}
```

Both tiers report it, because the table drives the test in each. Revert the early return.

## 7. Documented controls are real (SC-005)

```bash
cargo test docs_contract
```

Expect green after this feature. Before it, expect eight failures: four write-gate identifiers
(`DESTRUCTIVE_TOOLS_DISABLED`, `WRITE_SERVER_NOT_ALLOWED`, `write_allowed_servers`,
`IRIS_WRITE_ALLOWED_SERVERS`), and four inherited from spec 072 (`WS_SESSION_NOT_FOUND`,
`WS_TERMINAL_NOT_SUPPORTED`, `IRIS_WS_TIMEOUT_SECS`, `max_chars`), plus the stale
`read_only_hint` count.

## 8. Honest version string (SC-007)

Use a fresh clone, not your working tree — a dirty tree is exactly what produces the suffix, so
testing in one proves nothing.

```bash
git clone --depth 1 --branch v1.2.7 <repo-url> /tmp/iad-clean && cd /tmp/iad-clean
cargo metadata --locked --format-version 1 >/dev/null && echo "lock in sync"
cargo build --locked --release
./target/release/iris-agentic-dev --version    # no "+...-dirty" suffix
```

The lockfile check must pass **before** the build, because cargo silently reconciles the lockfile
during resolution and `build.rs` runs after that — which is how every 1.2.x release shipped
advertising `1.2.6+v1.2.6-dirty`.

Before the tag exists there is still something to run: clone the branch, copy in the manifests and
lockfile under test, commit so the tree is clean, and point a tag at that commit. `cargo metadata
--locked` then proves the lockfile, and the built binary's version string proves the rest — plain
`1.2.6` from the clean clone against `1.2.6+v1.2.6-dirty` from the working tree, same commit.

## Full suite

```bash
cargo test                                                     # unit, no IRIS
cargo test --features testing -- --include-ignored --test-threads=1   # live IRIS
cargo llvm-cov --features testing -- --include-ignored --test-threads=1
```

`--test-threads=1` is not optional. The gate-resolution tests are pure and thread-safe by design,
but the binary-invocation and live-IRIS tests share process environment and one container.

## Cleanup

```text
iris_global(mode="kill", global="^IADGate085")   # with the gate on
```

Probe globals use the `^IADGate085` prefix so a stray one is identifiable and safe to remove.
