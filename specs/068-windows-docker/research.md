# Research: 068 Windows Docker Cookbook

## Docker on GHA `windows-latest`

**Decision**: Use `ubuntu-latest` for the Docker smoke-test CI job, not `windows-latest`.

**Why**: `windows-latest` (Windows Server 2025) ships Moby 29.1.5 with Hyper-V, but running
Linux containers requires explicit daemon configuration (no one-liner SwitchLinuxEngine). The
Docker image is Linux x86_64; the correct test host is Linux. Windows users will run the image
via Docker Desktop's WSL2 backend (Linux execution). Testing on `ubuntu-latest` covers the
same code path with zero friction.

**Alternatives considered**:
- `windows-latest` with `daemon.json` Hyper-V config — works but complex setup; not worth it
  since we're testing a Linux image, not Windows containers.

---

## Dockerfile base image

**Decision**: `gcr.io/distroless/static-debian12`

**Why**: The project already builds a fully static musl binary in the `cross-compile-check` CI
job (`x86_64-unknown-linux-musl` via `cargo-zigbuild`, confirmed `not a dynamic executable`).
`distroless/static-debian12` adds CA certificates and tzdata at ~2 MB overhead with no shell,
no package manager, and minimal attack surface. `scratch` would require manually copying CA
certs from a build stage — more fragile for no meaningful gain.

**Alternatives considered**:
- `scratch` — minimal but requires manual CA cert copy; fragile if cert paths change.
- `debian:bookworm-slim` (~80 MB) — only appropriate for dynamically linked binaries. Overkill.
- `alpine` (~7 MB) — uses musl libc, works with our musl binary, but distroless is smaller
  and has less attack surface (no apk, no shell).

**Dockerfile shape** (two-stage build — avoids shipping cross-compile tooling in the final image):

```dockerfile
FROM ghcr.io/cross-rs/x86_64-unknown-linux-musl:edge AS build
# (or just COPY a pre-built release binary — CI builds it before docker build)

FROM gcr.io/distroless/static-debian12
COPY iris-agentic-dev /iris-agentic-dev
ENTRYPOINT ["/iris-agentic-dev"]
```

In CI the binary is built by cargo beforehand and copied in — no Rust toolchain needed in
the Docker build context. Keeps image build fast and image small.

---

## MCP stdio transport via Docker

**Decision**: Fully supported. Claude Code config pattern:

```json
{
  "mcpServers": {
    "iris-agentic-dev": {
      "command": "docker",
      "args": [
        "run", "--rm", "-i",
        "-e", "IRIS_HOST",
        "-e", "IRIS_WEB_PORT",
        "-e", "IRIS_USERNAME",
        "-e", "IRIS_PASSWORD",
        "ghcr.io/intersystems-community/iris-agentic-dev:latest",
        "mcp"
      ],
      "env": {
        "IRIS_HOST": "host.docker.internal",
        "IRIS_WEB_PORT": "52773",
        "IRIS_USERNAME": "_SYSTEM",
        "IRIS_PASSWORD": "SYS"
      }
    }
  }
}
```

`-i` keeps stdin open (no TTY). Claude Code spawns `docker run` as a subprocess over stdio.
`-e VARNAME` (without `=`) passes through from the host env; the `"env"` block in the config
injects into the `docker run` process which then propagates into the container.

`host.docker.internal` resolves to the host machine from inside a Docker container on
Windows (Docker Desktop) and macOS. On Linux it requires `--add-host=host.docker.internal:host-gateway`.
Document this caveat.

**Sources**: MCP spec (stdio transport), Claude Code docs (mcp-quickstart), grafana/mcp-grafana
production Docker stdio example.

---

## Image publishing (GHCR)

**Decision**: Use `docker/build-push-action` with GHCR (`ghcr.io/intersystems-community/iris-agentic-dev`).

**Why**: GHCR is already the natural registry for this repo (same GitHub org). No extra secrets
needed — `GITHUB_TOKEN` has `packages: write` permission. Images are public for public repos.

Tags: `<version>` (e.g., `v0.9.5`) and `latest`, produced by `docker/metadata-action`.

**Platform**: `linux/amd64` only for now. The musl cross-compile is x86_64. arm64 requires
a separate cross-compile target (`aarch64-unknown-linux-musl`) — out of scope.

---

## Performance

Per-message stdio overhead through Docker pipe: negligible (kernel pipe buffer, microsecond
latency). Container cold-start on Windows via Hyper-V: 2–10 seconds on first use. Subsequent
tool calls: no overhead beyond normal process stdio. Acceptable for the MCP use case.

---

## New Rust dependencies

None. The Dockerfile and CI changes are pure YAML + Dockerfile. No new Rust crates.

---

## Constitution compliance notes

- **I. Zero-Install**: Docker is an optional path for Windows users who can't run the native
  binary. The native binary remains the primary install path. ✅
- **II. ObjectScript Sanity**: No new ObjectScript. ✅
- **III. HTTP-First**: No new tools. ✅
- **IV. Test-First**: Smoke test written before/with implementation. ✅
- **V. Output Shape**: No new tools. ✅
- **VI. Environment Guard**: No new write tools. ✅
- **VII. Dependency Minimalism**: No new Rust crates. ✅
- **VIII. 90% Coverage**: No new Rust code paths; existing coverage unaffected. ✅
- **IX. Tool Lift**: No new MCP tools. N/A. ✅
- **X. ObjectScript Coverage**: No ObjectScript. N/A. ✅
