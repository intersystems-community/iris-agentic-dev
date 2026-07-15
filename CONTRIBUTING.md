# Contributing

Contributions are welcome — bug reports, feature requests, skills, and code.

## Quick start

```bash
git clone https://github.com/intersystems-community/iris-agentic-dev
cd iris-agentic-dev
cargo build
cargo test
```

See [CLAUDE.md](CLAUDE.md) for the full dev setup including the local IRIS container.

## What to work on

Issues labeled [`good first issue`][gfi] are scoped and self-contained.
Issues labeled [`help wanted`][hw] are higher impact but may need more context —
comment before starting so we can align on approach.

[gfi]: https://github.com/intersystems-community/iris-agentic-dev/labels/good%20first%20issue
[hw]: https://github.com/intersystems-community/iris-agentic-dev/labels/help%20wanted

## Pull requests

- Open an issue first for anything non-trivial so the approach can be agreed before code is written.
- **Tests:** ideally included for any code change, especially anything that touches IRIS behaviour.
  See the testing philosophy in [CLAUDE.md](CLAUDE.md) — live container tests are preferred over mocks.
- **Benchmarks:** ideally included for skill and tool contributions. See the skill section below.
- `cargo fmt --all` and `cargo clippy -- -D warnings` must pass before submitting.
- Keep PRs focused. One logical change per PR.

## Contributing a skill

Skills live in `skills/` (full) and `light-skills/skills/` (trimmed for token budget).
Each skill is a directory with a `SKILL.md` and optional `references/` subdirectory.

**Ideal acceptance bar:** demonstrate measurable lift — ideally **+20% pass-rate improvement**
on the benchmark suite compared to the no-skill baseline. This is a best-effort guideline,
not a hard gate; a well-written skill without benchmark coverage can still be accepted on merit.

How to show lift:

1. **If your skill addresses an existing task category** (GEN, MOD, DBG, SCM, LEG):
   run the benchmark harness (`benchmark/021/`) with and without the skill and include the
   before/after scores in your PR description.

2. **If your skill covers a new domain** (e.g. Python interoperability, HealthShare, TrakCare):
   ideally add at least **3 benchmark tasks** in `benchmark/021/tasks/` for that domain alongside
   the skill and show +20% lift on those tasks. Tasks paired with a skill are strongly preferred
   — they prove the skill works rather than documenting what you hope it does.

Run the harness:

```bash
cd iris-agentic-dev
export IRIS_HOST=localhost IRIS_WEB_PORT=52780
export IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS
export ANTHROPIC_API_KEY=sk-ant-...
python -m benchmark.021.runner
```

See [`benchmark/021/README.md`](benchmark/021/README.md) for full instructions and
[`light-skills/skills/objectscript-review/`](light-skills/skills/objectscript-review/)
for a reference skill example.

## Bug reports

Include:

- `iris-agentic-dev --version`
- IRIS version and edition (Community / Enterprise / HealthShare)
- Deployment (native Windows/Linux, Docker, VS Code extension)
- The exact tool call that failed and the full error output
- Whether the issue is reproducible or intermittent

## License

By contributing you agree that your contributions will be licensed under the
[MIT License](LICENSE).
