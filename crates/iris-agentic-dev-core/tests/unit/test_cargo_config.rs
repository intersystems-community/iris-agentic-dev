//! The committed `.cargo/config.toml` has to work on every platform the project builds on.
//!
//! 1.3.2 set `build.rustc-wrapper = "/usr/bin/env"` to keep sccache out of the build. That is a
//! real executable on macOS and Linux and no file at all on Windows, so `cargo build --locked`
//! on windows-latest died with `could not execute process `D:/usr/bin/env …rustc.exe -vV` (never
//! executed)` — the windows-handshake job had been green the release before. Nothing in the
//! test suite noticed, because the only machine that could notice was the one CI job that runs
//! on Windows.
//!
//! These tests read the real config and the real scripts. The passthrough cargo-llvm-cov needs
//! now lives in the coverage scripts, which are bash and only run on macOS or Linux, so this
//! also checks it is actually there — moving it out of the config and forgetting to put it
//! anywhere would silently break the coverage gate the same way the empty string did.

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/<member> is two levels below the workspace root")
        .to_path_buf()
}

/// A wrapper value cargo can resolve on any host: a bare program name it looks up on PATH.
/// Anything with a path separator or a drive letter is a promise about one operating system's
/// filesystem, and a committed config cannot make that promise.
fn is_portable_wrapper(value: &str) -> bool {
    !value.contains('/') && !value.contains('\\') && !value.contains(':')
}

#[test]
fn rustc_wrapper_in_committed_config_is_not_a_platform_path() {
    let path = workspace_root().join(".cargo/config.toml");
    let text = std::fs::read_to_string(&path).expect("read .cargo/config.toml");
    let config: toml::Value = toml::from_str(&text).expect("parse .cargo/config.toml");

    let wrapper = config
        .get("build")
        .and_then(|b| b.get("rustc-wrapper"))
        .and_then(|w| w.as_str());

    if let Some(wrapper) = wrapper {
        assert!(
            is_portable_wrapper(wrapper),
            "build.rustc-wrapper is {wrapper:?}, an absolute path. Windows has no /usr/bin/env, \
             so `cargo build` fails outright there. Use a bare program name or drop the key and \
             set CARGO_BUILD_RUSTC_WRAPPER where it is needed."
        );
    }
}

#[test]
fn coverage_scripts_set_the_llvm_cov_passthrough() {
    for script in ["scripts/coverage.sh", "scripts/check-coverage-floors.sh"] {
        let text = std::fs::read_to_string(workspace_root().join(script))
            .unwrap_or_else(|e| panic!("read {script}: {e}"));
        assert!(
            text.contains("CARGO_BUILD_RUSTC_WRAPPER"),
            "{script} does not set CARGO_BUILD_RUSTC_WRAPPER. cargo-llvm-cov reads \
             build.rustc-wrapper literally, and the config no longer supplies one, so without \
             this export the run picks up whatever the global cargo config says."
        );
    }
}

#[test]
fn portable_wrapper_rejects_both_values_this_repo_has_shipped() {
    // The Windows break.
    assert!(!is_portable_wrapper("/usr/bin/env"));
    assert!(!is_portable_wrapper(
        "C:\\Program Files\\Git\\usr\\bin\\env.exe"
    ));
    // What a working value looks like.
    assert!(is_portable_wrapper("sccache"));
    assert!(is_portable_wrapper("env"));
}
