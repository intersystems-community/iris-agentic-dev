# Next release notes (staging)

Baseline: v1.4.2, whose notes are final in `docs/release-notes/v1.4.2.md`. Nothing is
staged for the release after it.

I add an entry here as each user-facing change lands, so release notes are not
reconstructed from `git log` at tag time. At release time this drains into
`docs/release-notes/vX.Y.Z.md`, which is what `docs/release-checklist.md` step 4 gates on
and what becomes the GitHub release body via `gh release edit <tag> --notes`. There is no
`CHANGELOG.md` in this repo, so that release body is the changelog.

Rules for anything written here:

- Keep the three sections below, in this order, and end the drained file with a
  `vX.Y.Z...vA.B.C` compare link.
- Internal-only work (refactors, test infrastructure, CI) stays out unless a caller can
  observe the difference.
- Name the tool, flag, or file and give the number. "Improved performance" is not an entry;
  "suite went from 254 s to 61 s" is.
- Run `/no-ai-slop` detect on the drained file and fix every finding before publishing.
- First person singular. This is a one-person project.

## What's new

Nothing staged.

## Notable fixes

### The CLI handed out WebSocket tokens nothing could use

`iris-agentic-dev tool iris_ws_open` opened a session, printed a token, and exited. The token was a
handle into a pool of live sockets held in that process's memory, so the next command answered
`SESSION_STALE: Session token references an unknown server or expired session` — a timeout that had
not happened, against a server that was fine. Nothing in README or `docs/` said otherwise, and
nothing said what to do instead.

`tool` refuses `iris_ws_open`, `iris_ws_exec` and `iris_ws_close` now, before it resolves a
connection, and the refusal carries the `batch` script that works:

```bash
echo '[{"tool":"iris_ws_open","args":{}},
       {"tool":"iris_ws_exec","args":{"session":"{{0.session}}","code":"Set x=1"}},
       {"tool":"iris_ws_exec","args":{"session":"{{0.session}}","code":"Write x"}},
       {"tool":"iris_ws_close","args":{"session":"{{0.session}}"}}]' | iris-agentic-dev batch
```

`batch` has been there since 1.3.0 and appeared in no documentation at all — `grep batch README.md
docs/*.md` found nothing. It is in the README command list, the troubleshooting subcommand table,
and a troubleshooting section of its own now, and `docs/tools.md` explains the process boundary
where the session tools are documented. `iris_get_log` and `iris_doc`'s checkout prompt keep state
the same per-process way and need `batch` for the same reason.

`SESSION_STALE` itself is written for whoever is reading it: the CLI text names the process
boundary and the fix, the MCP text names a closed session or a restarted server, since an MCP client
has one process for the whole conversation and no CLI to run. Both call sites read one function, so
the two cannot drift.

`docs/tools.md` also promised that closing an already-closed session returns `already_closed: true`.
The handler has never returned that — it returns `SESSION_STALE`, because by then there is no
session to look up. The doc says so now.

### `iris_servers` reports the URL it actually calls

Every entry now carries `base_url`, prefix included. `docs/tools.md` has said "every entry carries
`base_url`, the URL iad actually calls" since 1.4.2 and step 5 of the 116 plan called for it, but the
listing never emitted the key — two instances behind one gateway still came back as the same host and
port with nothing to tell them apart. It is the same shape as the two CLI examples 1.4.2 fixed:
documented behaviour that no test asked for.

## Breaking changes

`iris-agentic-dev tool iris_ws_open` exits 1 instead of printing a session token. Any script that
read the token out of it was already broken — the very next call it made returned `SESSION_STALE`.
Under MCP the three tools are unchanged.
