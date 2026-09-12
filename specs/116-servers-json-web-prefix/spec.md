# Feature Specification: `web_prefix` for the iad-native server registry

**Feature Branch**: `116-servers-json-web-prefix`
**Created**: 2026-09-07
**Status**: Implemented — merged to master (`ed8064c`). Not yet released; ships in the next
release. Issue #129 stays open until the reporter confirms.
**Input**: GitHub issue [#129](https://github.com/intersystems-community/iris-agentic-dev/issues/129)
(isc-ndittber): "iad-native server registry (servers.json) can't store a web-server path prefix —
instances behind a gateway prefix are unreachable"

## Context

`servers.json` is one of four sources the connection pool reads. Two of the other three already
carry a web-server path prefix: VS Code Server Manager profiles read `webServer.pathPrefix` into
`path_prefix`, and `.iris-agentic-dev.toml` has `web_prefix`. `ServerEntry` does not have the
field at all, and the native branch of `load_pool` builds

```rust
let base_url = format!("{}://{}:{}", scheme, entry.host, entry.port);
```

(`crates/iris-agentic-dev-core/src/iris/connection_pool.rs:203`) against the VS Code branch's

```rust
let base_url = format!("{}://{}:{}{}", profile.scheme, profile.host, profile.port, path_part);
```

(same file, line 246). So the same deployment is reachable when discovered through VS Code and
unreachable when registered with `iris_add_server`.

The reporter is standing up two HealthShare 2026.1 instances on one host and port, told apart only
by prefix (`/hs20261` for UCR, `/hscv20261` for Clinical Viewer) — the standard shared-gateway
layout. Per-call `server` routing, which is the point of the 072 pool, does not work for them:
every registered entry resolves to the gateway root. Their workaround is to keep rewriting
`web_prefix` in `.iris-agentic-dev.toml`, which is exactly the hand-editing the registry exists to
replace.

The failure is quiet. An entry with a missing prefix is well-formed, `iris_servers` lists it, and
requests go to the gateway root, where a 404 or an unrelated instance answers.

## User Scenarios & Testing _(mandatory)_

### User Story 1 - Register a prefix-served instance (Priority: P1)

An operator with two HealthShare instances behind one gateway registers each by name, giving the
path prefix that distinguishes them, and then targets either one per call with `server`.

**Why this priority**: This is the reported defect. Without it the registry cannot describe the
deployment at all, so nothing else in the feature matters.

**Independent Test**: Register a server with a `web_prefix`, then read the pool back and assert the
resolved base URL contains the prefix. No live IRIS needed for the URL; a live check against
`iris-dev-iris` confirms a prefixed URL still reaches Atelier.

**Acceptance Scenarios**:

1. **Given** an empty registry, **When** `iris_add_server` is called with
   `web_prefix: "/hs20261"`, **Then** the entry persists the prefix and the pool resolves
   `http://host:8080/hs20261`.
2. **Given** two entries on the same host and port with different prefixes, **When** each is used
   as the `server` parameter, **Then** each call reaches its own instance.
3. **Given** an entry with no `web_prefix`, **When** the pool loads it, **Then** the base URL is
   unchanged from today — `scheme://host:port`, no trailing slash.

---

### User Story 2 - See the prefix in the registry listing (Priority: P2)

`iris_servers` reports each entry's prefix, so an operator can tell a prefix-less entry from a
prefixed one without opening `servers.json`.

**Why this priority**: The bug's cost was mostly diagnostic — the reporter had to read the Rust
source to learn the field did not exist. Surfacing it turns the next occurrence into something
visible from a tool call.

**Independent Test**: Call `iris_servers` against a registry holding one prefixed and one
prefix-less entry; assert both are reported and distinguishable.

**Acceptance Scenarios**:

1. **Given** a prefixed entry, **When** `iris_servers` runs, **Then** the entry's resolved
   URL or prefix field shows the prefix.
2. **Given** a prefix-less entry, **When** `iris_servers` runs, **Then** the field is absent or
   null rather than an empty string masquerading as a value.

---

### User Story 3 - Fix an entry that is already wrong (Priority: P3)

The reporter's existing `hs20252` entry is unreachable as written. They can correct it without
deleting and re-adding, which would mean re-entering the credential.

**Why this priority**: Real but narrow — remove-then-add already works, at the cost of the
password. Worth doing only if it does not complicate US1.

**Independent Test**: Register an entry, update only its `web_prefix`, confirm the credential still
resolves and the base URL changed.

**Acceptance Scenarios**:

1. **Given** an existing entry, **When** the same name is registered again with a `web_prefix`,
   **Then** the entry is updated and the stored credential survives.

---

### Edge Cases

- Prefix given with no leading slash (`hs20261`), with a trailing slash (`/hs20261/`), or with
  both. All three must produce the same URL. The VS Code branch already normalises with
  `trim_matches('/')`; the native branch must match it rather than invent a second rule.
- Multi-segment prefix (`/csp/healthshare/hs20261`).
- Empty string. `web_prefix: ""` must behave as "no prefix", not as a bare trailing slash — a
  double slash in the Atelier path is the bug fixed in 089 for Server Manager paths.
- A prefix containing a scheme or host (`http://other/hs`) — reject rather than concatenate.
- `servers.json` written by an older iad version: no `web_prefix` key, must still load.
- `servers.json` written by a newer version and read by an older one: the older binary must not
  fail to parse. `ServerEntry` is a plain `Deserialize` derive with no `deny_unknown_fields`, so an
  unknown key is ignored today — the new field must not change that.
- `pathPrefix` spelled the VS Code way in a hand-edited `servers.json`. Accepting it as an alias is
  a decision for the plan, not an assumption here.

## Requirements _(mandatory)_

### Functional Requirements

- **FR-001**: `ServerEntry` MUST carry an optional web-server path prefix, serialised only when
  set, so existing registry files round-trip byte-identical.
- **FR-002**: The iad-native branch of `load_pool` MUST append the prefix to the base URL using the
  same normalisation the VS Code branch uses, so one deployment resolves to one URL regardless of
  which source described it.
- **FR-003**: `iris_add_server` MUST accept an optional `web_prefix` parameter, typed and declared
  in its input schema (per 113 — no untyped passthrough).
- **FR-004**: `iris_servers` MUST report the prefix for each native entry.
- **FR-005**: An entry with no prefix MUST resolve to exactly the URL it resolves to today.
- **FR-006**: A prefix that is empty or slashes-only MUST resolve as no prefix, with no trailing or
  doubled slash in the resulting Atelier path.
- **FR-007**: A prefix containing `://` MUST be refused at registration time, by name, with a
  message that says what was passed and what is expected.
- **FR-008**: Registering an existing name with a new prefix MUST update the entry and preserve its
  credential.
- **FR-009**: A `servers.json` lacking the key MUST load unchanged (no migration step, no rewrite
  on read).

### Key Entities

- **ServerEntry**: one registered IRIS instance in `~/.config/iris-agentic-dev/servers.json` —
  host, port, namespace, username, optional description/scheme/plaintext password, and now an
  optional web path prefix.
- **Pool entry**: the resolved connection the registry produces — a base URL plus credentials,
  addressed by name through the `server` parameter.

## Test Coverage _(mandatory — constitution)_

Three layers, per the project's non-negotiable policy:

1. **Unit / JSON round-trip**: parse `servers.json` _strings_, not struct literals — one with the
   key, one without, one with each malformed prefix from Edge Cases. Assert the resolved base URL
   for each. Plus a guard that the native and VS Code branches produce the same URL for the same
   host/port/prefix triple, which is the inconsistency that caused this.
2. **Binary invocation**: spawn `iris-agentic-dev`, `tools/list`, assert `iris_add_server` declares
   `web_prefix`; then `tools/call` it against a temp registry and assert `iris_servers` reports the
   prefix back. No live IRIS.
3. **Live IRIS**: a prefixed base URL that reaches Atelier on `iris-dev-iris`, proving the
   assembled URL is one IRIS actually serves and not just a string that looks right.

## Success Criteria _(mandatory)_

### Measurable Outcomes

- **SC-001**: Two instances sharing a host and port, distinguished only by prefix, are both
  registered and both reachable by name — the reporter's deployment works with no `.toml` editing.
- **SC-002**: Every existing `servers.json` in the wild loads and resolves to the same URL as
  before; zero registry files require migration.
- **SC-003**: The four prefix spellings (`hs`, `/hs`, `hs/`, `/hs/`) resolve to one URL.
- **SC-004**: A prefix that cannot work is refused at registration, so no entry is written that
  will fail later with a 404 from the gateway root.

## Out of Scope

- The `[instance.*]` fleet-config source and `.iris-agentic-dev.toml` `web_prefix` — both already
  carry a prefix and are not changed here.
- Rewriting or migrating existing `servers.json` files on read.
- Probing an entry at registration time to check the prefix is live. `iris_test_server` already
  exists for that and is the caller's choice.
- Any other `ServerEntry` field.
