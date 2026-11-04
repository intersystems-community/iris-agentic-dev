#!/bin/sh
# spec-gates hook stub -- installed into .git/hooks as pre-commit,
# pre-merge-commit and commit-msg by /speckit.gates.init (issue #59).
#
# .git/hooks is shared by every branch; the projected runtime is not. This
# stub runs the CHECKED-OUT branch's projected hook
# (.specify/gates/hooks/<name>), so the hook version always matches the
# branch's runtime and an upgrade takes effect without reinstalling hooks.
# Invoked through bash, so a projected hook that lost its execute bit (zip
# installs drop modes) still runs.

name="$(basename "$0")"
# `pre-commit install` moves a hook it finds to <name>.legacy and runs it
# from there on every call (#201); the gates hook is still <name>.
name="${name%.legacy}"
root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
    echo "gates: $name: cannot resolve the work tree -- hook not run" >&2
    exit 1
}
hook="$root/.specify/gates/hooks/$name"
# A branch projected before pre-merge-commit shipped (#148) has only the
# pre-commit hook, and its checks are the ones a merge commit needs.
if [ "$name" = "pre-merge-commit" ] && [ ! -f "$hook" ] \
    && [ -f "$root/.specify/gates/hooks/pre-commit" ]; then
    hook="$root/.specify/gates/hooks/pre-commit"
    GATES_GIT_HOOK=pre-merge-commit
    export GATES_GIT_HOOK
fi
if [ ! -f "$hook" ]; then
    # Adopted means git tracks something under .specify/gates, in HEAD or
    # in the index. The directory is no evidence: gitignored files
    # (attestations.jsonl, a runtime projected but never committed)
    # survive a switch to a branch from before adoption (issue #125).
    if git rev-parse -q --verify "HEAD:.specify/gates" >/dev/null 2>&1 \
        || [ -n "$(git ls-files -- ':(top).specify/gates' 2>/dev/null)" ]; then
        # The branch has a gates runtime but its hook is gone (every
        # release projects hooks here): deleting it must not silently
        # turn enforcement off. Fail closed.
        echo "gates: $name refused -- .specify/gates is tracked but $hook is missing" >&2
        echo "  Project it (bash .specify/extensions/gates/runtime/project.sh) rather than committing around it." >&2
        exit 1
    fi
    # A branch from before gates was adopted has nothing to enforce.
    echo "gates: $name skipped -- this branch has no gates runtime" >&2
    exit 0
fi
exec bash "$hook" "$@"
