# Windows: Run via Docker

The native Windows binary is not yet signed by the corporate pipeline, so Windows users
can run iris-agentic-dev as a Docker container instead. The image is a static Linux binary
and works on any host with Docker Desktop (Windows, macOS, or Linux).

## Prerequisites

- [Docker Desktop](https://www.docker.com/products/docker-desktop/) installed and running
- Claude Code CLI installed

## Pull the image

```bash
docker pull ghcr.io/intersystems-community/iris-agentic-dev:latest
```

## Configure Claude Code

Register the server with `claude mcp add`; the `-e NAME=value` flags go to `docker run`:

```bash
claude mcp add iris-agentic-dev --scope user \
  -- docker run --rm -i \
     -e IRIS_HOST=host.docker.internal -e IRIS_WEB_PORT=52773 \
     -e IRIS_USERNAME=_SYSTEM -e IRIS_PASSWORD -e IRIS_NAMESPACE=USER \
     ghcr.io/intersystems-community/iris-agentic-dev:latest mcp
```

`--scope user` writes to `~/.claude.json`; `--scope project` writes a `.mcp.json` in the current
directory. Claude Code reads no `mcp.json` under `.claude/`. The same server as a hand-written
entry, in `.mcp.json` (project) or `~/.claude.json` (user), is:

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
        "IRIS_HOST",
        "-e",
        "IRIS_WEB_PORT",
        "-e",
        "IRIS_USERNAME",
        "-e",
        "IRIS_PASSWORD",
        "-e",
        "IRIS_NAMESPACE",
        "ghcr.io/intersystems-community/iris-agentic-dev:latest",
        "mcp"
      ],
      "env": {
        "IRIS_HOST": "host.docker.internal",
        "IRIS_WEB_PORT": "52773",
        "IRIS_USERNAME": "_SYSTEM",
        "IRIS_PASSWORD": "<the _SYSTEM password>",
        "IRIS_NAMESPACE": "USER"
      }
    }
  }
}
```

## Environment variables

| Variable         | Description          | Default in example                          |
| ---------------- | -------------------- | ------------------------------------------- |
| `IRIS_HOST`      | IRIS server hostname | `host.docker.internal` (your local machine) |
| `IRIS_WEB_PORT`  | Atelier REST port    | `52773`                                     |
| `IRIS_USERNAME`  | IRIS username        | `_SYSTEM`                                   |
| `IRIS_PASSWORD`  | IRIS password        | `<the _SYSTEM password>`                    |
| `IRIS_NAMESPACE` | Default namespace    | `USER` (if not set)                         |

`docker run` passes a variable into the container only when it carries its own `-e NAME`, so
forward every variable you set, including `IRIS_SCHEME` and `IRIS_WEB_PREFIX` for a TLS or
prefixed gateway. A fresh container's default password is expired and must be replaced first; see
[Getting started, step 1](getting-started.md#step-1--get-an-iris-running).

### Connecting to a remote IRIS instance

Replace `host.docker.internal` with the IP or hostname of your IRIS server:

```json
"IRIS_HOST": "myiris.example.com"
```

### Linux hosts

`host.docker.internal` is not available by default on Linux. Add
`--add-host=host.docker.internal:host-gateway` to the `args` list:

```json
"args": [
  "run", "--rm", "-i",
  "--add-host=host.docker.internal:host-gateway",
  "-e", "IRIS_HOST",
  ...
]
```

## Project files and trust

Inside the container, `IRIS_*` variables in the `env` block above are launch settings, which iad
trusts. A project's `.iris-agentic-dev.toml` is not: if you mount one in, its connection settings
apply, but anything that would widen what the agent may do (`write_tools_enabled = true`,
`tls_verify = false`, and so on) is set aside until you trust the file with
`iris-agentic-dev trust` and a restart, and `trust` needs a terminal and writes the entry to the operator file
(`~/.iris-agentic-dev.toml`) in the home directory of the process that reads it. A container started
with `--rm` has no operator file of its own to record it in, so put such settings in the `env`
block instead. The container has no OS keychain either, so a password given to `iris_add_server`
is written to `servers.json` inside the container, which `--rm` removes with it.
`check_config` shows what was set aside.

## Limits

The image holds only the binary, with no `docker` or `ssh` and no OS keychain. Over Docker, iad
reaches IRIS by HTTP only: `container`, `docker_only`, `nopws`, `ssh_host` and the `docker exec`
fallback do not work. Tools that read your project files (`iris_compile` with a path, `kb index`,
`iris_symbols_local`) see only the container's file system, so mount the project into the
container with `-v` and point `--workspace` at the mount, or use the native binary.

## Verify the connection

Once configured, ask Claude:

```text
Check my IRIS connection
```

Claude will call `check_config` through the Docker MCP server and report the connection status.

## Troubleshooting

**`docker: command not found`** — Docker Desktop is not installed or not on PATH. Install
Docker Desktop and restart your terminal.

**`Error response from daemon: pull access denied`** — The image is public, so check the image
name, run `docker logout ghcr.io` (a stale token causes this on a public image), and try
`docker pull ghcr.io/intersystems-community/iris-agentic-dev:latest` by hand. To pin a version and
verify its signature, see [verifying-releases.md](verifying-releases.md).

**Connection refused to IRIS** — On Windows/macOS, `host.docker.internal` should resolve
to your host machine. Verify your IRIS instance is running on the port you specified. On
Linux, add `--add-host=host.docker.internal:host-gateway` (see above).

**Slow first tool call** — The Docker container starts fresh on every Claude session. First
call latency is a few seconds (container startup). Subsequent calls within the same session
are fast.

## Performance

Per-message latency is negligible — stdio is passed directly through kernel pipes. The main
overhead is container cold-start (typically 1–3 seconds on Docker Desktop). For most IRIS
development workflows this is acceptable.
