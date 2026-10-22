---
name: iris-agentic-dev-setup
description: Install the iris-agentic-dev binary, get an IRIS instance running, and register the MCP server. Use when the IRIS tools are missing, when check_config reports disconnected, or when the user asks to set up, install, or connect iris-agentic-dev.
---

# Setting up iris-agentic-dev

Work through the steps in order. Each one ends in a command whose output decides the next step,
so run the command rather than assuming the state.

The goal state: `iris-agentic-dev` on PATH, one IRIS instance reachable and authenticating, an
MCP server registered against it, and a query returning rows.

## 1. Is the binary already here?

```bash
command -v iris-agentic-dev && iris-agentic-dev --version
```

Not found: step 2.

Found: compare its version with the latest release. The plugin's skills and hooks update with the
plugin, but the binary does not, so an older binary runs under newer skills that describe tools
it may not have:

```bash
curl -fsSL https://api.github.com/repos/intersystems-community/iris-agentic-dev/releases/latest \
  | sed -n 's/.*"tag_name": *"v\([^"]*\)".*/\1/p'
```

If the binary is older, tell the user and offer to upgrade it before going on. With Homebrew,
`brew upgrade iris-agentic-dev`; otherwise repeat step 2, writing the new binary over the old one
at the path `command -v iris-agentic-dev` printed. On Windows a running `.exe` cannot be
overwritten: close Claude Code and any other client using it first, or rename the old file and put
the new one in its place. Ask before upgrading; do not do it unprompted. If it is current, or the
user declines, skip to step 3.

## 2. Install the binary

Mac, Homebrew:

```bash
brew tap intersystems-community/tap
brew install iris-agentic-dev
```

The tap is `intersystems-community/tap`, which Homebrew resolves to the
`intersystems-community/homebrew-tap` repository. There is no per-project tap.

Without Homebrew, download the release binary for this machine. There is one per architecture
and they are not interchangeable: the wrong one gets as far as `chmod` and then fails with
`cannot execute binary file`, or on an Intel Mac with `Bad CPU type in executable`. So read the
architecture rather than picking a line:

```bash
case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) ASSET=iris-agentic-dev-macos-arm64 ;;
  Darwin-x86_64) ASSET=iris-agentic-dev-macos-x86_64 ;;
  Linux-x86_64) ASSET=iris-agentic-dev-linux-x86_64 ;;
  Linux-aarch64) ASSET=iris-agentic-dev-linux-aarch64 ;;
  *) echo "no published binary for $(uname -s)-$(uname -m); build from source" >&2 ;;
esac
curl -fsSL "https://github.com/intersystems-community/iris-agentic-dev/releases/latest/download/$ASSET" \
  -o /usr/local/bin/iris-agentic-dev && chmod +x /usr/local/bin/iris-agentic-dev
```

On a Mac, clear the download quarantine flag as well, or the first run is killed by Gatekeeper:

```bash
xattr -d com.apple.quarantine /usr/local/bin/iris-agentic-dev 2>/dev/null
```

If `/usr/local/bin` is not writable, put the binary in any directory on PATH and use that path
in step 5. An asset name that does not exist shows up as `curl: (56) ... error: 404` and writes
no file, so the `&&` stops before `chmod`. Either way, check that
`iris-agentic-dev --version` prints a version before moving on.

Windows: download `iris-agentic-dev-windows-x86_64.exe` from the
[releases page](https://github.com/intersystems-community/iris-agentic-dev/releases/latest) and
put it somewhere on PATH. The rest of this skill's Docker steps assume a Unix shell; on Windows
run them from PowerShell with Docker Desktop and adjust quoting.

Then note the absolute path, because step 5 needs it:

```bash
command -v iris-agentic-dev
```

## 3. Find an IRIS instance before starting one

Most developers already have one running. Starting a second wastes several minutes and then
confuses auto-discovery.

```bash
docker ps --filter expose=1972 --format '{{.Names}}\t{{.Image}}\t{{.Ports}}'
```

Every row is a candidate. If there is exactly one, use it and go to step 4. If there are
several, ask the user which one before touching any of them: instances belong to projects, and
writing to the wrong one is not something you can undo. If there are none, start one:

```bash
docker run -d --name iris-quickstart -p 1972:1972 -p 52773:52773 \
  intersystemsdc/iris-community:latest
```

Change the host side of both `-p` mappings if those ports are taken.

This image has no Docker healthcheck, so `docker inspect` reports an empty health status
forever. Wait on the REST endpoint instead, which is the thing that has to work:

```bash
until curl -sf -o /dev/null http://localhost:52773/api/atelier/ \
  || [ "$(curl -s -o /dev/null -w '%{http_code}' http://localhost:52773/api/atelier/)" = "401" ]; do
  sleep 5
done
```

A 401 means IRIS is up and asking for credentials, which is exactly where step 4 starts. First
boot takes about a minute.

## 4. Clear the expired password and confirm the port

A fresh community container ships `_SYSTEM` / `SYS` already expired. Every REST call returns
401 until that is cleared, and nothing in the 401 says so.

```bash
docker exec iris-quickstart iris session IRIS -U '%SYS' \
  '##class(Security.Users).UnExpireUserPasswords("*")'
```

Do not assume the web port is 52773. Read the mapping off the container, using the container
name from step 3:

```bash
docker port iris-quickstart 52773
```

The number after the colon is the web port for every step below. Anything else you may have
seen, including 1972, is the superserver port and will not answer REST.

Confirm both at once, substituting the port you just read:

```bash
curl -s -u _SYSTEM:SYS -o /dev/null -w '%{http_code}\n' http://localhost:52773/api/atelier/
```

`200` means ready. `401` means step 4's password command has not taken effect; re-run it and
check its exit status. Anything else, or no response, means IRIS is not serving yet.

## 5. Register the MCP server

Use the absolute path from step 2. A bare name works only if the host shell's PATH includes it,
which is not the same PATH your terminal has.

```bash
claude mcp add iris-dev /usr/local/bin/iris-agentic-dev mcp
```

If IRIS is not on the default port, or not at `_SYSTEM` / `SYS`, pin it rather than leaving it
to discovery:

```bash
claude mcp add iris-dev /usr/local/bin/iris-agentic-dev mcp \
  --env IRIS_WEB_PORT=52773 --env IRIS_CONTAINER=iris-quickstart
```

A per-project alternative is a `.iris-agentic-dev.toml` in the workspace, which the server
re-reads without a restart:

```bash
iris-agentic-dev init
```

Installing the plugin instead of registering by hand gets the server and all the ObjectScript
skills in one step, which is the better route when the user has not already chosen one:

```bash
claude plugin marketplace add intersystems-community/iris-agentic-dev
claude plugin install iris-dev@iris-agentic-dev
```

Restart the session after either route, so the host picks up the new server.

## 6. Verify against the instance you meant

```bash
iris-agentic-dev tool check_config --json
```

Read `connected`, `port` and `container`, and check the last two against what step 3 or 4 gave
you. `connected: true` on its own does not prove you reached the right instance.

A pin is advisory. `--container`, `--web-port` and their `IRIS_*` environment equivalents are
honoured while the target they name answers; the moment it does not, for a dead port or an
expired password alike, discovery carries on looking and settles on whatever else on the machine
does answer. It then reports `connected: true` against a database nobody asked about. Measured:
`check_config --container iris-nope --web-port 59999` came back `connected: true` on another
container entirely. The two fields it prints are honest about where it landed, which is why
reading them is the check, and `connected` on its own is not.

Then prove it can read:

```bash
iris-agentic-dev query "SELECT COUNT(*) AS Classes FROM %Dictionary.ClassDefinition"
```

A few thousand rows is a stock instance. That is setup finished.

To add the ObjectScript skills to a host that is not using the plugin:

```bash
iris-agentic-dev skill install
```

## When it does not connect

Probe the server directly. This never touches the active connection, and it separates the two
failures that look identical from the outside:

```bash
iris-agentic-dev tool iris_test_server --args '{"host":"localhost","web_port":52773,"username":"_SYSTEM","password":"SYS"}'
```

Read the two flags independently:

- `reachable: true, auth: true` — the instance is fine, so the problem is which connection the
  server resolved. Go back to step 5 and pin the port and container.
- `reachable: true, auth: false` with `Authentication failed (HTTP 401)` — TCP and HTTP both
  worked and the credentials were refused. On a fresh container the first cause to check is the
  expired default password from step 4, not a typo. The 401 is byte-for-byte identical whether
  the password is wrong or merely expired, so clear the expiry before you start doubting the
  password.
- `reachable: false` — nothing answered. Check that the container is up
  (`docker ps --filter name=iris-quickstart`), and that you are using the web port from
  `docker port` rather than 1972.

Pass `username` and `password` explicitly on this probe. Omitted, they are not filled in from
the active configuration, and a perfectly healthy server comes back `auth: false`, which sends
you chasing a credential problem that does not exist.

One error code lies: a 401 during connection setup can surface as `IRIS_UNREACHABLE`. The
instance is reachable; the credentials were rejected. Treat `IRIS_UNREACHABLE` alongside an
HTTP 401 as an authentication failure and work step 4, not the network.

Two more things that look like connection failures:

- **Enterprise 2026.2 builds have no private web server** (DPP-1192), so there is no REST
  endpoint to reach on any port. Those need a WebGateway in front, or Docker-only mode. See
  `skills/skills/iris-agentic-dev/nopws-setup/SKILL.md`.
- **Writes refused with the connection fine.** The write gate is separate from connectivity.
  `check_config`'s `write_tools_enabled` and `write_tools_source` say what is off and what
  decided it.
