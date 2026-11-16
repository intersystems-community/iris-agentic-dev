# Roadmap

**Reviewed for:** v1.7.1 on 2026-11-16

Work is planned in monthly blocks, each with one theme. A block holds what I plan to finish that
month, in order. Months are targets, not promises: a security fix or a bug that blocks people
jumps the queue, whatever the month's theme. The roadmap is reviewed at every release (step 3 of
[the release checklist](docs/release-checklist.md)), and CI fails when the line above names an
older version than the one being built, or when the first block is a month that has passed.

To ask for something, or to argue for moving it up, open an
[issue](https://github.com/intersystems-community/iris-agentic-dev/issues). To report a
vulnerability, follow [SECURITY.md](SECURITY.md) instead.

## November 2026: security

1.7.0 and 1.7.1 closed the known ways around the gates. This month makes that hold.

- **Every scanner finding fixed, or dismissed with a written reason.** CodeQL, Dependabot and the
  OpenSSF Scorecard all run on this repository. After that, a new high or critical finding blocks
  the merge instead of waiting for a review.
- **Fuzz the parsers the gates depend on:** the SQL lexer that decides whether a query writes,
  the checks that find code IRIS would run while compiling, and the config file reader. A gate is
  only as good as its parser.
- **No plain-text password on Windows** for an MCP client that runs outside VS Code. Today the
  fallback is a password in a file. (#141)
- **TLS that works behind a corporate proxy.** Trust the operating system's certificate store,
  not only the bundled roots, so nobody has to switch verification off to get through a proxy
  that re-signs traffic. (#127)
- **Discovery stops sending a password to a server it found by scanning.** Today the first
  local listener that looks like IRIS gets one login attempt, and any local process can fake
  that look. Discovery will list what it found and connect only to a server you
  named. Zero-config setups that rely on the default password will need one setting.
- **Homebrew formula changes go through a checked pull request.** Before `brew upgrade` can
  serve a new formula, a check confirms that each download comes from the project's own GitHub
  release and matches its checksum, and that an iad binary carries that release's build
  provenance. The release job opens that pull request instead of writing to the tap directly.
- **A signed Windows binary.** Until it is signed, SmartScreen may block it; the Docker image is
  the workaround ([docs/windows-docker.md](docs/windows-docker.md)).

Also this month, outside the theme:

- **winget package.** `winget install InterSystemsCommunity.IrisAgenticDev`. Built and tested
  ([spec 152](specs/152-winget/spec.md)); the first submission to Microsoft's package repository
  is next, then Microsoft's review. (#102)
- **Servers behind a web-gateway path prefix**, which `servers.json` cannot describe today.
  (#129)
- **AI Hub EAP images** detected as having no web server when their REST API works. (#165)
- **Bugs:** `iris_global` with `subtree=true` returns nothing (#146); `iris_doc mode=list` fails
  for MAC, INT and INC (#140); the first message to the agent points at the wrong place for
  skill descriptions (#167).
- **A CHANGELOG.md** at the repository root. (#136)

## December 2026: interoperability and mapped globals

Match what [iris-interop-dev](https://github.com/intersystems-ib/iris-interop-dev) offers, then add
SQL storage maps.

- Add and remove production items in a way that survives a recompile.
- Line coverage for production code.
- Query plan cost, for comparing two ways of writing a query.
- SQL Gateway connections.
- Server discovery from VS Code settings before any network scan.
- **SQL storage maps for tables over existing globals.** Read a map, draw it, check it against
  the data and edit it without losing anything, with a compile check before the real class is
  touched. These are tools a GUI like Studio's mapping editor could be built on.
  ([vscode-objectscript#1701](https://github.com/intersystems-community/vscode-objectscript/issues/1701))

Each of these goes through the same gates as every other tool: write and destructive limits,
per-server policy, the service account.

## January 2027: getting started

- **A container setup to copy.** A docker-compose file with IRIS, an agent and iad, with network
  settings that make it safe to let the agent work without asking about each step.
- **Template repositories** for common starting points (a REST API, an interoperability
  production, FHIR), with skills and context already in place.
- **A published benchmark** anyone can re-run: the same IRIS tasks with and without iad, and with
  and without skills, each scored by code that runs on live IRIS.
- **Use the password InterSystems Studio saved** on Windows, so a Studio user does not type it
  again. ([spec 153](specs/153-studio-credentials/spec.md))

## Backlog

Specified or asked for, not scheduled.

- **Submit each release to winget automatically**, once a dedicated bot account exists
  ([spec 152](specs/152-winget/spec.md), decision D2).
- **MCP sampling for log analysis.** (#24)
- **AI Hub deployment mode.** Ship iad's tools and skills as AI Hub tools and skills, served by the
  IRIS instance that runs AI Hub, and add a relay so that instance can also manage other IRIS
  servers, each under the same gates and per-server policy iad applies today.

## Shipped

Recent releases. The full history is in [docs/release-notes/](docs/release-notes/).

- **1.7.1** (2026-11-01): the rest of the security fixes, under advisory
  [GHSA-3wfx-w6qw-hqf4](https://github.com/intersystems-community/iris-agentic-dev/security/advisories/GHSA-3wfx-w6qw-hqf4),
  and signed releases with build provenance.
  [Release notes](docs/release-notes/v1.7.1.md)
- **1.7.0** (2026-10-25): a repository's config file can no longer loosen a limit until you trust
  it, every gate judges the server the call runs on, and agent code runs as the service account
  or not at all. [Release notes](docs/release-notes/v1.7.0.md)
- **1.6.4** (2026-10-23): output capture on Windows IRIS, and MCP clients follow the binary the
  VS Code extension updates. [Release notes](docs/release-notes/v1.6.4.md)
- **1.6.3** (2026-10-22): a cloned repository cannot send your IRIS password to its own server,
  and Live instances are recognised as Live. [Release notes](docs/release-notes/v1.6.3.md)
