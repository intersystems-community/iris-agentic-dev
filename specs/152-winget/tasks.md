# Tasks: winget package (152)

Tests first; each task names the test that fails before it.

- [x] T001 Tests for the generator: schema, fields, refusals (`tests/e2e/test_winget_manifests.py`)
- [x] T002 Vendor the 1.10.0 version, defaultLocale and installer schemas
- [x] T003 `packaging/winget/winget_manifests.py` (T001 green: 20 tests)
- [x] T004 Tests for the submit script: hash from `SHA256SUMS`, writes only under `--submit`, no token
- [x] T005 `scripts/winget-submit.sh`; dry run against v1.7.1 verified the asset and wrote the three files
- [x] T006 Test for the CI job; `winget-install` job in `ci.yml` (actionlint and zizmor clean)
- [x] T007 `jsonschema` in `.github/requirements/test.txt` (hash-pinned) so CI runs the schema tests
- [x] T008 README winget line (marked pending review), release checklist step and asset list
- [ ] T009 First CI run of `winget-install` on a pushed branch (needs Tom's go to push)
- [ ] T010 First submission: `scripts/winget-submit.sh 1.7.1 --submit` as isc-tdyar (needs Tom's go, and the Microsoft CLA)
- [ ] T011 After the PR merges: release body line, README drops "pending"
