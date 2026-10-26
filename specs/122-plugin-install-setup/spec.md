# Feature Specification: install and set up from inside Claude Code

**Feature Branch**: `122-plugin-install-setup`
**Created**: 2026-09-22
**Status**: Draft
**Input**: "What about just telling Claude Code to install iris-agentic-dev? Does that work? If we have
a skill that handles installation/setup, then maybe install the skill and then run it."

## Measured starting point

Asking an agent to install this does not work today, and the path that should make it work is
unreachable. Four facts, each checked on 2026-09-22:

1. A session with no iad server configured (`claude -p --strict-mcp-config`, web tools allowed,
   the `iris-agentic-dev` skill present on the machine) could not name the install channel:
   _"I don't know the publish channel for this binary — it's your Rust tool, not a public crate I
   can verify."_ It offered `brew install <your-tap>/iris-agentic-dev` as a placeholder. Steps
   after the install were right, because the skill covers them.
2. `claude plugin marketplace add ~/ws/iris-agentic-dev` fails:
   `Marketplace file not found at .claude-plugin/marketplace.json`. The plugin cannot be
   installed by anyone.
3. `.claude-plugin/plugin.json` sets `mcpServers.iris-dev.command` to `iris-dev`. Homebrew
   installs one executable, `iris-agentic-dev`. The manifest works on my machine only because a
   stale `~/.local/bin/iris-dev` from 2026-07-30 is on PATH, which is also feeding the
   stale-MCP-process warnings from the container guard.
4. Plugin skills load from `skills/<name>/SKILL.md` at the plugin root. The 34 bundled skills are at
   `skills/skills/<name>/SKILL.md`, so `/plugin install` ships only the three that happen to sit at
   the root (`iris-coverage-run`, `iris-coverage-setup`, `pyprod`) and none of the bundled ones.
   `claude plugin details probe-b` reports `Skills (0)` for exactly this layout; see `research.md`.

The binary cannot install itself, so the only channel that reaches a machine with nothing on it is
the plugin marketplace. That is what makes the skill idea work: the plugin carries the skill, and
the skill carries the install.

## User Scenarios & Testing

### User Story 1 - Newcomer with nothing installed (Priority: P1)

Someone with Claude Code, Docker, and no IRIS types two commands and then asks in English. They end
with tools that answer from a live instance.

```text
/plugin marketplace add intersystems-community/iris-agentic-dev
/plugin install iris-dev
> set up iris-agentic-dev against a local IRIS container
```

**Why this priority**: it is the whole feature. Everything else here is a component of it.

**Independent Test**: a probe session on a machine where the binary is absent from PATH, driven by
that prompt, ends with a successful `iris_query` against a container the session started. The
measurement is the transcript, not the reader's impression.

**Acceptance Scenarios**:

1. **Given** no `iris-agentic-dev` on PATH, **When** the plugin is installed and the prompt is
   given, **Then** the session installs the binary from `intersystems-community/tap` without
   guessing the channel.
2. **Given** Docker running and no IRIS container, **When** the setup skill runs, **Then** it
   starts `intersystemsdc/iris-community:latest`, clears the expired default password, and says
   which container and port it chose.
3. **Given** setup finished, **When** the session calls `check_config`, **Then** `connected` is
   true and `iris_version` is a version string rather than null.
4. **Given** setup finished, **When** the session runs a query, **Then** rows come back from the
   instance it just configured.

### User Story 2 - Docker is there but IRIS is not reachable (Priority: P2)

The binary is installed and a container exists, but nothing answers. The skill separates the three
causes rather than retrying.

**Why this priority**: this is the failure a newcomer actually hits, and the expired default
password produces a 401 that the CLI currently reports as `IRIS_UNREACHABLE` with a host/port hint.
A reader following that hint edits the one thing that was already correct.

**Independent Test**: point the setup skill at a fresh container whose password has not been
cleared and check that it names the password as the cause.

**Acceptance Scenarios**:

1. **Given** a fresh container, **When** setup verifies, **Then** `iris_test_server` reports
   `reachable: true` and `auth: false`, and the skill says to clear the expired password.
2. **Given** a container on a port that is not 52773 (OrbStack assigns dynamically), **When** setup
   resolves the port, **Then** it reads the mapping from `docker port` rather than assuming.
3. **Given** no Docker daemon answering, **When** setup runs, **Then** it says so and stops instead
   of reporting an IRIS problem.

### User Story 3 - Plugin install brings the skills (Priority: P3)

`/plugin install iris-dev` puts the ObjectScript skills in front of the agent, with no
`iris-agentic-dev skill install` and no binary needed for that part.

**Why this priority**: the skills are measured at nothing on the graded holdout so far, so shipping
them is not urgent. The layout defect is worth fixing because the manifest currently claims to ship
skills and ships four unrelated directories.

**Independent Test**: install the plugin from a local path and assert the loaded skill names match
the directories under the skills root.

**Acceptance Scenarios**:

1. **Given** the plugin installed from a local checkout, **When** the skills are listed, **Then**
   every skill the binary's `skill install` would write is present.
2. **Given** a skill added to the repo, **When** the plugin is installed, **Then** it appears
   without a manifest edit.

### Edge Cases

- **No Homebrew** (Linux, Windows): the skill has to pick the `curl` release asset for the platform,
  and on Windows say what to do rather than pretend.
- **Binary already installed somewhere odd** (`~/.cargo/bin`, `~/.local/bin`): registering a bare
  command name fails when a GUI-launched host has a different PATH. The absolute path from
  `command -v` is the answer, and a stale copy on PATH is worse than none — fact 3 above is that
  exact situation.
- **`${IRIS_HOST}` unset**: a plugin manifest that expands unset variables hands the server empty
  strings, which is worse than absent because discovery never runs and `check_config` reports a
  host of `""`. The repo's `empty-config-value` antipattern check exists for this shape.
- **An IRIS already running** that the reader cares about: setup must not start a second container
  or repoint an existing registration without saying so.
- **Agent with no permission to run `brew`**: the skill has to print the command rather than fail
  silently, and the reader runs it.

## Requirements

### Functional Requirements

- **FR-001**: The repo MUST contain `.claude-plugin/marketplace.json` that
  `claude plugin marketplace add <path-or-repo>` accepts and `claude plugin validate` reports no
  errors for. The one warning about `CLAUDE.md` at the plugin root is expected and allowlisted: the
  plugin root is the repo root, that file is contributor context, and the warning is not actionable.
  `--strict` is therefore not the gate. See `research.md`.
- **FR-002**: `plugin.json`'s MCP command MUST be an executable that a released install provides.
  `iris-agentic-dev` is the only one.
- **FR-003**: The plugin's MCP entry MUST NOT set connection variables to `${VAR}` placeholders
  that may be unset. Connection resolution stays with `.iris-agentic-dev.toml`, explicit env, and
  discovery, in the order `docs/connecting.md` documents.
- **FR-004**: The plugin MUST ship every skill the binary bundles, declared as a directory root in
  `plugin.json` rather than as a list of names, so a new skill appears without a manifest edit. The
  three skills already at `skills/` root ship too; they are named in the parity test so a fourth
  cannot appear unnoticed.
- **FR-005**: A setup skill MUST cover, in order: detect the binary, install it for the platform,
  find or start an IRIS, clear the expired default password, resolve the web port, register the MCP
  server, verify with `check_config` and one real query.
- **FR-006**: Every document and skill that names the Homebrew tap MUST name
  `intersystems-community/tap`. (Guarded as of `4a233d4`; the guard has to keep covering the new
  skill.)
- **FR-007**: The setup skill MUST separate unreachable from unauthenticated using
  `iris_test_server`, and MUST name the expired default password as the first cause of a 401 on a
  fresh container.
- **FR-008**: `marketplace.json`'s version MUST agree with `plugin.json` and the workspace
  `Cargo.toml`, asserted by extending the existing version-consistency test rather than by review.
- **FR-009**: `README.md` and `docs/getting-started.md` MUST name the plugin path, and the guide's
  executable-commands guard MUST cover whatever they name.

### Key Entities

- **Marketplace manifest** (`.claude-plugin/marketplace.json`): what `claude plugin marketplace
add` reads. Names the owner and the plugins the repo offers, with a source path per plugin.
- **Plugin manifest** (`.claude-plugin/plugin.json`): name, version, MCP servers, hooks. Already
  present and wrong in the two ways above.
- **Setup skill**: an instruction file, not code. Its correctness is measured by whether a session
  following it arrives at a working connection, which is why US1's test is a probe session rather
  than an assertion about the text.

## Success Criteria

### Measurable Outcomes

- **SC-001**: On a machine with no `iris-agentic-dev` on PATH, a single plain-English prompt after
  `/plugin install` ends in a successful `iris_query` against a running IRIS. Measured by a probe
  session, transcript kept.
- **SC-002**: The channel guess disappears. The same probe that produced _"I don't know the publish
  channel for this binary"_ names `intersystems-community/tap` with no placeholder.
- **SC-003**: `claude plugin validate --json` reports an empty `errors` array for both manifests, and
  its only warning is the allowlisted `CLAUDE.md`-at-root one.
- **SC-004**: The skills the plugin loads are every bundled skill plus the three named root ones.
  Asserted, not eyeballed.
- **SC-005**: Time from `/plugin marketplace add` to a query returning rows is under five minutes on
  a machine that already has Docker, with the image pull excluded.
