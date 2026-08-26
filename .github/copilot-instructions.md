# Copilot instructions — iris-agentic-dev

MCP server in Rust (2021, two crates: `iris-agentic-dev-core` for tools and the server,
`iris-agentic-dev-bin` for the CLI) that gives AI coding agents tools against a live IRIS
instance.

`AGENTS.md` has the build commands, the dev container, and the file map. Read it rather
than guessing — this file only covers the rules that trip up a first PR.

## Tests

Write the test before the code. Three layers, and a change usually needs more than one:

1. **Config round-trip** — parse a TOML _string_ with `toml::from_str`, not a struct
   literal, then assert the resulting fields. A struct literal cannot catch a key that
   serde silently drops because the field is missing.
2. **Binary invocation** — for any CLI flag or `mcp.rs` wiring, spawn the binary and drive
   it over stdio with `initialize` + `tools/list` or `tools/call`, then assert on the
   JSON-RPC response. Catches a flag that parses but was never wired to anything.
3. **Live IRIS** — anything that talks to IRIS runs `#[ignore]` against the real container.

**Never mock IRIS.** No mocked Atelier client, no stubbed responses. A mocked IRIS test
passes while the real code path is broken, which is worse than no test.

Integration and e2e runs need `--test-threads=1`; the test binaries share env vars and
race each other otherwise.

The question to ask before opening a PR: if I changed this flag, field, or file silently,
would any test fail? If not, the test is missing.

## Enforcement checks that will fail CI

- `cargo fmt --all -- --check` and `cargo clippy --all-targets -- -D warnings`.
- Every cargo invocation in CI passes `--locked`. A drifting `Cargo.lock` dirties the
  version string that `check_config` reports, so commit the lock with the change.
- Adding a skill means updating three manifests plus the skill on disk:
  `crates/iris-agentic-dev-core/src/skills/bundled.rs`, `[provides] skills` in
  `iris-agentic-dev.toml`, and `skills.sh.json`. `test_skill_manifest_sync` fails if any
  one is missed.
- Anything that must agree with the workspace version (`Cargo.toml`, `package.json`,
  `.claude-plugin/plugin.json`) needs a cross-file assertion test when it is added.

## Writing ObjectScript

Skill files and test fixtures contain ObjectScript. Follow `skills/skills/objectscript-guardrails`
and `objectscript-review` — first line `Set tSC = $$$OK`, last line `Quit tSC`, `$$$ISERR()`
never a bare `$`, params `pName`, locals `tName`, no `$GET` on object properties, no `text`
columns in SQL, no `ORDER BY` with `SELECT TOP`.

## Conventions

- Conventional commit subjects (`feat:`, `fix:`, `docs:`, `chore:`). A commit-msg hook
  rejects anything else.
- Never credit an AI assistant in a commit message, changelog, or doc.
- After editing Markdown, run `markdownlint-cli2 --fix` and `prettier --write` on it.
