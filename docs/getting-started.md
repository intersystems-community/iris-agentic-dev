# Getting started

Fifteen minutes from nothing to Claude Code reading and writing ObjectScript on IRIS. This
assumes one IRIS running in Docker on your own machine and no prior IRIS experience. Six steps,
each one ending in output you can compare against what is printed here.

If you already run IRIS somewhere else, or more than one instance, or Enterprise IRIS behind a
Web Gateway, read [connecting.md](connecting.md) instead. Everything here still applies; only
step 3 changes.

Three things can be wrong: the IRIS instance, the connection, and Claude Code's config. Steps 1
to 5 rule out the first two, so when step 6 fails you already know which one you are looking at.
Another editor works the same way; only step 6 changes.

Every command below was run while writing this, against the container step 1 starts, and
`tests/e2e/test_getting_started.py` checks that each subcommand, tool and argument named here
still exists.

## Step 1 — Get an IRIS running

```bash
docker run -d --name iris-quickstart -p 127.0.0.1:1972:1972 -p 127.0.0.1:52773:52773 \
  intersystemsdc/iris-community:latest
```

The `127.0.0.1:` in front of each port keeps the instance reachable from this machine only. Without
it Docker publishes on every network interface, and on Linux its own firewall rules sit ahead of
ufw and firewalld. Drop the prefix only if you want other machines to log in.

That image is on Docker Hub and needs no login. Give it a minute, then:

```bash
docker ps --filter name=iris-quickstart
```

The default password starts out expired. Every HTTP call gets back `401 Unauthorized` until you
change it, and nothing in the 401 says that is why. Give `_SYSTEM` a new password. Do not clear the
expiry on every account: that leaves `SuperUser`, `Admin` and `CSPSystem` with the published
default `SYS`.

```bash
PW="$(openssl rand -base64 18)"
printf 'Set p("Password")="%s",p("ChangePassword")=0\nSet sc=##class(Security.Users).Modify("_SYSTEM",.p)\nWrite $Select(sc:"password changed",1:$System.Status.GetErrorText(sc)),!\nHalt\n' "$PW" \
  | docker exec -i iris-quickstart iris session IRIS -U %SYS
echo "_SYSTEM password: $PW"
```

It prints `password changed` when it works, and the password goes in on stdin so it never appears
in a process list. Copy the password somewhere you will find it: the container does not store it
in a form you can read back. If the command prints `Sign-on and JOB inhibited`, IRIS is still
starting; wait ten seconds and run it again.

## Step 2 — Install `iris-agentic-dev`, and prove it runs

```bash
brew tap intersystems-community/tap
brew install iris-agentic-dev
iris-agentic-dev --version
```

```text
iris-agentic-dev 1.4.2
```

Linux, direct download and Windows are in the README's [Install](../README.md#install) section.
One binary, no runtime, no Python or Node.

Now list the tools:

```bash
iris-agentic-dev tool --list
```

```text
agent_history                  Return recent tool call history for this session.
agent_stats                    Return learning agent status: skill count, pattern count, KB size.
capability_matrix              Show the roles assigned to a user on an IRIS instance.
check_config                   Return the active IRIS connection state without making any IRIS network calls.
...
```

Eighty-one lines. This reads the tool router and opens no connection, so it works with IRIS
down, with the wrong port, with no config file. If it prints 81 tools the binary is installed
correctly, and everything that fails after this is a connection problem, not an install problem.

To see what one tool takes:

```bash
iris-agentic-dev tool iris_query --schema
```

That prints the description and the full JSON Schema: every parameter, its type, its default.
Free too, and still no connection.

## Step 3 — Point it at IRIS, and check what it aimed at

Five environment variables, matching the container from step 1:

```bash
export IRIS_HOST=localhost
export IRIS_WEB_PORT=52773
export IRIS_USERNAME=_SYSTEM
export IRIS_PASSWORD='<the _SYSTEM password from step 1>'
export IRIS_NAMESPACE=USER
```

`IRIS_WEB_PORT` is the web port, not the `1972` you may have seen in JDBC strings. These tools
speak HTTP to IRIS, so `52773` is the one that matters.

Check where it is pointed before running anything that writes:

```bash
iris-agentic-dev tool check_config
```

```json
{
  "connected": true,
  "connection_source": "env_vars",
  "host": "localhost",
  "port": 52773,
  "namespace": "USER",
  "iris_version": "IRIS for UNIX ... 2026.1 (Build 234U) ...",
  "write_tools_enabled": true,
  "destructive_tools_enabled": false
}
```

Two fields to read. `connection_source` says where the target came from, and `env_vars` means it
used what you just exported. `iris_version` is only filled in if IRIS answered, so
`connected: true` with `iris_version: null` means the port is open and your credentials were
refused, which is what you get if you skipped the password step above.

With no environment variables and no config file, the binary goes looking instead: a named
`IRIS_CONTAINER`, a scan of localhost ports, running Docker containers, then VS Code settings and
Server Manager. The port scan sends your credentials only to a listener that has shown it is IRIS.
With one IRIS running that usually lands on the right one and says so in `connection_source`. With several, it picks one.
Pin the target in a file before that matters:

```bash
iris-agentic-dev init        # writes ./.iris-agentic-dev.toml
```

Set `container` or `host`/`web_port` in it and commit it; credentials stay in the environment,
not in the file. A file like this is a project file: its connection settings work at once, but a
setting that widens what the agent may do (`write_tools_enabled = true`, `tls_verify = false`) is
set aside until you run `iris-agentic-dev trust`, and `check_config` says so. The full precedence
order and every field are in [connecting.md](connecting.md).

## Step 4 — Run a query

```bash
iris-agentic-dev tool iris_query \
  --args '{"query":"SELECT TOP 3 Name FROM %Dictionary.ClassDefinition WHERE Name %STARTSWITH '\''%SYSTEM.OBJ'\''"}'
```

```json
{
  "count": 2,
  "namespace": "USER",
  "rows": [{ "Name": "%SYSTEM.OBJ" }, { "Name": "%SYSTEM.OBJ.FM2Class" }],
  "success": true
}
```

That table is IRIS describing its own classes, which is a decent first thing to poke at if
ObjectScript is new to you. The whole loop ran there: your terminal, HTTP, Atelier, IRIS, and
back, with no MCP server and no editor in it.

## Step 5 — Three more things worth doing from the terminal

Read a class out of IRIS:

```bash
iris-agentic-dev tool iris_doc --args '{"mode":"get","name":"%SYSTEM.OBJ.cls"}'
```

Run ObjectScript:

```bash
iris-agentic-dev tool iris_execute --args '{"code":"Write $ZVERSION"}'
```

```json
{
  "output": "IRIS for UNIX ... 2026.1 (Build 234U) ...",
  "namespace": "USER",
  "execution_path": "atelier",
  "identity": {
    "user": "_SYSTEM",
    "source": "primary",
    "path": "atelier",
    "verified": true,
    "server": "default"
  },
  "auth_user": "_SYSTEM",
  "success": true
}
```

See what namespaces exist:

```bash
iris-agentic-dev tool iris_namespace_list
```

```json
{ "count": 2, "namespaces": ["%SYS", "USER"], "success": true }
```

A fresh container has those two. `USER` is yours to experiment in; leave `%SYS` alone until you
know why you are in it.

All 81 tools work this way and [tools.md](tools.md) is the catalogue.

## Step 6 — Hand it to Claude Code

One command, no config file to edit by hand:

```bash
claude mcp add iris-agentic-dev --scope user \
  -e IRIS_HOST=localhost -e IRIS_WEB_PORT=52773 \
  -e IRIS_USERNAME=_SYSTEM -e IRIS_PASSWORD="$IRIS_PASSWORD" -e IRIS_NAMESPACE=USER \
  -- iris-agentic-dev mcp
```

```text
Added stdio MCP server iris-agentic-dev with command: iris-agentic-dev mcp to user config
```

The `-e` flags repeat step 3 because Claude Code starts the server itself, with a fresh
environment. It does not inherit the `export` lines from your shell.

`--scope user` makes the server available in every directory you run `claude` in.
`--scope project` writes a `.mcp.json` in the current directory instead, which you can commit for
a team; that one reads `⏸ Pending approval` until someone opens Claude Code there and accepts it.

Confirm the server starts before opening a session:

```bash
claude mcp list
```

```text
iris-agentic-dev: iris-agentic-dev mcp - ✔ Connected
```

Now run `claude` and ask for something only IRIS can answer:

```text
Use iris_namespace_list and tell me what namespaces this instance has.
```

You see the tool call, then `%SYS` and `USER`, the same two as step 5. If the answer comes back
with no tool call in it, add "do not answer from memory". The model knows what an IRIS namespace
is and will guess.

Past that you can stop naming tools and describe the job: "read %SYSTEM.OBJ out of IRIS and tell
me what Compile does with its qspec argument". Claude Code picks the tool, and
[tools.md](tools.md) is there for when you want to name one anyway.

**If Claude Code shows no iad tools**, test the server on its own. This is the same handshake
Claude Code performs:

```bash
printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' \
  | iris-agentic-dev mcp | head -2
```

Two JSON-RPC responses: `serverInfo` naming the version, then 81 tools. If that works, the
problem is the command path rather than the binary. A GUI-launched editor does not inherit your
shell `PATH`, so pass the absolute path from `which iris-agentic-dev`.

**Other editors.** OpenCode, Cursor and VS Code + Copilot each want a different config shape.
They are in the README's three Quick start sections, and Cursor has its own walkthrough in
[cursor-quickstart.md](cursor-quickstart.md).

**Skills are separate.** Installing the binary installs no skills, and installing skills needs
no binary at runtime:

```bash
iris-agentic-dev skill install
iris-agentic-dev skill list
```

```text
SKILL                        CLAUDE CODE      OPENCODE     COPILOT
objectscript-review          installed        installed    n/a
objectscript-guardrails      installed        installed    n/a
...
```

On a graded holdout the skills are so far measured at nothing. The numbers are in the README's
[Skills](../README.md#skills--improve-ai-output-for-objectscript) section; read it before
deciding to install them.

## When it does not work

The errors you are most likely to hit first, verbatim.

**401 Unauthorized on everything.** Almost always the expired password from step 1. Run step 1's
password command and try again. To see whether the problem is the network or the
credentials, ask for the two answers separately. This tool takes the target explicitly rather
than reading your environment:

```bash
iris-agentic-dev tool iris_test_server \
  --args '{"host":"localhost","web_port":52773,"username":"_SYSTEM","password":"<password>"}'
```

```json
{
  "reachable": true,
  "auth": false,
  "error": "Authentication failed (HTTP 401)",
  "latency_ms": 5
}
```

`reachable: true` with `auth: false` is a credentials problem. Unreachable is a host, port or
prefix problem. They get fixed in different places.

The same 401 reaching you through a different tool looks worse than it is:

```json
{
  "error": "HTTP 401 Unauthorized",
  "error_code": "IRIS_UNREACHABLE",
  "hint": "Check IRIS_HOST and IRIS_WEB_PORT (and IRIS_WEB_PREFIX if using a non-root gateway)",
  "attempted_url": "http://localhost:52773/api/atelier/v1/USER/action/query"
}
```

Trust the `error` line and ignore the `hint`: 401 is your credentials, and the host and port in
`attempted_url` were both fine. The code and hint are wrong for this case and are being fixed.

**Wrong field name.** The message lists the fields that exist, which is faster than the docs:

```text
error: bad params for iris_query: unknown field `sql`, expected one of `query`, `parameters`,
`namespace`, `force`, `confirm`, `mode`, `table`, `max_rows_affected`, `server`
```

**Wrong tool name.** Same idea, and it prints the catalogue rather than guessing:

```text
error: unknown tool 'iris_quer'
available tools:
  agent_history
  ...
```

**Wrong port, or IRIS down.** This one is ugly, and the shape is the tell rather than the text:

```text
error: ErrorData { code: ErrorCode(-32603), message: "HTTP error: error sending request for
url (http://localhost:59999/api/atelier/v1/USER/action/query)", data: None }
```

Nothing is listening at that URL. Check the port in the message against `docker ps`. A raw
struct like this instead of a JSON error means the failure happened below the layer that formats
errors properly.

**No iad tools in Claude Code.** Run `claude mcp list`. Anything other than `✔ Connected` is the
server failing to start, so run the step 6 handshake next; if that prints 81 tools, the binary is
fine and the registered command path is wrong.

The symptom table in [troubleshooting.md](troubleshooting.md) covers the rest, including
`SESSION_STALE`, Web Gateway prefixes, and Windows Server Manager credentials.

## Where to go next

| You want to                                  | Read                                                    |
| -------------------------------------------- | ------------------------------------------------------- |
| Connect to something other than local Docker | [connecting.md](connecting.md)                          |
| Know what a tool does before calling it      | [tools.md](tools.md), or `tool <name> --schema`         |
| Understand the skill system                  | [skills.md](skills.md)                                  |
| Work out who did what to an instance         | [agent-attribution.md](agent-attribution.md)            |
| See whether any of this is measured to help  | [results.md](../specs/121-benchmark-program/results.md) |

When you are done with the container:

```bash
docker rm -f iris-quickstart
```
