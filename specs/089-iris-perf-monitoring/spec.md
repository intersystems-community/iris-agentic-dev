# Feature Specification: IRIS Mirror Status and Database Free Space

**Feature Branch**: `089-iris-perf-monitoring`
**Created**: 2026-09-01
**Status**: Draft

## Overview

Two new or extended MCP tools for iris-agentic-dev that give agents visibility into IRIS
instance topology and database capacity: checking mirror membership and role, and reporting
database free space alongside existing database metrics.

**Note on Performance Profiler (Feature 1 from original scope)**: The `run^SystemPerformance`
entrypoint is absent from community IRIS 2026.2 (Enterprise-only). System Monitor history
(`^SYS.History.Performance`) is also inactive in the community container. Performance
profiling will be filed as a separate spec targeting an Enterprise container.

---

## User Scenarios & Testing

### User Story 1 — Mirror status (Priority: P1)

An agent performing fleet checks or pre-action validation needs to know whether
the target IRIS instance is a mirror member, its role (primary, backup, or
async), and whether it is currently the primary — before deciding whether an
action is safe to perform on that instance.

**Why this priority**: Mirror topology affects which operations are safe. An
agent that promotes a backup or writes to the wrong node can cause data
divergence. This is a common pre-flight check in ops automation.

**Independent Test**: Can be fully tested against any IRIS instance — a
non-mirror instance returns `{is_member: false}` which is a valid and
unambiguous result.

**Acceptance Scenarios**:

1. **Given** a non-mirror IRIS instance, **When** `iris_mirror_status` is
   called, **Then** the result contains `is_member: false` and no other mirror
   fields are required.
2. **Given** a mirror primary instance, **When** `iris_mirror_status` is
   called, **Then** the result contains `is_member: true`, `is_primary: true`,
   `member_type`, and `mirror_name`.
3. **Given** a mirror backup instance, **When** `iris_mirror_status` is
   called, **Then** the result contains `is_member: true`, `is_primary: false`,
   `member_type: "backup"` (or async), and `mirror_name`.
4. **Given** the %SYS Mirror API is unavailable, **When** the tool is called,
   **Then** the tool returns a structured error with a clear message — not a
   panic or empty response.

---

### User Story 2 — Database free space (Priority: P2)

An operator reviewing instance health needs to know not just which databases
exist but how full they are, so they can spot databases approaching capacity
before they cause problems.

**Why this priority**: Extends an existing tool rather than adding a new one.
Useful for ops monitoring but lower risk than User Story 1. Graceful
degradation (returning existing data when free space query fails) limits blast
radius.

**Independent Test**: Can be fully tested by calling `iris_database_list` and
verifying that each database entry includes numeric `size_mb`, `free_space_mb`,
and `max_size_mb` fields.

**Acceptance Scenarios**:

1. **Given** a connected IRIS instance with %SYS access, **When**
   `iris_database_list` is called, **Then** each database entry includes
   `size_mb`, `free_space_mb`, and `max_size_mb` as numeric values.
2. **Given** an instance where the free space query fails (insufficient
   privileges), **When** `iris_database_list` is called, **Then** the tool
   returns the existing database list with a `free_space_note` field explaining
   that free space data was unavailable — it does not fail the whole call.
3. **Given** a database with no max size limit, **When** the tool is called,
   **Then** `max_size_mb` is `null` (not an error, not zero).

---

### Edge Cases

- Mirror API call on a version of IRIS that predates mirror support: structured
  error, not a panic.
- `iris_database_list` on an instance where %SYS is accessible but
  `DatabaseQuery_FreeSpace` is not defined (older IRIS): graceful fallback to
  existing data.

---

## Requirements

### Functional Requirements

iris_mirror_status

- **FR-001**: Tool MUST return `{is_member: false}` for non-mirror instances.
- **FR-002**: Tool MUST return `{is_member, mirror_name, member_type,
is_primary}` for mirror instances.
- **FR-003**: Tool MUST use `%SYSTEM.Mirror` class methods — no helper class,
  no dynamic class creation.
- **FR-004**: Tool MUST execute in %SYS namespace context.

iris_database_list (extended)

- **FR-005**: Tool MUST add `size_mb`, `free_space_mb`, and `max_size_mb` to
  each database entry when %SYS access is available.
- **FR-006**: Tool MUST degrade gracefully when the free space query fails —
  returning existing data plus a `free_space_note` field.
- **FR-007**: `max_size_mb` MUST be `null` when the database has no configured
  max size.

### Key Entities

- **Mirror Status**: membership flag, mirror name, member type
  (primary/backup/async), primary flag.
- **Database Entry**: name, directory, size, free space, max size.

---

## Success Criteria

### Measurable Outcomes

- **SC-001**: Mirror role is available to an agent in a single tool call, with
  no ObjectScript knowledge required.
- **SC-002**: `iris_database_list` returns free space data for all databases in
  a single call on any IRIS instance where %SYS access is granted.
- **SC-003**: Both tools handle error conditions (missing privileges,
  unsupported IRIS version) without crashing — they return structured,
  actionable error messages.
- **SC-004**: Both tools pass live IRIS integration tests against iris-dev-iris
  (community 2026.2) with `--test-threads=1`.

---

## Assumptions

- The iris-dev-iris test container (community 2026.2, localhost:52780) is not
  in a mirror — `iris_mirror_status` tests assert `is_member: false`.
- %SYS namespace access is available in the test container under the `_SYSTEM`
  credential used by iad integration tests.
- `%SYSTEM.Mirror` class methods return well-defined values for non-mirror
  instances: `IsMember()=0`, `MirrorName()=""`, `GetMemberType()="Not Member"`,
  `IsPrimary()=0`.
- `%SYS.DatabaseQuery:FreeSpace` columns are: `DatabaseName`, `SizeInt` (MB),
  `AvailableNum` (MB), `Free` (%), `MaxSize` (string "Unlimited" or size),
  as verified against iris-dev-iris.

---

## Out of Scope

- SystemPerformance profiler (Enterprise-only — separate spec pending Enterprise
  container).
- System Monitor history (^SYS.History.Performance absent in community IRIS).
- Mirror configuration changes (read-only status only).
- Database creation, deletion, or configuration.
- YASPE plot generation.
