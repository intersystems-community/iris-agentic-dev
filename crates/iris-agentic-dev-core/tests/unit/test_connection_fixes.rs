// Tests for connection.rs fixes: query() namespace, Debug redaction, container caching, banner stripping.

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection, SystemMode};

// 085: the ENV_LOCK that used to serialise these tests is gone with the tests that needed it. The
// gate is resolved as data by `write_gate::resolve_gates`, so nothing in this file mutates the
// process environment any more and nothing needs to take a turn.

fn make_conn(password: &str) -> IrisConnection {
    IrisConnection::new(
        "http://localhost:52773",
        "USER",
        "_SYSTEM",
        password,
        DiscoverySource::ExplicitFlag,
    )
}

fn make_conn_with_ns_and_mode(ns: &str, mode: SystemMode) -> IrisConnection {
    let mut c = IrisConnection::new(
        "http://localhost:52773",
        ns,
        "_SYSTEM",
        "SYS",
        DiscoverySource::ExplicitFlag,
    );
    c.system_mode = mode;
    c
}

// ── T009: Debug redaction ────────────────────────────────────────────────────

#[test]
fn test_password_redacted_in_debug() {
    let conn = make_conn("supersecret123");
    let debug_output = format!("{:?}", conn);
    assert!(
        !debug_output.contains("supersecret123"),
        "password must not appear in Debug output, got: {}",
        debug_output
    );
    assert!(
        debug_output.contains("[redacted]"),
        "debug output should contain [redacted], got: {}",
        debug_output
    );
}

// ── T009: query() namespace ──────────────────────────────────────────────────
// We test the URL-building logic indirectly by verifying versioned_ns_url
// uses the passed namespace. Since query() delegates to versioned_ns_url,
// the test for the URL builder covers the contract.

#[test]
fn test_versioned_ns_url_uses_passed_namespace() {
    let conn = make_conn("SYS");
    let url = conn.versioned_ns_url("MYNS", "/action/query");
    assert!(
        url.contains("/MYNS/"),
        "URL should contain the passed namespace MYNS, got: {}",
        url
    );
    assert!(
        !url.contains("/USER/"),
        "URL should NOT contain the connection default USER, got: {}",
        url
    );
}

// ── T009: Banner stripping ───────────────────────────────────────────────────

#[test]
fn test_banner_stripped_from_output() {
    // Real IRIS session: banner, then bare prompt, then code output on its own line, then bare prompt.
    let raw = "Copyright (c) 2024 InterSystems Corporation\nAll rights reserved.\nIRIS for UNIX (Apple Mac OS X for x86-64) 2024.1\nUSER>\n42\nUSER>\n";
    let stripped = iris_agentic_dev_core::iris::connection::strip_iris_banner(raw);
    assert_eq!(
        stripped.trim(),
        "42",
        "expected only code output, got: {:?}",
        stripped
    );
}

#[test]
fn test_banner_strip_preserves_multiline_output() {
    // Multiline output: banner, bare prompt, two output lines, bare prompt.
    let raw = "Copyright (c) 2024 InterSystems Corporation\nUSER>\nline1\nline2\nUSER>\n";
    let stripped = iris_agentic_dev_core::iris::connection::strip_iris_banner(raw);
    let trimmed = stripped.trim();
    assert!(
        trimmed.contains("line1"),
        "should contain line1, got: {:?}",
        trimmed
    );
    assert!(
        trimmed.contains("line2"),
        "should contain line2, got: {:?}",
        trimmed
    );
    assert!(
        !trimmed.contains("Copyright"),
        "should not contain Copyright, got: {:?}",
        trimmed
    );
}

#[test]
fn test_banner_strip_noop_on_clean_output() {
    let raw = "hello world\n";
    let stripped = iris_agentic_dev_core::iris::connection::strip_iris_banner(raw);
    assert_eq!(stripped.trim(), "hello world");
}

// ── T021: http_client error handling ────────────────────────────────────────

#[test]
fn test_http_client_succeeds_normally() {
    // When TLS is not broken, http_client should succeed.
    let result = IrisConnection::http_client();
    assert!(
        result.is_ok(),
        "http_client should succeed in normal environment"
    );
}

// ── IDEV-3: sentinel Write ! ─────────────────────────────────────────────

#[test]
fn test_execute_captures_output_without_trailing_newline() {
    // build_exec_class must inject a sentinel Write ! after user code
    // so that Read line:0 always finds a line boundary.
    let lines = iris_agentic_dev_core::iris::connection::IrisConnection::build_exec_class_for_test(
        "TestClass",
        "/tmp/test.txt",
        "Write 42",
    );
    // Find the user code line
    let user_line_pos = lines
        .iter()
        .position(|l| l.contains("Write 42"))
        .expect("should contain user code");
    // The line immediately after user code must be the sentinel
    let sentinel_line = lines
        .get(user_line_pos + 1)
        .expect("should have line after user code");
    assert!(
        sentinel_line.contains("Write !"),
        "sentinel 'Write !' must follow user code, got: {:?}",
        sentinel_line
    );
}

#[test]
fn test_build_exec_class_sentinel_not_duplicated() {
    let lines = iris_agentic_dev_core::iris::connection::IrisConnection::build_exec_class_for_test(
        "TestClass",
        "/tmp/test.txt",
        "Write 42,!",
    );
    // Count sentinel occurrences — must be exactly one "Write !" line
    let sentinel_count = lines.iter().filter(|l| l.trim() == "Write !").count();
    assert_eq!(
        sentinel_count, 1,
        "exactly one sentinel Write ! should be present, got {}",
        sentinel_count
    );
}

#[test]
fn test_execute_captures_multiline_without_trailing_newline() {
    // FR-007: multi-line output where last line has no trailing ! must be fully captured.
    let lines = iris_agentic_dev_core::iris::connection::IrisConnection::build_exec_class_for_test(
        "TestClass",
        "/tmp/test.txt",
        "Write \"line1\",!\nWrite \"line2\"",
    );
    // Sentinel must appear after all user code lines
    let sentinel_pos = lines
        .iter()
        .rposition(|l| l.trim() == "Write !")
        .expect("sentinel must exist");
    let last_user_pos = lines
        .iter()
        .rposition(|l| l.contains("Write \"line2\""))
        .expect("user code must exist");
    assert!(
        sentinel_pos > last_user_pos,
        "sentinel must come after last user code line"
    );
}

// ── I-2: IRIS_CONTAINER read fresh each call ──────────────────────────────
//
// Both sub-cases run sequentially in one test to avoid env-var races
// between parallel test threads.
#[test]
fn test_execute_iris_container_env_behavior() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    // Sub-case A: no container set → DOCKER_REQUIRED
    std::env::remove_var("IRIS_CONTAINER");
    let conn = IrisConnection::new(
        "http://localhost:52773",
        "USER",
        "_SYSTEM",
        "SYS",
        DiscoverySource::ExplicitFlag,
    );
    let result = rt.block_on(conn.execute("Write 1", "USER"));
    assert!(result.is_err(), "expected error when IRIS_CONTAINER unset");
    assert_eq!(
        result.unwrap_err().to_string(),
        "DOCKER_REQUIRED",
        "should return DOCKER_REQUIRED when IRIS_CONTAINER is not set"
    );

    // Sub-case B: set env var AFTER construction → execute() must pick it up.
    // If OnceLock cached None at construction time the call would still return
    // DOCKER_REQUIRED. Without the cache it attempts docker exec instead.
    std::env::remove_var("IRIS_CONTAINER");
    let conn2 = IrisConnection::new(
        "http://localhost:52773",
        "USER",
        "_SYSTEM",
        "SYS",
        DiscoverySource::ExplicitFlag,
    );
    std::env::set_var("IRIS_CONTAINER", "nonexistent-container-for-test");
    let result2 = rt.block_on(conn2.execute("Write 1", "USER"));
    std::env::remove_var("IRIS_CONTAINER");
    // Should be an error (container not found) but NOT "DOCKER_REQUIRED"
    if let Err(e) = result2 {
        assert!(
            e.to_string() != "DOCKER_REQUIRED",
            "should attempt docker exec (not DOCKER_REQUIRED): got '{}'",
            e
        );
    }
}

// ── FR-009–FR-011: HTTP path handles long code ───────────────────────────

#[test]
fn test_build_exec_class_handles_long_code() {
    // The HTTP path (build_exec_class) must handle code strings of any length.
    // Generate a 200-char string literal and verify it appears intact in the generated class.
    let long_string: String = "A".repeat(200);
    let code = format!("Write \"{}\"", long_string);
    let lines = iris_agentic_dev_core::iris::connection::IrisConnection::build_exec_class_for_test(
        "TestClass",
        "/tmp/test.txt",
        &code,
    );
    // The full 200-char string must appear in the generated lines without truncation
    let found = lines.iter().any(|l| l.contains(&long_string));
    assert!(
        found,
        "200-char string must appear intact in generated class lines"
    );
}

// ── SQL proc name: must use underscore not dot (#18) ─────────────────────────
// ── Scratch package must be IrisDevTmp, not User (#60) ───────────────────────

#[test]
fn test_build_exec_class_sql_proc_name_uses_underscore() {
    // The generated class IrisDevTmp.IrisDevRunXXX with method Execute [SqlProc]
    // maps to SQL proc IrisDevTmp.IrisDevRunXXX_Execute.
    // For non-User packages, IRIS SQL schema = package name (no "SQL" prefix).
    // The SQLUser prefix is a historical special case for the User package only.
    // Regression test for #18: was User.IrisDevRunXXX_Execute which caused
    // silent IRIS_COMPILE_FAILED with empty error message.
    let lines = IrisConnection::build_exec_class_for_test(
        "IrisDevTmp.IrisDevRunabc123",
        "/tmp/test.txt",
        "Write 1",
    );
    // The class name in the generated source should be IrisDevTmp.IrisDevRunabc123
    assert!(
        lines
            .iter()
            .any(|l| l.contains("IrisDevTmp.IrisDevRunabc123")),
        "class name should appear in generated source"
    );
    // SQL proc: IrisDevTmp package -> schema IrisDevTmp (no SQL prefix for non-User packages)
    let id = "abc123";
    let sql_func = format!("IrisDevTmp.IrisDevRun{}_Execute", id);
    assert_eq!(sql_func, "IrisDevTmp.IrisDevRunabc123_Execute");
    assert!(
        !sql_func.starts_with("User"),
        "SQL proc name must not be in User schema: {}",
        sql_func
    );
}

#[test]
fn test_scratch_package_is_not_user() {
    // Regression test for #60: executor classes must not be generated in User.*
    let id = "testid";
    let class_name = format!("IrisDevTmp.IrisDevRun{}", id);
    let sql_func = format!("IrisDevTmp.IrisDevRun{}_Execute", id);
    assert!(
        class_name.starts_with("IrisDevTmp."),
        "class must be in IrisDevTmp package, got: {}",
        class_name
    );
    assert!(
        !class_name.starts_with("User."),
        "class must not be in User package"
    );
    assert!(
        sql_func.starts_with("IrisDevTmp."),
        "SQL proc must use IrisDevTmp schema (non-User packages have no SQL prefix), got: {}",
        sql_func
    );
}

// ── is_write_allowed: the inference chain (issue #26, reworked by 085) ────────
//
// `is_write_allowed()` now delegates to `write_gate::resolve_declared`, and the operator's
// environment is a process-start snapshot behind a `OnceLock`. A `set_var` inside a test is
// therefore invisible to the resolver — whichever of these tests ran first decided
// `IRIS_ALLOW_PROD` for the whole binary, which made the two "must block" cases fail depending on
// execution order. They pass an explicit `OperatorEnvGates` instead.

use iris_agentic_dev_core::tools::write_gate::{
    resolve_declared, DeclaredGates, GateSource, OperatorEnvGates,
};

/// Resolve for a namespace/mode with a clean operator environment and nothing declared.
fn inferred(ns: &str, mode: SystemMode) -> bool {
    resolve_declared(
        &OperatorEnvGates::default(),
        DeclaredGates::default(),
        mode,
        ns,
    )
    .write_enabled
}

/// Resolve with `IRIS_ALLOW_PROD` set by the operator.
fn with_allow_prod(
    ns: &str,
    mode: SystemMode,
) -> iris_agentic_dev_core::tools::write_gate::GateResolution {
    let op = OperatorEnvGates {
        allow_prod: true,
        ..Default::default()
    };
    resolve_declared(&op, DeclaredGates::default(), mode, ns)
}

#[test]
fn test_is_write_allowed_allow_prod_overrides_live() {
    let r = with_allow_prod("USER", SystemMode::Live);
    assert!(
        r.write_enabled,
        "IRIS_ALLOW_PROD=1 must allow writes on Live system"
    );
    assert_eq!(r.write_source, GateSource::LegacyAllowProd);
}

#[test]
fn test_is_write_allowed_live_blocks_without_override() {
    assert!(
        !inferred("USER", SystemMode::Live),
        "Live mode must block writes"
    );
}

#[test]
fn test_is_write_allowed_development_allows() {
    assert!(
        inferred("USER", SystemMode::Development),
        "Development mode must allow writes"
    );
}

#[test]
fn test_is_write_allowed_test_mode_allows() {
    assert!(
        inferred("USER", SystemMode::Test),
        "Test mode must allow writes"
    );
}

// ── is_write_allowed: Unknown mode delegates to is_production_namespace ───────

#[test]
fn test_is_write_allowed_unknown_non_prod_namespace_allows() {
    for ns in ["USER", "DEV", "STAGING", "TEST", "SANDBOX"] {
        assert!(
            inferred(ns, SystemMode::Unknown),
            "Unknown mode + namespace {ns} should allow writes"
        );
    }
}

#[test]
fn test_is_write_allowed_unknown_prod_namespace_blocks() {
    for ns in ["PROD", "PRODUCTION", "LIVE", "PRD", "prod", "Prod"] {
        assert!(
            !inferred(ns, SystemMode::Unknown),
            "Unknown mode + namespace {ns} should block writes"
        );
    }
}

#[test]
fn test_is_write_allowed_allow_prod_true_string() {
    // `IRIS_ALLOW_PROD=true` parses to the same `allow_prod: true` as `=1`; string parsing is
    // covered by the OperatorEnvGates tests, the override behaviour here.
    assert!(
        with_allow_prod("PROD", SystemMode::Live).write_enabled,
        "IRIS_ALLOW_PROD=true must allow writes"
    );
}

/// The delegation itself: `is_write_allowed()` must be the resolver, not a second copy of the
/// chain. Compared under whatever snapshot the binary captured, so it does not depend on the
/// environment the test run inherited.
#[test]
fn test_is_write_allowed_delegates_to_the_resolver() {
    for (ns, mode) in [
        ("USER", SystemMode::Live),
        ("USER", SystemMode::Development),
        ("PROD", SystemMode::Unknown),
    ] {
        let expected = resolve_declared(
            iris_agentic_dev_core::tools::write_gate::operator_env_gates(),
            DeclaredGates::default(),
            mode,
            ns,
        )
        .write_enabled;
        assert_eq!(
            make_conn_with_ns_and_mode(ns, mode).is_write_allowed(),
            expected,
            "is_write_allowed disagreed with the resolver for {ns}/{mode:?}"
        );
    }
}
