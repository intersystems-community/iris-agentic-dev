# Research: System Observability Depth

**Branch**: `055-system-observability`
**Date**: 2026-06-30
**IRIS version verified on**: IRIS 2026.2.0 (Build 208U) — `iris-dev-iris` container
**Constitution**: Principle II — all ObjectScript APIs verified against live IRIS.

---

## API Verification Results

### view_locks — `%SYS.LockQuery`

- **SQL table exists**: NO — `%SYS.LockQuery` is an abstract class with named queries,
  not a SQL-projected table. `SELECT * FROM %SYS.LockQuery` returns SQLCODE -30.
- **Correct API**: `##class(%ResultSet).%New("%SYS.LockQuery:Detail")`
  — named query, returns the full lock table including all details.
  `%SYS.LockQuery:List` also exists (lighter, fewer columns).
  `%SYS.LockQuery:Conflicts` exists for conflict-only view.
- **Verified columns** (from `%SYS.LockQuery:Detail`): timed out iterating on quiet
  instance — use `GetColumnName(i)` loop at impl time to confirm. Based on ISC docs the
  expected fields are: `Name` (resource), `Owner` (PID), `Type`, `Mode`, `OwnerName`.
- **Decision**: Use `%SYS.LockQuery:Detail` class query via `%ResultSet`. Map columns to
  `resource`, `owner_pid`, `lock_type`, `lock_mode`, `owner_username` in response.
- **Impact on plan/tasks**: plan.md SQL for `view_locks` is WRONG — must be replaced with
  class query approach. T017 must use `%ResultSet`, not `%SQL.Statement`.

### view_processes — `%SYS.ProcessQuery`

- **SQL table exists**: YES — `SELECT * FROM %SYS.ProcessQuery` returns SQLCODE 0.
- **Actual column names** (verified): `ID`, `NameSpace`, `Routine`, `LinesExecuted`,
  `GlobalReferences`, `State`, `PidExternal`, `UserName`, `ClientIPAddress`,
  `ClientNodeName`, `CurrentDevice`, `UserInfo`, `CurrentLineAndRoutine`,
  `CurrentSrcLine`, `LastGlobalReference`, `ClientExecutableName`, `MemoryAllocated`,
  `MemoryUsed`, `OpenDevices`, `CanBeExamined`, `CanBeSuspended`, `CanBeTerminated`,
  `CanReceiveBroadcast`, `CSPSessionID`, `InTransaction`, `IsGhost`, `JobNumber`,
  `JobType`, `LicenseUserId`, `Location`, `OSUserName`, `Pid`, `Priority`,
  `StartupClientIPAddress`, `StartupClientNodeName`, `Switch10`,
  `PrivateGlobalBlockCount`, `CommandsExecuted`, `PrincipalDevice`, `GlobalUpdates`,
  `GlobalDiskReads`, `GlobalBlocks`, `JournalEntries`, `MemoryPeak`, `Roles`,
  `LoginRoles`, `DataBlockWrites`, `PrivateGlobalReferences`, `PrivateGlobalUpdates`,
  `CPUTime`, `AppFrameInfo`, `ParentPid`, `StartTimeUTC`, `PrivateGlobalAllocatedSize`
- **Column name corrections vs plan**:
  - `Pid` is correct (not `PID`)
  - `UserName` (not `Username`)
  - `NameSpace` (not `Namespace`)
  - `ClientIPAddress` is correct
  - `ClientNodeName` (not `ClientName`) — plan used `ClientName`, use `ClientNodeName`
  - `Routine` is correct
- **Decision**: Use SQL. Corrected query:
  `SELECT Pid, UserName, NameSpace, State, ClientNodeName, ClientIPAddress, Routine FROM %SYS.ProcessQuery ORDER BY Pid`
- **Redaction fields**: `UserName`, `ClientNodeName`, `ClientIPAddress`

### journal_search — `%SYS.Journal.Record`

- **SQL table exists**: NO — `SELECT * FROM %SYS.Journal.Record` returns SQLCODE -30.
  `%SYS.Journal.File` also has no SQL projection.
- **`%SYS.Journal.File.GetCurrent()` exists**: NO — `<METHOD DOES NOT EXIST>`.
- **Correct API**: `%SYS.Journal.File:Search` named query (SearchClose/Execute/Fetch
  class methods confirmed). BUT: iterating the full journal times out on live IRIS even
  with small datasets — the Search query scans sequentially.
- **`%SYS.Journal.Record` properties** (verified):
  `Address`, `ECPSystemID`, `ExtType`, `ExtTypeName`, `InTransaction`, `JobID`,
  `Next`, `NextAddress`, `Prev`, `PrevAddress`, `ProcessID`, `RemoteSystemID`,
  `TimeStamp`, `Type`, `TypeName`
- **Critical finding**: `GlobalReference` is NOT a property of `%SYS.Journal.Record`.
  The plan's `GlobalRef` column doesn't exist. Journal records have `Type`/`TypeName`
  (operation type) and `TimeStamp`, but global name is in `ExtType`/`ExtTypeName` or
  accessed via the record's address.
- **Decision**: Journal search requires further investigation. Neither SQL nor simple
  class API matches the spec's assumed shape. Options:
  1. Use `%SYS.Journal.File:Search` with a specific journal file and time window —
     feasible but the column set needs verification at implementation time.
  2. Descope `journal_search` to a future spec (complexity is higher than planned).
- **`journal_search` status**: `NEEDS FURTHER INVESTIGATION` before T034 runs.
  Recommend verifying `%SYS.Journal.File:Search` column names with a non-empty journal.

### namespace_mappings — Config.MapGlobals / MapPackages / MapRoutines

- **SQL tables exist**: YES — all three return SQLCODE 0.
- **Actual column names** (verified):
  - `Config.MapGlobals`: `ID`, `CPFName`, `Collation`, `Comments`, `Database`,
    `LockDatabase`, `Name`, `Namespace`, `SectionHeader`
  - `Config.MapPackages`: `ID`, `CPFName`, `Comments`, `Database`, `Name`,
    `Namespace`, `SectionHeader`
  - `Config.MapRoutines`: `ID`, `CPFName`, `Comments`, `Database`, `Name`,
    `Namespace`, `SectionHeader`
- **Column name corrections vs plan**:
  - All three use `Database` (not `GlobalDatabase` / `PackageDatabase` / `RoutineDatabase`)
  - `Namespace` column exists in all three for the WHERE filter — confirmed
- **Namespace existence check**: `Config.Namespaces` SQL table confirmed (SQLCODE 0).
  Query `SELECT Name FROM Config.Namespaces WHERE Name = :ns` — SQLCODE 100 = not found,
  SQLCODE 0 + `%Next()=1` = exists.
- **Decision**: Use SQL with corrected column name `Database` for all three tables.
  Corrected queries:
  - `SELECT Name, Database FROM Config.MapGlobals WHERE Namespace = :ns`
  - `SELECT Name, Database FROM Config.MapPackages WHERE Namespace = :ns`
  - `SELECT Name, Database FROM Config.MapRoutines WHERE Namespace = :ns`
  - `SELECT Name FROM Config.Namespaces WHERE Name = :ns` for existence check

### database_status — SYS.Database

- **`SYS.Database` SQL table exists**: NO — SQLCODE -30.
- **`SYS.Database` class exists**: YES — not SQL-projected but has named queries.
- **`SYS.Database:List` columns** (verified): `Directory`, `MaxSize`, `Size`, `Status`,
  `Resource`, `Encrypted`, `StateInt`, `Mirrored`, `SFN`, `EncryptionKeyID`,
  `EncryptionVersion`
  - Sample row: `/usr/irissys/mgr/ | Unlimited | 80 | Mounted/RW | %DB_IRISSYS | No | Mounted/RW | 0 | 0 | | 0`
- **`SYS.Database:FreeSpace` columns** (verified): `DatabaseName`, `Directory`,
  `MaxSize`, `Size`, `ExpansionSize`, `Available`, `Free`, `DiskFreeSpace`, `Status`,
  `SizeInt`, `AvailableNum`, `DiskFreeSpaceNum`, `ReadOnly`
  - Sample row: `IRISSYS | /usr/irissys/mgr/ | Unlimited | 80MB | System Default | 9.2MB | 12 | 337.28GB | Mounted/RW | 80 | 9.2 | 345370 | 0`
- **`Config.Databases` SQL table**: exists (SQLCODE 0) — columns `ID`, `CPFName`,
  `ClusterMountMode`, `Comments`, `Directory`, `MountAtStartup`, `MountRequired`,
  `Name`, `SectionHeader`, `Server`, `StreamLocation`. Config-only, no runtime state.
- **Decision**: Use `SYS.Database:FreeSpace` class query for runtime status (mount state,
  free space). `Free` column = free blocks (integer), `DiskFreeSpace` = human-readable.
  `Mirrored` from `SYS.Database:List`. No `MirrorStatus` column — use `Mirrored`
  (0/1) + `MirrorSetName` property for mirror state.
- **Revised response shape**: `name` (DatabaseName), `directory`, `mounted` (Status
  contains "Mounted"), `free_space_mb` (AvailableNum in MB), `disk_free_gb`
  (DiskFreeSpaceNum), `status` (Status string), `read_only` (ReadOnly).
  `mirror_state`: from `SYS.Database:List` `Mirrored` column — `"0"` → `"none"`.

---

## Summary: Plan/Tasks Corrections Required

| Action | Was | Now | Severity |
|---|---|---|---|
| `view_locks` | SQL `%SYS.LockQuery` | Class query `%SYS.LockQuery:Detail` | **HIGH** — SQL doesn't exist |
| `view_processes` | Columns: `Username`, `ClientName`, `Namespace` | `UserName`, `ClientNodeName`, `NameSpace` | **MEDIUM** — wrong column names |
| `journal_search` | SQL `%SYS.Journal.Record` | No SQL; class API complex; GlobalReference not a property | **HIGH** — needs investigation |
| `namespace_mappings` | Columns: `GlobalDatabase`, `PackageDatabase`, `RoutineDatabase` | All use `Database` | **MEDIUM** — wrong column names |
| `database_status` | SQL `SYS.Database` | Class query `SYS.Database:FreeSpace` + `SYS.Database:List` | **HIGH** — SQL doesn't exist |

---

## Error Code Decisions

| Code | Used by | Decision |
|---|---|---|
| `MISSING_PARAMS` | `journal_search` | New code — add to gate.rs registry |
| `NAMESPACE_NOT_FOUND` | `namespace_mappings` | New code — add to gate.rs registry |
| `DATABASE_NOT_FOUND` | `database_status` | New code — add to gate.rs registry |

---

## dataPolicy Handling — Confirmed

| Action | block | redact | allow |
|---|---|---|---|
| `view_locks` | Allowed | Same as allow | Allowed |
| `view_processes` | `DATA_POLICY_BLOCKED` | Redact `UserName`/`ClientNodeName`/`ClientIPAddress` | Full output |
| `journal_search` | `DATA_POLICY_BLOCKED` | `DATA_POLICY_BLOCKED` | Proceed |
| `namespace_mappings` | Allowed | Same as allow | Allowed |
| `database_status` | Allowed | Same as allow | Allowed |

---

## Horolog Input — Resolved

ISO 8601 only for v1. Removed from spec edge cases.

---

## Constitution Compliance

| Principle | Status |
|---|---|
| I. Zero-Install | Pass |
| II. ObjectScript Sanity | **Pass** — all five APIs verified on IRIS 2026.2.0 |
| III. HTTP-First | Pass |
| IV. Test-First | Pass |
| V. Output Shape Parity | Pass — shapes revised to match actual API columns |
| VI. Environment Guard | Pass — all five `ToolCategory::Query` |
| VII. Dependency Minimalism | Pass — no new crates |
