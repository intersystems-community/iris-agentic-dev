//! Unit tests for iris_mirror_status, iris_database_list free space, and
//! iris_system_performance (089). No IRIS connection required.

use iris_agentic_dev_core::tools::admin_tools::parse_max_size_mb;

// ── parse_max_size_mb ─────────────────────────────────────────────────────────

#[test]
fn max_size_unlimited_returns_none() {
    assert_eq!(parse_max_size_mb("Unlimited"), None);
}

#[test]
fn max_size_unlimited_case_insensitive() {
    assert_eq!(parse_max_size_mb("unlimited"), None);
    assert_eq!(parse_max_size_mb("UNLIMITED"), None);
}

#[test]
fn max_size_mb_suffix() {
    assert_eq!(parse_max_size_mb("500MB"), Some(500));
    assert_eq!(parse_max_size_mb("1024MB"), Some(1024));
    assert_eq!(parse_max_size_mb("128MB"), Some(128));
}

#[test]
fn max_size_gb_converts_to_mb() {
    assert_eq!(parse_max_size_mb("2GB"), Some(2048));
    assert_eq!(parse_max_size_mb("1GB"), Some(1024));
}

#[test]
fn max_size_empty_returns_none() {
    assert_eq!(parse_max_size_mb(""), None);
}

#[test]
fn max_size_unrecognized_returns_none() {
    assert_eq!(parse_max_size_mb("System Default"), None);
    assert_eq!(parse_max_size_mb("???"), None);
}

// ── normalize_mirror_type ─────────────────────────────────────────────────────

use iris_agentic_dev_core::tools::admin_tools::normalize_mirror_type;

#[test]
fn not_member_string_normalizes_to_none() {
    assert_eq!(normalize_mirror_type("Not Member"), None);
}

#[test]
fn empty_string_normalizes_to_none() {
    assert_eq!(normalize_mirror_type(""), None);
}

#[test]
fn primary_passes_through() {
    assert_eq!(
        normalize_mirror_type("primary"),
        Some("primary".to_string())
    );
}

#[test]
fn backup_passes_through() {
    assert_eq!(normalize_mirror_type("backup"), Some("backup".to_string()));
}

#[test]
fn async_member_passes_through() {
    assert_eq!(normalize_mirror_type("async"), Some("async".to_string()));
}

// ── mirror_status JSON shape (non-member) ─────────────────────────────────────

use iris_agentic_dev_core::tools::admin_tools::build_mirror_status_json;

#[test]
fn non_member_shape_has_false_and_nulls() {
    let v = build_mirror_status_json(false, "", "Not Member", false);
    assert_eq!(v["is_member"], serde_json::Value::Bool(false));
    assert_eq!(v["is_primary"], serde_json::Value::Bool(false));
    assert_eq!(v["mirror_name"], serde_json::Value::Null);
    assert_eq!(v["member_type"], serde_json::Value::Null);
}

#[test]
fn member_shape_has_name_and_type() {
    let v = build_mirror_status_json(true, "MIRROR1", "primary", true);
    assert_eq!(v["is_member"], serde_json::Value::Bool(true));
    assert_eq!(v["is_primary"], serde_json::Value::Bool(true));
    assert_eq!(v["mirror_name"], serde_json::json!("MIRROR1"));
    assert_eq!(v["member_type"], serde_json::json!("primary"));
}

#[test]
fn backup_member_is_not_primary() {
    let v = build_mirror_status_json(true, "MIRROR1", "backup", false);
    assert_eq!(v["is_member"], serde_json::Value::Bool(true));
    assert_eq!(v["is_primary"], serde_json::Value::Bool(false));
    assert_eq!(v["member_type"], serde_json::json!("backup"));
}

// ── SystemPerfMode parsing ─────────────────────────────────────────────────────

use iris_agentic_dev_core::tools::admin_tools::SystemPerfMode;

#[test]
fn mode_start_parses() {
    assert_eq!(SystemPerfMode::parse("start"), Some(SystemPerfMode::Start));
    assert_eq!(SystemPerfMode::parse("START"), Some(SystemPerfMode::Start));
}

#[test]
fn mode_status_parses() {
    assert_eq!(
        SystemPerfMode::parse("status"),
        Some(SystemPerfMode::Status)
    );
}

#[test]
fn mode_last_runid_parses() {
    assert_eq!(
        SystemPerfMode::parse("last_runid"),
        Some(SystemPerfMode::LastRunId)
    );
}

#[test]
fn mode_unknown_returns_none() {
    assert_eq!(SystemPerfMode::parse(""), None);
    assert_eq!(SystemPerfMode::parse("run"), None);
    assert_eq!(SystemPerfMode::parse("begin"), None);
}

#[test]
fn mode_status_requires_run_id_is_documented() {
    // Verify the Status variant exists and is distinct from Start/LastRunId
    assert_ne!(
        SystemPerfMode::parse("status"),
        SystemPerfMode::parse("start")
    );
    assert_ne!(
        SystemPerfMode::parse("status"),
        SystemPerfMode::parse("last_runid")
    );
}

// ── SystemPerformance ObjectScript generation ─────────────────────────────────
//
// `run^SystemPerformance` takes a profile name. `Do run^SystemPerformance` with no
// argument throws <UNDEFINED> pname at run+4, which is what mode=start shipped with.
// These tests pin the generated code so the argument can't silently go missing again.

use iris_agentic_dev_core::tools::admin_tools::{
    sysperf_last_runid_code, sysperf_profile_or_default, sysperf_start_code, sysperf_status_code,
    sysperf_status_code_checked,
};

#[test]
fn start_code_passes_profile_to_run_entry_point() {
    let code = sysperf_start_code("test");
    assert!(
        code.contains(r#"$$run^SystemPerformance("test")"#),
        "start must call the extrinsic form with a profile argument; got:\n{code}"
    );
    assert!(
        !code.contains("Do run^SystemPerformance\n"),
        "the bare argument-less call throws <UNDEFINED> pname; got:\n{code}"
    );
}

#[test]
fn start_code_uses_the_returned_runid_not_a_global_scan() {
    // An in-flight run lives under ("run",<runid>); ("history",<runid>) only appears on
    // completion. Scanning history right after starting returns the *previous* run's ID.
    let code = sysperf_start_code("test");
    assert!(
        !code.contains(r#"^IRIS.SystemPerformance("history""#),
        "start must report the run ID that run^SystemPerformance returned, not the newest \
         completed run; got:\n{code}"
    );
}

#[test]
fn profile_defaults_to_test() {
    assert_eq!(sysperf_profile_or_default(None).unwrap(), "test");
    assert_eq!(sysperf_profile_or_default(Some("  ")).unwrap(), "test");
}

#[test]
fn profile_accepts_the_shipped_profile_names() {
    for p in &["test", "30mins", "4hours", "8hours", "12hours", "24hours"] {
        assert_eq!(sysperf_profile_or_default(Some(p)).unwrap(), *p);
    }
}

#[test]
fn profile_rejects_objectscript_injection() {
    // The profile is interpolated into a quoted ObjectScript string literal.
    for bad in &[
        r#"test") Do ^%ZSTOP //"#,
        "test\"",
        "a b",
        "test;halt",
        "^oddDEF",
    ] {
        assert!(
            sysperf_profile_or_default(Some(bad)).is_err(),
            "profile {bad:?} must be rejected before reaching ObjectScript"
        );
    }
}

#[test]
fn last_runid_code_reads_in_flight_runs_too() {
    let code = sysperf_last_runid_code();
    assert!(
        code.contains(r#"^IRIS.SystemPerformance("history""#),
        "must read completed runs; got:\n{code}"
    );
    assert!(
        code.contains(r#"^IRIS.SystemPerformance("run""#),
        "must also read the ('run') subtree — an in-flight run has no history node yet, so \
         history-only lookup returns the previous run or null mid-collection; got:\n{code}"
    );
}

/// `run^SystemPerformance` leaves the current device pointing somewhere else, so the `Write`
/// that follows it lands nowhere and the generator returns an empty string with a residual
/// `<NAMESPACE>` in $ZERROR. Capturing `$IO` before the call and re-selecting it after is what
/// makes the run ID come back.
#[test]
fn start_code_reselects_the_capture_device_before_writing() {
    let code = sysperf_start_code("test");
    let io = code.find("Set io=$IO").expect(
        "must snapshot the capture device before calling run^SystemPerformance; got:\n{code}",
    );
    let call = code.find("$$run^SystemPerformance").unwrap();
    let use_io = code
        .find("Use io")
        .expect("must re-select the capture device after the call; got:\n{code}");
    let write = code.find("Write tRun").unwrap();
    assert!(
        io < call && call < use_io && use_io < write,
        "order must be snapshot → call → re-select → write; got:\n{code}"
    );
}

/// `waittime^SystemPerformance` runs the same device-clobbering code path as `run`.
#[test]
fn status_code_reselects_the_capture_device_before_writing() {
    let code = sysperf_status_code("20260904_161059_test");
    assert!(
        code.contains("Set io=$IO") && code.contains("Use io"),
        "status must survive the device switch too; got:\n{code}"
    );
    assert!(
        code.contains(r#"$$waittime^SystemPerformance("20260904_161059_test")"#),
        "status must pass the run ID to the extrinsic form; got:\n{code}"
    );
}

#[test]
fn status_code_rejects_a_run_id_that_would_break_out_of_the_literal() {
    assert!(sysperf_status_code_checked(r#"x") Do ^%ZSTOP //"#).is_err());
    assert!(sysperf_status_code_checked("20260904_161059_test").is_ok());
}
