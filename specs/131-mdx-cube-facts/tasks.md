# Tasks: 131 MDX cube facts

Test-first inside every phase. Live runs use `--test-threads=1` against iris-dev-iris.

## Phase 1: Setup

- [x] T001 Confirm `%DeepSee` builds a cube in USER on iris-dev-iris with a throwaway probe; record the result in `specs/131-mdx-cube-facts/research.md` § Probe

## Phase 2: US1 `sa_schema` says what it is

- [x] T002 [P] [US1] Unit tests (red): name check, guidance text, no skill named, in `crates/iris-agentic-dev-core/tests/unit/test_sa_schema_131.rs`, `mod` line in `tests/unit/main.rs`
- [x] T003 [P] [US1] Binary tests (red): `tools/list` description and `tools/call name=HoleFoods` error, in `crates/iris-agentic-dev-core/tests/binary/sa_schema_131.rs`, `mod` line in `tests/binary/main.rs`
- [x] T004 [US1] Live tests (red): deepsee URL grammar, unknown URL `SA_SCHEMA_NOT_FOUND`, in `crates/iris-agentic-dev-core/tests/integration/test_mdx_131_live.rs`, `mod` line in `tests/integration/main.rs`
- [x] T005 [US1] Implement `SA_SCHEMA_GUIDANCE`, `sa_schema_name_problem`, the 404/empty mapping and the `name` doc in `crates/iris-agentic-dev-core/src/tools/info.rs`
- [x] T006 [US1] Fix the `iris_info` description in `crates/iris-agentic-dev-core/src/tools/mod.rs`; update `docs/tools.md` if it repeats "SQL Analytics"
- [x] T007 [US1] Gate: T002–T004 green; `cargo fmt --all -- --check`, clippy, unit suite green

## Phase 3: US2 cube fixture

- [x] T008 [US2] Live test (red): fixture builds, `%GetCubeList` lists both cubes, `%GetDimensionList` lists the dimensions, `SELECT FROM IadLive131Sales` equals the row count, teardown leaves no `IadLive131.*`, in `test_mdx_131_live.rs`
- [x] T009 [US2] Write `crates/iris-agentic-dev-core/tests/fixtures/mdx131/` (`Sale.cls`, `SalesCube.cls`, `OtherCube.cls`, `README.md`) and the setup/teardown helpers in `test_mdx_131_live.rs`
- [x] T010 [US2] Gate: T008 green serially

## Phase 4: US3 claims measured

- [x] T011 [US3] One live test per claim in `research.md` § Claims (red first against the fixture, asserting IRIS's answer) in `test_mdx_131_live.rs`
- [x] T012 [US3] Fill `specs/131-mdx-cube-facts/research.md` § Claims: quote, observed, verdict, test
- [x] T013 [US3] Gate: whole `test_mdx_131_live` green serially; fixture package empty afterwards

## Phase 5: US4 review draft

- [x] T014 [US4] Update `~/.claude/jobs/44a6964a/tmp/pr142-review.md` with the verdicts; run `/no-ai-slop` on it; do not post
- [x] T015 [US4] Check draft verdicts against `research.md` row for row

## Phase 6: Polish

- [x] T016 CLAUDE.md Recent Changes entry for 131
- [x] T017 Full unit + binary suites, `test_test_target_layout`, fmt, clippy; commit locally

## Phase 7: US5 `&sql` inside `iris_execute`

- [x] T018 [P] [US5] Unit tests (red): position reads, `TOP` kept in the SQL, no block syntax, `SQLCODE`/`%msg`/`%ROWCOUNT` set, in `crates/iris-agentic-dev-core/tests/unit/test_sql_translate_131.rs`, `mod` line in `tests/unit/main.rs`
- [x] T019 [P] [US5] Live tests (red): HTTP and `docker_only` cases, in `crates/iris-agentic-dev-core/tests/integration/test_exec_sql_131_live.rs`, `mod` line in `tests/integration/main.rs`
- [x] T020 [US5] `translate_select_into` / `translate_dml` rewrite and the HTTP/terminal code split in `crates/iris-agentic-dev-core/src/tools/mod.rs`; description and `translate_sql` doc; `docs/tools.md`
- [x] T021 [US5] Move the 035 tests to the new contract (`tests/unit/test_sql_translate.rs`, `tests/integration/test_sql_translate_e2e.rs`)
- [x] T022 [US5] Gate: T018–T019 green, lib + unit + binary + misc suites, live sql tests serially, fmt, clippy; commit locally
