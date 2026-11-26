# Quickstart: index linking in `propose` (173)

## Run the tests

```bash
# pure modules, no IRIS (parallel is fine)
RUST_TEST_THREADS=12 cargo test --features testing --test unit storage_map_link storage_map_candidates \
  storage_map_master storage_map_typing storage_map_bitmap storage_map_usability storage_map_docs_173

# live, on iris-dev-iris (web port 52780, USER); serial
docker ps --filter name=iris-dev-iris
nice -n 15 cargo test --features testing --test integration -j 2 -- --test-threads=1 --include-ignored _173_live

# binary schema check
cargo build && IAD_BINARY=./target/debug/iris-agentic-dev \
  cargo test --features testing --test binary storage_map_schema_173 -- --include-ignored

# rewrite the pinned propose answers after a deliberate change
IAD_REGEN_GOLDEN=1 cargo test --features testing --test integration storage_map_corpus_173 -- --test-threads=1 --include-ignored
```

On the laptop run one test file at a time with `nice -n 15 … -j 2`; the full integration target runs on mac-studio.

## Try it

```text
# a global with index globals beside it
iris_storage_map action=propose global=^IadSm173A

# name an index global yourself
iris_storage_map action=propose global=^IadSm173A index_globals=["^IadSm173AX"]

# a class that already has its master map: draft the index maps it lacks
iris_storage_map action=propose class=IadSm173.Person

# does a proposed index map get used?
iris_storage_map action=check class=IadSm173.Person model=<an index map from propose>
```

## Fixtures

Classes under `IadSm173` and globals starting `^IadSm173`, loaded and dropped by the live tests' rig
(`tests/integration/storage172_rig.rs` gains `load_fixture_173`). Each seed file carries the known answer (field,
transform, length, null marker, ID position, verdict) the test asserts.
