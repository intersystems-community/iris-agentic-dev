//! Guards the aggregated test-target layout, in both crates.
//!
//! Every test file in the workspace is a module of one of the aggregate targets declared in its
//! crate's `Cargo.toml` — `unit`, `integration`, `binary`, `misc` in `iris-agentic-dev-core`, and
//! `bin_unit`, `bin_integration`, `bin_misc` in `iris-agentic-dev-bin`. The failure mode that buys
//! is silent: add `tests/unit/test_new_thing.rs`, forget the `mod test_new_thing;` line in
//! `tests/unit/main.rs`, and the file compiles nowhere and runs never. `cargo test` still says ok.
//! That is the same shape as the #110 serde silent-drop bug, so it gets the same treatment — a test
//! that fails.
//!
//! `autotests = false` is what makes the omission silent. With autodiscovery on, an unlisted file
//! would at least still be picked up as its own target.
//!
//! This file lives in the core crate and reaches sideways into the bin crate, because the core
//! `unit` target is the cheapest place to run it — it needs no IRIS and no binary.
//!
//! Known limit: this file is itself reached through `tests/unit/main.rs`. Deleting its own `mod`
//! line would take the guard down with it. Nothing inside the test tree can close that loop; the
//! `mod` lists are short and a diff that removes one is visible in review.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn core_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// `crates/iris-agentic-dev-bin`, reached from this crate's manifest dir.
fn bin_root() -> PathBuf {
    core_root()
        .parent()
        .expect("core crate dir has a parent")
        .join("iris-agentic-dev-bin")
}

/// A crate's aggregated layout: where it lives, which directories hold aggregators, and the
/// complete set of `path = "tests/..."` values its `Cargo.toml` is allowed to declare.
struct CrateLayout {
    /// For assertion messages.
    name: &'static str,
    root: PathBuf,
    /// Directories relative to the crate root, each of which must hold a `main.rs` aggregator
    /// listing every other `.rs` file beside it. `"tests"` is the flat top-level set.
    aggregated_dirs: &'static [&'static str],
    /// Every `[[test]]` path the manifest declares, aggregators plus any file deliberately left as
    /// its own target.
    expected_test_paths: &'static [&'static str],
}

fn layouts() -> Vec<CrateLayout> {
    vec![
        CrateLayout {
            name: "iris-agentic-dev-core",
            root: core_root(),
            aggregated_dirs: &["tests", "tests/unit", "tests/integration", "tests/binary"],
            expected_test_paths: &[
                "tests/main.rs",
                "tests/unit/main.rs",
                "tests/integration/main.rs",
                "tests/binary/main.rs",
                // tests/skills/ holds one file, so it stays its own target — an aggregator for a
                // set of one would be a `mod` line and nothing else.
                "tests/skills/nopws_skill_test.rs",
            ],
        },
        CrateLayout {
            name: "iris-agentic-dev-bin",
            root: bin_root(),
            aggregated_dirs: &["tests", "tests/unit", "tests/integration"],
            expected_test_paths: &[
                "tests/main.rs",
                "tests/unit/main.rs",
                "tests/integration/main.rs",
            ],
        },
    ]
}

/// The `mod x;` names declared in an aggregator.
fn declared_mods(aggregator: &Path) -> BTreeSet<String> {
    let src = std::fs::read_to_string(aggregator)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", aggregator.display()));
    src.lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix("mod ")?;
            Some(rest.trim_end_matches(';').trim().to_string())
        })
        .collect()
}

/// The `.rs` files a directory holds, excluding the aggregator itself.
fn files_on_disk(dir: &Path) -> BTreeSet<String> {
    std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .map(|e| e.expect("dir entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "rs"))
        .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()))
        .filter(|stem| stem != "main")
        .collect()
}

/// Each aggregator lists exactly the files sitting next to it: nothing missing, nothing stale.
#[test]
fn every_test_file_is_reachable_from_its_aggregator() {
    for layout in layouts() {
        for dir_name in layout.aggregated_dirs {
            let dir = layout.root.join(dir_name);
            let aggregator = dir.join("main.rs");
            assert!(
                aggregator.is_file(),
                "{} has no main.rs, so nothing pulls its files in",
                dir.display()
            );

            let declared = declared_mods(&aggregator);
            let on_disk = files_on_disk(&dir);

            let unlisted: Vec<_> = on_disk.difference(&declared).collect();
            assert!(
                unlisted.is_empty(),
                "{}/{dir_name}/main.rs does not list {unlisted:?}. Those files compile nowhere and \
                 their tests never run — `cargo test` will still report ok. Add `mod <name>;` for \
                 each.",
                layout.name
            );

            let phantom: Vec<_> = declared.difference(&on_disk).collect();
            assert!(
                phantom.is_empty(),
                "{}/{dir_name}/main.rs lists {phantom:?} but no such file exists next to it",
                layout.name
            );
        }
    }
}

/// The aggregate targets are the only `[[test]]` blocks. A per-file block reintroduces the
/// 233-target spawn cost one entry at a time, and a file declared both as its own target and as a
/// module of an aggregate compiles twice.
#[test]
fn cargo_toml_declares_only_the_aggregate_test_targets() {
    for layout in layouts() {
        let manifest = std::fs::read_to_string(layout.root.join("Cargo.toml"))
            .unwrap_or_else(|e| panic!("read {}/Cargo.toml: {e}", layout.name));

        let paths: BTreeSet<&str> = manifest
            .lines()
            .filter_map(|l| l.trim().strip_prefix("path = \""))
            .filter_map(|l| l.strip_suffix('"'))
            .filter(|p| p.starts_with("tests/"))
            .collect();

        let expected: BTreeSet<&str> = layout.expected_test_paths.iter().copied().collect();

        assert_eq!(
            paths, expected,
            "unexpected [[test]] paths in {}. Every test file belongs to an aggregate target; \
             adding a per-file target puts back the process-spawn cost the aggregates exist to \
             remove.",
            layout.name
        );

        assert!(
            manifest.contains("autotests = false"),
            "autotests = false is missing from {}/Cargo.toml, so cargo will auto-discover every \
             tests/*.rs as its own target and compile the aggregated files a second time",
            layout.name
        );
    }
}

/// The bin crate's aggregates gate on `--features testing`, so that feature has to exist there. A
/// `required-features` entry naming a feature the crate does not declare is not an error cargo
/// reports during a plain `cargo test` — it just never builds the target.
#[test]
fn bin_crate_declares_the_testing_feature_its_targets_require() {
    let manifest = std::fs::read_to_string(bin_root().join("Cargo.toml")).expect("read Cargo.toml");

    assert!(
        manifest.contains("[features]") && manifest.contains("testing = []"),
        "iris-agentic-dev-bin's test targets declare required-features = [\"testing\"], but the \
         crate has no `testing` feature. Cargo would resolve that to \"never build these targets\" \
         and `cargo test` would report ok having run none of them."
    );
}
