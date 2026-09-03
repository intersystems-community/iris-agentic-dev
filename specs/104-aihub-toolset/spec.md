# Spec 076 — AI Hub ToolSet (stdio pass-through)

## Overview

Ship an importable ObjectScript class `IAD.ToolSet.IrisAgenticDev` that wires an
AI Hub agent to the iris-agentic-dev MCP server over stdio. No changes to the
iris-agentic-dev Rust binary — this is purely an ObjectScript artifact that lives
in `contrib/aihub/` and documents how to connect AI Hub agents to all
iris-agentic-dev tools without writing a single wrapper method.

## Problem

AI Hub developers who want to use iris-agentic-dev tools in their agents have no
documented path. They would have to read the `%AI.ToolSet` XData spec, find the
iris-agentic-dev binary, figure out the env vars, and write the XML themselves.
That's too much friction for a tool that should be a one-import setup.

## Goals

- One importable `.xml` export containing `IAD.ToolSet.IrisAgenticDev` and
  `IAD.ToolSet.IrisAgenticDevReadOnly` (read-only subset)
- Works with the binary on PATH or at a configurable absolute path
- Credential injection via IRIS Secure Wallet (`@{wallet:…}`) or env vars
- Tested against a live AI Hub IRIS instance (aihub-iris-116, port 21972)
- Documented in `contrib/aihub/README.md` and referenced from `aihub-eap` skill

## Non-goals

- HTTP/remote transport (Spec 077)
- ObjectScript skill wrappers (Spec 078)
- Auto-install of the iris-agentic-dev binary from ObjectScript

## Functional requirements

### FR-001 — Full toolset class

`IAD.ToolSet.IrisAgenticDev` exposes all iris-agentic-dev tools via `<MCP><Stdio>`.
Env vars injected: `IRIS_HOST`, `IRIS_WEB_PORT`, `IRIS_USERNAME`, `IRIS_PASSWORD`,
`IRIS_NAMESPACE`. All support `@{wallet:…}` and `@{env:…}` references.

### FR-002 — Read-only subset class

`IAD.ToolSet.IrisAgenticDevReadOnly` extends IrisAgenticDev with `<Exclude>` rules
that remove write-gated tools: `iris_compile`, `iris_doc` (put/delete modes),
`iris_execute`, `iris_query` (write mode), `iris_global` (set/kill), `iris_source_control`,
`iris_admin` (create/delete), `iris_production_item`, `iris_credential_manage`,
`iris_lookup_manage` (write), `iris_lookup_transfer` (import).

### FR-003 — Platform-specific executable path

`<Stdio>` uses a platform fallback chain:

- macOS ARM: `/opt/homebrew/bin/iris-agentic-dev`
- macOS x86: `/usr/local/bin/iris-agentic-dev`
- Linux: `/usr/local/bin/iris-agentic-dev`
- Catch-all: `iris-agentic-dev` (PATH lookup)

### FR-004 — Configurable executable via env

`IRIS_AGENTIC_DEV_BIN` env var overrides the executable path when set.

### FR-005 — Export artifact

`contrib/aihub/IAD.ToolSet.xml` is a valid IRIS export importable via
`Do $system.OBJ.Load("IAD.ToolSet.xml", "ck")`. Ships in the repo.

### FR-006 — README

`contrib/aihub/README.md` covers: prerequisites (IRIS AI Hub build 162+,
iris-agentic-dev binary), import steps, wallet setup, example agent snippet,
read-only variant, troubleshooting.

### FR-007 — aihub-eap skill update

`skills/skills/aihub-eap/SKILL.md` gains a section on importing and using the
ToolSet classes, cross-referencing `contrib/aihub/README.md`.

## Test requirements

### T-076-01 — Import smoke test

Import `IAD.ToolSet.xml` into the aihub-iris-116 container. Verify compile succeeds
with zero errors.

### T-076-02 — Tool discovery

Instantiate `IAD.ToolSet.IrisAgenticDev`, call `%Discover()`, verify result contains
at least 20 tool names including `iris_execute`, `iris_doc`, `iris_query`, `check_config`.

### T-076-03 — Read-only excludes write tools

Instantiate `IAD.ToolSet.IrisAgenticDevReadOnly`, call `%Discover()`, verify
`iris_compile`, `iris_execute`, `iris_source_control` are absent.

### T-076-04 — Agent round-trip

Create an `%AI.Agent` with the full toolset. Call `check_config` via the agent.
Verify the response contains `"connected": true` or `"connection_source"`.

## Acceptance criteria

- `IAD.ToolSet.xml` imports cleanly into a fresh AI Hub namespace
- T-076-02 through T-076-04 pass against aihub-iris-116
- `contrib/aihub/README.md` exists and passes markdownlint
- `aihub-eap` skill references the new contrib artifact
