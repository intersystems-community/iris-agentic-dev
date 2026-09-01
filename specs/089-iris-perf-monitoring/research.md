# Research: IRIS Performance and Fleet Monitoring Tools (089)

All findings verified 2026-09-01 against iris-dev-iris (community 2026.2.0L Build 208U,
Ubuntu ARM64).

---

## Feature 2: iris_mirror_status

### API Verification

All four `%SYSTEM.Mirror` class methods verified via `iris session IRIS -U "%SYS"`:

| Method                                    | Return (non-mirror instance) | Verified |
| ----------------------------------------- | ---------------------------- | -------- |
| `##class(%SYSTEM.Mirror).IsMember()`      | `0`                          | ✅       |
| `##class(%SYSTEM.Mirror).MirrorName()`    | `""` (empty)                 | ✅       |
| `##class(%SYSTEM.Mirror).GetMemberType()` | `"Not Member"`               | ✅       |
| `##class(%SYSTEM.Mirror).IsPrimary()`     | `0`                          | ✅       |

### Implementation Decision

Use `iris_execute` in `%SYS` namespace. Four classmethod calls, results assembled into
a JSON object. `GetMemberType()` returns `"Not Member"` (not an empty string) when
`IsMember()=0` — normalize this to `null` in the JSON output so callers get a clean
`{is_member: false}` shape.

**Decision**: Return `member_type: null` and `mirror_name: null` when `is_member=false`,
so downstream code can do `if result.is_member { ... }` without checking for sentinel
strings.

---

## Feature 3: iris_database_list free space

### API Verification

`%SYS.DatabaseQuery:FreeSpace` ResultSet verified. Actual column names (not what Luca's
POC assumed):

| Column             | Type    | Notes                                         |
| ------------------ | ------- | --------------------------------------------- |
| `DatabaseName`     | string  | Database name (NOT `Name`)                    |
| `Directory`        | string  | Filesystem path                               |
| `MaxSize`          | string  | e.g. `"Unlimited"` or `"500MB"` — NOT numeric |
| `Size`             | string  | e.g. `"80MB"` — string with unit              |
| `SizeInt`          | integer | Size in MB, numeric ✅                        |
| `Available`        | string  | e.g. `"9.7MB"` — string with unit             |
| `AvailableNum`     | float   | Available in MB, numeric ✅                   |
| `Free`             | integer | Free space as percentage (0-100)              |
| `DiskFreeSpace`    | string  | Total disk free, e.g. `"1.969TB"`             |
| `DiskFreeSpaceNum` | integer | Total disk free in MB                         |
| `ExpansionSize`    | string  | e.g. `"System Default"`                       |
| `Status`           | string  | e.g. `"Mounted/RW"`                           |
| `ReadOnly`         | integer | 0 or 1                                        |

**Key finding**: `MaxSize` is a string `"Unlimited"` not a number. Represent as
`max_size_mb: null` when unlimited, numeric otherwise (parse the string).

**Key finding**: Use `SizeInt` (not `Size`) for numeric size. Use `AvailableNum` (not
`Available`) for numeric free space. `Free` is a percentage.

### Implementation Decision

Add free space query to existing `iris_database_list` handler. Execute
`%SYS.DatabaseQuery:FreeSpace` in `%SYS` namespace, build a `HashMap<String, FreeSpaceRow>`
keyed by `DatabaseName`, then merge into the existing database list entries.

Graceful degradation: if the query fails or %SYS access is not available, include
`free_space_status: "unavailable"` in the response root and omit free space fields from
individual entries.

---

## Feature 1: iris_system_performance — CRITICAL FINDING

### SystemPerformance Does NOT Exist in Community IRIS

`$TEXT(run^SystemPerformance)` returns empty string — the routine is **absent** from
community 2026.2. This is confirmed: SystemPerformance is an **Enterprise-only** feature
that ships in licensed IRIS instances but not in the community image used for development
and CI.

### What DOES Exist: SYS.History.Performance

The community image includes `SYS.History.Performance` with two methods: `PropList` and
`SetSummary`. These expose the **System Monitor** continuous performance history data — a
lighter, always-on collection that records metrics like CPU, memory, and lock table usage
every ~60 seconds automatically. This is NOT the same as the on-demand profiler
(`run^SystemPerformance`) but it IS available in community IRIS and captures real
performance data.

The history data is stored in `^SYS.History.Performance` (different from
`^IRIS.SystemPerformance` which is the profiler output global).

### Revised Feature 1 Scope

**Decision**: Redesign `iris_system_performance` to query the System Monitor history
(`SYS.History.Performance`) rather than the Enterprise profiler. This gives real,
testable behavior against community IRIS.

Modes revised:

- `mode=latest` — return the most recent System Monitor data snapshot (CPU%, memory, lock
  table, etc.) from `^SYS.History.Performance`
- `mode=summary` — return a time-range summary of key performance metrics (last N minutes,
  configurable, default 60)
- `mode=check_profiler` — report whether the full SystemPerformance profiler is available
  on this instance (Enterprise check)

**Rationale**: An agent that can read current CPU/memory/lock pressure is genuinely useful
for automated ops decisions regardless of whether the instance has a license. The Enterprise
profiler path can be added in a follow-up spec once we have an Enterprise test container.

**Alternative considered and rejected**: Shipping a stub that says "Enterprise only" — too
thin to justify a new tool. The history data path is real and useful.

### SYS.History.Performance API — VERIFIED: NOT USABLE

Further verification confirmed: `^SYS.History.Performance` global `$D=0` (does not exist)
in community IRIS 2026.2.0L. The System Monitor is not collecting data in this container
and the global is never written. `PropList()` returns metric names but there is no data
to read. `SetSummary()` throws `<PARAMETER>`. This approach is abandoned.

### Final Decision: Drop Feature 1 From This Spec

No viable Feature 1 implementation exists for community IRIS:

- `run^SystemPerformance` absent — Enterprise only
- `^SYS.History.Performance` empty — System Monitor inactive in community container
- `%SYSTEM.*` performance APIs severely limited in community

**Decision**: Deliver Feature 2 (iris_mirror_status) and Feature 3 (iris_database_list
free space) in this spec. File Feature 1 (performance profiler) as a separate spec
targeted at an Enterprise IRIS container. Both remaining features are fully verified and
testable against iris-dev-iris.

---

## Dependency Minimalism (Principle VII)

No new Rust crates required. All three features use existing `iris_execute` / `execute_via_generator`
infrastructure plus the existing `%SYS.DatabaseQuery` ResultSet pattern already used in
`iris_database_list`.

---

## Toolset Registration (Constitution — Additional Constraints)

All three tools target `Nostub + Merged` tier — they work over HTTP with no Docker
requirement and have no `NOT_IMPLEMENTED` stubs.

- `iris_mirror_status`: new tool, read-only, no write gate needed
- `iris_system_performance`: new tool, read-only, no write gate needed
- `iris_database_list`: existing tool extension, no tier change
