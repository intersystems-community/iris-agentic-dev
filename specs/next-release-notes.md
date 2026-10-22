# Next release notes (staging)

Baseline: v1.6.2, whose notes are final in `docs/release-notes/v1.6.2.md`. What follows is
staged for the release after it.

I add an entry here as each user-facing change lands, so release notes are not
reconstructed from `git log` at tag time. At release time this drains into
`docs/release-notes/vX.Y.Z.md`, which is what `docs/release-checklist.md` step 4 gates on
and what becomes the GitHub release body via `gh release edit <tag> --notes`. The VS Code
extension's `CHANGELOG.md` is built from those files (#136).

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

- **`iris_symbols_local` reads real classes.** The lockfile held tree-sitter-objectscript 1.9.13,
  which left a parse error in 32.5% of 5,485 documents from a USER namespace and missed 22% of
  class members, mostly at member headers such as `[ InitialExpression = 0 ]`, `[ ZenMethod ]`
  and `[ CodeMode = generator ]`. 1.10.3 leaves one in 1.1%. JavaScript `ClientMethod`s are listed
  too.
- **`iris_test` and `iris_coverage` survive a growing `messages.log`.** `%UnitTest.Manager` checks
  `messages.log` during a run, and if it grew it closes the file without putting the caller's
  device back. Over HTTP that lost the run's output: `iris_test` answered `NO_TESTS_FOUND` for a
  test that had passed, whenever any process wrote to `messages.log` mid-run. Both tools now run
  the manager in a background job and read its output back.
- **A garbled answer from IRIS is an error.** iad read an Atelier answer that was not JSON, an
  HTTP error, or an error in Atelier's status as an empty success, so `iris_execute` could report
  success with no output. It now returns an error that shows the start of the answer.
- **An empty `IRIS_CONTAINER` means no container.** iad ran `docker exec ''`, and that error hid
  the reason the call had failed.
- **`objectscript-unit-test`** says not to run `%UnitTest.Manager` through `iris_execute`, and why.

## Breaking changes

- **`.iris-agentic-dev.toml` outranks `IRIS_*` at startup**, as `docs/connecting.md` has always
  said: a `--host` flag, then the workspace file, then the environment. Before, any `IRIS_HOST`
  overrode the file when the server started, while a live edit of the file won. If you set
  `IRIS_HOST` to reach a different server than the workspace file names, remove the file or pass
  `--host`. iris-interop-dev made the same change (their #434).
