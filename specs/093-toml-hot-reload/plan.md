# Implementation Plan: TOML Pool Hot-Reload

**Branch**: `093-toml-hot-reload` | **Date**: 2026-09-02 | **Spec**: [spec.md](spec.md)

## Summary

Add `iris_reload_pool` tool that atomically swaps `Arc<ConnectionPool>` by calling the
existing `load_pool(config_file)` function, and extend `check_reload` (called on every
tool invocation) to also rebuild the pool when the config file mtime changes. The new
tool uses the same Arc-swap pattern established in spec 034. Fail-safe: on parse error,
existing pool is preserved and the error is returned.

## Technical Context

**Language/Version**: Rust 2021
**Primary Dependencies**: existing `connection_pool::load_pool`, `Arc<ConnectionPool>`, `ConfigWatcher` — no new crates
**Storage**: `~/.iris-agentic-dev.toml` and `~/.config/iris-agentic-dev/servers.json` (read-only)
**Testing**: `cargo test`, `cargo llvm-cov --include-ignored` for coverage
**Target Platform**: Linux + macOS
**Project Type**: Single Rust workspace (two crates)
**Performance Goals**: N/A — on-demand reload, not hot path
**Constraints**: Atomic swap — no tool call sees a partially-built pool; fail-safe on bad toml
**Scale/Scope**: ~50 lines new code in mod.rs; extend check_reload (~10 lines); new test file

## Constitution Check

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Zero-Install Binary | PASS | No new install step |
| II. ObjectScript Sanity | N/A | No ObjectScript — pure Rust config reload |
| III. HTTP-First Execution | PASS | New tool reads config only; no Docker required |
| IV. Test-First, Fixture-Driven | PASS | Three test layers defined in spec; tests before impl |
| V. Output Shape Parity | PASS | New tool; response shape defined in spec |
| VI. Environment Guard | PASS | `iris_reload_pool` classified read-only in write gate |
| VII. Dependency Minimalism | PASS | No new crates |
| VIII. 90% Coverage Gate | PASS | Polish phase includes coverage check |
| IX. Tool Lift Requirement | PASS | New tool — benchmark required before merge (Principle IX) |
| X. ObjectScript Coverage | N/A | Pure Rust feature |

## Project Structure

### Documentation (this feature)

```text
specs/093-toml-hot-reload/
├── plan.md              ← this file
├── research.md          ← Phase 0 output (below)
├── data-model.md        ← Phase 1 output (below)
├── contracts/           ← Phase 1 output (below)
└── tasks.md             ← Phase 2 output (/speckit.tasks)
```

### Source Code (files changed)

```text
crates/iris-agentic-dev-core/src/tools/
├── mod.rs               # iris_reload_pool tool impl + extend check_reload
└── write_gate.rs        # add iris_reload_pool as ReadOnly tool

crates/iris-agentic-dev-core/src/iris/
└── connection_pool.rs   # load_pool already exists; no change needed

crates/iris-agentic-dev-core/tests/integration/
└── test_server_pool_e2e.rs     # binary invocation layer-2 test (e2e_server_add_remove)
```

---

## Phase 0: Research

### R-001: Arc<ConnectionPool> and pool field (verified 2026-09-02)

File: `crates/iris-agentic-dev-core/src/tools/mod.rs`
Line 2274: `pub pool: Arc<crate::iris::connection_pool::ConnectionPool>`

Arc swap for atomic reload:
```rust
// In iris_reload_pool handler:
let new_pool = connection_pool::load_pool(config_path);
let new_arc = Arc::new(new_pool);
// Replace via unsafe Arc::ptr_eq swap? No — need RwLock or ArcSwap.
// Actual pattern: IrisTools wraps pool in Arc<RwLock<ConnectionPool>> or uses ArcSwap.
// NEEDS CLARIFICATION: check how 034 hot-reload did it.
```

**Action**: Verify the 034 hot-reload pattern in git or existing code to determine correct swap mechanism.

### R-002: check_reload hook (verified 2026-09-02)

File: `crates/iris-agentic-dev-core/src/tools/mod.rs:2918`
Called at: lines 2708, 4617, 4716, 4820, 8500

`check_reload` is `async fn check_reload(&self)` on `IrisTools`. It reads mtime via `ConfigWatcher::has_changed()`. Currently it only reloads the IRIS connection — does NOT reload the pool.

Extension: after `has_changed()` returns true and connection is reloaded, also call `load_pool` and swap the pool Arc.

### R-003: load_pool function (verified 2026-09-02)

File: `crates/iris-agentic-dev-core/src/iris/connection_pool.rs:191`
Signature: `pub fn load_pool(config_file: Option<&std::path::Path>) -> ConnectionPool`
Returns a fresh `ConnectionPool` by loading from toml + servers.json. No async, no panics on missing file (returns empty pool).
Reuse as-is: `Arc::new(load_pool(config_path.as_deref()))`.

### R-004: Pool swap mechanism (NEEDS CLARIFICATION)

The current `IrisTools.pool` field is `Arc<ConnectionPool>` — a single Arc, not `Arc<RwLock<...>>` or `arc_swap::ArcSwap`. To do an atomic swap from `&self` (immutable), the field must be wrapped in `Arc<ArcSwap<ConnectionPool>>` or `Arc<RwLock<ConnectionPool>>`.

**Check**: Does `IrisTools.pool` already use interior mutability? If not, the field type must change to enable hot-reload.

**Resolution path**: Use `std::sync::RwLock<Arc<ConnectionPool>>` — no new crate. Nested Arc pattern:
- `pool: Arc<RwLock<Arc<ConnectionPool>>>`
- Read: `self.pool.read().unwrap().clone()` → `Arc<ConnectionPool>`
- Swap: `*self.pool.write().unwrap() = Arc::new(new_pool)`

### R-005: write_gate.rs for iris_reload_pool (verified 2026-09-02)

File: `crates/iris-agentic-dev-core/src/tools/write_gate.rs`
New tool must be registered as `ReadOnly` (it reads config, does not write IRIS state).
Add: `ro("iris_reload_pool")` in the classification table.

---

## Phase 1: Design

### data-model.md

**iris_reload_pool response**:
```json
{
  "success": true,
  "servers_loaded": 2,
  "servers": ["dev-iris", "prod-iris"],
  "note": "To see new servers in the model's tool list, restart Claude Desktop (or re-run initialize)."
}
```

On parse error (fail-safe):
```json
{
  "success": false,
  "error": "TOML parse error at line 12: ...",
  "note": "Existing pool preserved — no servers removed."
}
```

**Pool swap state**:
- Field: `pool: Arc<RwLock<Arc<ConnectionPool>>>`
- Read path: `self.pool.read().unwrap().clone()` — gets current `Arc<ConnectionPool>`
- Swap path: `*self.pool.write().unwrap() = Arc::new(new_pool)`
- Background watcher in `check_reload`: same swap, no response needed (logged)

### contracts/

See `contracts/iris_reload_pool.md`.
