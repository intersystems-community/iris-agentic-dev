# Release notes — v0.9.5

## What's new

### Docker image — Windows workaround

iris-agentic-dev is now published as a Docker image
(`ghcr.io/intersystems-community/iris-agentic-dev`). The primary motivation is
Windows: the Windows `.exe` is not yet code-signed (signing is blocked on an
internal approval process), so Windows Defender flags it as untrusted. Running
the MCP server via Docker sidesteps the signing requirement entirely — Docker
Desktop on Windows is already trusted.

Configure Claude Desktop to use it via the Docker transport:

```json
{
  "mcpServers": {
    "iris-agentic-dev": {
      "command": "docker",
      "args": [
        "run",
        "--rm",
        "-i",
        "-e",
        "IRIS_HOST=host.docker.internal",
        "-e",
        "IRIS_WEB_PORT=52773",
        "ghcr.io/intersystems-community/iris-agentic-dev:latest",
        "mcp"
      ]
    }
  }
}
```

Use `host.docker.internal` to reach IRIS running on your Windows host (works on
Docker Desktop for Windows and macOS). On Linux, add
`--add-host=host.docker.internal:host-gateway` to the args instead.

See `docs/windows-docker.md` for the full cookbook including VS Code setup,
`.iris-agentic-dev.toml` placement, and volume-mount patterns.

## Fixes

### `check_config` no longer shows `config_file: null` for toml connections (issue #82)

`check_config` reported `config_file: null` at startup even when a
`.iris-agentic-dev.toml` was loaded — the path was only recorded after the first
hot-reload. The startup path is now threaded through to `ConnectionState`
immediately, so the field is populated from the first call.

### `web_prefix` / `IRIS_WEB_PREFIX` now applied to all commands (issue #85)

Connections to IRIS instances served behind an IIS path prefix (e.g. a
HealthShare layout where `/healthshareucr`, `/healthshareempi`, etc. share
one IIS:80 server) were broken in two ways:

1. `IRIS_WEB_PREFIX` was ignored by the `exec`, `compile`, `query`, and `doc`
   CLI commands — requests hit the root Atelier path and reached the wrong
   instance. Fixed: `web_prefix` and `scheme` are now fields on `ConnectionArgs`
   and are applied when building the connection URL.

2. When a `.iris-agentic-dev.toml` with `web_prefix` was loaded, a
   non-responsive IIS endpoint appeared to hang indefinitely. Fixed: the startup
   probe now uses a dedicated short-timeout client (5 s connect, 10 s total)
   instead of the 30 s operation client, so failures surface quickly.

## Upgrade

```bash
brew upgrade iris-agentic-dev
```
