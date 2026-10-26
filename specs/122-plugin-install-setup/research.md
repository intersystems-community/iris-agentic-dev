# Phase 0 research: what the plugin loader actually does

Every answer below is a command and its output, run on 2026-09-22 against
`claude` on this machine. Nothing here is inferred from documentation.

## Q1 — What does `marketplace.json` need? (T001)

### The minimum that validates clean

```json
{
  "name": "min-market",
  "description": "Minimum marketplace manifest that validates clean under --strict.",
  "owner": { "name": "probe" },
  "plugins": [{ "name": "min-plugin", "source": "./p" }]
}
```

`plugins[].description` is optional. `description` at the top level is not:

```console
$ claude plugin validate --strict /tmp/mp-min   # description removed
⚠ Found 1 warning:
  ❯ description: No marketplace description provided. Adding a description helps
    users understand what this marketplace offers
✘ Validation failed (--strict treats warnings as errors)
$ echo $?
1
```

Put it back and the same command exits 0:

```console
$ claude plugin validate --strict /tmp/mp-min
✔ Validation passed
$ echo $?
0
```

The plugin manifest each `source` points at needs `author`, or every plugin in the
marketplace draws a warning:

```console
$ claude plugin validate /tmp/skills-probe
⚠ Found 3 warnings:
  ❯ plugins[0] plugin.json → author: No author information provided. Consider adding
    author details for plugin attribution
  ❯ plugins[1] plugin.json → author: ...
  ❯ plugins[2] plugin.json → author: ...
```

`--json` gives a machine-readable report with `success`, `strict`, `target`,
`manifest.{errors,warnings,notes}` and `contents`, which is what a CI step should
assert on rather than grepping the human output.

### One warning this repo cannot clear

```console
$ claude plugin validate --strict ~/ws/iris-agentic-dev/.claude-plugin/plugin.json
Validating plugin: ~/ws/iris-agentic-dev/CLAUDE.md
⚠ Found 1 warning:
  ❯ root: CLAUDE.md at the plugin root is not loaded as project context. To ship
    context with your plugin, use a skill (skills/<name>/SKILL.md) instead.
✘ Validation failed (--strict treats warnings as errors)
$ echo $?
1
```

The plugin root is the repo root, and the repo root has a `CLAUDE.md` because it is
a repo someone works in. The warning is correct and not actionable: that file is
project context for contributors, not plugin content. So **`--strict` cannot be the
CI gate** here. FR-001 and SC-003 have to say "no errors, and the only warning is
this one", and the test should assert on the JSON report's `errors` array with an
allowlist of exactly this warning. A blanket "no warnings" gate would be a gate
nobody can ever pass, which is worse than no gate.

## Q2 — Is the skills root configurable? (T002)

Yes. Four scratch plugins, installed from a local marketplace, inspected with
`claude plugin details`:

| Probe | Layout                                              | `plugin.json` `skills` | Skills loaded             |
| ----- | --------------------------------------------------- | ---------------------- | ------------------------- |
| a     | `skills/root-skill/SKILL.md`                        | absent                 | `root-skill`              |
| b     | `skills/skills/nested-skill/SKILL.md`               | absent                 | none                      |
| c     | `skills/skills/nested-skill/SKILL.md`               | `["./skills/skills"]`  | `nested-skill`            |
| d     | `skills/stray-skill/` + `skills/skills/real-skill/` | `["./skills/skills"]`  | `real-skill, stray-skill` |

```console
$ claude plugin details probe-b
  Skills (0)

$ claude plugin details probe-c
  Skills (1)  nested-skill

$ claude plugin details probe-d
  Skills (2)  real-skill, stray-skill
```

Two things follow. A `skills` array in `plugin.json` takes a parent directory and
loads every `<name>/SKILL.md` under it, so one entry covers all 34. And probe-d
shows the declaration **adds to** the default `skills/*` scan rather than replacing
it — the stray root skill loaded anyway.

So the plugin will ship 37 skills: the 34 bundled ones from `skills/skills/`, plus
the three that already live at `skills/` root and have always been there
(`iris-coverage-run`, `iris-coverage-setup`, `pyprod`). `skills/kb/` has no
`SKILL.md` and does not load. The four "stray directories" in the spec's fact 4
were three skills and a reference directory, which is a better starting position
than the spec assumed.

## Q3 — What does a plugin MCP server with no `env` block resolve to? (T003)

The same connection the CLI resolves. Probe plugin, `mcpServers.probe-iad` with a
command and `args: ["mcp"]` and no `env` at all:

```console
$ claude mcp list
plugin:probe-e:probe-iad: ~/ws/iris-agentic-dev/target/debug/iris-agentic-dev mcp - ✔ Connected
```

A session calling `check_config` on it, with `iris-dev-iris` running and no
`IRIS_*` variable set anywhere in the environment:

```json
{
  "connected": true,
  "connection_source": "config_file",
  "config_file": "~/ws/iris-agentic-dev/.iris-agentic-dev.toml",
  "container": "iris-dev-iris",
  "host": "localhost",
  "port": 52780,
  "namespace": "USER",
  "iris_version": "IRIS for UNIX (Ubuntu Server LTS for ARM64 Containers) 2026.2.0L (Build 208U) Thu May 28 2026 00:52:48 EDT",
  "server_version": "1.4.2+v1.4.2-22-g9d1bbd8-dirty"
}
```

`connection_source: "config_file"` is the answer: the plugin-started server found
`.iris-agentic-dev.toml` in the host's working directory and used it, exactly as a
hand-registered server does. Deleting the `${IRIS_HOST}`-style `env` block per
FR-003 costs nothing and removes the empty-string failure mode.

Also worth recording: plugin servers appear in `claude mcp list` under
`plugin:<plugin-name>:<server-name>`, so a test can assert the plugin's server
connected without spawning a session.

## Scratch state

`/tmp/skills-probe` and `/tmp/mp-min` held the probes; the `skills-probe`
marketplace and its five plugins were removed after the runs so they do not sit in
user settings.

## Phase 1 round trip (T009)

Both manifests in place, the MCP command fixed and the `env` block gone, installed from this
checkout:

```console
$ claude plugin marketplace add ~/ws/iris-agentic-dev
✔ Successfully added marketplace: iris-agentic-dev (declared in user settings)

$ claude plugin install iris-dev@iris-agentic-dev
✔ Successfully installed plugin: iris-dev@iris-agentic-dev (scope: user)

$ claude mcp list | grep plugin:iris-dev
plugin:iris-dev:iris-dev: iris-agentic-dev mcp - ✔ Connected
```

`claude plugin details iris-dev` at this point, before the skills declaration lands:

```text
Component inventory
  Skills (3)  iris-coverage-run, iris-coverage-setup, pyprod
  Agents (0)
  Hooks (2)  PostToolUse, FileChanged  (harness-only — no model context cost)
  MCP servers (0)
  LSP servers (0)
```

Three skills, which is fact 4 from the spec reproduced exactly: the 34 bundled ones are
invisible until T011 declares their directory.

One inconsistency to know about: `plugin details` reports `MCP servers (0)` for a plugin whose
server `claude mcp list` shows connected. `mcp list` is the one to trust, and it is also the
one a test should read, since it reports the connection rather than just the declaration.

`plugin validate --json` on both manifests: `errors: []` for every report, and the only
warning anywhere is the allowlisted `CLAUDE.md`-at-root one, which appears under `contents[]`
rather than under `manifest` — worth knowing when writing the assertion, since a check that
only reads `manifest.warnings` would see a clean report and miss everything about the plugin's
actual contents.

## Phase 2: how a declared skills path is read (T012)

The `skills` array is not a list of parent directories. Each entry is read one of two ways,
and the difference decides whether a skill ships:

| `"skills"`                               | `plugin details` reports                                 |
| ---------------------------------------- | -------------------------------------------------------- |
| absent                                   | 3 — the `skills/` root scan only                         |
| `["./skills/skills"]`                    | 37 — root scan plus every child of that directory        |
| `["./skills/skills/iris-agentic-dev"]`   | 4 — the root scan plus **one** skill, `iris-agentic-dev` |
| `["./skills/skills", ".../nopws-setup"]` | 38 — everything in the tree                              |

The third row is the finding. `skills/skills/iris-agentic-dev` holds its own `SKILL.md`, so
the loader treated it as a single skill rather than as a parent, and its child `nopws-setup`
stayed invisible. The rule: a declared path with a `SKILL.md` is one skill; a path without one
is a parent of skills; neither recurses.

`nopws-setup` is not incidental. `tools/nopws.rs` and `tools/mod.rs` both tell a reader to go
read `skills/skills/iris-agentic-dev/nopws-setup/SKILL.md` when an Enterprise build with no
private web server refuses a connection, which is the moment someone most needs it. Shipping
the plugin without it would have pointed people at a file they do not have.

Final declaration, `plugin details iris-dev` reporting 38:

```json
"skills": ["./skills/skills", "./skills/skills/iris-agentic-dev/nopws-setup"]
```

`test_plugin_skills.py` models that rule and walks the whole `skills/` tree, so a new skill at
any depth that no entry reaches fails the test. Mutation-checked both ways: dropping the nested
entry fails two tests, dropping the array fails three.

## Phase 3: what a fresh container actually does (T015, T016)

A throwaway `iad-quickstart` (`intersystemsdc/iris-community:latest`, resolving to 2026.1 Build
234U on ARM64), ports 9772/9773.

**No healthcheck on the image.** Forty polls of
`docker inspect -f '{{.State.Health.Status}}'` returned empty every time, so the usual
wait-for-healthy loop never terminates. The REST endpoint is the readiness signal: a `401` from
`/api/atelier/` means IRIS is up and asking for credentials.

**The expired default password, and what it breaks.** Before
`##class(Security.Users).UnExpireUserPasswords("*")`:

```text
$ iris-agentic-dev tool iris_test_server --args '{"host":"localhost","web_port":9773,"username":"_SYSTEM","password":"SYS"}'
{"auth":false,"error":"Authentication failed (HTTP 401)","host":"localhost","iris_version":null,
 "latency_ms":10,"port":9773,"reachable":true}
```

After it:

```text
{"atelier_version":"8","auth":true,"error":null,"host":"localhost",
 "iris_version":"IRIS for UNIX (Ubuntu Server LTS for ARM64 Containers) 2026.1 (Build 234U) ...",
 "latency_ms":14,"port":9773,"reachable":true}
```

Same bytes as a wrong password in the failing case. Nothing distinguishes expired from wrong, which
is why the skill names the expiry as the first thing to check rather than the credentials.

**An ad-hoc probe does not inherit credentials.** Omitting `username`/`password` returns
`auth: false` against the same healthy server that answers `auth: true` when they are passed
explicitly. A reader who trusts the shorter form concludes the credentials are broken.

**Discovery walks away from an authentication failure.** `IRIS_WEB_PORT=9773` with the password
expired:

```text
{'connected': True, 'connection_source': 'auto_discovered', 'port': 45083,
 'iris_version': '... 2026.3.0AI (Build 141U) ...'}
```

Port 45083 is `dxlab-r39-iris`, an unrelated container. After clearing the expiry, the same command
reports `port: 9773, container: 'iad-quickstart'`. So an explicit port is honoured only while
authentication succeeds, and `connected: true` on its own says nothing about which database you
reached. On a one-instance laptop this is invisible; on a developer machine with ten it is a wrong
answer that looks right. The skill therefore has the reader compare `port` and `container` against
what they started, and pin both rather than leaving it to discovery.

**`claude mcp add` round trip**, run in a temp directory and removed afterwards:

```text
$ claude mcp add iad-scratch ~/ws/iris-agentic-dev/target/debug/iris-agentic-dev mcp \
    --env IRIS_WEB_PORT=9773 --env IRIS_CONTAINER=iad-quickstart
Added stdio MCP server iad-scratch with command: .../iris-agentic-dev mcp to local config
$ claude mcp list
iad-scratch: .../target/debug/iris-agentic-dev mcp - ✔ Connected
```

**The remote marketplace form does not work yet.**
`gh api repos/intersystems-community/iris-agentic-dev/contents/.claude-plugin/marketplace.json`
returns 404 on `master`, so `claude plugin marketplace add intersystems-community/iris-agentic-dev`
has nothing to read until this branch lands. The local-checkout round trip in Phase 1 is the same
path with a different source.

**Loader inventory after the setup skill landed:** `claude plugin details iris-dev` reports
`Skills (39)`, including `iris-agentic-dev-setup`. Thirty-eight before, so the implicit `skills/`
root scan picked it up with no manifest change.
