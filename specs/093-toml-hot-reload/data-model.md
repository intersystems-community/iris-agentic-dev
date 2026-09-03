# Data Model: 093-toml-hot-reload

## Pool State (IrisTools field change)

```rust
// Before (single Arc — no interior mutability)
pub pool: Arc<ConnectionPool>

// After (RwLock wrapper enables swap from &self)
pub pool: Arc<RwLock<Arc<ConnectionPool>>>
```

All read callsites: `self.pool.read().unwrap().clone()` → `Arc<ConnectionPool>`
Swap callsite (iris_reload_pool + check_reload): `*self.pool.write().unwrap() = Arc::new(new_pool)`

## iris_reload_pool response

### Success

```json
{
  "success": true,
  "servers_loaded": 2,
  "servers": ["dev-iris", "prod-iris"],
  "note": "To see new servers in the model's tool list, restart Claude Desktop (or re-run initialize)."
}
```

### Parse error (fail-safe — existing pool preserved)

```json
{
  "success": false,
  "error_code": "TOML_PARSE_ERROR",
  "error": "TOML parse error at line 12: unexpected key 'x'",
  "note": "Existing pool preserved — no servers were removed."
}
```

### No config file found

```json
{
  "success": true,
  "servers_loaded": 0,
  "servers": [],
  "note": "No config file found. Pool is empty but valid. To see new servers in the model's tool list, restart Claude Desktop (or re-run initialize)."
}
```

## Error codes

| Code               | Condition                                                                                                                                        |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| `TOML_PARSE_ERROR` | Returned as `{ success: false, error_code: "TOML_PARSE_ERROR", error: "<toml error>" }` when `toml::from_str` fails; existing pool is preserved. |

## write_gate entry

```rust
ro("iris_reload_pool"),
```

Reads config; does not modify IRIS state → ReadOnly classification.
