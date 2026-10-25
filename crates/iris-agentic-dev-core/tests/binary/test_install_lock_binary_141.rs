//! Spec 141 FR-026 (T-R4b-B06): `iris-agentic-dev install` through the binary.
//!
//! The lock is written into the working directory, so a repository that commits
//! `iris-agentic-dev.lock` as a symlink must not make the install write elsewhere, and a
//! dependency name must not add lines to the lock. No IRIS is involved.
//!
//! Run: `IAD_BINARY=$PWD/target/debug/iris-agentic-dev cargo test --features testing --test binary
//! test_install_lock_binary_141 -- --ignored --test-threads=1`

use iris_agentic_dev_core::testing::{clean_command, require_iad_binary};
use std::path::Path;
use tempfile::TempDir;

const MANIFEST: &str = "[package]\nname = \"consumer\"\nversion = \"1.0.0\"\n\n\
                        [dependencies.dep]\nversion = \"1.0.0\"\nrepository = \"dep-src\"\n";

/// A project directory with a local-source dependency, so `install` needs no network.
fn project() -> TempDir {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("iris-agentic-dev.toml"), MANIFEST).unwrap();
    let dep = dir.path().join("dep-src");
    std::fs::create_dir(&dep).unwrap();
    std::fs::write(
        dep.join("iris-agentic-dev.toml"),
        "[package]\nname = \"dep\"\nversion = \"1.0.0\"\n",
    )
    .unwrap();
    dir
}

fn install(dir: &Path) -> std::process::Output {
    let bin = require_iad_binary().expect("binary");
    clean_command(&bin)
        .arg("install")
        .current_dir(dir)
        .output()
        .expect("spawn")
}

#[cfg(unix)]
#[test]
#[ignore]
fn install_replaces_a_symlinked_lock_instead_of_writing_through_it() {
    let dir = project();
    let outside = dir.path().join("outside.txt");
    std::fs::write(&outside, "keep").unwrap();
    let lock = dir.path().join("iris-agentic-dev.lock");
    std::os::unix::fs::symlink(&outside, &lock).unwrap();

    let out = install(dir.path());
    assert!(
        out.status.success(),
        "install failed: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    assert_eq!(std::fs::read_to_string(&outside).unwrap(), "keep");
    assert!(
        !std::fs::symlink_metadata(&lock)
            .unwrap()
            .file_type()
            .is_symlink(),
        "the lock must be a regular file after install"
    );
    let parsed: toml::Table = toml::from_str(&std::fs::read_to_string(&lock).unwrap()).unwrap();
    assert_eq!(parsed["package"][0]["name"].as_str(), Some("dep"));
}

#[test]
#[ignore]
fn install_refuses_a_dependency_name_with_a_newline() {
    let dir = project();
    std::fs::write(
        dir.path().join("iris-agentic-dev.toml"),
        "[package]\nname = \"consumer\"\nversion = \"1.0.0\"\n\n\
         [dependencies.\"dep\\nextra line\\n#\"]\nversion = \"1.0.0\"\nrepository = \"dep-src\"\n",
    )
    .unwrap();

    let out = install(dir.path());
    assert!(
        !out.status.success(),
        "a control character must fail install"
    );
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("INVALID_MANIFEST"), "{text}");
    assert!(
        !dir.path().join("iris-agentic-dev.lock").exists(),
        "no lock may be written"
    );
}
