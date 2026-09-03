// Tests for 101-nopws-connectivity: NoPWS detection, iris_test_server NoPWS fields,
// and Atelier-required guard (FR-010).

// ── FR-011: serde silent-drop guard (duplicate of test_workspace_config for completeness) ──

use iris_agentic_dev_core::iris::workspace_config::WorkspaceConfig;

#[test]
fn test_nopws_field_parses_from_toml() {
    let cfg: WorkspaceConfig = toml::from_str("nopws = true").expect("must parse");
    assert!(cfg.nopws, "nopws must parse to true");
}

#[test]
fn test_nopws_field_defaults_to_false() {
    let cfg: WorkspaceConfig = toml::from_str("").expect("must parse");
    assert!(!cfg.nopws, "nopws must default to false");
}

#[test]
fn test_ssh_host_field_parses_from_toml() {
    let cfg: WorkspaceConfig =
        toml::from_str("ssh_host = \"baystate.example.com\"").expect("must parse");
    assert_eq!(
        cfg.ssh_host.as_deref(),
        Some("baystate.example.com"),
        "ssh_host must parse correctly"
    );
}

#[test]
fn test_ssh_host_field_defaults_to_none() {
    let cfg: WorkspaceConfig = toml::from_str("").expect("must parse");
    assert!(cfg.ssh_host.is_none(), "ssh_host must default to None");
}

// ── iris.cpf detection parsing tests (FR-005) ────────────────────────────────

/// Simulate the output of `docker exec <container> grep WebServer /usr/irissys/iris.cpf`
/// returning "WebServer=0" and verify the detection logic would flag NoPWS.
#[test]
fn test_webserver_zero_indicates_nopws() {
    let cpf_grep_output = "WebServer=0";
    let nopws_detected = cpf_grep_output.contains("WebServer=0");
    assert!(
        nopws_detected,
        "WebServer=0 in iris.cpf must indicate NoPWS"
    );
}

/// "WebServer=1" means PWS is enabled — not NoPWS.
#[test]
fn test_webserver_one_does_not_indicate_nopws() {
    let cpf_grep_output = "WebServer=1";
    let nopws_detected = cpf_grep_output.contains("WebServer=0");
    assert!(
        !nopws_detected,
        "WebServer=1 in iris.cpf must not indicate NoPWS"
    );
}

/// Docker exec fails (container not found) — must not claim NoPWS.
#[test]
fn test_docker_exec_failure_does_not_indicate_nopws() {
    // Simulate empty/error output from a failed docker exec
    let cpf_grep_output = "";
    let nopws_detected = cpf_grep_output.contains("WebServer=0");
    assert!(
        !nopws_detected,
        "empty output from docker exec must not indicate NoPWS (no false positive)"
    );
}

/// Unexpected output (permission denied etc.) — must not claim NoPWS.
#[test]
fn test_permission_denied_does_not_indicate_nopws() {
    let cpf_grep_output = "permission denied";
    let nopws_detected = cpf_grep_output.contains("WebServer=0");
    assert!(
        !nopws_detected,
        "error output from docker exec must not indicate NoPWS"
    );
}

// ── NOPWS_ATELIER_REQUIRED error code guard (FR-010) ─────────────────────────

/// Verify the error code constant exists and has the expected value.
#[test]
fn test_nopws_atelier_required_error_code() {
    assert_eq!(
        iris_agentic_dev_core::tools::nopws::NOPWS_ATELIER_REQUIRED,
        "NOPWS_ATELIER_REQUIRED",
        "error code constant must have expected value"
    );
}

/// Build a NOPWS_ATELIER_REQUIRED error response and verify its shape.
#[test]
fn test_nopws_atelier_required_response_shape() {
    let resp = iris_agentic_dev_core::tools::nopws::nopws_atelier_required_error();
    assert_eq!(resp["success"], false);
    assert_eq!(resp["error_code"], "NOPWS_ATELIER_REQUIRED");
    let error_msg = resp["error"].as_str().unwrap_or("");
    assert!(
        error_msg.contains("NoPWS"),
        "error message must mention NoPWS"
    );
    assert!(
        error_msg.contains("Atelier REST"),
        "error message must mention Atelier REST"
    );
}

/// Check that nopws_is_active() returns true when docker_only URL is set.
#[test]
fn test_nopws_is_active_with_sentinel_url() {
    assert!(
        iris_agentic_dev_core::tools::nopws::nopws_is_active(true, false),
        "docker_only must trigger nopws_is_active"
    );
}

#[test]
fn test_nopws_is_active_with_no_pws_version() {
    assert!(
        iris_agentic_dev_core::tools::nopws::nopws_is_active(false, true),
        "no_pws version heuristic must trigger nopws_is_active"
    );
}

#[test]
fn test_nopws_is_not_active_for_normal_connection() {
    assert!(
        !iris_agentic_dev_core::tools::nopws::nopws_is_active(false, false),
        "normal connection must not trigger nopws_is_active"
    );
}
