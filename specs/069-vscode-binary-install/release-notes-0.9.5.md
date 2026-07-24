# What's New in v0.9.5

## VS Code extension auto-installs the binary

The VS Code extension now downloads and manages the `iris-agentic-dev` binary automatically
on first activation. No separate install step is required — install the extension and start
using Copilot agent mode immediately.

**How it works:**

- On activation, the extension checks for the binary using a three-tier lookup:
  1. Explicit `iris-agentic-dev.serverPath` setting (if set)
  2. `iris-agentic-dev` on your PATH (e.g. via Homebrew — no download occurs if found)
  3. Managed download from GitHub Releases into VS Code global storage

- A progress notification appears during the download.
- The binary is versioned: on extension update, the new version is fetched automatically.
- On Windows, the existing binary is renamed (`.old`) before the new one is written to avoid
  file-lock errors. The old file is cleaned up on next activation.
- If a download fails, the extension falls back to any previously cached binary and logs
  the error to the **iris-agentic-dev** Output channel.

**Supported platforms for auto-install:** macOS Apple Silicon, macOS Intel, Linux x64,
Windows x64. For other platforms, download from
[GitHub Releases](https://github.com/intersystems-community/iris-agentic-dev/releases)
and set `iris-agentic-dev.serverPath`.

## Bug fixes

### config_file always null at startup (#82)

`ConnectionState.config_file` was always `None` when reported via `check_config` at startup
because the config path was not threaded through to state construction. The path is now
propagated from `apply_workspace_config_with_path` through `with_registry_and_toolset` and
set correctly in `ConnectionState` at startup.

### web_prefix ignored and .toml probe hung (#85)

Two separate issues were fixed:

- `ConnectionArgs` did not include `web_prefix` or `scheme` fields, so the
  `IRIS_WEB_PREFIX` and `IRIS_SCHEME` environment variables were ignored when CLI commands
  built the connection URL directly. Both fields are now included and applied.
- The probe timeout was 30 seconds, which felt like an indefinite hang on unreachable hosts.
  A dedicated `probe_client()` now uses a 5-second connect timeout and 10-second total
  timeout so failures are reported promptly.
