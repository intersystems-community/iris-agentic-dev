# 083 — iris_debug: drop DOCKER_REQUIRED, use execute_via_generator

## Problem

`iris_debug`'s three code-running actions (`map_int`, `capture`, `source_map`) call
`iris.execute()`, which requires a Docker connection and returns `DOCKER_REQUIRED` on
any HTTP-only connection (remote IRIS, native Windows IRIS, webgateway-fronted
instances). The error message tells users to set `IRIS_CONTAINER`, but these setups
have no container to set.

The identical ObjectScript runs fine through `execute_via_generator` on the same
HTTP-only connection — the production and skill tools already use that path since
commit `1226d7c`. The three `info.rs` call sites were left behind.

**Affected file:** `crates/iris-agentic-dev-core/src/tools/info.rs`, lines 229, 254, 273.

**Not affected:** `error_logs` action — it already skips execution entirely and returns
a static note.

## Fix

Three mechanical changes in `handle_iris_debug`:

1. Rename `_client: &reqwest::Client` → `client: &reqwest::Client` in the function
   signature (line 216).

2. Replace each `iris.execute(&code, ns).await` with
   `iris.execute_via_generator(&code, ns, client).await`.

3. Remove the three `Err(e) if e.to_string() == "DOCKER_REQUIRED"` match arms and
   their `err_result` bodies — they become unreachable on the HTTP path and the
   generic `Err(e) => err_json("EXECUTION_FAILED", ...)` arm handles any real failure.

**Result:** `map_int`, `capture`, and `source_map` work on any connection type that
supports `execute_via_generator` (HTTP Atelier REST). Docker connections continue to
work unchanged — `execute_via_generator` does not require Docker.

### Escaping note

The `map_int` and `source_map` code strings embed user input with `\"` escaping
(lines 227, 271). That escaping is correct for the ObjectScript string literal but may
be insufficient for pathological input (issue #92). This spec does not change the
escaping — #92 can be addressed independently.

## Tests

Per the three-layer policy (CLAUDE.md):

### Layer 1 — Unit (offline, no IRIS)

In a new test file `crates/iris-agentic-dev-core/tests/unit/test_iris_debug_actions.rs`:

- Parse a `DebugParams` from JSON for each of `map_int`, `capture`, `source_map` and
  verify the struct fields parse correctly (no serde silent-drop regression).
- Verify that `handle_iris_debug` is not called with `_client` — i.e. the function
  compiles with `client` used rather than ignored (compile-time check, no assertion
  needed beyond `cargo build`).

### Layer 2 — Binary invocation (offline, no IRIS)

In `crates/iris-agentic-dev-bin/tests/integration/test_mcp_binary_config.rs` or a
new `test_iris_debug_binary.rs`:

- Spawn `iris-agentic-dev mcp`, send `tools/list`, assert `iris_debug` is present.
- Spawn with no `IRIS_CONTAINER` set, call `iris_debug` with `action: "capture"`,
  assert the response does **not** contain `"DOCKER_REQUIRED"` (it will be
  `IRIS_UNREACHABLE` or a connection error, not a DOCKER_REQUIRED refusal).

This is the regression guard: if someone reintroduces a DOCKER_REQUIRED bail-out, this
test catches it without needing a live IRIS container.

### Layer 3 — Live IRIS integration (`#[ignore]`)

In `crates/iris-agentic-dev-core/tests/integration/test_e2e.rs` or a dedicated file:

- With `iris-dev-iris` running at localhost:52780 and **no** `IRIS_CONTAINER` set:
  - `iris_debug(action="capture")` → success, response contains `"error:"` and
    `"position:"` fields.
  - `iris_debug(action="map_int", error_string="<UNDEFINED>x+1^Unknown.Foo.1")` →
    success (may return empty string for unknown routine, but no DOCKER_REQUIRED).
  - `iris_debug(action="source_map", class_name="Unknown.DoesNotExist")` → success
    (may return empty mapping, but no DOCKER_REQUIRED).

## Success criteria

1. `cargo build` clean — no `_client` unused-variable warning.
2. `cargo test --test test_iris_debug_binary -- --include-ignored` passes (no
   DOCKER_REQUIRED on HTTP-only spawn).
3. `cargo test --test test_e2e -- iris_debug --include-ignored --test-threads=1`
   passes against `iris-dev-iris` with `IRIS_CONTAINER` unset.
4. Existing `iris_debug` tests (if any) still pass.
5. Docker-connected users see no behavior change.

## Out of scope

- `error_logs` action — already works on HTTP (returns static empty list).
- Input escaping improvements — tracked in #92.
- Expanding `iris_debug` with new actions.
