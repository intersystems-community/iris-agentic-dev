# iris-agentic-dev for VS Code

**MCP tools for ObjectScript and IRIS development — wired directly into GitHub Copilot.**

Ask your AI assistant to compile a class, run a query, search your codebase, inspect a
production, or look up InterSystems documentation — all from the chat panel, without
switching windows or terminals.

![iris-agentic-dev tools in the VS Code Copilot tools panel](https://raw.githubusercontent.com/intersystems-community/iris-agentic-dev/master/docs/images/copilot-tools-panel.png)

---

## What it does

This extension registers the `iris-agentic-dev` MCP server with VS Code Copilot agent mode.
Once installed, Copilot gains a full suite of IRIS-aware tools:

| Category          | What you can ask Copilot to do                                                    |
| ----------------- | --------------------------------------------------------------------------------- |
| **Compile & run** | Compile a class or package, execute ObjectScript inline, run `%UnitTest` suites   |
| **Query**         | Run SQL against any namespace, inspect globals, list namespaces                   |
| **Search**        | Full-text search across your IRIS codebase, find subclass implementations         |
| **Source**        | Read and write class/routine source, manage documents                             |
| **Interop**       | Inspect productions, query event logs, view message traces, manage business rules |
| **Docs**          | Search docs.intersystems.com, introspect classes and methods                      |
| **Config**        | Check connection state, list containers, manage credentials                       |

Connection is automatic — the extension reads your `objectscript.conn` (or InterSystems
Server Manager) so there's nothing extra to configure.

---

## Getting started

### 1. Install the binary

```bash
# macOS (Homebrew)
brew install intersystems-community/tap/iris-agentic-dev

# Or download from GitHub Releases
# https://github.com/intersystems-community/iris-agentic-dev/releases
```

Place it on your PATH, or set `iris-agentic-dev.serverPath` to the full path.

### 2. Install this extension

Search for **iris-agentic-dev** in the VS Code Extensions panel, or install from the
[Marketplace page](https://marketplace.visualstudio.com/items?itemName=intersystems-community.vscode-iris-agentic-dev).

### 3. Open Copilot Chat in agent mode

Press `Ctrl+Shift+I` (or `Cmd+Shift+I` on Mac), switch to **Agent** mode, and ask:

```text
Check my IRIS connection and show me what's running.
```

Copilot calls `check_config` and returns your active connection details:

![check_config result in Copilot chat](https://raw.githubusercontent.com/intersystems-community/iris-agentic-dev/master/docs/images/check-config-result.png)

---

## Example prompts

```text
Compile MyApp.REST.Dispatch and show me any errors.
```

```text
Run the unit tests in MyApp.Tests and summarize failures.
```

```text
Search for all classes that extend %CSP.REST in the USER namespace.
```

```text
What's the current status of the PatientIntake production?
```

```text
Find the InterSystems docs for $ZDateTimeH and show me the format codes.
```

---

## Requirements

- **VS Code 1.99+** with GitHub Copilot (agent mode)
- **iris-agentic-dev binary** — [download from GitHub Releases](https://github.com/intersystems-community/iris-agentic-dev/releases) or install via Homebrew
- **InterSystems ObjectScript extension** with an active server connection
  (`intersystems-community.vscode-objectscript`)

---

## Settings

| Setting                          | Default     | Description                                                               |
| -------------------------------- | ----------- | ------------------------------------------------------------------------- |
| `iris-agentic-dev.serverPath`    | _(auto)_    | Full path to the iris-agentic-dev binary. Leave empty to use PATH.        |
| `iris-agentic-dev.containerName` | _(empty)_   | Docker container name for tools that need direct container access.        |
| `iris-agentic-dev.namespace`     | _(conn ns)_ | Namespace override. Leave empty to use the objectscript.conn namespace.   |
| `iris-agentic-dev.toolset`       | `baseline`  | `baseline` — standard tools. `merged` — adds interop and container tools. |
| `iris-agentic-dev.tlsVerify`     | `true`      | Set `false` for self-signed TLS certificates.                             |
| `iris-agentic-dev.scheme`        | _(auto)_    | Force `http` or `https`. Leave empty to auto-detect.                      |

---

## Troubleshooting

**Tools don't appear in Copilot**
Run `check_config` from a terminal to verify the binary is reachable:

```bash
iris-agentic-dev tool check_config --args '{}'
```

If the binary isn't found, set `iris-agentic-dev.serverPath` in VS Code settings.

**Connection not detected**
Make sure the ObjectScript extension has an active connection (green status bar icon).
`check_config` shows exactly which connection source was used and what was resolved.

**Self-signed certificate errors**
Set `iris-agentic-dev.tlsVerify: false` in your workspace settings.

---

## Links

- [GitHub repository](https://github.com/intersystems-community/iris-agentic-dev)
- [Full tool reference](https://github.com/intersystems-community/iris-agentic-dev/blob/master/docs/tools.md)
- [Connection configuration](https://github.com/intersystems-community/iris-agentic-dev/blob/master/docs/connecting.md)
- [Release notes](https://github.com/intersystems-community/iris-agentic-dev/releases)
- [Report an issue](https://github.com/intersystems-community/iris-agentic-dev/issues)
