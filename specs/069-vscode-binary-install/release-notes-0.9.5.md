# What's New in v0.9.5

## VS Code extension auto-installs the binary

Install the extension and start using Copilot agent mode — no separate binary install needed.
On first activation, the extension downloads `iris-agentic-dev` from GitHub Releases and
caches it in VS Code global storage. When you update the extension, it re-downloads the
matching binary version.

- Progress notification appears during the download.
- On Windows, the existing binary is renamed (`.old`) before the new one is written to avoid
  file-lock errors. The renamed file is deleted on next activation.
- On download failure, the extension falls back to any previously cached binary and logs
  the error to the **iris-agentic-dev** Output channel.

Binary lookup order: explicit `iris-agentic-dev.serverPath` setting → `iris-agentic-dev` on
PATH (e.g. via Homebrew, no download occurs) → managed download.

**Supported platforms for auto-install:** macOS Apple Silicon, macOS Intel, Linux x64,
Windows x64. For other platforms, download from
[GitHub Releases](https://github.com/intersystems-community/iris-agentic-dev/releases)
and set `iris-agentic-dev.serverPath`.

**Windows note:** The binary downloads correctly but is unsigned, so Windows SmartScreen
may block it from running. If that happens, you may be able to work around it using Docker —
see [docs/windows-docker.md](https://github.com/intersystems-community/iris-agentic-dev/blob/master/docs/windows-docker.md).

## Bug fixes

### config_file always null at startup (#82)

`ConnectionState.config_file` was always `None` at startup — the config path was never
threaded through to state construction. The path is now propagated from
`apply_workspace_config_with_path` through `with_registry_and_toolset` and set correctly
in `ConnectionState` at startup.

### web_prefix ignored and .toml probe hung (#85)

Two fixes:

- `ConnectionArgs` lacked `web_prefix` and `scheme` fields, so `IRIS_WEB_PREFIX` and
  `IRIS_SCHEME` were ignored when CLI commands built the connection URL directly. Both
  fields are now included.
- The probe timeout was 30 seconds. A new `probe_client()` uses a 5-second connect timeout
  and 10-second total timeout, so failures are reported promptly instead of hanging.
