# Implementation Plan: `web_prefix` for the iad-native server registry

**Branch**: `116-servers-json-web-prefix` | **Date**: 2026-09-12 | **Spec**: [spec.md](./spec.md)
**Issue**: [#129](https://github.com/intersystems-community/iris-agentic-dev/issues/129)

**Written retrospectively.** The spec was written on 2026-09-07 (commit `74c3225`); this plan
records the approach that shipped in commit `ed8064c`.

## Summary

`ServerEntry` gains `web_prefix`, but adding the field is the small half. The reason the registry
was missing it is that prefix normalisation existed twice already — once inline in the VS Code
Server Manager branch of `load_pool`, once inline in `workspace_config::instance_base_url` — so
there was no single thing for a third source to reuse. The fix adds the field and reduces all
three sources to one helper.

## Technical Context

**Language/Version**: Rust 2021
**Primary Dependencies**: none added
**Storage**: `~/.config/iris-agentic-dev/servers.json` (schema unchanged in version number; the
new key is optional and skipped when absent)
**Testing**: `cargo test --features testing`; live container `iris-dev-iris` (localhost:52780)
**Project Type**: single workspace, two crates
**Constraints**: a registry file written by an older iad must load unchanged, and an older iad
must tolerate a file containing the new key (unknown keys already ignored)
**Scale/Scope**: 6 source files, 3 test files

## Root cause

Four sources feed `load_pool`. Two already carried a prefix — Server Manager profiles
(`webServer.pathPrefix`) and `[instance.*]` TOML (`web_prefix`) — and each normalised it with its
own copy of the same `trim_matches('/')`. `ServerEntry` had no field, and the native branch built

```rust
let base_url = format!("{}://{}:{}", scheme, entry.host, entry.port);
```

so `iris_add_server` could not describe a gateway-published instance at all. Duplicated assembly
is the actual defect; the missing field is its symptom.

Two further places rebuilt a URL rather than using the connection's:

- `probe_server(host, web_port, ...)` composed `http://{host}:{web_port}`, so `iris_test_server`
  and `iris_servers(probe: true)` probed the gateway root. The root answers, so a prefixed entry
  was reported healthy while every real call went elsewhere — a false green, worse than an error.
- `iris_import_servers` dropped a profile's `path_prefix`, so importing an instance that worked
  in VS Code produced one that did not.

Neither was in the issue; both are the same bug.

## Approach

1. **One helper.** `connection_pool::web_prefix_path_part(Option<&str>) -> String` — trims
   whitespace and slashes, returns `""` or `/segment`. `native_base_url(&ServerEntry)` composes
   it with scheme/host/port. The VS Code branch and `instance_base_url` both call the helper; no
   inline trimming survives.
2. **The field.** `#[serde(default, alias = "pathPrefix", skip_serializing_if =
"Option::is_none")] pub web_prefix: Option<String>`. The alias means a block copied out of VS
   Code `settings.json` loads unchanged; `skip_serializing_if` means a prefix-less file does not
   gain the key on save.
3. **Validation, not normalisation, at the boundary.** `validate_web_prefix` refuses only a value
   carrying a scheme or host (`://`, leading `//`), which would concatenate into
   `http://host:8080http://other/hs` and surface much later as a connection error. Everything
   else — empty, bare, slash-wrapped, multi-segment — is accepted and normalised at URL-build
   time. Refusal happens before any write.
4. **Probe the connection, not a rebuilt URL.** `probe_base_url(base_url, ...)` becomes the real
   implementation; `probe_server` is a thin wrapper kept for the ad-hoc `host`/`port` path.
   `iris_servers` and `iris_test_server` pass the pool connection's own `base_url`.
5. **Report it.** `iris_servers` entries carry `base_url` always and `web_prefix` when there is
   one, via `extract_web_prefix_from_url` so every source is covered, not just the native one.
   Two instances behind one gateway share host and port; without `base_url` the listing cannot
   distinguish them.
6. **Update in place.** `iris_add_server` on an existing name preserves the stored password, so a
   forgotten prefix can be added without re-entering the credential.

## Alternatives considered

**Store the assembled `base_url` in the registry instead of parts.** Fewer moving pieces, but it
breaks `scheme`, `host`, and `port` as separate fields that other code reads, and makes the file
harder to hand-edit. Rejected.

**Normalise on write.** Storing `/hs20261` no matter what the operator typed would make the
stored value diverge from what they entered, and still needs the build-time helper for the other
two sources. Rejected: store as typed, normalise at use.

**Only fix `ServerEntry` (the reported scope).** Would have left `iris_test_server` reporting
prefixed entries healthy and `iris_import_servers` dropping the prefix — both facets of the same
bug, both reachable from the reporter's own workflow. Rejected.

## Constitution Check

| Principle                      | Status | Notes                                                             |
| ------------------------------ | ------ | ----------------------------------------------------------------- |
| I. Zero-Install Binary         | PASS   | No new install step                                               |
| II. ObjectScript Sanity        | N/A    | No ObjectScript APIs involved                                     |
| III. HTTP-First Execution      | PASS   | Changes which URL is requested, not the transport                 |
| IV. Test-First, Fixture-Driven | PASS   | Unit tests parse `servers.json` strings, not struct literals      |
| V. Output Shape Parity         | PASS   | `web_prefix` omitted when absent; existing keys unchanged         |
| VI. Environment Guard          | N/A    | No gate surface touched                                           |
| VII. Dependency Minimalism     | PASS   | No new crate                                                      |
| VIII. 90% Coverage Gate        | PASS   | Field, validation, all three sources, and both probe paths tested |
| IX. Tool Lift Requirement      | N/A    | Existing tools; `iris_add_server` gains one optional parameter    |
| X. ObjectScript Coverage       | N/A    | Pure Rust feature                                                 |

## Project Structure

```text
specs/116-servers-json-web-prefix/
├── spec.md
├── plan.md   # this file
└── tasks.md
```

No `research.md`: no external API needed verification — the prefix semantics were already
established by the two sources that supported it. No `contracts/`: the one output-shape change
(`web_prefix` on an `iris_servers` entry) is declared in `output_schemas.rs` and asserted in the
binary-layer tests.

### Source changes

```text
crates/iris-agentic-dev-core/
├── src/iris/servers_config.rs                        # web_prefix field; validate_web_prefix
├── src/iris/connection_pool.rs                       # web_prefix_path_part; native_base_url
├── src/iris/workspace_config.rs                      # instance_base_url uses the helper
├── src/tools/server_tools.rs                         # AddServerParams.web_prefix; probe_base_url
├── src/tools/mod.rs                                  # iris_servers / add / import wiring
├── src/tools/output_schemas.rs                       # ServerEntry.web_prefix
├── tests/unit/test_servers_json_web_prefix.rs        # 14 — URLs from parsed JSON strings
├── tests/binary/servers_json_web_prefix.rs           # 5 — stdio JSON-RPC, temp HOME
└── tests/integration/test_web_prefix_live.rs         # 3 — live iris-dev-iris
```

## Known limit

The ad-hoc mode of `iris_test_server` (`host` + `web_port`, no pool entry) has no prefix
parameter and still probes the root. Register the server to probe a prefixed path. Flagged to the
reporter rather than guessed at, since it only matters if discovery-before-registration is part
of their workflow.
