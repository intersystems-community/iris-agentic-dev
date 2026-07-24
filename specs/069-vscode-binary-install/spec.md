# Feature Specification: VS Code Extension Auto-Installs Binary

**Feature Branch**: `069-vscode-binary-install`
**Created**: 2026-07-24
**Status**: Draft

## User Scenarios & Testing _(mandatory)_

### User Story 1 — First-time Windows user installs the extension (Priority: P1)

A Windows developer installs the iris-agentic-dev VS Code extension from the Marketplace.
They have no binary on their machine and no package manager set up. On first activation the
extension downloads the correct Windows binary silently, shows a progress notification, and
the MCP server starts — no command prompt, no manual download, no PATH editing required.

**Why this priority**: Windows is the primary pain point. The unsigned `.exe` means manual
installation is a hostile experience; auto-install removes the barrier entirely.

**Independent Test**: Install the extension on a clean Windows machine with no existing binary.
Confirm the MCP server starts and tools respond correctly without any manual steps.

**Acceptance Scenarios**:

1. **Given** the extension is installed and no binary exists locally, **When** VS Code activates
   the extension, **Then** a progress notification appears, the binary is downloaded for the
   current platform/arch, and the MCP server starts successfully.
2. **Given** the binary has already been downloaded for the current version, **When** VS Code
   activates the extension again, **Then** no download occurs and the server starts immediately.
3. **Given** the extension activates on Windows, **When** a previous binary is running and
   locked by the OS, **Then** the old binary is renamed rather than overwritten, the new binary
   is placed, and activation completes without an error.

---

### User Story 2 — Existing user with brew-installed binary (Priority: P2)

A macOS developer already has `iris-agentic-dev` installed via Homebrew and in their PATH.
After installing the VS Code extension, they expect their existing binary to be used — not a
second copy downloaded to some hidden directory.

**Why this priority**: Existing users should not get a shadow copy that diverges from their
managed installation.

**Independent Test**: Install the extension on a machine where `iris-agentic-dev` is already
in PATH. Confirm the extension uses the PATH binary and no download occurs.

**Acceptance Scenarios**:

1. **Given** `iris-agentic-dev` is on PATH, **When** the extension activates, **Then** the
   PATH binary is used and no download is triggered.
2. **Given** a user sets `iris-agentic-dev.serverPath` to a specific file path, **When** the
   extension activates, **Then** that exact path is used regardless of PATH or cached downloads.
3. **Given** `iris-agentic-dev.serverPath` is set to a path that does not exist or is not
   executable, **When** the extension activates, **Then** a clear error message identifies the
   bad path and suggests corrective action.

---

### User Story 3 — Extension updates binary when a new version ships (Priority: P3)

A user has an older managed binary cached from a previous extension version. They update the
VS Code extension. On next activation the extension detects the version mismatch and downloads
the new binary, replacing the old one.

**Why this priority**: Stale binaries cause confusing tool errors. Auto-update on extension
upgrade keeps binary and extension in sync without user intervention.

**Independent Test**: Simulate a version mismatch by downgrading the cached version marker.
Confirm a fresh download replaces the old binary on next activation.

**Acceptance Scenarios**:

1. **Given** the cached binary version does not match the current extension version, **When**
   the extension activates, **Then** the new binary is downloaded and the version marker updated.
2. **Given** a download fails (network error, GitHub unavailable), **When** an older cached
   binary exists, **Then** the extension falls back to the cached binary and logs a warning
   rather than failing to start.
3. **Given** a download fails and no cached binary exists, **When** the extension activates,
   **Then** a user-facing error explains the failure and links to the manual install docs.

---

### Edge Cases

- What happens on an unsupported platform or architecture (e.g., Linux arm64)?
- How does the extension behave when GitHub Releases is rate-limited or unreachable?
- What if the download is interrupted mid-transfer (partial file)?
- What if the user has no write access to the extension storage directory?
- What if two VS Code windows activate the extension simultaneously?

## Requirements _(mandatory)_

### Functional Requirements

- **FR-001**: The extension MUST download the correct platform/arch binary from GitHub Releases
  on first activation when no usable binary is found.
- **FR-002**: The extension MUST show a progress notification during download so the user knows
  activation is in progress.
- **FR-003**: The extension MUST cache the downloaded binary in persistent per-user storage
  (survives VS Code restarts and workspace changes).
- **FR-004**: The extension MUST check the cached binary version against the current extension
  version on every activation and re-download when they differ.
- **FR-005**: The extension MUST use a user-configured `iris-agentic-dev.serverPath` setting
  as the highest-priority binary source, bypassing PATH lookup and managed download.
- **FR-006**: The extension MUST check PATH for an existing `iris-agentic-dev` binary as the
  second-priority source, using it without downloading when found.
- **FR-007**: On Windows, the extension MUST rename an existing locked binary before replacing
  it to avoid OS file-locking errors.
- **FR-008**: When a download fails and a previous cached binary exists, the extension MUST
  fall back to the cached binary rather than failing to start.
- **FR-009**: The extension MUST verify the downloaded binary is executable before recording
  it as a successful install (and set executable permission on mac/linux).
- **FR-010**: The extension MUST surface a clear, actionable error message when no binary can
  be found or downloaded and the server cannot start.

### Key Entities

- **ManagedBinary**: The downloaded binary — platform, arch, version, storage path, download URL.
- **BinarySource**: The resolved binary and which priority tier provided it (setting / PATH /
  managed download).
- **VersionMarker**: A small file stored alongside the managed binary recording which version
  it corresponds to.

## Success Criteria _(mandatory)_

### Measurable Outcomes

- **SC-001**: A user with no existing binary can go from "extension installed" to "MCP server
  running" with zero manual steps on Windows, macOS, and Linux.
- **SC-002**: On a machine with an existing PATH binary, activation completes in under 1 second
  (no download, no unnecessary delay).
- **SC-003**: When a download is required, the user sees a progress indicator within 500 ms of
  activation.
- **SC-004**: A partial or interrupted download never leaves a corrupt binary in cache — the
  server either starts cleanly or reports a clear error.
- **SC-005**: All four platform/arch combinations (macos-arm64, macos-x86_64, linux-x86_64,
  windows-x86_64) resolve to the correct binary name and download URL.

## Assumptions

- The extension's `package.json` version is kept in sync with the binary release version —
  the extension and binary ship together as part of the same release.
- GitHub Releases remains the authoritative download source; no CDN mirror or Artifactory
  mirror is required at this time.
- Linux arm64 is out of scope for now — no binary is published for that platform.
- The extension already manages the MCP server process lifecycle (start/stop/restart); this
  spec covers only binary resolution, not server process management.
