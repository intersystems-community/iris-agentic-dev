//! Regression tests for generator failures that used to be reported as success.
//!
//! `execute_via_generator` reports an IRIS-side failure *inside* the output string — the HTTP call
//! and the compile succeeded, so `Result` has nothing to carry. Every site below read that string
//! as data: an error message became a username, a roles list, a stopped monitor, a deleted skill.
//! The pure decision each site now makes is tested here against all four failure shapes; the live
//! counterparts are in `tests/integration/test_generator_false_success_live.rs`.
//!
//! The four shapes come from `is_generator_error` / `generator_error_message`, which is the only
//! sanctioned place to know about them.

use iris_agentic_dev_core::tools::admin::decode_user_roles_output;
use iris_agentic_dev_core::tools::admin_tools::username_from_generator_output;
use iris_agentic_dev_core::tools::coverage::parse_stop_output;
use iris_agentic_dev_core::tools::interop::{
    build_production_recover_code, build_production_start_code, build_production_stop_code,
    build_production_update_code,
};
use iris_agentic_dev_core::tools::skills_tools::forget_confirmed;

/// Real text of each failure shape, as IRIS and the generator wrapper actually produce it.
const FAILURE_SHAPES: [&str; 4] = [
    // Catch block in build_exec_class.
    "ERROR: <UNDEFINED>zExecute+9^IrisDevTmp.IrisDevRun4f2a1c9b.1 *gRef\n",
    // Body wrote nothing and left $ZERROR set.
    "ERROR($ZERROR): <PROTECT>zExecute+4^IrisDevTmp.IrisDevRun4f2a1c9b.1 ^SKILLS\n",
    // Body left the current device somewhere else, so its output is unreadable.
    "ERROR($DEVICE): the called code left the current device set to \"|TRM|:\" instead of the capture file, so its output was written elsewhere and is lost.\n",
    // Tool-generated ObjectScript sentinel, no space after the colon.
    "ERROR:SOME_SENTINEL:something specific went wrong\n",
];

// ── my_access / capability_matrix: the $USERNAME read ─────────────────────────

#[test]
fn username_rejects_every_failure_shape() {
    for shape in FAILURE_SHAPES {
        let got = username_from_generator_output(shape);
        assert!(
            got.is_err(),
            "failure text must not be accepted as a username: {shape:?} produced {got:?}"
        );
    }
}

#[test]
fn username_error_carries_the_iris_message() {
    let err = username_from_generator_output(FAILURE_SHAPES[1]).unwrap_err();
    assert!(
        err.contains("<PROTECT>"),
        "the IRIS error text is the only actionable part; got: {err}"
    );
    assert!(
        !err.contains("ERROR($ZERROR)"),
        "the prefix belongs to the transport, not the message; got: {err}"
    );
}

#[test]
fn username_rejects_empty_output() {
    // No output means the `Write $USERNAME` never ran. An empty username makes the Security.Users
    // lookup miss, and the miss branch answers `"roles": []` with success — the same lie without
    // any error text to notice.
    assert!(username_from_generator_output("").is_err());
    assert!(username_from_generator_output("   \n").is_err());
}

#[test]
fn username_accepts_real_output() {
    assert_eq!(
        username_from_generator_output("_SYSTEM\n").unwrap(),
        "_SYSTEM"
    );
    // A username may legitimately contain "error" — only the prefixes are failures.
    assert_eq!(
        username_from_generator_output("errorhandler\n").unwrap(),
        "errorhandler"
    );
}

// ── iris_admin list_user_roles: the roles decode ──────────────────────────────

#[test]
fn roles_decode_rejects_generator_failures() {
    for shape in &FAILURE_SHAPES[..3] {
        let (code, msg) = decode_user_roles_output("_SYSTEM", shape)
            .expect_err("failure text must not decode into a roles list");
        assert_eq!(code, "IRIS_EXECUTE_ERROR", "message was: {msg}");
        assert!(
            msg.contains("_SYSTEM"),
            "the message must name the user asked about; got: {msg}"
        );
    }
}

#[test]
fn roles_decode_keeps_the_user_not_found_sentinel() {
    let (code, msg) = decode_user_roles_output(
        "nosuchuser",
        "ERROR:USER_NOT_FOUND:User not found: nosuchuser",
    )
    .expect_err("USER_NOT_FOUND must stay an error");
    assert_eq!(
        code, "USER_NOT_FOUND",
        "the specific code must survive the generic check; got {code}: {msg}"
    );
}

#[test]
fn roles_decode_reads_a_real_roles_line() {
    let roles = decode_user_roles_output("_SYSTEM", "%All,%Manager\n").unwrap();
    assert_eq!(roles, vec!["%All".to_string(), "%Manager".to_string()]);
    assert_eq!(
        decode_user_roles_output("nobody", "\n").unwrap(),
        Vec::<String>::new()
    );
}

// ── iris_coverage mode=stop ───────────────────────────────────────────────────

#[test]
fn stop_rejects_every_failure_shape() {
    for shape in FAILURE_SHAPES {
        let v = parse_stop_output(shape);
        assert_eq!(
            v["success"].as_bool(),
            Some(false),
            "stop must not report success on: {shape:?} (got {v})"
        );
        assert_eq!(v["error_code"].as_str(), Some("IRIS_EXECUTE_ERROR"));
    }
}

#[test]
fn stop_requires_the_confirmation_marker() {
    // The monitor holds a single process-wide slot. A stop that did not run leaves it held, and the
    // next start fails with MONITOR_IN_USE pointing nowhere near the real cause.
    let v = parse_stop_output("");
    assert_eq!(v["success"].as_bool(), Some(false), "got {v}");
    let v = parse_stop_output("<SYNTAX>zExecute+3^IrisDevTmp.IrisDevRun1.1\n");
    assert_eq!(v["success"].as_bool(), Some(false), "got {v}");
}

#[test]
fn stop_accepts_the_confirmed_stop() {
    let v = parse_stop_output("OK|stopped\n");
    assert_eq!(v["success"].as_bool(), Some(true), "got {v}");
    assert_eq!(v["stopped"].as_bool(), Some(true), "got {v}");
    // Docker exec adds surrounding blank lines; the marker still has its own line.
    let v = parse_stop_output("\nOK|stopped\n\n");
    assert_eq!(v["success"].as_bool(), Some(true), "got {v}");
}

// ── skill_forget (docker exec path) ───────────────────────────────────────────

#[test]
fn forget_needs_the_marker_on_its_own_line() {
    assert!(forget_confirmed("OK\n"));
    assert!(forget_confirmed("\nOK\n"));
    assert!(!forget_confirmed(""));
    assert!(!forget_confirmed("\n\n"));
}

#[test]
fn forget_rejects_the_terminal_echo_of_the_failing_line() {
    // Captured from `iris session` in iris-dev-iris: on error the terminal echoes the line back
    // uppercased, with the caret and the error underneath, and still exits 0. The echo contains
    // `WRITE "OK"`, so any `contains("OK")` check passes on exactly this case.
    let echoed = "KILL ^SKILLS(\"a\\\"b\") WRITE \"OK\"\n                     ^\n<SYNTAX>\n";
    assert!(
        !forget_confirmed(echoed),
        "the echoed source line is not a confirmation"
    );
}

// ── Ens.Director production operations: $IO snapshot/restore ──────────────────

/// The call must sit between `Set tIO=$IO` and `Use tIO`, and the verdict `Write` must come after
/// the restore — otherwise the verdict is written to whatever device the production left current
/// and the handler reads empty output.
fn assert_device_bracketed(code: &str, method: &str) {
    let lines: Vec<&str> = code.lines().map(str::trim).collect();
    let index_of = |pred: &dyn Fn(&str) -> bool| -> usize {
        lines
            .iter()
            .position(|l| pred(l))
            .unwrap_or_else(|| panic!("line not found in {method} code:\n{code}"))
    };
    let snapshot = index_of(&|l: &str| l == "Set tIO=$IO");
    let call = index_of(&|l: &str| l.contains(&format!("##class(Ens.Director).{method}")));
    let restore = index_of(&|l: &str| l == "Use tIO");
    let verdict = index_of(&|l: &str| l.starts_with("If $System.Status.IsError(sc)"));
    assert!(
        snapshot < call,
        "{method}: $IO must be snapshotted before the call:\n{code}"
    );
    assert!(
        call < restore,
        "{method}: $IO must be restored after the call:\n{code}"
    );
    assert!(
        restore < verdict,
        "{method}: the verdict Write must come after the restore:\n{code}"
    );
    assert!(
        !lines[call].contains("Write"),
        "{method}: nothing may be written on the call line — the device is unknown there:\n{code}"
    );
}

#[test]
fn production_start_brackets_the_device() {
    let code = build_production_start_code("Demo.Production");
    assert!(code.contains("\"Demo.Production\""), "got:\n{code}");
    assert_device_bracketed(&code, "StartProduction");
}

#[test]
fn production_stop_brackets_the_device() {
    let code = build_production_stop_code(30, true);
    assert!(code.contains("StopProduction(30,1)"), "got:\n{code}");
    assert_device_bracketed(&code, "StopProduction");
    assert!(
        build_production_stop_code(10, false).contains("StopProduction(10,0)"),
        "force=false must pass 0"
    );
}

#[test]
fn production_update_brackets_the_device() {
    let code = build_production_update_code(45, false);
    assert!(code.contains("UpdateProduction(45,0)"), "got:\n{code}");
    assert_device_bracketed(&code, "UpdateProduction");
}

#[test]
fn production_recover_brackets_the_device() {
    let code = build_production_recover_code();
    assert!(code.contains("RecoverProduction()"), "got:\n{code}");
    assert!(
        code.contains("Set tIO=$IO") && code.contains("Use tIO"),
        "recover must snapshot and restore $IO:\n{code}"
    );
    assert_device_bracketed(&code, "RecoverProduction");
}
