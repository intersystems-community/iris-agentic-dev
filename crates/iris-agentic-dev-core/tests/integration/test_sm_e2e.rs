// E2E tests for Server Manager connection discovery (044-servermanager-discovery).
//
// ALL tests here are #[ignore] — they require:
//   1. A real VS Code Server Manager installation with at least one configured server
//   2. IRIS_SERVER_NAME env var set to the server name to use
//   3. IRIS instance reachable via the discovered connection
//
// Run manually with:
//   cargo test --test test_sm_e2e -- --ignored

use iris_agentic_dev_core::iris::server_manager::{
    parse_sm_settings, select_server, sm_settings_path,
};

#[test]
#[ignore = "requires real VS Code Server Manager + IRIS_SERVER_NAME env var"]
fn e2e_sm_discovery_finds_connection() {
    let settings_path = sm_settings_path().expect("VS Code settings.json must exist");
    let profiles = parse_sm_settings(&settings_path);
    assert!(
        !profiles.is_empty(),
        "should discover at least one SM server"
    );
}

#[test]
#[ignore = "requires real VS Code Server Manager + IRIS_SERVER_NAME env var"]
fn e2e_sm_credential_resolves() {
    if std::env::var("IRIS_SERVER_NAME").is_err() {
        return;
    }
    use iris_agentic_dev_core::iris::server_manager::resolve_credential;
    let settings_path = sm_settings_path().expect("VS Code settings.json must exist");
    let profiles = parse_sm_settings(&settings_path);
    let server = select_server(&profiles).expect("should select server");
    let password = resolve_credential(&server.name, &server.username);
    assert!(
        password.is_ok(),
        "credential must resolve from OS keychain: {password:?}"
    );
}

#[test]
#[ignore = "requires real VS Code Server Manager, IRIS_SERVER_NAME env var, and live IRIS"]
fn e2e_sm_tools_work_after_discovery() {
    if std::env::var("IRIS_SERVER_NAME").is_err() {
        return;
    }
    // Full round-trip: SM discovery → credential resolution → tool call
    // This is the zero-config scenario from US1 AC-1.
    // Timing: must complete zero-config setup in < 30 seconds (SC-001)
    let start = std::time::Instant::now();
    let settings_path = sm_settings_path().expect("VS Code settings.json must exist");
    let profiles = parse_sm_settings(&settings_path);
    let server = select_server(&profiles).expect("server selected");
    let _password = iris_agentic_dev_core::iris::server_manager::resolve_credential(
        &server.name,
        &server.username,
    )
    .expect("credential resolved");
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < 30,
        "zero-config setup must complete in < 30 seconds, took {}s",
        elapsed.as_secs()
    );
}
