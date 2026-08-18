# 079 — Upgrade rmcp from 1.4 to 3.1.3

## Status

Implementation complete. All 1108 unit tests pass; clippy and fmt are clean.

## API Changes: 1.x → 3.x

### 1. `Content` renamed to `ContentBlock`

In 1.x, `model::Content` was a type alias for `Annotated<RawContent>`. In 3.x the
top-level content type is `ContentBlock` (a plain enum). The constructor
`Content::text(s)` becomes `ContentBlock::text(s)`.

### 2. `.raw` field removed from content blocks

In 1.x, content items had a `.raw` field holding the inner `RawContent` enum, so
`as_text()` was called via `content.raw.as_text()`. In 3.x, `as_text()` lives
directly on `ContentBlock`.

Old: `content.first().and_then(|c| c.raw.as_text())`
New: `content.first().and_then(|c| c.as_text())`

The returned `TextContent` still has a `.text: String` field, so no further
changes to field access after `as_text()`.

### 3. `schema_for_output` is now infallible

Old: `schema_for_output::<T>() -> Result<Arc<JsonObject>, String>` (callers called `.unwrap()`)
New: `schema_for_output::<T>() -> Arc<JsonObject>` — returns the schema directly

### 4. `call_tool` return type changed to `CallToolResponse`

Old: `async fn call_tool(...) -> Result<CallToolResult, McpError>`
New: `async fn call_tool(...) -> Result<CallToolResponse, McpError>`

`CallToolResponse` is an enum (`Complete(CallToolResult)`, `InputRequired(...)`,
`Task(...)`). `CallToolResult` implements `Into<CallToolResponse>`. The internal
`tool_router.call()` already returns `CallToolResponse` so only the declared return
type on the `ServerHandler` impl needs updating.

### 5. `ListToolsResult` struct gained new fields

The `paginated_result!` macro now adds `result_type: Option<ResultType>`,
`ttl_ms: Option<u64>`, and `cache_scope: Option<CacheScope>`. All are `Option` and
default to `None`. The struct literal was replaced with
`ListToolsResult::with_all_items(tools)` + setting `next_cursor` explicitly.

### 6. Feature flags — unchanged

`server`, `macros`, `schemars`, `transport-io`, `transport-streamable-http-server`
all exist in 3.1.3 with identical semantics.

## Files Changed

| File                                    | What changed                                                                                                                               |
| --------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| `Cargo.toml` (workspace root)           | `"1.4"` → `"3.1.3"`                                                                                                                        |
| `src/tools/admin.rs`                    | `Content::text` → `ContentBlock::text`                                                                                                     |
| `src/tools/admin_tools.rs`              | `Content::text` → `ContentBlock::text`; `.raw.as_text()` → `.as_text()` (test section)                                                     |
| `src/tools/comparison_tools.rs`         | `Content::text` → `ContentBlock::text`                                                                                                     |
| `src/tools/dict.rs`                     | `rmcp::model::Content::text` → `rmcp::model::ContentBlock::text`                                                                           |
| `src/tools/doc.rs`                      | Same `Content` → `ContentBlock`; `.raw.as_text()` → `.as_text()` (4 src + 14 test lines)                                                   |
| `src/tools/info.rs`                     | `rmcp::model::Content::text` → `rmcp::model::ContentBlock::text`                                                                           |
| `src/tools/interop.rs`                  | `Content::text` → `ContentBlock::text`; `.raw.as_text()` → `.as_text()`                                                                    |
| `src/tools/mod.rs`                      | `Content::text` × 2; `.raw.as_text()` × 1; `schema_for_output().unwrap()` × 18 removed; `call_tool` return type; `ListToolsResult` literal |
| `src/tools/observability.rs`            | `Content::text` → `ContentBlock::text`                                                                                                     |
| `src/tools/scm.rs`                      | `rmcp::model::Content::text` → `rmcp::model::ContentBlock::text`                                                                           |
| `src/tools/search.rs`                   | `rmcp::model::Content::text` → `rmcp::model::ContentBlock::text`                                                                           |
| `src/tools/skills_tools.rs`             | `rmcp::model::Content::text` → `rmcp::model::ContentBlock::text`; `.raw.as_text()` → `.as_text()`                                          |
| `cmd/dispatch.rs` (bin)                 | `.raw.as_text()` → `.as_text()`                                                                                                            |
| `cmd/tool.rs` (bin)                     | `.raw.as_text()` → `.as_text()`                                                                                                            |
| `tests/` (all integration + unit files) | `.raw.as_text()` → `.as_text()` (bulk sed across 19 files)                                                                                 |

## Test Results

- `cargo check` — clean (0 errors)
- `cargo clippy -- -D warnings` — clean
- `cargo fmt --all -- --check` — clean
- `cargo test --lib` — **1108 passed, 0 failed**

## Tasks

- [x] T001: Research rmcp 1.x → 3.x API changes
- [x] T002: Update `Cargo.toml` version
- [x] T003: Fix `Content` → `ContentBlock` in all source files
- [x] T004: Fix `.raw.as_text()` → `.as_text()` across all files
- [x] T005: Fix `schema_for_output().unwrap()` → `schema_for_output()` (18 call sites)
- [x] T006: Fix `call_tool` return type `CallToolResult` → `CallToolResponse`
- [x] T007: Fix `ListToolsResult` struct literal → `with_all_items` constructor
- [x] T008: `cargo check` clean
- [x] T009: `cargo clippy -- -D warnings` clean
- [x] T010: `cargo fmt --all` clean
- [x] T011: `cargo test --lib` passes (1108/1108)
