//! Guards the aggregated test-target layout.
//!
//! Every test file in this crate is a module of one of the aggregate targets declared in
//! `Cargo.toml` (`unit`, `integration`, `binary`, `misc`). The failure mode that buys is silent: add
//! `tests/unit/test_new_thing.rs`, forget the `mod test_new_thing;` line in `tests/unit/main.rs`,
//! and the file compiles nowhere and runs never. `cargo test` still says ok. That is the same shape
//! as the #110 serde silent-drop bug, so it gets the same treatment — a test that fails.
//!
//! `autotests = false` is what makes the omission silent. With autodiscovery on, an unlisted file
//! would at least still be picked up as its own target.
//!
//! Known limit: this file is itself reached through `tests/unit/main.rs`. Deleting its own `mod`
//! line would take the guard down with it. Nothing inside the test tree can close that loop; the
//! `mod` lists are short and a diff that removes one is visible in review.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
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
    // `tests` is the flat top-level set behind the `misc` target; the rest are subdirectories.
    for dir_name in ["tests", "tests/unit", "tests/integration", "tests/binary"] {
        let dir = crate_root().join(dir_name);
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
            "{dir_name}/main.rs does not list {unlisted:?}. Those files compile nowhere and their \
             tests never run — `cargo test` will still report ok. Add `mod <name>;` for each."
        );

        let phantom: Vec<_> = declared.difference(&on_disk).collect();
        assert!(
            phantom.is_empty(),
            "{dir_name}/main.rs lists {phantom:?} but no such file exists next to it"
        );
    }
}

/// The aggregate targets are the only `[[test]]` blocks. A per-file block reintroduces the 207-target
/// spawn cost one entry at a time, and a file declared both as its own target and as a module of an
/// aggregate compiles twice.
#[test]
fn cargo_toml_declares_only_the_aggregate_test_targets() {
    let manifest =
        std::fs::read_to_string(crate_root().join("Cargo.toml")).expect("read Cargo.toml");

    let paths: BTreeSet<&str> = manifest
        .lines()
        .filter_map(|l| l.trim().strip_prefix("path = \""))
        .filter_map(|l| l.strip_suffix('"'))
        .filter(|p| p.starts_with("tests/"))
        .collect();

    let expected: BTreeSet<&str> = [
        "tests/main.rs",
        "tests/unit/main.rs",
        "tests/integration/main.rs",
        "tests/binary/main.rs",
        // tests/skills/ holds one file, so it stays its own target — an aggregator for a set of one
        // would be a `mod` line and nothing else.
        "tests/skills/nopws_skill_test.rs",
    ]
    .into_iter()
    .collect();

    assert_eq!(
        paths, expected,
        "unexpected [[test]] paths. Every test file belongs to an aggregate target; adding a \
         per-file target puts back the process-spawn cost the aggregates exist to remove."
    );

    assert!(
        manifest.contains("autotests = false"),
        "autotests = false is missing, so cargo will auto-discover every tests/*.rs as its own \
         target and compile the aggregated files a second time"
    );
}
