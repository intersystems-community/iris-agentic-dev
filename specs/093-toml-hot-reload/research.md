# Research: 093-toml-hot-reload

## R-001: IrisTools.pool field type (verified 2026-09-02)

File: `crates/iris-agentic-dev-core/src/tools/mod.rs:2274`
`pub pool: Arc<crate::iris::connection_pool::ConnectionPool>`

Single `Arc<ConnectionPool>` — no interior mutability. Cannot be atomically swapped from `&self`.

**Decision**: Wrap in `Arc<RwLock<Arc<ConnectionPool>>>` — enables read/write from `&self` with no new crate.
- Read: `self.pool.read().unwrap().clone()` → `Arc<ConnectionPool>`
- Swap: `*self.pool.write().unwrap() = Arc::new(new_pool)`

**All existing uses of `self.pool`** must be updated to `self.pool.read().unwrap().clone()`.

## R-002: check_reload pattern (verified 2026-09-02)

File: `mod.rs:2918`
Currently: `check_reload` detects config mtime change, reloads `IrisConnection`.
Extension: when `has_changed()` is true, also call `load_pool(config_path)` and swap pool Arc.
Fail-safe: if `load_pool` panics (it shouldn't, but guard anyway), keep existing pool.

## R-003: load_pool signature (verified 2026-09-02)

File: `connection_pool.rs:191`
`pub fn load_pool(config_file: Option<&std::path::Path>) -> ConnectionPool`

Called at startup: `Arc::new(connection_pool::load_pool(None))` (line 2334).
Also called at: `Arc::new(connection_pool::load_pool(config_path.as_deref()))` (line 2682).

Reuse as-is for hot reload — no changes needed to this function.

## R-004: write_gate.rs iris_reload_pool classification

New tool: `iris_reload_pool` — reads config only, no IRIS mutations.
Must be classified `ReadOnly`. Add `ro("iris_reload_pool")` to the CLASSIFICATION table.

## R-005: Concurrent reload safety

Two concurrent `iris_reload_pool` calls: `RwLock` ensures only one writer at a time.
First write wins; second call sees the updated pool.

Tool call holding an `Arc<ConnectionPool>` from before the swap: continues normally
(Arc reference counting keeps old pool alive until all holders drop it).

## R-006: config_path reference in IrisTools

`IrisTools` stores `config_path` as `Option<std::path::PathBuf>` — needed for `load_pool(config_path.as_deref())`.
Verify field name before implementing: `grep -n "config_path" mod.rs`.
