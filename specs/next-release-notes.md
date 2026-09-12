# Next release notes (staging)

Baseline: v1.4.1, whose notes are final in `docs/release-notes/v1.4.1.md`. Nothing is
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

- `iris_add_server` takes a `web_prefix` parameter, and `servers.json` stores it. An instance
  published under a path — `http://gateway:8080/hs20261/api/atelier/`, the usual HealthShare
  layout — could not be described in the registry at all, so registering one produced an entry
  pointing at the gateway root. VS Code Server Manager profiles already carried the prefix as
  `webServer.pathPrefix`, which iad read; the native registry was the gap. That spelling is
  accepted as an alias on the JSON key, so a block copied out of `settings.json` loads
  unchanged, and `iris_import_servers` now carries the prefix across. Slashes are optional and
  multi-segment prefixes work. Re-registering an existing name updates it in place and keeps
  the stored password, so you can add a forgotten prefix without re-entering the credential.
  A prefix carrying a scheme or a host is refused at registration rather than becoming a
  connection error later. Reported by @isc-ndittber (#129).

## Notable fixes

- `iris_ws_exec` now runs the same gates as `iris_execute`. It ran none of them: the WebSocket
  terminal accepted `Do $system.OBJ.Delete("MyApp.Foo")`, a write to `^oddDEF`, and
  `Kill ^SomeGlobal` — all three refused on `iris_execute` — because `dispatch_gate` step `[0]`
  branched on three literal tool names and the handler called the gate nowhere. The write tier
  was the only thing covering it, so anyone developing with `write_tools_enabled = true` had the
  whole policy surface reachable through a session. Reported by @devecchijr in
  [#137](https://github.com/intersystems-community/iris-agentic-dev/issues/137). The tool takes a
  `confirmed` parameter now, for the role gate, the same as `iris_execute`.

  The tool set step `[0]` scans is a declared constant (`CODE_EXEC_TOOLS`) rather than an
  if/else chain, and a test fails if a tool in the router has a `code` parameter and is not in
  it. That is what did not exist before: `iris_ws_exec` was the fourth tool to ship past this
  gate, after `iris_global` set/kill, `iris_lookup_manage` set/delete, and
  `iris_execute_method`.

- `iris_servers` reports each entry's `base_url` and, where there is one, its `web_prefix`.
  Two instances behind one gateway share a host and port, so the old listing gave you no way
  to tell them apart.
- `iris_test_server` and `iris_servers(probe: true)` probe the URL the server actually uses.
  Both rebuilt one from host and port, so a prefixed instance came back healthy on the
  strength of the gateway root answering while every real call went somewhere else.

## Breaking changes

None staged.
