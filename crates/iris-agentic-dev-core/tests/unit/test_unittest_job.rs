//! `%UnitTest.Manager` checks `messages.log` after each test class and when it closes. If the
//! file grew, it opens it, makes it the current device, reads it and closes it, without putting
//! the caller's device back, so `$IO` falls to `$PRINCIPAL`. In a CSP process that is the gateway
//! socket: the captured output is lost, and if it happens mid-test the manager's own error text
//! goes out ahead of the HTTP headers. Any process writing to `messages.log` triggers it
//! (`.iad-local/bisect-130-device.md`). iad now runs the manager in a JOB whose principal device
//! is a file, then reads the file back; and a query answer that is not JSON is an error, never an
//! empty success. Measured live in `tests/integration/test_unittest_device_race_live.rs`.

use iris_agentic_dev_core::iris::connection::query_output;
use iris_agentic_dev_core::tools::coverage::build_coverage_run_code;
use iris_agentic_dev_core::tools::{build_test_run_code, run_in_job};

fn assert_runs_in_a_job(code: &str, call: &str) {
    assert!(
        code.contains(&format!("Job {call}")),
        "the manager must run in a JOB: {code}"
    );
    assert!(
        code.contains(":($NAMESPACE::"),
        "the JOB's principal device must be a file: {code}"
    );
    for line in code.lines() {
        let l = line.trim_start().to_ascii_lowercase();
        assert!(
            !(l.starts_with("do ") && l.contains(&call.to_ascii_lowercase())),
            "the manager is still called in the capturing process: {line}"
        );
    }
}

#[test]
fn every_iris_test_path_runs_the_manager_in_a_job() {
    let c = build_test_run_code("MyApp.Tests", "/verbose=1/nodelete/noload", "tok", false);
    assert_runs_in_a_job(&c, "##class(%UnitTest.Manager).RunTest(");
    let c = build_test_run_code("MyApp/Tests", "/verbose=1/nodelete", "tok", false);
    assert_runs_in_a_job(&c, "##class(%UnitTest.Manager).RunTest(");
    assert!(c.contains("^UnitTestRoot"), "the root is still set: {c}");
    let c = build_test_run_code("MyApp.ProdTest", "/verbose=1/nodelete/noload", "tok", true);
    assert_runs_in_a_job(&c, "##class(MyApp.ProdTest).Run(");
}

#[test]
fn iris_coverage_runs_the_manager_in_a_job() {
    let c = build_coverage_run_code(&["Demo.A.1".to_string()], "Demo", "USER");
    assert_runs_in_a_job(&c, "##class(%UnitTest.Manager).RunTest(");
}

/// The same code runs through docker exec, where a terminal reads one line at a time, so the
/// wrapper uses no braces and every statement fits on its line.
#[test]
fn the_job_wrapper_is_single_line_statements() {
    let w = run_in_job("##class(%UnitTest.Manager).RunTest(\"A\",\"/noload\")");
    assert!(!w.contains('{') && !w.contains('}'), "{w}");
    for line in w.lines() {
        assert!(
            line.starts_with(' '),
            "each line is a statement line: {line:?}"
        );
    }
    assert!(w.contains("TranslateTable=\"UTF8\""), "{w}");
    assert!(!w.contains("$$$"), "a terminal does not expand macros: {w}");
    assert!(
        w.contains("%Library.File).Delete("),
        "the file is removed: {w}"
    );
}

#[test]
fn a_query_answer_that_is_not_json_is_an_error() {
    let body = "ERROR #5001: A UnitTest changed the 'Device Redirect Flag'\r\nCacheSP: chd=1;\r\nHTTP/1.1 200 OK";
    let e = query_output(200, body).expect_err("not JSON").to_string();
    assert!(e.contains("not JSON"), "{e}");
    assert!(e.contains("ERROR #5001"), "the first bytes are shown: {e}");
}

#[test]
fn an_http_error_or_a_status_error_is_an_error() {
    let e = query_output(500, "{}").expect_err("500").to_string();
    assert!(e.contains("500"), "{e}");
    let body = r#"{"status":{"errors":[{"error":"ERROR #5540: SQLCODE: -400"}]},"result":{}}"#;
    let e = query_output(200, body)
        .expect_err("status error")
        .to_string();
    assert!(e.contains("#5540"), "{e}");
}

#[test]
fn a_well_formed_answer_still_reads_as_before() {
    let ok = r#"{"status":{"errors":[]},"result":{"content":[{"result":"a\u0001b"}]}}"#;
    assert_eq!(query_output(200, ok).unwrap(), "a\nb");
    let empty = r#"{"status":{"errors":[]},"result":{"content":[{"result":null}]}}"#;
    assert_eq!(query_output(200, empty).unwrap(), "");
    let structured = r#"{"status":{"errors":[]},"result":{"content":[{"result":[1,2]}]}}"#;
    assert_eq!(query_output(200, structured).unwrap(), "[1,2]");
}

/// The skill sends agents to `iris_test`, and says why the manager is not safe under capture.
#[test]
fn the_unit_test_skill_says_not_to_run_the_manager_under_capture() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../skills/skills/objectscript-unit-test/SKILL.md");
    let t = std::fs::read_to_string(path).unwrap();
    assert!(t.contains("messages.log"), "say what sets it off");
    assert!(t.contains("$PRINCIPAL"), "say where the device goes");
    assert!(t.contains("`Job`"), "say how to run the manager safely");
}

/// A blank `IRIS_CONTAINER` means no container. Read as a name, it sent `docker exec ''`, whose
/// error replaced the reason the HTTP call had failed (found while testing spec 140's precedence).
#[test]
fn a_blank_container_name_is_no_container() {
    use iris_agentic_dev_core::iris::connection::non_blank;
    assert_eq!(non_blank(None), None);
    assert_eq!(non_blank(Some(String::new())), None);
    assert_eq!(non_blank(Some("  ".into())), None);
    assert_eq!(non_blank(Some("iris-e2e".into())), Some("iris-e2e".into()));
}

/// A status error that comes with a result is not a failure: the result is what the call produced.
#[test]
fn a_status_error_beside_a_result_returns_the_result() {
    let body = r#"{"status":{"errors":[{"error":"ERROR #5035: General exception Name 'Parsing error'"}]},"result":{"content":[{"result":"dispatch"}]}}"#;
    assert_eq!(query_output(200, body).unwrap(), "dispatch");
}

/// The generator's result carries a leading $Char(2) so Atelier returns it as text; it is removed.
#[test]
fn the_result_marker_is_removed() {
    let body =
        "{\"status\":{\"errors\":[]},\"result\":{\"content\":[{\"result\":\"\\u0002[1,2\"}]}}";
    assert_eq!(query_output(200, body).unwrap(), "[1,2");
}
