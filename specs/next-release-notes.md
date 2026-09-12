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

Nothing staged.

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

## Breaking changes

None staged.
