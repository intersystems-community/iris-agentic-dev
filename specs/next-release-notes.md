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

### `iris_servers` reports the URL it actually calls

Every entry now carries `base_url`, prefix included. `docs/tools.md` has said "every entry carries
`base_url`, the URL iad actually calls" since 1.4.2 and step 5 of the 116 plan called for it, but the
listing never emitted the key — two instances behind one gateway still came back as the same host and
port with nothing to tell them apart. It is the same shape as the two CLI examples 1.4.2 fixed:
documented behaviour that no test asked for.

## Breaking changes

None staged.
