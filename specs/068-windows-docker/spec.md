# Spec 068: Windows Docker Cookbook

**Branch**: `068-windows-docker`  
**Status**: Draft

## Overview

Windows users cannot run the native `iris-agentic-dev` binary because the corporate signing
pipeline is not available for this project. This spec delivers a signed Linux Docker image as
a drop-in alternative: users run `docker run --rm -i ghcr.io/…/iris-agentic-dev:latest mcp`
and configure it as an MCP server in Claude Code exactly as they would a native binary.

The image is Linux x86_64 (statically linked musl binary from the existing cross-compile
pipeline), published to GHCR on every release. A CI smoke test on `ubuntu-latest` validates
the image builds and the binary responds correctly via stdio.

## User Stories

### US1 — Windows user installs via Docker

**As a** Windows user who cannot run the unsigned native binary,  
**I want** a Docker image I can point Claude Code at as an MCP server,  
**so that** I can use iris-agentic-dev without waiting for corporate code signing.

**Acceptance criteria:**
- Running `docker run --rm -i ghcr.io/intersystems-community/iris-agentic-dev:latest mcp` starts an MCP server over stdio
- Claude Code config example documented in `docs/windows-docker.md`
- Image is published to GHCR automatically on each release tag

### US2 — CI smoke test validates the image

**As a** maintainer,  
**I want** a CI job that builds and smoke-tests the Docker image on every push to master,  
**so that** a broken image is caught before release.

**Acceptance criteria:**
- `docker-smoke` job added to `ci.yml`, runs on `ubuntu-latest`
- Job builds the image from `Dockerfile`
- Job runs `docker run --rm iris-agentic-dev-smoke --version` and asserts exit 0
- Job sends a minimal MCP `initialize` JSON-RPC message via stdin and asserts a valid response
- Job is non-blocking (uses `continue-on-error: false` — it IS a real gate)

### US3 — Release workflow publishes the image

**As a** Windows user,  
**I want** the GHCR image to be updated automatically on every release,  
**so that** I always get the current version without manual steps.

**Acceptance criteria:**
- `release.yml` builds and pushes `ghcr.io/intersystems-community/iris-agentic-dev:<tag>` and `:latest` on every `v*` tag
- Image is multi-platform: `linux/amd64` (only — arm64 is a future concern)
- Image digest is recorded in the release notes

## Out of Scope

- Windows native signing (blocked on corporate pipeline)
- arm64 / Apple Silicon Docker image (future)
- Docker Compose workflow
- Kubernetes / Helm

## Success Criteria

| ID | Criterion |
|----|-----------|
| SC-001 | `docker run --rm -i <image> mcp` starts and responds to MCP `initialize` over stdio |
| SC-002 | CI `docker-smoke` job passes on `ubuntu-latest` for every push |
| SC-003 | GHCR image published on release tag; `:latest` also updated |
| SC-004 | `docs/windows-docker.md` documents full Claude Code config with env-var passthrough |
| SC-005 | Image size ≤ 20 MB (musl static binary + CA certs only) |
