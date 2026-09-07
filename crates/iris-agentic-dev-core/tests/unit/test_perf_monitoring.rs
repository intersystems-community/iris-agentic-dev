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

// ── Profile management and report retrieval (096) ──────────────────────────────
//
// 089 shipped start/status/last_runid, which is enough to collect data and nothing else: no way
// to see which profiles an instance has, no way to add one, and no way to find the report a
// finished run wrote. Everything here was measured against iris-dev-iris first — the shapes
// below are what IRIS actually stores and returns, not what the docs imply.

use iris_agentic_dev_core::tools::admin_tools::{
    parse_profile_line, parse_profile_write_result, parse_report_line, parse_run_line,
    sysperf_add_profile_code, sysperf_delete_profile_code, sysperf_description_checked,
    sysperf_interval_checked, sysperf_list_profiles_code, sysperf_list_runs_code,
    sysperf_profile_name_checked, sysperf_report_code, sysperf_sample_count_checked,
};

#[test]
fn the_new_modes_parse() {
    assert_eq!(
        SystemPerfMode::parse("list_profiles"),
        Some(SystemPerfMode::ListProfiles)
    );
    assert_eq!(
        SystemPerfMode::parse("add_profile"),
        Some(SystemPerfMode::AddProfile)
    );
    assert_eq!(
        SystemPerfMode::parse("delete_profile"),
        Some(SystemPerfMode::DeleteProfile)
    );
    assert_eq!(
        SystemPerfMode::parse("list_runs"),
        Some(SystemPerfMode::ListRuns)
    );
    assert_eq!(
        SystemPerfMode::parse("report"),
        Some(SystemPerfMode::Report)
    );
    assert_eq!(
        SystemPerfMode::parse("REPORT"),
        Some(SystemPerfMode::Report)
    );
}

/// A profile node is `$LB(description, interval_seconds, sample_count)` — measured on
/// iris-dev-iris, where `test` reads `A 5 minute TEST run sampling every 30 seconds~30~10`.
/// The description is emitted last because it is free text and may contain the delimiter.
#[test]
fn profile_line_decodes_name_interval_count_and_description() {
    let v = parse_profile_line("test|30|10|A 5 minute TEST run sampling every 30 seconds")
        .expect("a well-formed profile line must parse");
    assert_eq!(v["name"], "test");
    assert_eq!(v["interval_seconds"], 30);
    assert_eq!(v["sample_count"], 10);
    assert_eq!(
        v["description"],
        "A 5 minute TEST run sampling every 30 seconds"
    );
}

/// 30 s × 10 samples is the 5 minutes the profile's own description claims. Reporting the
/// duration is the difference between "which of these is safe to start right now" and reading
/// two numbers and doing the arithmetic by hand.
#[test]
fn profile_line_reports_the_run_duration() {
    let v = parse_profile_line("test|30|10|five minutes").unwrap();
    assert_eq!(v["duration_minutes"], 5.0);
    let v = parse_profile_line("24hours|10|8640|a day").unwrap();
    assert_eq!(v["duration_minutes"], 1440.0);
}

#[test]
fn profile_line_keeps_a_description_containing_the_delimiter() {
    let v = parse_profile_line("odd|1|60|before|after").unwrap();
    assert_eq!(v["description"], "before|after");
}

#[test]
fn profile_line_survives_a_node_that_is_not_a_list() {
    // `$LISTVALID` fails and the ObjectScript emits empty interval/count rather than throwing
    // <LIST> and taking the whole listing down with it.
    let v = parse_profile_line("legacy|||just a string").unwrap();
    assert_eq!(v["name"], "legacy");
    assert!(v["interval_seconds"].is_null());
    assert!(v["sample_count"].is_null());
    assert!(v["duration_minutes"].is_null());
}

#[test]
fn profile_line_rejects_the_done_marker_and_junk() {
    assert!(parse_profile_line("DONE|7").is_none());
    assert!(parse_profile_line("").is_none());
    assert!(parse_profile_line("no-delimiter").is_none());
}

#[test]
fn list_profiles_code_reads_the_profile_subtree_and_marks_the_end() {
    let code = sysperf_list_profiles_code();
    assert!(
        code.contains(r#"^IRIS.SystemPerformance("profile""#),
        "profiles live in the ('profile') subtree; got:\n{code}"
    );
    assert!(
        code.contains("$LISTVALID"),
        "one corrupt node must not throw <LIST> and lose the whole listing; got:\n{code}"
    );
    assert!(
        code.contains(r#"Write "DONE|""#),
        "the listing needs an end marker, or a truncated read looks like an empty instance; \
         got:\n{code}"
    );
}

/// `$$addprofile^SystemPerformance("bad name",...)` returns **1** and stores the profile as
/// `badname` — measured. The caller is told it succeeded and the name it asked for does not
/// exist. Rejecting the name here is the only way the caller learns.
#[test]
fn add_profile_rejects_a_name_iris_would_silently_rewrite() {
    assert!(sysperf_profile_name_checked(Some("bad name")).is_err());
    assert!(sysperf_profile_name_checked(Some("my-profile")).is_err());
    assert!(sysperf_profile_name_checked(Some(r#"x") Do ^%ZSTOP //"#)).is_err());
    assert_eq!(
        sysperf_profile_name_checked(Some(" perfdemo1s ")).unwrap(),
        "perfdemo1s"
    );
}

#[test]
fn add_profile_requires_a_name() {
    assert!(sysperf_profile_name_checked(None).is_err());
    assert!(sysperf_profile_name_checked(Some("   ")).is_err());
}

#[test]
fn description_is_escaped_not_rejected_for_ordinary_prose() {
    // Descriptions are prose: apostrophes, commas and parentheses all have to survive.
    assert_eq!(
        sysperf_description_checked(Some("1 s samples, 4 min (iad demo)")).unwrap(),
        "1 s samples, 4 min (iad demo)"
    );
    // A double quote would close the ObjectScript literal; doubling it is the ObjectScript
    // escape, so the description keeps its meaning instead of being refused.
    assert_eq!(
        sysperf_description_checked(Some(r#"the "fast" one"#)).unwrap(),
        r#"the ""fast"" one"#
    );
}

#[test]
fn description_rejects_a_line_break_that_would_split_the_command() {
    // The code is newline-delimited ObjectScript, so an embedded newline is a new command.
    assert!(sysperf_description_checked(Some("first\nDo ^%ZSTOP")).is_err());
    assert!(sysperf_description_checked(Some("carriage\rreturn")).is_err());
}

#[test]
fn description_is_required_because_addprofile_takes_four_arguments() {
    assert!(sysperf_description_checked(None).is_err());
    assert!(sysperf_description_checked(Some("  ")).is_err());
}

#[test]
fn interval_and_count_must_be_positive_integers() {
    assert_eq!(sysperf_interval_checked(Some(1)).unwrap(), 1);
    assert_eq!(sysperf_sample_count_checked(Some(240)).unwrap(), 240);
    assert!(sysperf_interval_checked(Some(0)).is_err());
    assert!(sysperf_interval_checked(Some(-5)).is_err());
    assert!(sysperf_sample_count_checked(Some(0)).is_err());
    assert!(sysperf_interval_checked(None).is_err());
    assert!(sysperf_sample_count_checked(None).is_err());
}

#[test]
fn add_profile_code_calls_addprofile_with_all_four_arguments() {
    let code = sysperf_add_profile_code("perfdemo1s", "1 second samples", 1, 240);
    assert!(
        code.contains(r#"$$addprofile^SystemPerformance("perfdemo1s","1 second samples",1,240)"#),
        "got:\n{code}"
    );
    assert!(
        code.contains("Set io=$IO") && code.contains("Use io"),
        "the SystemPerformance entry points move the current device; got:\n{code}"
    );
}

/// `addprofile` returning 1 is not proof the profile exists under the requested name — that is
/// exactly how the `bad name` → `badname` rewrite hides. The generated code reads the node back.
#[test]
fn add_profile_code_reads_the_stored_node_back() {
    let code = sysperf_add_profile_code("perfdemo1s", "d", 1, 240);
    assert!(
        code.contains(r#"$D(^IRIS.SystemPerformance("profile","perfdemo1s"))"#),
        "must confirm the profile landed under the name the caller asked for; got:\n{code}"
    );
}

#[test]
fn delete_profile_code_calls_delprofile() {
    let code = sysperf_delete_profile_code("perfdemo1s");
    assert!(
        code.contains(r#"$$delprofile^SystemPerformance("perfdemo1s")"#),
        "got:\n{code}"
    );
}

/// Both writers answer `1` or `0^<reason>` — measured: a duplicate name gives
/// `0^profile name exists already`.
#[test]
fn profile_write_result_carries_the_reason_iris_gave() {
    assert_eq!(parse_profile_write_result("1"), Ok(()));
    assert_eq!(
        parse_profile_write_result("0^profile name exists already"),
        Err("profile name exists already".to_string())
    );
}

#[test]
fn profile_write_result_does_not_invent_success_from_empty_output() {
    // An empty generator result means the Write never landed, not that the write worked.
    assert!(parse_profile_write_result("").is_err());
    assert!(parse_profile_write_result("0").is_err());
}

#[test]
fn run_line_decodes_the_history_node() {
    // History node is `<$H of completion>^<output directory>`; the ObjectScript converts the
    // $HOROLOG with $ZDT(...,3) so no date arithmetic happens in Rust.
    let v = parse_run_line("20260907_135415_test|2026-09-07 13:54:35|/usr/irissys/mgr/")
        .expect("a well-formed history line must parse");
    assert_eq!(v["run_id"], "20260907_135415_test");
    assert_eq!(v["completed_at"], "2026-09-07 13:54:35");
    assert_eq!(v["output_dir"], "/usr/irissys/mgr/");
    assert_eq!(v["profile"], "test");
}

#[test]
fn run_line_reads_the_profile_out_of_the_run_id() {
    // Run IDs are `YYYYMMDD_HHMMSS_<profile>`, and a profile name may contain underscores.
    let v = parse_run_line("20260907_011924_perf_demo_1s|2026-09-07 01:23:00|/x/").unwrap();
    assert_eq!(v["profile"], "perf_demo_1s");
}

#[test]
fn run_line_rejects_the_done_marker() {
    assert!(parse_run_line("DONE|33").is_none());
    assert!(parse_run_line("").is_none());
}

#[test]
fn list_runs_code_returns_the_newest_runs_first_and_caps_the_listing() {
    let code = sysperf_list_runs_code();
    assert!(
        code.contains(r#"^IRIS.SystemPerformance("history""#),
        "got:\n{code}"
    );
    assert!(
        code.contains(",-1)"),
        "iterate descending: an instance can hold hundreds of runs and the newest are the ones \
         worth reporting; got:\n{code}"
    );
    assert!(code.contains(r#"Write "DONE|""#), "got:\n{code}");
}

/// The report file is `<logdir><nodename>_<instance>_<runid>.html` — measured as
/// `/usr/irissys/mgr/57eb64844aa4_IRIS_20260907_135415_test.html`. `$ZU(110)` is the piece that
/// matches: `%SYS.System.GetNodeName()` returns it upper-cased and the file name is lower.
#[test]
fn report_code_builds_the_file_name_from_the_node_name_and_instance() {
    let code = sysperf_report_code("20260907_135415_test");
    assert!(code.contains("$ZU(110)"), "got:\n{code}");
    assert!(
        code.contains("20260907_135415_test"),
        "the run ID is part of the file name; got:\n{code}"
    );
    assert!(
        code.contains("%File"),
        "existence and size come from %File, not from a guess; got:\n{code}"
    );
}

/// Reconstructing the name assumes the node name has not changed since the run. It can (rename
/// the host, move the container), so a miss falls back to matching the run ID in the directory.
#[test]
fn report_code_falls_back_to_scanning_the_directory() {
    let code = sysperf_report_code("20260907_135415_test");
    assert!(
        code.contains("FileSet"),
        "a reconstructed name that does not exist must be looked up, not reported as missing; \
         got:\n{code}"
    );
}

#[test]
fn report_code_defaults_to_the_newest_completed_run() {
    let code = sysperf_report_code("");
    assert!(
        code.contains(r#"$O(^IRIS.SystemPerformance("history",""),-1)"#),
        "mode=report with no run_id must resolve the newest completed run; got:\n{code}"
    );
}

#[test]
fn report_code_rejects_a_run_id_that_would_break_out_of_the_literal() {
    // Empty is legal (newest run); anything outside the run-ID charset is not.
    assert!(sysperf_report_code_checked("").is_ok());
    assert!(sysperf_report_code_checked("20260907_135415_test").is_ok());
    assert!(sysperf_report_code_checked(r#"x") Do ^%ZSTOP //"#).is_err());
}

use iris_agentic_dev_core::tools::admin_tools::sysperf_report_code_checked;

#[test]
fn report_line_decodes_the_resolved_file() {
    let v = parse_report_line(
        "20260907_135415_test|2026-09-07 13:54:35|/usr/irissys/mgr/|\
         /usr/irissys/mgr/57eb64844aa4_IRIS_20260907_135415_test.html|4509627|1",
    )
    .expect("a well-formed report line must parse");
    assert_eq!(v["run_id"], "20260907_135415_test");
    assert_eq!(v["completed_at"], "2026-09-07 13:54:35");
    assert_eq!(v["output_dir"], "/usr/irissys/mgr/");
    assert_eq!(
        v["report_path"],
        "/usr/irissys/mgr/57eb64844aa4_IRIS_20260907_135415_test.html"
    );
    assert_eq!(v["size_bytes"], 4509627);
    assert_eq!(v["exists"], true);
}

/// A run that is still collecting has no history node and no file. Saying so beats returning a
/// path that is not there yet.
#[test]
fn report_line_reports_a_missing_report_as_missing() {
    let v = parse_report_line("20260907_140000_test|||||0").unwrap();
    assert_eq!(v["exists"], false);
    assert!(v["report_path"].is_null());
    assert!(v["size_bytes"].is_null());
    assert!(v["completed_at"].is_null());
}
