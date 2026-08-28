# Contract: Caller Marker (`User-Agent`)

**Feature**: 086-agent-attribution-audit | Covers FR-001 … FR-012

The marker is the externally observable interface of this feature. Operators grep for it in
access logs and match on it in gateway rules, so its grammar is a contract, not an
implementation detail.

## Grammar

```text
marker      = "iris-agentic-dev/" version SP "(" mode [ "; " label ] [ "; " client ] ")"
version     = <exact workspace version, e.g. 1.2.7>
mode        = "mcp" / "cli"
label       = <sanitized IRIS_AGENT_LABEL, 1-64 chars>
client      = <MCP client name> "/" <MCP client version>
```

## Examples

| Situation                                 | Value                                                            |
| ----------------------------------------- | ---------------------------------------------------------------- |
| MCP session, no label, no `clientInfo`    | `iris-agentic-dev/1.2.7 (mcp)`                                   |
| MCP session, label set, client identified | `iris-agentic-dev/1.2.7 (mcp; build-agent-7; claude-code/2.1.0)` |
| One-shot CLI dispatch, label set          | `iris-agentic-dev/1.2.7 (cli; nightly-ci)`                       |
| CLI dispatch, `IRIS_AGENT_LABEL=""`       | `iris-agentic-dev/1.2.7 (cli)`                                   |
| Label `"team a\r\nX-Evil: 1"`             | `iris-agentic-dev/1.2.7 (cli; team a X-Evil: 1)`                 |

The last row is the sanitizing contract: CR and LF become spaces and whitespace runs collapse,
so the injected header name survives as **text inside the label** and cannot split the request.
Dropping the label silently would hide the attempt; keeping it as text makes it visible in the
log. This is asserted by an existing unit test.

## Invariants

1. Every request this tool sends to IRIS carries the marker — the two connection clients, the
   four discovery clients, the search `sync_client`, the doc `batch_client`, the CSP-cookie
   client, and the WebSocket handshake (FR-001, FR-009, FR-010).
2. `HeaderValue::from_str(marker)` succeeds for every reachable input (FR-007).
3. No empty parentheses and no dangling `"; "` when the label or client is absent (FR-008).
4. The mode reflects process invocation only. No config key or environment variable can change
   it, and the first `set_caller_mode` wins (FR-004).
5. The label is capped at 64 characters, measured in `char_indices` so a multi-byte label is
   never cut mid-character.
6. Requests that are not IRIS-bound — LLM provider calls, registry and package downloads — do
   **not** carry the marker.

## Non-goals stated as contract

The marker is caller-asserted. Any client can send any `User-Agent`, so it is evidence for
auditing and a basis for default-deny-by-environment rules, **not** a security boundary against
a hostile caller. Enforcement remains distinct per-environment credentials and roles (FR-014).

## Observability

Server-side read-back, verified live:

```objectscript
Write %request.CgiEnvs("HTTP_USER_AGENT")
```

Access logs: IIS, Apache and the Web Gateway record `User-Agent`. The Private Web Server keeps
no access log at all (error log only), so on a PWS-only instance the marker is visible from
ObjectScript and from `%SYS.Audit` records but not from a log file.

## Transports that cannot carry it

`docker_only = true` connections execute through `docker exec … iris session` with the script
on stdin. No HTTP request exists, so no header exists. The connection warns once, naming the
transport and the consequence, rather than losing attribution silently (FR-011). This path is
mandatory for Enterprise 2026.2.0AI builds, which ship no PWS (DPP-1192).
