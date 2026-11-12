# Quickstart: SQL storage maps (172)

## Run the tests

```bash
# pure modules, no IRIS
RUST_TEST_THREADS=12 cargo test --features testing --test unit storage_map

# live, on iris-dev-iris (web port 52780, USER)
docker ps --filter name=iris-dev-iris
cargo test --features testing --test integration -- --test-threads=1 --include-ignored storage_map storage172 doc_put_result_status_172

# binary schema check
cargo build && IAD_BINARY=./target/debug/iris-agentic-dev \
  cargo test --features testing --test binary storage_map -- --include-ignored

# rewrite the golden files after a deliberate model change (then bump model_version)
IAD_REGEN_GOLDEN=1 cargo test --features testing --test unit test_storage_map_golden_172

# regenerate the pairwise corpus and its forbidden-pair list on live IRIS (then review the diff)
IAD_REGEN_GENERATED=1 cargo test --features testing --test integration test_storage_map_generated_172 -- --test-threads=1 --include-ignored

# local only: compare a private corpus directory with the public fixtures
IAD_STORAGE_CORPUS=/path/to/dir cargo test --features testing --test integration storage_map_corpus_coverage -- --include-ignored
```

The Mermaid render test needs `mmdc` (`npm i -g @mermaid-js/mermaid-cli`); without it the test fails unless
`IAD_ALLOW_SKIP=1`.

## Try it

```text
iris_storage_map action=read class=IadSm172.Person
iris_storage_map action=diagram class=IadSm172.Person
iris_storage_map action=uses global=^IadSm172Person
iris_storage_map action=edit class=IadSm172.Person version=<from read> \
  ops=[{"op":"add","path":"/maps/Master/data/Email","value":{"node":"\"contact\"","delimiter":"\"^\"","piece":"1"}}]
```

## Fixtures

Load and seed the fixtures by hand with the helper the live tests use
(`tests/integration/storage172_rig.rs`, `load_fixture(name)` / `drop_fixture(name)`). Every fixture class is in
package `IadSm172`, and its globals start with `^IadSm172`.
