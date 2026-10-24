# Security policy

iris-agentic-dev is an open-source (MIT) MCP server that lets an AI coding assistant work against an
InterSystems IRIS instance. It is maintained by Thomas Dyar (InterSystems) with community contributors. It
is a community project: there is no support contract and nothing here is a service-level guarantee.

## Reporting a vulnerability

Email **<tdyar@intersystems.com>** with "iris-agentic-dev security" in the subject. Please include the version
(`iris-agentic-dev --version`), what you did, what happened, and what you expected. Do not open a public
issue for a vulnerability.

Reports in scope: anything that lets a caller get past a safety gate (the write and destructive tiers, the
data-policy and protected-data checks, the system-global blocklist, a connection's `[policy.*]` block),
leak or mishandle a credential, run code or reach data the configured IRIS account could not, or take over
the process through the MCP interface.

Out of scope: what a configured, authorised IRIS account can already do; the behaviour of the AI assistant or
model provider you connect (see `docs/production-profile.md`); vulnerabilities in IRIS itself (report those to
InterSystems through the usual channel).

## Supported versions

Only the latest release receives security fixes. Releases are listed at
<https://github.com/intersystems-community/iris-agentic-dev/releases>.

## What to expect

- An acknowledgement within 5 business days.
- An assessment of severity and a plan, shared with the reporter.
- A fix, a release and a notice, with credit to the reporter if they want it.
- Coordinated disclosure: please give me up to 90 days before publishing details, and I will tell you
  sooner if a fix ships earlier.

Targets for a fix to be released, counted from the day I confirm the report. They are targets and not a guarantee:

| Severity | Meaning                                                                        | Target to release a fix |
| -------- | ------------------------------------------------------------------------------ | ----------------------- |
| Critical | Bypass of a safety gate or credential disclosure that needs no special access  | 14 days                 |
| High     | The same, but needs a configuration or access the attacker is unlikely to have | 30 days                 |
| Medium   | Hardening gaps and denial of service                                           | the next release        |
| Low      | Anything else                                                                  | best effort             |

## How you hear about a fix

Every release has release notes on GitHub. A security fix is named as one there and, once fixed, is also
published as a GitHub security advisory. The VS Code extension updates through the Marketplace, and the
binary through GitHub releases and Homebrew. There is no mailing list: watch the repository's releases, or
ask to be told directly if you operate it in a regulated environment.

## What is tested before a release

- Every pull request runs `cargo fmt`, `cargo clippy` with warnings as errors, and every Rust test target,
  with no IRIS container. Tests that need live IRIS skip themselves in that run; the pure-logic tests,
  including those for the safety gates, run in full.
- Every push to master and to a `fix/**` or `release/**` branch also runs the `e2e-tests` job: the whole
  suite with `--include-ignored` against a fresh IRIS Community 2025.3 container. That is where the live
  regression tests for each safety-gate bypass fixed so far run. Pull requests do not run this job, so a
  change reaches live IRIS in CI only after it is pushed to one of those branches. A release branch must
  be green here before it is merged (`docs/release-checklist.md`).
- `e2e-tests` skips a short list of live tests that need what a runner cannot have, such as an AI Hub early-access
  image, an Enterprise image, a namespace with Samples-BI built, VS Code Server Manager credentials, or an
  LLM API key. The AI Hub and Samples-BI tests are run by hand against a local instance before each
  release; the others are not run in CI.
- Every pull request, every push to master and a weekly schedule run `cargo audit` against the locked Rust
  dependencies and `npm audit` against the VS Code extension's production dependencies. A known
  vulnerability fails the job.
- A gate check (`objectscript-splice`) fails the build when a tool puts a parameter inside a quote of
  generated ObjectScript or SQL without escaping it, and a live test per tool tries a crafted parameter
  against a sentinel global with write tools off (spec 137).
- Each release publishes a CycloneDX SBOM (`iris-agentic-dev-<version>.cdx.json`) listing every locked
  dependency.

Not done: there is no independent penetration test, no fuzzing and no static-analysis service beyond the
compiler and clippy.

## Release integrity

Each release attaches `SHA256SUMS` covering all five binaries. The Homebrew formula pins the same hashes.
The published binaries are not signed with a publisher certificate: the macOS build carries an ad-hoc
signature, and the Windows release binary is unsigned. A signing route for Windows exists in CI and is not yet
part of the release. To avoid trusting a download, check it against `SHA256SUMS` from the same tagged release,
or build from source: `cargo build --locked --release -p iris-agentic-dev`.
See `docs/production-profile.md` for an install that pins a version and downloads nothing at run time.

### Verifying a download

Download the binary and `SHA256SUMS` from the same tagged release into one directory, then run:

```sh
shasum -a 256 -c --ignore-missing SHA256SUMS   # macOS
sha256sum -c --ignore-missing SHA256SUMS       # Linux
```

Each prints `<file>: OK` for a match. On Windows, compare
`(Get-FileHash .\iris-agentic-dev-windows-x86_64.exe).Hash` with the line for that file in `SHA256SUMS`.
The direct-download commands in `README.md` run this check before installing.

Releases will also carry signed build provenance; this section will say how to check it.

## Running it against real data

Read `docs/production-profile.md` before pointing it at an instance that holds personal or health
information. It lists what leaves the machine, the settings that reduce the risk, and what no setting
protects.
