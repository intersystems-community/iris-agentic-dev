# Data Model: 069-vscode-binary-install

No persistent database. State is two files in `globalStorageUri`.

## VersionMarker

Plain-text file at `globalStorageUri/iris-agentic-dev.version`.

| Field   | Type   | Notes                                                                              |
| ------- | ------ | ---------------------------------------------------------------------------------- |
| content | string | Exact version string, e.g. `"0.9.5"`. No newline trimming required — trim on read. |

**Lifecycle**: Created on first successful download. Updated on every re-download.
Deleted (or absent) → treated as cache miss → triggers download.

## ManagedBinary

File at `globalStorageUri/iris-agentic-dev-{version}/iris-agentic-dev[.exe]`.

| Field      | Type             | Notes                                                                 |
| ---------- | ---------------- | --------------------------------------------------------------------- |
| path       | string (fs path) | Absolute path to the executable                                       |
| platform   | string           | `macos-arm64` \| `macos-x86_64` \| `linux-x86_64` \| `windows-x86_64` |
| version    | string           | Matches `context.extension.packageJSON.version`                       |
| executable | boolean          | `chmod 0o755` applied on mac/linux before VersionMarker is written    |

**Lifecycle states**:

- **Absent** — no file at path, no version marker
- **Downloading** — `.tmp` file being written; version marker not yet updated
- **Ready** — file exists, is executable, version marker matches extension version
- **Stale** — file exists but version marker differs from extension version → triggers re-download

## BinarySource (runtime, not persisted)

Result of `resolveServerBinary()`. Not written to disk.

| Field | Type                                   | Notes                                |
| ----- | -------------------------------------- | ------------------------------------ |
| path  | string                                 | Absolute path to the resolved binary |
| tier  | `"setting"` \| `"path"` \| `"managed"` | Which priority tier provided it      |
