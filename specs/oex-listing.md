# OEX Listing — iris-agentic-dev

**Status: DRAFT, not submitted.** Field copy for a future Open Exchange submission. Nothing
here is published.

Submit at: <https://openexchange.intersystems.com> (Add Package → VS Code Extension)

## Fields

**Title:** iris-agentic-dev

**Short description:** MCP server connecting AI coding assistants to a live IRIS instance

**Category:** Developer Environment

**Tags:** `ai`, `mcp`, `copilot`, `tool`

**Compatible platforms:** InterSystems IRIS, HealthShare

**License:** MIT (<https://github.com/intersystems-community/iris-agentic-dev/blob/master/LICENSE>)

**About URL:** <https://github.com/intersystems-community/iris-agentic-dev>

**Community article URL:** _(add once Part 1 is published on community.intersystems.com)_

## Links

- Repository: <https://github.com/intersystems-community/iris-agentic-dev>
- Issue tracker: <https://github.com/intersystems-community/iris-agentic-dev/issues/new>
- Releases: <https://github.com/intersystems-community/iris-agentic-dev/releases>
- VS Marketplace: <https://marketplace.visualstudio.com/items?itemName=intersystems-community.vscode-iris-agentic-dev>
- Homebrew tap: <https://github.com/intersystems-community/homebrew-tap>

## Screenshots

- `docs/images/copilot-tools-panel.png`
- `docs/images/check-config-result.png`

## Long description

`iris-agentic-dev` gives GitHub Copilot, Claude Code, Cursor, and OpenCode a live
connection to an IRIS instance through the Model Context Protocol. The AI can answer
questions about your namespace without you opening a single file.

### Install: VS Code + GitHub Copilot

Install the **iris-agentic-dev for IRIS** extension from the VS Code Marketplace. On
first activation it locates or downloads the server binary and registers itself with
Copilot Agent mode. It reads your existing `objectscript.conn` or
`intersystems.servers` connection — no additional config needed. If Server Manager is
installed, credentials come from the OS keychain automatically.

### Install: Claude Code, Cursor, OpenCode

```bash
# Mac
brew tap intersystems-community/tap
brew install iris-agentic-dev
```

Binary downloads for Mac (Intel/ARM), Linux, and Windows are on the
[releases page](https://github.com/intersystems-community/iris-agentic-dev/releases/latest).

Register with Claude Code:

```bash
claude mcp add --scope user iris-agentic-dev -- iris-agentic-dev mcp
```

### What the AI can do

- Search the full namespace — full-text, regex, by category — without opening files
- Compile classes and get errors back with line numbers
- Execute ObjectScript and SQL against any namespace
- Introspect class definitions — properties, methods, parameters, inheritance chains
- Inspect Ensemble/HealthShare productions — item status, message flow, business rule logic, config drift
- Run %UnitTest suites
- Map INT line numbers back to original source lines

### Notes

- Requires IRIS 2023.1 or later
- Atelier REST API must be reachable (PWS on port 52773, or ISC Web Gateway)
- Enterprise standalone images without a web gateway are not supported
- All tools work over Atelier REST — no Docker container required (v0.9.9+)
