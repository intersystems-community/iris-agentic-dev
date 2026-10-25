//! Spec 141 T183: no test process touches the developer's real keychain or config directory.
//!
//! A unit run once hung for 34 minutes on a macOS keychain approval dialog, because a test in the
//! shared process initialised the real keychain store and a later test read an item from it. These
//! tests pin the three ways that can come back:
//!
//! - a test target that does not isolate its process on load,
//! - a keychain store that is the OS one after the code under test initialises it,
//! - a config path that is the developer's real one.
//!
//! They run with `IAD_NO_KEYCHAIN` unset by the caller on purpose: the protection is the harness's,
//! not the shell's.

use std::path::{Path, PathBuf};

use iris_agentic_dev_core::iris::server_manager::init_platform_keystore;
use iris_agentic_dev_core::iris::servers_config::native_config_path;
use iris_agentic_dev_core::testing::KEYSTORE_LOCK;

/// The marker every test target's `main.rs` carries: a load-time call that isolates the process.
const ISOLATE_ON_LOAD: &str = "testing::isolate_process()";

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the core crate sits in crates/")
        .to_path_buf()
}

#[test]
fn every_test_target_isolates_its_process_on_load() {
    let crates = crates_dir();
    let targets = [
        "iris-agentic-dev-core/tests/main.rs",
        "iris-agentic-dev-core/tests/unit/main.rs",
        "iris-agentic-dev-core/tests/integration/main.rs",
        "iris-agentic-dev-core/tests/binary/main.rs",
        "iris-agentic-dev-bin/tests/main.rs",
        "iris-agentic-dev-bin/tests/unit/main.rs",
        "iris-agentic-dev-bin/tests/integration/main.rs",
        // The in-source unit tests of each crate run in their own binary too.
        "iris-agentic-dev-core/src/lib.rs",
        "iris-agentic-dev-bin/src/lib.rs",
    ];
    let missing: Vec<&str> = targets
        .iter()
        .copied()
        .filter(|t| {
            let text = std::fs::read_to_string(crates.join(t))
                .unwrap_or_else(|e| panic!("cannot read test target {t}: {e}"));
            !text.contains(ISOLATE_ON_LOAD)
        })
        .collect();
    assert!(
        missing.is_empty(),
        "these test targets do not call {ISOLATE_ON_LOAD} from a #[ctor::ctor] function, so their \
         tests can reach the real keychain and the real config directory: {missing:?}"
    );
}

#[test]
fn the_process_has_no_keychain_even_when_the_shell_set_none() {
    let _guard = KEYSTORE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    keyring_core::unset_default_store();
    // Code under test that reaches the keychain does it through here.
    init_platform_keystore();
    let store = keyring_core::get_default_store();
    keyring_core::unset_default_store();
    assert!(
        store.is_none(),
        "init_platform_keystore() installed a real keychain store ({:?}) in a test process",
        store.map(|s| s.vendor())
    );
}

#[test]
fn the_native_config_path_is_not_the_developers() {
    let path = native_config_path();
    let temp = std::env::temp_dir();
    assert!(
        path.starts_with(&temp),
        "native_config_path() = {} is outside the temp dir {}: a test would read or write the \
         developer's real servers.json",
        path.display(),
        temp.display()
    );
}

#[test]
fn the_home_directory_is_not_the_developers() {
    let home = dirs::home_dir().expect("the isolated process has a home directory");
    let temp = std::env::temp_dir();
    assert!(
        home.starts_with(&temp),
        "HOME = {} is outside the temp dir {}: ~/.iris-agentic-dev.toml, the audit log and the \
         skill directories would be the developer's",
        home.display(),
        temp.display()
    );
}

/// `EnvGuard::clean()` unsets the behavior variables but must leave the process isolated: with
/// `IAD_NO_KEYCHAIN` gone, a test in the same binary that inits the keystore reaches the real
/// keychain, and the keystore test that reads the variable without the guard's lock flaked.
#[test]
fn a_clean_guard_keeps_the_process_away_from_the_keychain() {
    let _clean = iris_agentic_dev_core::testing::EnvGuard::clean();
    assert_eq!(
        std::env::var("IAD_NO_KEYCHAIN").as_deref(),
        Ok("1"),
        "EnvGuard::clean() must not undo isolate_process()"
    );
}
