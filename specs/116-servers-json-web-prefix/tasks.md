# 116 — `web_prefix` for the iad-native server registry: tasks

Written retrospectively alongside `plan.md`; every box reflects work that shipped in commit
`ed8064c`, verified by the runs recorded under Done criteria.

## Phase 1: Unit tests (written first)

Every test resolves its URL from a parsed `servers.json` **string**, never a struct literal. A
field the struct has but serde never fills is the #110 pattern, and a struct literal cannot catch
it.

- [x] T001 `a_prefixed_entry_resolves_to_a_prefixed_url` and
      `two_instances_on_one_host_and_port_resolve_to_different_urls` — the reporter's layout: two
      HealthShare instances behind one gateway, told apart only by prefix.
- [x] T002 `an_entry_without_the_key_resolves_unchanged` and `https_and_a_prefix_compose` — no
      regression for the common case.
- [x] T003 `the_vscode_spelling_pathprefix_is_accepted_as_an_alias`.
- [x] T004 `the_four_prefix_spellings_resolve_to_one_url` and
      `an_empty_or_slashes_only_prefix_means_no_prefix` — the edge cases named in the spec.
- [x] T005 `the_native_and_vscode_branches_agree_on_one_url` — asserts the shared helper against
      each branch's own format string, so re-inlining either one fails.
- [x] T006 `a_prefix_containing_a_url_is_refused` and `ordinary_prefixes_pass_validation`.
- [x] T007 `saving_an_entry_without_a_prefix_writes_no_key` and
      `an_unknown_key_does_not_break_the_load` — file compatibility in both directions.
- [x] T008 `add_server_declares_web_prefix` — the parameter is advertised, not just accepted.

## Phase 2: Implementation

- [x] T009 `src/iris/servers_config.rs` — `web_prefix: Option<String>` with `#[serde(default,
  alias = "pathPrefix", skip_serializing_if = "Option::is_none")]`, plus
      `validate_web_prefix` refusing a value carrying a scheme or host.
- [x] T010 `src/iris/connection_pool.rs` — `web_prefix_path_part` and `native_base_url`; the
      native branch and the VS Code branch both call the helper, replacing two inline copies.
- [x] T011 `src/tools/server_tools.rs` — `AddServerParams.web_prefix`; `probe_base_url` becomes
      the implementation and `probe_server` a wrapper over it.
- [x] T012 `src/tools/mod.rs` — `iris_servers` reports `base_url` and `web_prefix` (derived with
      `extract_web_prefix_from_url`, so all four sources are covered) and probes each entry's own
      `base_url`; `iris_add_server` validates before any write and preserves an existing
      password; `iris_import_servers` carries `path_prefix` across.
- [x] T013 `src/tools/output_schemas.rs` — `ServerEntry.web_prefix` with `skip_serializing_if`.
- [x] T014 Tool descriptions for `iris_servers` and `iris_add_server` updated — the description is
      what an agent reads to decide whether the parameter exists.

## Phase 3: Binary invocation tests (no live IRIS)

- [x] T015 `tests/binary/servers_json_web_prefix.rs`, 5 `#[ignore]` tests over stdio JSON-RPC:
      `iris_add_server_advertises_web_prefix`, `iris_servers_reports_the_prefix`,
      `a_prefix_less_entry_reports_no_prefix`,
      `iris_add_server_persists_and_updates_the_prefix` (temp `HOME`; asserts the written file,
      re-registers with a new prefix, asserts one entry, then clears the real keychain via
      `iris_reload_pool` + `iris_remove_server`),
      `a_prefix_containing_a_url_is_refused_at_registration` (asserts `INVALID_PARAMS`, that the
      message names `web_prefix`, and that no registry file was written).

## Phase 4: Live IRIS integration tests

- [x] T016 `tests/integration/test_web_prefix_live.rs`, 3 `#[ignore]` tests against
      `iris-dev-iris`. The container is registered twice — once at the root as a control, once
      under `/iad116-no-such-prefix` — and the prefixed one must come back
      `reachable == false` with `http_status >= 400`. A prefix asserted only as a string proves
      the `format!` call, not that IRIS was asked for a different path.
      `iris_servers_probes_the_prefixed_url_not_the_root` covers the false-green facet: `auth ==
  false` is what proves the probe asked for the prefixed path.
- [x] T017 Verified the tests leave no residue — no `test-116` entries in the real
      `servers.json`, no matching keychain items.

## Phase 5: The third source

- [x] T018 `workspace_config::instance_base_url` had its own copy of the same slash trimming;
      collapsed onto `web_prefix_path_part`. Found while writing the docs, not by a test — which
      is why T019 exists.
- [x] T019 `the_toml_branch_agrees_with_the_registry_on_one_url` — all three sources resolve one
      host/port/prefix triple to one URL, across six prefix spellings including whitespace. The
      pre-existing `test_workspace_config` prefix tests still pass unchanged.

## Phase 6: Docs and polish

- [x] T020 `docs/connecting.md` — a `servers.json` schema example, a section on instances behind a
      gateway (including that the symptom of a missing prefix is a 404 from every tool), the
      `pathPrefix` alias, and the `[instance.*]` equivalent.
- [x] T021 `docs/tools.md` — `iris_add_server` parameter table with `web_prefix`, the
      `iris_servers` output fields, the probe behaviour change, and the ad-hoc `iris_test_server`
      limit.
- [x] T022 `specs/next-release-notes.md` — What's new entry crediting @isc-ndittber and naming
      #129, plus two Notable fixes entries for the listing and probe facets.
- [x] T023 `cargo fmt --all -- --check` and `cargo clippy --features testing --all-targets --
  -D warnings` clean; `.specify/gates/verify.sh` `failed=0 warnings=0` (needed `npm ci` in the
      worktree — the parity gate caught a global prettier older than the pin).

## Done criteria

- Unit: 14 new tests green; `unit` target 2380 passed / 0 failed ✓
- Binary: 5 new tests green with `--include-ignored --test-threads=1` ✓
- Live IRIS: 3 new tests green against `iris-dev-iris` ✓
- Non-integration sweep across all six targets, 0 failed ✓
- fmt, clippy, gates clean ✓
- `docs/connecting.md`, `docs/tools.md`, release notes updated ✓
- No test residue in the real registry or keychain ✓

## Not done here

- Ad-hoc `iris_test_server` (`host` + `web_port`) still probes the root; no prefix parameter.
  Raised with the reporter rather than assumed.
