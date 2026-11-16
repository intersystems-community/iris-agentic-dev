# Feature Specification: use the password InterSystems Studio saved (Windows)

**Feature Branch**: `153-studio-credentials`
**Created**: 2026-11-03
**Status**: Scheduled for January 2027 (getting started). Not started. See `ROADMAP.md`.
**Input**: Tom, 2026-11-03. An InterSystems user on Windows had passwords saved in Studio, and iad
could not use them; he called re-entering them "a big pain" (2026-11-02). Related: #141 (no
plaintext credential for a stdio MCP client on Windows), spec 088 (VS Code `state.vscdb`
fallback).

## Context

On Windows, iad finds a saved password in two places: Windows Credential Manager, then VS Code
Server Manager's `state.vscdb` (spec 088). A developer who has used InterSystems Studio, or the
Windows Server Manager that ships with IRIS, has a third store that iad never reads.

Studio's login dialog has a **Remember password** box. When it is ticked, Studio saves the
password for that server name in the current user's registry:

| Where                                                                 | Value                                                     | Content                                     |
| --------------------------------------------------------------------- | --------------------------------------------------------- | ------------------------------------------- |
| `HKCU\Software\InterSystems\Cache\Servers\<name>`                     | `Server User Name`                                        | plain text                                  |
| `HKCU\Software\InterSystems\Cache\Servers\<name>`                     | `Server Password`                                         | base64 of a DPAPI blob (current-user scope) |
| `HKLM\Software\InterSystems\Cache\Servers\<name>` (and `Wow6432Node`) | `Address`, `Port`, `WebServerAddress`, `WebServerPort`, … | the server definition                       |

The DPAPI call passes no extra entropy, and the plaintext it seals is the password as UTF-16LE.
So the same Windows user can unseal it with `CryptUnprotectData`, which iad already calls for
spec 088. The key path still says `Cache` on current IRIS builds. The source references behind
this table are internal and kept outside the repository; T001 re-checks every row on a real
Windows install before any code is written.

## Open decisions (Tom, before planning)

- **D1. Discovery or credential only.** (a) Read only the password, for a server iad already
  knows by the same name from another source. (b) Also list the registry's server definitions as
  servers iad can connect to, like Server Manager entries. Recommended: (b), because a Studio
  user's servers usually exist nowhere else.
- **D2. On by default or opt-in.** Recommended: on by default, like the `state.vscdb` fallback.
  The store belongs to the operator's own Windows account, not to a repository.
- **D3. Web port.** Studio connects through the superserver `Port` (usually 1972), while iad
  needs the web port. `WebServerPort` is often empty. Recommended: use `WebServerAddress` and
  `WebServerPort` when set; otherwise list the server as needing a web port (`check_config`
  says which value to add), and never guess a port.

## User Scenarios & Testing

### User Story 1 — A Studio password works without re-entering it (P1)

A Windows developer who ticked **Remember password** in Studio points iad at that server. iad
connects with the saved user name and password, and nothing is written to disk in plain text.

**Acceptance**:

1. Given a registry entry with `Server User Name` and a `Server Password` sealed by the same
   Windows user, when iad resolves a credential for that server and Windows Credential Manager
   and `state.vscdb` have none, then it uses the registry pair, and `check_config` names the
   source as `studio_registry`.
2. Given a `Server Password` sealed by a different Windows user, then the lookup fails with an
   error that names the current Windows user, as in spec 088 FR-003, and iad falls through to
   the next source.
3. Given an empty `Server Password` (the box was not ticked), then the registry is not a
   source.

### User Story 2 — The password goes only where Studio sent it (P1)

**Acceptance**:

1. The registry password is sent only to the host and port of the registry definition with the
   same name. A workspace `.iris-agentic-dev.toml`, `.vscode/settings.json` or `[instance.<name>]`
   that reuses the name with a different host gets no credential, and `check_config` reports
   `credentials_withheld`. These are the 1.7.0 rules for a known server name, with registry names
   added to the known set.
2. No workspace file can switch the lookup on, point it at another key, or name a server it
   applies to.
3. The password never appears in a log, a tool response, `check_config` output or an error
   message.

### User Story 3 — Studio's servers appear in iad (P2, only if D1 = b)

**Acceptance**:

1. `iris_servers` lists each registry definition with source `studio_registry`, its address and
   its web port, or `web_port: null` with a hint when none is set.
2. A registry server whose name is already taken by `servers.json`, Server Manager or the
   operator file is not listed twice; the earlier source wins, and `check_config` says so.

## Requirements

- **FR-001**: Windows only (`#[cfg(target_os = "windows")]`); no change on macOS or Linux.
- **FR-002**: The credential lookup order on Windows becomes: Windows Credential Manager,
  `state.vscdb`, then the Studio registry.
- **FR-003**: Unsealing uses `CryptUnprotectData` on the base64-decoded value and decodes the
  result as UTF-16LE. The code is in `iris-agentic-dev-core`, next to the spec 088 code.
- **FR-004**: Both registry views are read (64-bit and `Wow6432Node`), because Studio is a
  32-bit program on some versions.
- **FR-005**: Every refusal or fallthrough is reported in `check_config` by kind, never with the
  password.
- **FR-006**: Tests at the three layers in `CLAUDE.md`: unit (parse a captured registry export,
  UTF-16LE decode, the host-pairing rule with both spellings of a name); binary (`check_config`
  over stdio on a Windows runner, with a registry entry the test writes and seals itself); live
  (a Windows runner against an IRIS instance, using a password the test seals with
  `CryptProtectData`).

## Out of scope

- Writing to Studio's registry keys, or migrating its passwords to another store.
- The superserver protocol: iad stays on HTTP and the web port.
- Studio on macOS or Linux, which does not exist.

## Risks

- **Studio is deprecated.** How many developers still have it installed is unmeasured, and so
  is whether newer kits still create these keys; T001 checks the second.
- **DPAPI user scope.** An MCP client running as another Windows user (a service, or an
  elevated shell) cannot unseal the value. That is the same limit spec 088 has, with the same
  error.
- **Widening where a password can come from.** Every new source is something an attacker can
  aim at. User Story 2 is the guard, and its tests come first.

## Tasks (outline, for when this is scheduled)

- [ ] T001 On a Windows machine with IRIS and Studio: tick Remember password, export the key, and
      confirm every row of the Context table, including UTF-16LE and the `Cache` path.
- [ ] T002 Tests for User Story 2 (host pairing, workspace refusal, no password in any output),
      written first and failing.
- [ ] T003 Tests for User Story 1, then the lookup.
- [ ] T004 If D1 = b: tests for User Story 3, then discovery.
- [ ] T005 Docs: `docs/connecting.md` (Windows credential sources), release notes.
