//! Spec 141 T200, live: `iris_coverage` and the line monitor.
//!
//! Against iris-dev-iris. The monitor is one per instance, shared by every session, so each test
//! first requires that none is running and refuses to go on if one is: a monitor that was already
//! there belongs to someone else and is never stopped here. A test that starts a monitor of its own
//! stops it before it asserts anything, so a failing assertion cannot leave it running.
//!
//! - a class's counted lines are the ones the run executed (the first element of a monitor row is
//!   the `RtnLine` count; the second is time);
//! - a class that compiles to several routines is counted whole (`Pkg.Cls.*`, not only `.1`);
//! - a class the monitor does not know is an error, not 0%, and the test run never starts;
//! - 0 of 0 lines is `pct: null`, and a target is neither met nor missed;
//! - a monitor already running is refused, and left running;
//! - a line that throws between start and stop is caught, the monitor stops, and the error is
//!   reported with the coverage gathered so far.
//!
//! Run, serialised behind the shared lock:
//!
//! ```text
//! lockf -k /tmp/iad-141-iris.lock cargo test --features testing --test integration \
//!     coverage_diag_141 -- --ignored --test-threads=1
//! ```

use super::test_message_body_redact_141_live::{conn, run};
use iris_agentic_dev_core::tools::coverage::guard_line;
use iris_agentic_dev_core::tools::{IrisTools, Toolset};
use serde_json::{json, Value};

const TARGET: &str = "Class IadCovDiag141.Target Extends %RegisteredObject\n{\n\nClassMethod Hit() As %Integer\n{\n    Set x = 1\n    Set y = x + 1\n    Quit y\n}\n\nClassMethod Miss() As %Integer\n{\n    Set a = 1\n    Quit a\n}\n\n}\n";
const EMPTY: &str = "Class IadCovDiag141.Empty Extends %RegisteredObject\n{\n\n}\n";
const PROD: &str = "Class IadCovDiag141.Prod Extends Ens.Production\n{\n\nXData ProductionDefinition\n{\n<Production Name=\"IadCovDiag141.Prod\" TestingEnabled=\"true\" LogGeneralTraceEvents=\"false\">\n  <Description></Description>\n  <ActorPoolSize>1</ActorPoolSize>\n</Production>\n}\n\n}\n";
const TEST: &str = "Class IadCovDiag141.ProdTest Extends %UnitTest.TestProduction\n{\n\nParameter PRODUCTION = \"IadCovDiag141.Prod\";\n\nMethod TestHit()\n{\n    Do $$$AssertEquals(##class(IadCovDiag141.Target).Hit(), 2, \"Hit returns 2\")\n}\n\n}\n";

const FIXTURES: [(&str, &str); 4] = [
    ("IadCovDiag141.Target.cls", TARGET),
    ("IadCovDiag141.Empty.cls", EMPTY),
    ("IadCovDiag141.Prod.cls", PROD),
    ("IadCovDiag141.ProdTest.cls", TEST),
];

fn body(r: &rmcp::model::CallToolResult) -> Value {
    r.structured_content.clone().unwrap_or(Value::Null)
}

/// The monitor's routine count and whether it reports itself active, read the same way the tool
/// reads them. A monitor started on routines that do not exist has a count of 0 and is active.
async fn monitor_state() -> (i64, bool) {
    let out = run(r#"Try { Set a=+##class(%Monitor.System.LineByLine).IsActive() } Catch ex { Set a="ERR "_ex.DisplayString() }
Write "N|"_##class(%Monitor.System.LineByLine).GetRoutineCount()_"|A|"_a"#)
    .await;
    let line = out
        .lines()
        .find(|l| l.trim().starts_with("N|"))
        .unwrap_or_else(|| panic!("the monitor state is readable: {out:?}"));
    let parts: Vec<&str> = line.trim().split('|').collect();
    let count = parts[1]
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("{out:?}"));
    (count, parts[3].trim() == "1")
}

/// Whether a monitor is running at all: routines monitored, or started and active.
async fn running() -> bool {
    let (count, active) = monitor_state().await;
    count > 0 || active
}

/// Stop the monitor, which the caller has just established it started.
async fn stop_own_monitor() {
    run("Do ##class(%Monitor.System.LineByLine).Stop() Write 1").await;
}

/// `^UnitTestRoot` as the instance had it: `had|value`, to put back.
static SAVED_ROOT: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Point `^UnitTestRoot` at a directory that exists, as the instance's owner would. A fresh
/// instance has none, `%UnitTest.Manager` then finds no tests, and iad does not set it (T218), so a
/// test that runs tests sets it itself and [`restore_unit_test_root`] puts back what was there.
/// `unset` leaves it undefined instead.
pub async fn use_unit_test_root(unset: bool) {
    let found = run(r#"Write "OLD|",$Data(^UnitTestRoot)#2,"|",$Get(^UnitTestRoot),"|""#).await;
    let old = found
        .lines()
        .find_map(|l| l.trim().strip_prefix("OLD|"))
        .unwrap_or_else(|| panic!("the unit test root is readable: {found:?}"))
        .to_string();
    {
        let mut saved = SAVED_ROOT.lock().unwrap();
        // A second call before a restore keeps the first, which is the instance's own.
        saved.get_or_insert(old);
    }
    let set = if unset {
        "Kill ^UnitTestRoot".to_string()
    } else {
        r#"Set ^UnitTestRoot=##class(%File).NormalizeDirectory($System.Util.ManagerDirectory()_"Temp")"#
            .to_string()
    };
    run(&format!("{set} Write 1")).await;
}

/// Put `^UnitTestRoot` back as [`use_unit_test_root`] found it.
pub async fn restore_unit_test_root() {
    let Some(old) = SAVED_ROOT.lock().unwrap().take() else {
        return;
    };
    let (had, value) = old.split_once('|').unwrap_or(("0", ""));
    let value = value.strip_suffix('|').unwrap_or(value);
    let code = if had == "1" {
        format!(
            "Set ^UnitTestRoot={} Write 1",
            iris_agentic_dev_core::objectscript::os_str_expr(value)
        )
    } else {
        "Kill ^UnitTestRoot Write 1".to_string()
    };
    run(&code).await;
}

/// Fixtures in, and the guarantee that no monitor is running. Panics, without stopping anything, if
/// one is.
async fn setup() -> IrisTools {
    let tools = IrisTools::for_test_gates_open(Some(conn()), Toolset::Merged).expect("tools");
    let (count, active) = monitor_state().await;
    assert!(
        count == 0 && !active,
        "a line monitor is already running on iris-dev-iris ({count} routines, active={active}). \
         It is not this test's, so it is not stopped: wait for its owner, or stop it where it \
         was started."
    );
    use_unit_test_root(false).await;
    for (name, content) in FIXTURES {
        let r = tools
            .call_for_test(
                "iris_doc",
                json!({"mode": "put", "name": name, "content": content, "compile": true, "namespace": "USER"}),
            )
            .await
            .expect("iris_doc answers");
        assert_ne!(r.is_error, Some(true), "put {name}: {:?}", body(&r));
    }
    tools
}

/// Fixtures out. Anything still active here was started during this test, after `setup` found the
/// monitor idle and while the shared lock kept every other run away, so it is stopped.
async fn teardown(tools: &IrisTools) {
    restore_unit_test_root().await;
    let (count, active) = monitor_state().await;
    if count > 0 || active {
        stop_own_monitor().await;
    }
    for (name, _) in FIXTURES.iter().rev() {
        let _ = tools
            .call_for_test(
                "iris_doc",
                json!({"mode": "delete", "name": name, "namespace": "USER"}),
            )
            .await;
    }
}

async fn cover(tools: &IrisTools, args: Value) -> Value {
    body(
        &tools
            .call_for_test("iris_coverage", args)
            .await
            .expect("iris_coverage answers"),
    )
}

fn run_args(classes: &[&str]) -> Value {
    json!({"mode": "run", "classes": classes, "test_path": "IadCovDiag141.ProdTest", "namespace": "USER"})
}

#[tokio::test]
#[ignore = "live IRIS on iris-dev-iris"]
async fn the_counted_lines_are_the_executed_ones_and_every_routine_of_the_class_counts() {
    let tools = setup().await;
    let b = cover(&tools, run_args(&["IadCovDiag141.Target"])).await;
    let left = running().await;
    teardown(&tools).await;

    assert_eq!(b["success"], true, "{b}");
    assert!(!left, "the monitor must be stopped after a run");
    let target = &b["classes"][0];
    assert_eq!(target["class"], "IadCovDiag141.Target", "{b}");
    // Hit() runs three lines: `Set x`, `Set y`, `Quit y`. Miss() and the rest do not run.
    assert_eq!(target["hit"], 3, "executed lines, not lines with time: {b}");
    let total = target["total"].as_i64().unwrap();
    assert!(total > 3, "{b}");
    let routines: Vec<&str> = target["routines"]
        .as_array()
        .unwrap_or_else(|| panic!("a class lists the routines it was counted over: {b}"))
        .iter()
        .map(|r| r.as_str().unwrap())
        .collect();
    assert_eq!(
        routines,
        ["IadCovDiag141.Target.0", "IadCovDiag141.Target.1"],
        "a class compiles to more than its `.1` routine: {b}"
    );
    assert_eq!(b["hits"], 3, "{b}");
}

/// A fresh instance has no `^UnitTestRoot`. `%UnitTest.Manager` finds no tests, and the monitor's
/// rows are empty for that reason: the answer names the root instead of calling the class "not
/// loaded" (spec 141, CI on IRIS 2025.3).
#[tokio::test]
#[ignore = "live IRIS on iris-dev-iris"]
async fn an_unset_unit_test_root_still_gives_a_real_run_and_none_afterwards() {
    // A fresh instance has no `^UnitTestRoot`. The run makes one of its own for the job, so the
    // TestProduction runs and its lines count, and afterwards the global is gone again (T218).
    let tools = setup().await;
    use_unit_test_root(true).await;
    let b = cover(&tools, run_args(&["IadCovDiag141.Target"])).await;
    let left = running().await;
    let root = run(r#"Write "ROOT|",$Data(^UnitTestRoot)#2,"|""#).await;
    teardown(&tools).await;

    assert_eq!(b["success"], true, "{b}");
    assert_eq!(
        b["classes"][0]["hit"], 3,
        "the test ran and its lines count: {b}"
    );
    assert!(
        root.contains("ROOT|0|"),
        "the run left a root behind: {root}"
    );
    assert!(!left, "the monitor was stopped");
}

#[tokio::test]
#[ignore = "live IRIS on iris-dev-iris"]
async fn a_class_the_monitor_does_not_know_is_an_error_and_nothing_is_left_running() {
    let tools = setup().await;
    let typo = cover(
        &tools,
        run_args(&["IadCovDiag141.Target", "IadCovDiag141.Targt"]),
    )
    .await;
    let after_run = running().await;
    let start = cover(
        &tools,
        json!({"mode": "start", "classes": ["IadCovDiag141.Targt"], "namespace": "USER"}),
    )
    .await;
    let after_start = running().await;
    teardown(&tools).await;

    assert_eq!(typo["success"], false, "{typo}");
    assert_eq!(typo["error_code"], "COVERAGE_ROUTINE_UNKNOWN", "{typo}");
    assert!(
        typo["message"]
            .as_str()
            .unwrap()
            .contains("IadCovDiag141.Targt"),
        "the error names the class: {typo}"
    );
    assert!(
        typo.get("classes").is_none(),
        "no coverage figure for it: {typo}"
    );
    assert!(!after_run, "the run stopped the monitor it started");
    assert_eq!(start["error_code"], "COVERAGE_ROUTINE_UNKNOWN", "{start}");
    assert!(!after_start, "start stopped the monitor it started");
}

#[tokio::test]
#[ignore = "live IRIS on iris-dev-iris"]
async fn zero_of_zero_lines_is_null_and_meets_no_target() {
    let tools = setup().await;
    let mut args = run_args(&["IadCovDiag141.Empty"]);
    args["target_pct"] = json!(50.0);
    let b = cover(&tools, args).await;
    let left = running().await;
    teardown(&tools).await;

    assert_eq!(b["success"], true, "{b}");
    assert!(!left);
    assert_eq!(b["classes"][0]["total"], 0, "{b}");
    assert!(b["classes"][0]["pct"].is_null(), "0 of 0 is not 0%: {b}");
    assert!(b["total_pct"].is_null(), "{b}");
    assert!(b["meets_target"].is_null(), "no lines, no verdict: {b}");
}

#[tokio::test]
#[ignore = "live IRIS on iris-dev-iris"]
async fn a_monitor_that_is_already_running_is_refused_and_left_running() {
    let tools = setup().await;
    // This test's own monitor, standing in for another session's.
    let started = run(r#"Set sc=##class(%Monitor.System.LineByLine).Start($lb("IadCovDiag141.Target.*"),"","") Write "STARTED|"_$System.Status.IsOK(sc)"#).await;
    let (check, run_mode, start, still) = if started.contains("STARTED|1") {
        let check = cover(&tools, json!({"mode": "check", "namespace": "USER"})).await;
        let run_mode = cover(&tools, run_args(&["IadCovDiag141.Target"])).await;
        let start = cover(
            &tools,
            json!({"mode": "start", "classes": ["IadCovDiag141.Empty"], "namespace": "USER"}),
        )
        .await;
        let still = monitor_state().await;
        (check, run_mode, start, still)
    } else {
        (Value::Null, Value::Null, Value::Null, (-1, false))
    };
    // Stopped by the test that started it, before anything is asserted.
    stop_own_monitor().await;
    let after = running().await;
    teardown(&tools).await;

    assert!(
        started.contains("STARTED|1"),
        "test monitor did not start: {started}"
    );
    for (label, b) in [("check", &check), ("run", &run_mode), ("start", &start)] {
        assert_eq!(b["success"], false, "{label}: {b}");
        assert_eq!(b["error_code"], "MONITOR_IN_USE", "{label}: {b}");
    }
    assert_eq!(
        still.0, 2,
        "the other session's monitor (Target.0 and Target.1) is still running after all three calls"
    );
    assert!(!after);
}

/// Stop on every path, against real IRIS: a guarded line that throws is caught and the lines after it
/// run, so the `Stop()` that ends the program is reached.
#[tokio::test]
#[ignore = "live IRIS on iris-dev-iris"]
async fn a_line_that_throws_between_start_and_stop_still_reaches_stop() {
    let tools = setup().await;
    let code = [
        " Set tErr=\"\"".to_string(),
        " Set sc=##class(%Monitor.System.LineByLine).Start($lb(\"IadCovDiag141.Target.*\"),\"\",\"\")".to_string(),
        guard_line(" Throw ##class(%Exception.General).%New(\"injected\",5001,,\"boom\")"),
        guard_line(" Set tAfter=1"),
        " Do ##class(%Monitor.System.LineByLine).Stop()".to_string(),
        " Write \"ERR|\"_$Piece(tErr,\" \",1,2)_\"|AFTER|\"_$Get(tAfter),!".to_string(),
    ]
    .join("\n");
    let out = run(&code).await;
    let left = running().await;
    if left {
        // Started above by this test, so stopped by it.
        stop_own_monitor().await;
    }
    teardown(&tools).await;

    assert!(
        out.contains("|AFTER|1"),
        "the line after the throw must run: {out}"
    );
    assert!(out.contains("ERR|"), "{out}");
    assert!(!out.contains("ERR||"), "the throw must be recorded: {out}");
    assert!(!left, "the program reached its Stop(): {out}");
}
