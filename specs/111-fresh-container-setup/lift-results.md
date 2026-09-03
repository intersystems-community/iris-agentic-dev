# Lift Results: 099-fresh-container-setup

## Scope

`iris_admin` actions: `clear_password_change_flag`, `unlock_user`, `fresh_container_setup`.

## GEPA Eval Harness Status

**Coverage gap — N/A with justification.**

The GEPA eval harness (`tools/gepa-optimizer/`) currently has no scenarios covering
`iris_admin` admin actions (fresh container setup, user unlock, password flag clearing).
All existing scenarios cover discovery, execution, and code navigation tools.

Constitution IX lift measurement is scoped here to tool description accuracy and action
routing (does the agent pick the right action for a fresh-container task). Since no harness
scenarios exist for these actions, formal A/B measurement could not be run.

## Known Coverage Gap

`iris_admin` admin actions are not represented in the GEPA eval harness scenarios. This
should be addressed before the next `iris_admin` change. Suggested scenario:

> "I have a fresh IRIS community container. Authentication is failing with a
> forced-password-change error. Use iris_admin to fix it."

Expected agent behavior: call `iris_admin action=fresh_container_setup` (not a manual
sequence of calls).

## Manual Verification

The tool description update in this spec corrects the stale `IRIS_ADMIN_TOOLS=1` wording
to `IRIS_WRITE_TOOLS_ENABLED=1` and adds the three new actions to the description with
parameter documentation. An agent reading the updated description can discover and use
all three actions without additional guidance.
