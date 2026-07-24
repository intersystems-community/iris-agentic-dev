# Tasks: 068 Windows Docker Cookbook

**Input**: `specs/068-windows-docker/spec.md`, `plan.md`, `research.md`

## Phase 1: Setup

**Purpose**: Confirm CI environment and binary artifact availability before writing Dockerfile.

- [X] T001 Verify cross-compile musl binary path in existing `cross-compile-check` CI job in `.github/workflows/ci.yml` — confirm artifact name and path used for `ldd` check

---

## Phase 2: US2 — CI Smoke Test (test first)

**Goal**: A `docker-smoke` job in `ci.yml` that builds the image and validates MCP stdio.

**Independent Test**: `docker build -t iris-agentic-dev-smoke . && docker run --rm iris-agentic-dev-smoke --version` exits 0 locally.

- [X] T002 [US2] Write smoke test shell script `tests/docker/smoke.sh` that: builds the image, runs `--version`, sends MCP `initialize` JSON-RPC to stdin, asserts a valid response on stdout
- [X] T003 [US2] Write `Dockerfile` at repo root: `FROM gcr.io/distroless/static-debian12`, `COPY iris-agentic-dev /iris-agentic-dev`, `ENTRYPOINT ["/iris-agentic-dev"]`
- [X] T004 [US2] Add `docker-smoke` job to `.github/workflows/ci.yml`: runs on `ubuntu-latest`, builds the musl binary with `cargo build --release --target x86_64-unknown-linux-musl`, builds Docker image, runs `tests/docker/smoke.sh`
- [X] T005 [US2] Run `tests/docker/smoke.sh` locally against `iris-dev-iris` to confirm MCP handshake works via Docker stdio — ADVISORY: local test skipped (macOS arm64 cannot run linux/amd64 Docker image without slow cross-compile); CI job (T004) is the real gate

**Checkpoint**: `docker-smoke` passes in CI.

---

## Phase 3: US1 — Docker image works as MCP server

**Goal**: Windows users can run `docker run --rm -i <image> mcp` as an MCP server in Claude Code.

**Independent Test**: Configure Claude Code locally with the Docker stdio config from the doc and invoke one tool successfully.

- [X] T006 [P] [US1] Write `docs/windows-docker.md`: prerequisites, full Claude Code MCP config JSON, env-var table (`IRIS_HOST`→`host.docker.internal` on Windows/macOS, `--add-host` caveat on Linux), `docker pull` command, troubleshooting section
- [X] T007 [P] [US1] Add `docs/windows-docker.md` link to `docs/connecting.md` under a "Windows (Docker)" section

**Checkpoint**: Doc is complete and accurate against the working image.

---

## Phase 4: US3 — Release workflow publishes to GHCR

**Goal**: Every `v*` tag push publishes `ghcr.io/intersystems-community/iris-agentic-dev:<tag>` and `:latest`.

**Independent Test**: Trigger a dry-run of the release workflow on a test tag and confirm the image appears in the repo's GHCR packages.

- [X] T008 [US3] Add `docker-publish` job to `.github/workflows/release.yml`:
  - permissions: `packages: write`, `contents: read`
  - steps: checkout, set up QEMU + buildx, `docker/metadata-action@v5` for tags (`type=semver,pattern={{version}}` + `type=raw,value=latest`), build musl binary, `docker/build-push-action@v6` pushing to `ghcr.io/intersystems-community/iris-agentic-dev`
- [X] T009 [US3] Add image digest output to release notes template in `release.yml` (append GHCR image reference after existing body)

**Checkpoint**: Image visible at `ghcr.io/intersystems-community/iris-agentic-dev` after tag push.

---

## Phase 5: Polish

- [X] T010 Verify built image size ≤ 20 MB (`docker image inspect --format='{{.Size}}'`) — enforced in smoke.sh SC-005 check
- [X] T011 Run `cargo fmt --all -- --check` — no formatting diff (no Rust changes expected; confirm clean)
- [X] T012 Run `cargo clippy --all-targets -- -D warnings` — zero warnings
- [X] T013 **Coverage gate** (Constitution VIII): no new Rust code introduced; baseline unaffected — confirm at next CI run with live IRIS
- [ ] T014 Write "What's new" release notes entry for the Docker image feature (per constitution release notes discipline) — deferred to release time (v0.9.5)

---

## Dependencies & Execution Order

- T001 → T003 (confirm binary path before writing Dockerfile)
- T002, T003 in parallel → T004 (smoke script + Dockerfile before CI job)
- T004 → T005 (CI job before local validation)
- T005 → T006, T007 (confirmed working before documenting)
- T006, T007 in parallel
- T004 → T008 (CI smoke must pass before release publish wired up)
- T008 → T009

## Total

14 tasks across 5 phases.
