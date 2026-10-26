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

Without Homebrew, download the release binary for this machine and check it against the
release's `SHA256SUMS` before installing it. There is one binary per architecture and they are
not interchangeable: the wrong one installs and then fails with `cannot execute binary file`,
or on an Intel Mac with `Bad CPU type in executable`. So read the architecture rather than
picking a line:

```bash
case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) ASSET=iris-agentic-dev-macos-arm64 ;;
  Darwin-x86_64) ASSET=iris-agentic-dev-macos-x86_64 ;;
  Linux-x86_64) ASSET=iris-agentic-dev-linux-x86_64 ;;
  Linux-aarch64) ASSET=iris-agentic-dev-linux-aarch64 ;;
  *) echo "no published binary for $(uname -s)-$(uname -m); build from source" >&2 ;;
esac
VERSION=1.7.0
base="https://github.com/intersystems-community/iris-agentic-dev/releases/download/v$VERSION"
DL="$(mktemp -d)"
curl -fsSL "$base/$ASSET" -o "$DL/$ASSET" &&
  curl -fsSL "$base/SHA256SUMS" -o "$DL/SHA256SUMS" &&
  (cd "$DL" && if command -v sha256sum >/dev/null; then sha256sum -c --ignore-missing SHA256SUMS;
     else shasum -a 256 -c --ignore-missing SHA256SUMS; fi) &&
  install -m 0755 "$DL/$ASSET" /usr/local/bin/iris-agentic-dev
```

`VERSION` is the release this skill ships with. When step 1 found a newer release and the user
agreed to upgrade, set `VERSION` to that release instead; the checksum file comes from the same
tag. The check prints `<asset>: OK` on success. If it prints `FAILED` or anything else, stop,
show the user the output, and do not install the file by other means.

On a Mac, once the check has passed, clear the download quarantine flag if one is set. The
binary has only an ad-hoc signature, so Gatekeeper kills the first run while the flag is there:

```bash
xattr -d com.apple.quarantine /usr/local/bin/iris-agentic-dev 2>/dev/null
```

If `/usr/local/bin` is not writable, install into any directory on PATH and use that path
in step 5. An asset name that does not exist shows up as `curl: (56) ... error: 404` and writes
no file, so the `&&` chain stops before the check. Either way, check that
`iris-agentic-dev --version` prints a version before moving on.

Windows: download `iris-agentic-dev-windows-x86_64.exe` and `SHA256SUMS` from the
[releases page](https://github.com/intersystems-community/iris-agentic-dev/releases), compare
`(Get-FileHash .\iris-agentic-dev-windows-x86_64.exe).Hash` with that file's line in
`SHA256SUMS`, and put it somewhere on PATH. The rest of this skill's Docker steps assume a Unix
shell; on Windows run them from PowerShell with Docker Desktop and adjust quoting.

Then note the absolute path, because step 5 needs it:

```bash
command -v iris-agentic-dev
```

## 3. Find an IRIS instance before starting one

Most developers already have one running. Starting a second wastes several minutes and then
confuses auto-discovery. But a container that already exists is the user's, with their work in it,
so finding one is a reason to ask, not a reason to change anything.

```bash
docker ps --filter expose=1972 --format '{{.Names}}\t{{.Image}}\t{{.Ports}}'
```

Every row is a candidate.

- **Exactly one.** Tell the user which container you found and ask for the credentials of that
  instance. Do not run step 4 on it. If they say its password is expired, ask before changing it,
  say that the account that changes is `_SYSTEM` and no other, and only then run step 4's command
  with that container's name in place of `iris-quickstart`.
- **Several.** Ask the user which one before touching any of them: instances belong to projects,
  and writing to the wrong one is not something you can undo.
- **Never** stop, recreate or re-publish an existing container, even when `docker ps` shows it
  bound to `0.0.0.0`. Tell the user about that binding and leave it.
- **None.** Start one. This is the only case where step 4 runs without asking, because a
  container you just started holds nothing yet:

```bash
docker run -d --name iris-quickstart -p 127.0.0.1:1972:1972 -p 127.0.0.1:52773:52773 \
  intersystemsdc/iris-community:latest
```

The `127.0.0.1:` in front of each port decides who can reach the instance: only this machine.
Without it Docker publishes on every interface, and on Linux its own firewall rules sit ahead of
ufw and firewalld, so having a firewall does not help. Remove the prefix only if the user wants
other machines on the network to log in. Change the host side of both mappings if those ports
are taken.

This image has no Docker healthcheck, so `docker inspect` reports an empty health status
forever. Wait on the REST endpoint instead, which is the thing that has to work:

```bash
until curl -sf -o /dev/null http://localhost:52773/api/atelier/ \
  || [ "$(curl -s -o /dev/null -w '%{http_code}' http://localhost:52773/api/atelier/)" = "401" ]; do
  sleep 5
done
```

A 401 means IRIS is up and asking for credentials, which is exactly where step 4 starts. First
boot takes about a minute. The web server answers a little before the instance accepts a
session, so if step 4 prints `Sign-on and JOB inhibited`, wait ten seconds and run it again.

## 4. Set a new `_SYSTEM` password and confirm the port

A fresh community container ships `_SYSTEM` / `SYS` already expired. Every REST call returns
401 until that is dealt with, and nothing in the 401 says so. The fix is to give `_SYSTEM` a new
password. Do not clear the expiry on every account: that leaves `SuperUser`, `Admin` and
`CSPSystem` with a password that is published in the documentation.

This step applies only to the container step 3 started. Never run it on a container this skill
did not start; step 3 says what to do with one.

Generate the password on the host and send the ObjectScript on `docker exec -i` stdin, so the
password is never on a command line, in `docker inspect` or in a file:

```bash
PW="$(openssl rand -base64 18)"
printf 'Set p("Password")="%s",p("ChangePassword")=0\nSet sc=##class(Security.Users).Modify("_SYSTEM",.p)\nWrite $Select(sc:"password changed",1:$System.Status.GetErrorText(sc)),!\nHalt\n' "$PW" \
  | docker exec -i iris-quickstart iris session IRIS -U %SYS
echo "_SYSTEM password: $PW"
```

`password changed` in the output is the success line. Never send the password through iad: do
not hand it to `iris_admin`, `iris_credential_manage` or any other tool to set. Those calls need a working
login, which an expired password does not give, and an iad older than the 1.7.0 fix would carry
the password in a scratch class's source. Tell the user the password once, in your reply, because
nothing stores it for them. It passes through this session that one time.

Starting the container with `-e IRIS_PASSWORD=<password>` also sets `_SYSTEM`'s password on the `intersystemsdc` images, but the value then sits in `docker inspect` and in the `docker run` command line, so this skill does not use it.

Do not assume the web port is 52773. Read the mapping off the container, and read the address
too:

```bash
docker port iris-quickstart
```

Each line should read `-> 127.0.0.1:<port>`. The number after the colon beside `52773/tcp` is the
web port for every step below. Anything else you may have seen, including 1972, is the
superserver port and will not answer REST. An address of `0.0.0.0` means the `docker run` in
step 3 lost its prefix.

Confirm both at once, substituting the port you just read. `curl -K -` reads the credential from
stdin, so the password stays off the process list:

```bash
printf 'user = "_SYSTEM:%s"\n' "$PW" \
  | curl -s -K - -o /dev/null -w '%{http_code}\n' http://localhost:52773/api/atelier/
```

`200` means ready. `401` means the password command has not taken effect; re-run it and read its
output. Anything else, or no response, means IRIS is not serving yet. Shell variables do not
survive between separate tool calls, so run the blocks that use `$PW` in one call, or set `PW=`
again from the value you gave the user.

## 5. Register the MCP server

Use the absolute path from step 2. A bare name works only if the host shell's PATH includes it,
which is not the same PATH your terminal has.

The password goes into the registration's launch settings, which the host keeps in the user's own
configuration. It does not go in a workspace file: `.iris-agentic-dev.toml` sits in a project
directory that gets committed and shared.

```bash
claude mcp add iris-dev /usr/local/bin/iris-agentic-dev mcp \
  --env IRIS_USERNAME=_SYSTEM --env IRIS_PASSWORD="$PW"
```

If IRIS is not on the default port, pin it rather than leaving it to discovery:

```bash
claude mcp add iris-dev /usr/local/bin/iris-agentic-dev mcp \
  --env IRIS_USERNAME=_SYSTEM --env IRIS_PASSWORD="$PW" \
  --env IRIS_WEB_PORT=52773 --env IRIS_CONTAINER=iris-quickstart
```

A per-project alternative is a `.iris-agentic-dev.toml` in the workspace, which the server
re-reads without a restart. It names the container and the ports and nothing secret:

```bash
iris-agentic-dev init
```

Installing the plugin instead of registering by hand gets the server and all the ObjectScript
skills in one step, which is the better route when the user has not already chosen one. Then
give the server the password through its launch settings in the same way:

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
iris-agentic-dev tool iris_test_server --args '{"host":"localhost","web_port":52773,"username":"_SYSTEM","password":"<password>"}'
```

Read the two flags independently:

- `reachable: true, auth: true` — the instance is fine, so the problem is which connection the
  server resolved. Go back to step 5 and pin the port and container.
- `reachable: true, auth: false` with `Authentication failed (HTTP 401)` — TCP and HTTP both
  worked and the credentials were refused. On a fresh container the first cause to check is the
  expired default password, not a typo. The 401 is byte-for-byte identical whether the password
  is wrong or merely expired, so confirm that step 4 ran before you start doubting the
  password.
- `reachable: false` — nothing answered. Check that the container is up
  (`docker ps --filter name=iris-quickstart`), and that you are using the web port from
  `docker port` rather than 1972.

Replace `<password>` with the one step 4 generated. Pass `username` and `password` explicitly on
this probe. Omitted, they are not filled in from
the active configuration, and a perfectly healthy server comes back `auth: false`, which sends
you chasing a credential problem that does not exist.

One error code lies: a 401 during connection setup can surface as `IRIS_UNREACHABLE`. The
instance is reachable; the credentials were rejected. Treat `IRIS_UNREACHABLE` alongside an
HTTP 401 as an authentication failure and work the credentials (step 4), not the network.

Two more things that look like connection failures:

- **Enterprise 2026.2 builds have no private web server** (DPP-1192), so there is no REST
  endpoint to reach on any port. Those need a WebGateway in front, or Docker-only mode. See
  `skills/skills/iris-agentic-dev/nopws-setup/SKILL.md`.
- **Writes refused with the connection fine.** The write gate is separate from connectivity.
  `check_config`'s `write_tools_enabled` and `write_tools_source` say what is off and what
  decided it.
