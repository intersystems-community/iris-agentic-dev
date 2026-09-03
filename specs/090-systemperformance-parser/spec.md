# Feature Specification: SystemPerformance / pButtons Parser

**Feature Branch**: `090-systemperformance-parser`
**Created**: 2026-09-02
**Status**: Draft

## Overview

SystemPerformance and pButtons produce HTML files containing tab-separated performance
data sections. Today an agent can trigger a run and get a run ID — but cannot read or
reason over the results. The output format is opaque text that requires parsing before
any useful analysis is possible.

This feature adds a new tool, `iris_parse_performance_report`, that reads a
SystemPerformance or pButtons HTML file and returns structured JSON an agent can
analyze directly — without requiring YASPE, Python, or any external tool.

---

## User Scenarios & Testing

### User Story 1 — Parse a completed SystemPerformance run (Priority: P1)

An operator has run SystemPerformance on a production instance and wants to know
whether there are performance problems — high CPU wait, disk saturation, lock table
pressure, or low gloref cache ratio — without reading a wall of HTML.

**Why this priority**: This is the primary use case. The HTML report exists but is
unreadable by an agent. Structured JSON unlocks all downstream analysis.

**Independent Test**: Feed a sample SystemPerformance HTML file; assert the returned
JSON contains the expected sections and numeric values.

**Acceptance Scenarios**:

1. Given a valid SystemPerformance HTML file path, When `iris_parse_performance_report`
   is called, Then the result contains a `mgstat` section with timestamped rows and
   numeric columns (Glorefs, PhyRds, PhyWrs, WDpass, etc.).
2. Given a file with vmstat data (Linux host), When the tool is called, Then the result
   contains a `vmstat` section with CPU columns (us, sy, wa, id) and memory columns.
3. Given a file with iostat data, When the tool is called, Then the result contains an
   `iostat` section keyed by device name with r/s, w/s, await, %util.
4. Given a file path that does not exist, When the tool is called, Then the result is
   a structured error — not a panic.
5. Given a pButtons file (Caché format), When the tool is called, Then it parses
   correctly — same sections, same output shape.

### User Story 2 — Summarize and flag anomalies (Priority: P2)

An agent analyzing a run wants a plain-language summary: what was abnormal, what
thresholds were exceeded, what to investigate.

**Why this priority**: Structured JSON is necessary but not sufficient. The agent still
has to write analysis logic. A built-in summary mode lets the tool do the interpretation.

**Independent Test**: Feed a file with known high wa% values; assert the summary flags
CPU wait as elevated.

**Acceptance Scenarios**:

1. When `mode=summary` is passed, Then the result includes a `findings` array where each
   entry names the metric, the observed value, the threshold, and a one-line explanation.
2. When all metrics are within normal ranges, Then `findings` is empty and `status` is
   `"normal"`.
3. Thresholds are based on InterSystems documented guidance (e.g. wa% > 20 = concern,
   Rdratio < 90% = cache pressure, WDpass spikes = write daemon pressure).

---

## Functional Requirements

- **FR-001**: `iris_parse_performance_report` accepts a `file_path` parameter (local
  path or a path accessible to the running iad process).
- **FR-002**: `mode` parameter: `parse` (default, returns raw structured data) or
  `summary` (returns findings + status).
- **FR-003**: Output always includes `format` (`systemperformance` or `pbuttons`),
  `iris_version` (if present in file header), `host_info` (OS, CPUs, memory from
  System Overview section), and `sections` (array of section names found).
- **FR-004**: `mgstat` section returns rows as array of objects keyed by column name;
  all numeric columns are numbers (not strings).
- **FR-005**: `vmstat` section returns rows with a derived `total_cpu` column
  (us + sy + wa).
- **FR-006**: `iostat` section is keyed by device name; each device has an array of
  timestamped rows.
- **FR-007**: `summary` mode thresholds are based on InterSystems documentation and
  common YASPE guidance. Thresholds are documented in code and overridable via
  optional `thresholds` parameter (JSON object).
- **FR-008**: The tool works on files already on disk — it does not trigger a new
  SystemPerformance run. Use `iris_system_performance` to run and retrieve the file
  path, then pass it here.
- **FR-009**: Windows Perfmon sections are parsed but column extraction is
  best-effort (column names vary by counter selection).

---

## Key Entities

- **PerformanceReport**: parsed representation of one HTML file
  - `format`: `systemperformance` | `pbuttons`
  - `iris_version`: string or null
  - `host_info`: `{ os, cpus, memory_gb, shared_memory_gb }`
  - `sections`: array of section names present
  - `mgstat`: array of row objects
  - `vmstat`: array of row objects (Linux only)
  - `iostat`: map of device → array of row objects (Linux only)
  - `perfmon`: map of counter → array of values (Windows only)

- **Finding** (summary mode only):
  - `metric`: e.g. `"vmstat.wa"`
  - `observed`: peak or average value
  - `threshold`: the reference value
  - `severity`: `"warn"` | `"critical"`
  - `explanation`: one-line human-readable description

---

## Success Criteria

- A sample SystemPerformance HTML file parses in under 2 seconds on a typical developer
  machine regardless of file size (up to 24 hours of data at 5-second intervals).
- `summary` mode on a file with known anomalies returns at least one finding per
  anomalous metric.
- The tool is usable standalone — no live IRIS connection required. File path is all
  that is needed.
- Round-trip test: trigger a run via `iris_system_performance mode=start`, wait for
  completion, retrieve the HTML path, parse with this tool, assert `mgstat` rows > 0.

---

## Out of Scope

- Generating charts or HTML output (that's YASPE's job).
- Parsing the raw `^IRIS.SystemPerformance` global directly (HTML file is the
  authoritative output).
- Uploading or forwarding reports anywhere.
- Real-time streaming of in-progress runs.

---

## Assumptions

- SystemPerformance HTML format has been stable across IRIS versions since Caché 2016.
  pButtons is the Caché predecessor format; same sections, slightly different header.
- File is accessible to the iad process (local path or mounted volume).
- mgstat columns are consistent across platforms; vmstat/iostat are Linux-only;
  Perfmon is Windows-only.
