# 087 — iris_execute gate bypass

## Status

Draft — unimplemented.

## Problem

`iris_execute` accepts arbitrary ObjectScript and runs it without checking the write or
destructive gates. A model (or any caller) can bypass `iris_global`'s ☠ destructive gate
by routing a `Kill` through `iris_execute`:

```text
iris_execute(code="Kill ^SomeGlobal")          # bypasses destructive gate
iris_execute(code="Set ^SomeGlobal(1)=1")      # bypasses write gate
```

This was discovered during e2e test authorship: a test cleanup step used `iris_global`
kill (correctly ☠-gated), which failed in CI because `destructive_tools_enabled` was not
set. The fix attempted was to route cleanup through `iris_execute` instead — which worked,
exposing the bypass.

The code-edit guard blocks `iris_execute` from editing compiled code (`%Dictionary.*`,
`$system.OBJ`, `^rOBJ`, etc.), but it does not block arbitrary global reads, writes, or
kills. The gate system's two tiers (`write_tools_enabled`, `destructive_tools_enabled`)
are only checked in `call_tool` dispatch for tools that declare themselves write- or
destructive-gated. `iris_execute` is not declared as either, so it skips both checks
entirely.

## Scope

`iris_execute` must enforce the write gate for any code that sets or kills a global, and
the destructive gate for any code that kills a global. The PHI and system-globals checks
that apply to `iris_global` must apply equally to `iris_execute` global operations.

This is not about blocking legitimate ObjectScript work — `Set` to a local variable,
`Write`, `Do ##class(...)` are all fine. The gate applies only when the code touches
globals that would be blocked if called via `iris_global`.

## Out of scope

- `iris_ws_exec` (WebSocket terminal) — separate analysis needed; same class of problem.
- Blocking `KILL` of local variables — not a data-safety concern.

## Design notes (to be resolved in plan)

Three options for where to enforce:

1. **Static analysis before execution** — scan the code string for `Set ^`, `Kill ^`,
   `Merge ^` patterns before sending to IRIS. Catches the obvious literal cases only.
   Does NOT catch:
   - `Kill @variable` — indirect global reference via `@` sigil
   - `Set code = "Kill ^Foo"  Xecute code` — double indirection via `Xecute`
   - `Do ##class(MyApp.Util).Cleanup()` — method call that internally kills a global
   - `&sql(DELETE FROM MyApp.Patient WHERE 1=1)` — SQL DML, no `^` visible
   - `$XECUTE` with a computed string
     Any of these bypasses the scan trivially. The check is defense-in-depth against
     unsophisticated/accidental cases, not a security boundary.

2. **Gate on write_tools_enabled/destructive_tools_enabled at call_tool level** —
   declare `iris_execute` as write-gated. Any `iris_execute` call requires
   `write_tools_enabled = true`. Kills (matching `Kill ^` pattern in the literal code
   string) additionally require `destructive_tools_enabled`. This is coarse but honest:
   if writes are off, running arbitrary ObjectScript that might write is also off.

3. **Hybrid** — declare `iris_execute` write-gated always (option 2), and additionally
   apply the PHI and system-globals static checks from `iris_global` to detect obvious
   global name matches in the code string before sending. Static check covers
   unsophisticated/accidental cases only; the gate declaration is the real enforcement.

Recommendation: option 3. The gate declaration (option 2) is the minimum viable fix
and the honest enforcement boundary. The static check (option 1, added on top) catches
inadvertent sloppiness — a well-intentioned model writing `Kill ^Foo` literally without
thinking through the gate implications. It does not catch deliberate indirection and
should not be presented as doing so.

`iris_ws_exec` has the same gap and needs the same fix in a follow-on (see Out of
scope).

The real enforcement for adversarial scenarios is IRIS-native: credentials with
restricted privileges, resource-based access control, and the `mcpTemplate` env gate
which already blocks `iris_execute` on `test`/`live` instances.

## Trust asymmetry

This is a self-reported gate enforced in the agent process, not in IRIS. A rebuilt binary
or a direct HTTP call to Atelier bypasses it regardless. The gates are a safety rail for
honest use, not a security boundary against a hostile caller. This spec should say so
plainly rather than implying the gate makes `iris_execute` safe against an adversary.
