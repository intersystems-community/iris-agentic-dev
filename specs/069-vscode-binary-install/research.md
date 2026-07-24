# Research: 069-vscode-binary-install

## Existing Extension Audit

The extension lives at `vscode-iris-agentic-dev/src/extension.ts`. The relevant
entry point is `findIrisDev()` (lines 9-18), which today implements two of the three
priority tiers:

1. `iris-agentic-dev.serverPath` setting (explicit path)
2. `which.sync('iris-agentic-dev')` PATH lookup

When both return nothing it returns `null`, which causes `provideMcpServerDefinitions`
to show an error message and bail (lines 103-110). The managed-download tier needs to
slot in here, making `findIrisDev` async and adding the download path before the null
return.

`activate()` is synchronous today. It will need to become async (or launch an async
init task) to await the download before registering the provider — or the provider must
handle a not-yet-ready binary gracefully.

---

## Decision 1 — Binary Storage Location

**Decision**: `context.globalStorageUri.fsPath`

**Rationale**: Persists across workspaces and VS Code updates; the extension's own
`extensionUri` directory is read-only and wiped on reinstall. Codebook, OLS, and
clangd all use `globalStorageUri` for this purpose.

**Alternatives considered**:

- `storageUri` — workspace-scoped, undefined when no folder is open. Rejected.
- `extensionUri` — read-only, wiped on update. Rejected.
- OS temp dir — not persistent across reboots. Rejected.

**Storage layout**:

```text
globalStorageUri/
  iris-agentic-dev.version      ← plain-text version string, e.g. "0.9.5"
  iris-agentic-dev-0.9.5/
    iris-agentic-dev[.exe]      ← the cached binary
```text

---

## Decision 2 — Version Matching Strategy

**Decision**: Compare cached version (from `iris-agentic-dev.version` file) against
`context.extension.packageJSON.version`. Re-download when they differ.

**Rationale**: Clarified in spec (Q1). Extension and binary are co-versioned and ship
together. No GitHub API call needed at runtime — deterministic and offline-safe after
first install.

**Download URL pattern** (derived from existing releases):

```text
https://github.com/intersystems-community/iris-agentic-dev/releases/download/v{VERSION}/iris-agentic-dev-{PLATFORM}-{ARCH}[.exe]
```text

Platform/arch mapping from `process.platform` / `process.arch`:

| VS Code            | Binary name segment         |
| ------------------ | --------------------------- |
| `darwin` + `arm64` | `macos-arm64`               |
| `darwin` + `x64`   | `macos-x86_64`              |
| `linux` + `x64`    | `linux-x86_64`              |
| `win32` + `x64`    | `windows-x86_64` (+ `.exe`) |

Linux arm64 is out of scope (no binary published).

---

## Decision 3 — Download Implementation

**Decision**: Node built-in `https` module + `stream/promises.pipeline`. No new npm
dependency.

**Rationale**: Constitution Principle VII (Dependency Minimalism) and the fact that
the download is ~5 MB of raw binary — no archive extraction needed. The `which` dep
is already present. Node's `https` handles redirects (GitHub Releases returns 302 →
S3 presigned URL) with manual redirect following (up to 10 hops).

**Write pattern**: Download to `<dest>.tmp`, then `fs.promises.rename()` to final path.
Atomic on same filesystem. On Windows, rename across drives fails — both paths are in
`globalStorageUri` so same drive is guaranteed.

**Alternatives considered**:

- `node-fetch` — unnecessary dep for a single use case. Rejected.
- `axios` — same. Rejected.

---

## Decision 4 — UX During Download

**Decision**: `vscode.window.withProgress` with `ProgressLocation.Notification` and
percentage reporting. Non-cancellable. Blocks `activate()` via `await`.

**Rationale**: Clarified in spec (Q2) — silent download with progress notification,
no user prompt. Blocking activation is correct: the extension is useless without the
binary. VS Code's activation is async so `await` in `activate()` is fully supported.

**Concurrency guard**: module-level `let downloadInProgress: Promise<string> | undefined`
to prevent duplicate downloads when multiple VS Code windows activate simultaneously.

---

## Decision 5 — Windows File Locking

**Decision**: Before overwriting an existing binary on Windows, rename the old file
to `iris-agentic-dev-old.exe`. Attempt to delete the renamed file on the next
activation. Silently ignore rename/delete failures.

**Rationale**: Windows locks running executables. `fs.promises.rename` of the old
binary (which may or may not be running) to a temp name frees the target path for
the new download without requiring the old process to be terminated.

---

## Decision 6 — Activation Flow Restructure

Today `activate()` is synchronous. New flow:

```text
activate(context)
  └── ensureBinary(context)          ← async, awaited
        ├── checkServerPathSetting() ← returns path if configured
        ├── findInPath()             ← returns path if on PATH
        └── managedDownload()        ← downloads if needed, returns path
  └── register provider with resolved binary path
```text

`findIrisDev()` is replaced by `resolveServerBinary(context): Promise<string | null>`.
The provider receives the resolved path at construction time rather than calling
`findIrisDev()` at `provideMcpServerDefinitions` time.

**Why change call site**: `provideMcpServerDefinitions` is called repeatedly (on every
config change). Moving binary resolution to activation means we download once, not on
every config refresh.

---

## Decision 7 — Constitution Compliance

| Principle                  | Status   | Notes                                                                                                  |
| -------------------------- | -------- | ------------------------------------------------------------------------------------------------------ |
| I. Zero-Install Binary     | **PASS** | Extension auto-installs binary — this IS the zero-install story for VS Code users                      |
| II. ObjectScript Sanity    | **N/A**  | No ObjectScript APIs touched                                                                           |
| III. HTTP-First            | **N/A**  | No new MCP tools                                                                                       |
| IV. Test-First             | **PASS** | Unit tests for URL construction, platform detection, version check; integration test for download mock |
| V. Output Shape Parity     | **N/A**  | No tool responses                                                                                      |
| VI. Environment Guard      | **N/A**  | No IRIS writes                                                                                         |
| VII. Dependency Minimalism | **PASS** | Zero new runtime deps; built-in `https` + `fs`                                                         |
| VIII. 90% Coverage Gate    | **N/A**  | TypeScript extension, not `iris-agentic-dev-core` Rust crate                                           |
| IX. Tool Lift              | **N/A**  | No new MCP tools                                                                                       |
| X. ObjectScript Coverage   | **N/A**  | No ObjectScript                                                                                        |

No gate violations.

---

## New Files

```text
vscode-iris-agentic-dev/src/
  download.ts        ← downloadBinary(url, dest, onProgress): Promise<void>
  platform.ts        ← getBinaryName(), getDownloadUrl(version)
  managedInstall.ts  ← resolveServerBinary(context): Promise<string | null>
```text

`extension.ts` is modified: `findIrisDev()` replaced by `resolveServerBinary()`;
`activate()` becomes async.

Tests:

```text
vscode-iris-agentic-dev/test/
  platform.test.cjs    ← URL/name construction for all 4 platforms
  managedInstall.test.cjs  ← version check, cache hit/miss, error fallback
```text
