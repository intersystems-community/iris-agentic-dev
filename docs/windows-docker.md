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

Add the following to your Claude Code MCP config (`.claude/mcp.json` in your project, or
`~/.claude/mcp.json` globally):

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
        "ghcr.io/intersystems-community/iris-agentic-dev:latest",
        "mcp"
      ],
      "env": {
        "IRIS_HOST": "host.docker.internal",
        "IRIS_WEB_PORT": "52773",
        "IRIS_USERNAME": "_SYSTEM",
        "IRIS_PASSWORD": "SYS"
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
| `IRIS_PASSWORD`  | IRIS password        | `SYS`                                       |
| `IRIS_NAMESPACE` | Default namespace    | `USER` (if not set)                         |

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

## Verify the connection

Once configured, ask Claude:

```text
Check my IRIS connection
```

Claude will call `check_config` through the Docker MCP server and report the connection status.

## Troubleshooting

**`docker: command not found`** — Docker Desktop is not installed or not on PATH. Install
Docker Desktop and restart your terminal.

**`Error response from daemon: pull access denied`** — Image not yet published. Run
`docker pull ghcr.io/intersystems-community/iris-agentic-dev:latest` manually to check.

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
