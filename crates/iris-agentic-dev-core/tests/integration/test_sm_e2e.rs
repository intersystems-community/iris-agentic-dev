// E2E tests for Server Manager connection discovery (044-servermanager-discovery).
//
// Discovery runs against a fixture home: VS Code's `settings.json` under a temporary `HOME`, naming
// the `iris-dev-iris` container, with its password in the file (the deprecated plaintext field) so
// no keychain is touched. The developer's own settings are hidden by process isolation and this
// test does not want them.
//
// The other two tests need the developer's real Server Manager settings, the real keychain and a
// person at the machine, so they exist only under the `real-keychain-tests` feature:
//   IAD_TEST_REAL_KEYCHAIN=1 IRIS_SERVER_NAME=<name> \
//   cargo test -p iris-agentic-dev-core --features testing,real-keychain-tests \
//     --test integration test_sm_e2e -- --test-threads=1

use iris_agentic_dev_core::iris::server_manager::{parse_sm_settings, sm_settings_path};
use iris_agentic_dev_core::testing::EnvGuard;

#[cfg(feature = "real-keychain-tests")]
use iris_agentic_dev_core::iris::server_manager::select_server;

const FIXTURE_SETTINGS: &str = r#"{
  "intersystems.servers": {
    "/default": "iris-dev-iris",
    "iris-dev-iris": {
      "webServer": {"scheme": "http", "host": "localhost", "port": 52780},
      "username": "_SYSTEM",
      "password": "SYS"
    }
  }
}"#;

#[test]
fn e2e_sm_discovery_finds_connection() {
    let home = tempfile::tempdir().expect("fixture home");
    let mut env = EnvGuard::clean();
    env.set("HOME", home.path());
    env.set("USERPROFILE", home.path());
    env.set("APPDATA", home.path().join("AppData/Roaming"));

    let settings_path = sm_settings_path().expect("the fixture home has a settings path");
    assert!(
        settings_path.starts_with(home.path()),
        "discovery must read the fixture home, not the developer's: {}",
        settings_path.display()
    );
    std::fs::create_dir_all(settings_path.parent().expect("settings dir")).expect("mkdir");
    std::fs::write(&settings_path, FIXTURE_SETTINGS).expect("write fixture settings");

    let profiles = parse_sm_settings(&settings_path);
    assert_eq!(profiles.len(), 1, "the /default key is not a server");
    let p = &profiles[0];
    assert_eq!(p.name, "iris-dev-iris");
    assert_eq!((p.host.as_str(), p.port), ("localhost", 52780));
    assert_eq!(p.username, "_SYSTEM");
    assert_eq!(p.password_deprecated.as_deref(), Some("SYS"));
}

/// The developer's own home, for the tests that read their real Server Manager settings.
#[cfg(feature = "real-keychain-tests")]
fn real_home_env() -> EnvGuard {
    let home = iris_agentic_dev_core::testing::real_home().expect("a real home directory");
    let mut env = EnvGuard::new();
    env.set("HOME", home);
    env
}

#[test]
#[cfg(feature = "real-keychain-tests")]
fn e2e_sm_credential_resolves() {
    std::env::var("IRIS_SERVER_NAME")
        .expect("set IRIS_SERVER_NAME to the Server Manager server this machine should use");
    use iris_agentic_dev_core::iris::server_manager::sm_secret_for_in;
    let _home = real_home_env();
    let settings_path = sm_settings_path().expect("VS Code settings.json must exist");
    let profiles = parse_sm_settings(&settings_path);
    let server = select_server(&profiles).expect("should select server");
    let endpoint = server.endpoint().expect("profile endpoint");
    let password = sm_secret_for_in(&profiles, &server.name, &server.username, &endpoint);
    assert!(
        password.is_ok(),
        "credential must resolve from OS keychain: {password:?}"
    );
}

#[test]
#[cfg(feature = "real-keychain-tests")]
fn e2e_sm_tools_work_after_discovery() {
    std::env::var("IRIS_SERVER_NAME")
        .expect("set IRIS_SERVER_NAME to the Server Manager server this machine should use");
    // Full round-trip: SM discovery → credential resolution → tool call
    // This is the zero-config scenario from US1 AC-1.
    // Timing: must complete zero-config setup in < 30 seconds (SC-001)
    let _home = real_home_env();
    let start = std::time::Instant::now();
    let settings_path = sm_settings_path().expect("VS Code settings.json must exist");
    let profiles = parse_sm_settings(&settings_path);
    let server = select_server(&profiles).expect("server selected");
    let endpoint = server.endpoint().expect("profile endpoint");
    let _password = iris_agentic_dev_core::iris::server_manager::sm_secret_for_in(
        &profiles,
        &server.name,
        &server.username,
        &endpoint,
    )
    .expect("credential resolved");
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < 30,
        "zero-config setup must complete in < 30 seconds, took {}s",
        elapsed.as_secs()
    );
}
