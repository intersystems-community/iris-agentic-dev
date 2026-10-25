//! iris_coverage — ObjectScript line coverage via %Monitor.System.LineByLine.

use crate::tools::exec::{ExecContext, RunOpts};
use schemars::JsonSchema;
use serde::Deserialize;

fn err_json(code: &str, msg: &str) -> serde_json::Value {
    serde_json::json!({"success": false, "error_code": code, "message": msg})
}

// ── Params ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrisCoverageParams {
    /// Confirms the call on a `subject` server, where a write is refused until it is set. No
    /// effect on other servers.
    #[serde(default)]
    pub confirm: bool,
    /// Mode: "start" | "stop" | "report" | "run" | "check"
    #[schemars(extend("enum" = ["start", "stop", "report", "run", "check"]))]
    pub mode: String,
    /// Explicit list of class names (without .1) — mutually exclusive with package
    pub classes: Option<Vec<String>>,
    /// Auto-discover all concrete classes in package via %Dictionary.ClassDefinition
    pub package: Option<String>,
    /// Test pattern for mode=run, run the way `iris_test` runs it: a compiled class pattern (e.g.
    /// "MyApp.Tests") is looked up in the database, a directory is scanned, and a
    /// `%UnitTest.TestProduction` subclass runs through `.Run()`.
    pub test_path: Option<String>,
    /// Coverage target percentage (default 90.0)
    pub target_pct: Option<f64>,
    /// IRIS namespace (defaults to connection default)
    pub namespace: Option<String>,
    /// Write Cobertura XML to this path (requires TestCoverage IPM package)
    pub cobertura_path: Option<String>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default)]
    pub server: Option<String>,
}

// ── Routine name helpers ──────────────────────────────────────────────────────

/// `"MyApp.MyClass"` → `"MyApp.MyClass.*"`: the pattern that selects every routine the class compiled
/// to. The monitor appends `.obj` to a name and resolves it through `$$LIST^%R`, which understands a
/// trailing `*` and nothing else, so a bare `Pkg.Cls.1` misses the class's other routines (`Pkg.Cls.0`
/// is the include routine of every class; a class with no methods has no `.1` at all). The dot before
/// the `*` keeps `Pkg.Cls*` from also taking `Pkg.ClsOther`.
pub fn build_routine_name(class: &str) -> String {
    format!("{}.*", class)
}

/// `"MyApp.MyClass.1"` or `"MyApp.MyClass.*"` → `"MyApp.MyClass"`: the class a routine name or pattern
/// belongs to. A name with neither suffix is returned as it is.
pub fn strip_routine_suffix(routine: &str) -> String {
    match routine.rsplit_once('.') {
        Some((class, tail))
            if tail == "*" || (!tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit())) =>
        {
            class.to_string()
        }
        _ => routine.to_string(),
    }
}

/// `line` as one statement that cannot end the generated program: a throw is caught, the first one
/// is kept in `tErr`, and the next line runs. One line with its braces closed, so the docker exec
/// terminal reads it as it reads the rest.
pub fn guard_line(line: &str) -> String {
    format!(
        " Try {{ {} }} Catch tEx {{ If tErr=\"\" Set tErr=tEx.DisplayString() }}",
        line.trim()
    )
}

// ── ObjectScript code builders ────────────────────────────────────────────────

// execute_via_generator cannot return content containing `{` characters.
// All output uses pipe-delimited format; JSON is assembled in Rust.
//
// Coverage output protocol:
//   CHECK mode:  "OK|bbsiz_state" or "BBSIZ_NOT_CONFIGURED|<message>"
//   RUN mode:    Lines of "CLASS|routine|hit|total" then "TOTAL|hit|total|truncated"
//   ERROR:       "ERROR|<code>|<message>"

/// The monitor class, spelled once for the generated code.
const MONITOR: &str = "##class(%Monitor.System.LineByLine)";

/// The metric the monitor is asked for: executions per line. The first element of a result row is
/// then the count; with the default metrics the second is time, and reading it as a count (as the
/// code did until 141) called a line hit only when it took measurable time.
const METRIC_LIST: &str = "$lb(\"RtnLine\")";

/// Lines that end the program when a monitor is already running. The monitor is one per instance
/// and belongs to whoever started it: another session, another tool, a TestCoverage run. Stopping
/// it to make room (what every mode did before 141) takes their data away. A monitor started on
/// routines that do not exist has no routines to count, and `Start` below reports it.
fn refuse_if_running_lines() -> Vec<String> {
    vec![
        format!(" Set iadN={MONITOR}.GetRoutineCount()"),
        format!(
            r#" If iadN>0  Write "ERROR|MONITOR_IN_USE|a line monitor is already running on this instance ("_iadN_" routines, the first is "_{MONITOR}.GetRoutineName(1)_"). It may belong to another session, so iris_coverage left it alone. Stop it where it was started, or call mode=stop if it is yours.",$C(10)  Quit"#
        ),
    ]
}

/// Lines that end the program when a requested class has no routine in the monitor. `Start` accepts
/// a pattern that matches nothing and answers OK, so a typo is otherwise indistinguishable from a
/// class that was monitored and never ran. Every compiled class has at least `Pkg.Cls.0`, so a class
/// with no monitored routine under `Pkg.Cls.` is not a compiled class in this namespace. `stop` ends
/// a monitor this program started.
fn unknown_class_lines(classes: &[String], stop: bool) -> Vec<String> {
    let stop = if stop {
        format!("Do {MONITOR}.Stop()  ")
    } else {
        String::new()
    };
    let mut lines = vec![format!(" Set iadN={MONITOR}.GetRoutineCount()")];
    for class in classes {
        let prefix = format!("{class}.");
        let message = format!(
            "{class} has no compiled routine in this namespace, so there is nothing to count. Check the class name and that it is compiled."
        );
        lines.push(format!(
            " Set iadFound=0  For iadI=1:1:iadN  Set:$Extract({MONITOR}.GetRoutineName(iadI),1,{})={} iadFound=1",
            prefix.chars().count(),
            crate::objectscript::os_str_expr(&prefix)
        ));
        lines.push(format!(
            r#" If 'iadFound  {stop}Write "ERROR|COVERAGE_ROUTINE_UNKNOWN|"_{},$C(10)  Quit"#,
            crate::objectscript::os_str_expr(&message)
        ));
    }
    lines
}

/// Build ObjectScript that pre-flight checks the monitor by attempting a dry Start.
/// Output: "OK|ready", "BBSIZ_NOT_CONFIGURED|<msg>", or "ERROR|MONITOR_IN_USE|<msg>" when a monitor
/// is already running (which is left alone).
///
/// Uses single-line commands (no multi-line blocks) so the code works via both
/// execute_via_generator (HTTP/Atelier) and execute (docker exec terminal).
pub fn build_coverage_check_code(namespace: &str) -> String {
    let mut lines = vec![format!(
        " New $NAMESPACE  Set $NAMESPACE={}",
        crate::objectscript::os_str_expr(namespace)
    )];
    lines.extend(refuse_if_running_lines());
    lines.extend([
        format!(" Set sc={MONITOR}.Start($lb(\"%Library.RegisteredObject.1\"),\"\",\"\")"),
        r#" If $System.Status.IsError(sc)  Write "BBSIZ_NOT_CONFIGURED|Start() failed: "_$System.Status.GetErrorText(sc)_" — increase gmheap (Management Portal > System Administration > Configuration > Additional Settings > Advanced Memory > gmheap, set to 256+, restart IRIS)",$C(10)  Quit"#.to_string(),
        format!(" Do {MONITOR}.Stop()"),
        r#" Write "OK|ready",$C(10)"#.to_string(),
    ]);
    lines.join("\n")
}

/// Build the routine list fragment for ObjectScript ($lb("R1","R2",...))
fn build_routine_list_fragment(routines: &[String]) -> String {
    let quoted: Vec<String> = routines
        .iter()
        .map(|r| crate::objectscript::os_str_expr(r))
        .collect();
    format!("$lb({})", quoted.join(","))
}

/// The lines that read the monitor: one `Class|routine|hit|total` row per monitored routine, then
/// `TOTAL|hit|total`. The routines are enumerated from the monitor, not from the request, because a
/// class pattern (`Pkg.Cls.*`) stands for however many routines the class compiled to. A row's first
/// element is the `RtnLine` count (`METRIC_LIST`); a line counts as hit when it ran, and the lines
/// counted are all those the monitor lists for the routine. One line, with its braces closed.
fn collect_lines() -> Vec<String> {
    vec![
        " Set iadTotalHit=0  Set iadTotalExec=0".to_string(),
        format!(
            r#" For iadI=1:1:{MONITOR}.GetRoutineCount() {{ Set iadNm={MONITOR}.GetRoutineName(iadI)  Set iadRs=##class(%ResultSet).%New("%Monitor.System.LineByLine:Result")  Do iadRs.Execute(iadNm)  Set iadHit=0  Set iadExec=0  While iadRs.Next() {{ Set iadEc=$ListGet(iadRs.GetData(1),1)  Set iadExec=iadExec+$Select(iadEc>=0:1,1:0)  Set iadHit=iadHit+$Select(iadEc>0:1,1:0) }}  Write $Piece(iadNm,".",1,$Length(iadNm,".")-1)_"|"_iadNm_"|"_iadHit_"|"_iadExec,$C(10)  Set iadTotalHit=iadTotalHit+iadHit  Set iadTotalExec=iadTotalExec+iadExec }}"#
        ),
        r#" Write "TOTAL|"_iadTotalHit_"|"_iadTotalExec,$C(10)"#.to_string(),
    ]
}

/// Build ObjectScript that runs start→RunTest→stop→collect in one call.
///
/// `routines` are monitor patterns, one per class (`Pkg.Cls.*`); the class is the pattern without
/// its suffix. Output: `COVERAGE_DATA_START`, then the rows of [`collect_lines`], then, if a line
/// threw, `RUN_ERROR|<text>`. On refusal the first line is `ERROR|<code>|<msg>` and the monitor is
/// as it was found (a running one is never stopped) or, for an unknown class, stopped again.
///
/// Every line between `Start` and the final `Stop` is guarded ([`guard_line`]), so the `Stop` is
/// reached whatever a line does, and what was collected before a throw is reported with it. Each
/// statement is a single line (blocks close on their own line), so the code executes in both a
/// compiled ClassMethod (execute_via_generator/HTTP) and an interactive terminal (docker exec).
pub fn build_coverage_run_code(routines: &[String], test_path: &str, namespace: &str) -> String {
    build_coverage_run_code_with(routines, test_path, namespace, false)
}

/// [`build_coverage_run_code`] for a pattern that names a `%UnitTest.TestProduction` subclass
/// (`is_test_production`), which `iris_test` runs through `.Run()` rather than `RunTest`.
pub fn build_coverage_run_code_with(
    routines: &[String],
    test_path: &str,
    namespace: &str,
    is_test_production: bool,
) -> String {
    let classes: Vec<String> = routines.iter().map(|r| strip_routine_suffix(r)).collect();
    let mut lines = vec![format!(
        " New $NAMESPACE  Set $NAMESPACE={}",
        crate::objectscript::os_str_expr(namespace)
    )];
    lines.extend(refuse_if_running_lines());
    lines.extend([
        format!(" Set routines={}", build_routine_list_fragment(routines)),
        format!(" Set sc={MONITOR}.Start(routines,{METRIC_LIST},\"\")"),
        r#" If $System.Status.IsError(sc)  Write "ERROR|MONITOR_IN_USE|"_$System.Status.GetErrorText(sc),$C(10)  Quit"#
            .to_string(),
    ]);
    lines.extend(unknown_class_lines(&classes, true));
    lines.push(" Set tErr=\"\"".to_string());
    // The test run is `iris_test`'s: one builder, so the flags and the `TestProduction` dispatch
    // are the same code (spec 141 R5 C17). The
    // monitor was started with no process list, which covers every process, the JOB's too.
    let test_run = crate::tools::build_test_run_code(
        test_path,
        crate::tools::test_run_flags(),
        "",
        is_test_production,
    );
    lines.extend(
        test_run
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(guard_line),
    );
    // Read the data BEFORE Stop() — Stop() clears the monitor. The marker cannot throw.
    lines.push(r#" Write $C(10),"COVERAGE_DATA_START",$C(10)"#.to_string());
    lines.extend(collect_lines().iter().map(|l| guard_line(l)));
    lines.push(format!(" Do {MONITOR}.Stop()"));
    lines.push(r#" If tErr'=""  Write "RUN_ERROR|"_tErr,$C(10)"#.to_string());
    lines.join("\n")
}

/// Build ObjectScript that expands a package to its concrete class names.
/// Output lines: one class name per line, then "DONE|count"
///
/// Uses single-line form (no curly-brace blocks) for cross-context compatibility.
pub fn build_package_expand_code(package: &str, namespace: &str) -> String {
    let prefix = format!("{}.", package);
    // The prefix is a bound parameter, so nothing the caller sent lands in the statement text.
    // Compiled classes only: a class that failed to compile has no routines to monitor, and one such
    // class in a package must not turn the whole package into `COVERAGE_ROUTINE_UNKNOWN`.
    let sql = "SELECT Name FROM %Dictionary.ClassDefinition WHERE Name %STARTSWITH ? AND Abstract = 0 AND Name IN (SELECT Name FROM %Dictionary.CompiledClass)";
    [
        format!(" New $NAMESPACE  Set $NAMESPACE={}", crate::objectscript::os_str_expr(namespace)),
        " Set count=0".to_string(),
        format!(" Set stmt=##class(%SQL.Statement).%New()  Set sc=stmt.%Prepare({})", crate::objectscript::os_str_expr(sql)),
        r#" If $System.Status.IsError(sc)  Write "ERROR|SQL_ERROR|"_$System.Status.GetErrorText(sc),$C(10)  Quit"#.to_string(),
        format!(" Set rs=stmt.%Execute({})", crate::objectscript::os_str_expr(&prefix)),
        " For  Quit:'rs.%Next()  Write rs.%Get(\"Name\"),$C(10)  Set count=count+1".to_string(),
        r#" Write "DONE|"_count,$C(10)"#.to_string(),
    ]
    .join("\n")
}

// ── Output parsers ────────────────────────────────────────────────────────────

/// Parse check mode output.
/// Input: "OK|ready" or "BBSIZ_NOT_CONFIGURED|<msg>" or empty/error
pub fn parse_check_output(output: &str) -> serde_json::Value {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return err_json("IRIS_EXECUTE_ERROR", "empty response from IRIS");
    }
    // Try JSON pass-through (for tests that feed JSON directly)
    if trimmed.starts_with('{') {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
            return v;
        }
    }
    let first_line = trimmed.lines().next().unwrap_or(trimmed);
    let parts: Vec<&str> = first_line.splitn(2, '|').collect();
    match parts.first().copied() {
        Some("OK") => serde_json::json!({"ok": true, "bbsiz_state": "ready"}),
        Some("BBSIZ_NOT_CONFIGURED") => {
            let msg = parts
                .get(1)
                .copied()
                .unwrap_or("monitor memory not configured");
            serde_json::json!({
                "success": false,
                "error_code": "BBSIZ_NOT_CONFIGURED",
                "message": msg,
                "fix": "Increase gmheap: Management Portal > System Administration > Configuration > Additional Settings > Advanced Memory > gmheap. Set to 256 or higher. Requires IRIS restart."
            })
        }
        Some("ERROR") => {
            // `ERROR|<code>|<message>`: the message says what the refusal is about.
            let mut fields = first_line.splitn(3, '|').skip(1);
            let code = fields.next().unwrap_or("IRIS_EXECUTE_ERROR");
            let mut v = serde_json::json!({"success": false, "error_code": code});
            if let Some(message) = fields.next() {
                v["message"] = serde_json::json!(message);
            }
            v
        }
        _ => err_json(
            "PARSE_ERROR",
            &format!("unexpected check output: {first_line}"),
        ),
    }
}

/// Parse coverage run output (pipe-delimited lines).
/// Input: lines of "CLASS|routine|hit|total" then "TOTAL|hit|total", and "RUN_ERROR|text" after them
/// when a line threw. A class that compiles to several routines has one row for each; they are
/// summed into one entry that lists them.
///
/// A class with 0 of 0 lines has `pct: null`, and so has the total: no lines is not 0% covered, and
/// a target is neither met nor missed (a typo is caught earlier, as `COVERAGE_ROUTINE_UNKNOWN`).
pub fn parse_coverage_output(output: &str) -> serde_json::Value {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return err_json("IRIS_EXECUTE_ERROR", "empty response from IRIS");
    }
    // Try JSON pass-through (for tests that feed JSON directly)
    if trimmed.starts_with('{') {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if v.get("error_code").is_some() || v.get("success").is_some() {
                return v;
            }
        }
        return err_json("PARSE_ERROR", "unexpected JSON in coverage output");
    }

    // The tests were never found, so the monitor's rows below are empty for that reason and not
    // because the class was not loaded.
    if let Some(problem) = crate::tools::unit_test_root_problem(trimmed) {
        return err_json("UNIT_TEST_ROOT_INVALID", &problem);
    }

    // Skip RunTest stdout that precedes the sentinel line
    let data_section = if let Some(pos) = trimmed.find("COVERAGE_DATA_START") {
        let after = &trimmed[pos + "COVERAGE_DATA_START".len()..];
        after.trim_start_matches('\n').trim_start_matches('\r')
    } else {
        trimmed
    };

    // (class, routines, hit, total), in the order the classes first appear.
    let mut classes: Vec<(String, Vec<String>, i64, i64)> = Vec::new();
    let mut total_line: Option<(i64, i64)> = None;
    let mut run_error: Option<String> = None;

    for line in data_section.lines() {
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(4, '|').collect();
        match parts.first().copied() {
            Some("ERROR") => {
                let code = parts.get(1).copied().unwrap_or("IRIS_EXECUTE_ERROR");
                let msg = parts.get(2).copied().unwrap_or("unknown error");
                return err_json(code, msg);
            }
            Some("RUN_ERROR") => {
                run_error = Some(
                    line.split_once('|')
                        .map_or("unknown", |(_, text)| text)
                        .to_string(),
                );
            }
            Some("TOTAL") => {
                total_line = Some((
                    parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0),
                    parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0),
                ));
            }
            _ if parts.len() >= 4 => {
                let hit: i64 = parts[2].parse().unwrap_or(0);
                let total: i64 = parts[3].parse().unwrap_or(0);
                match classes.iter_mut().find(|c| c.0 == parts[0]) {
                    Some(entry) => {
                        entry.1.push(parts[1].to_string());
                        entry.2 += hit;
                        entry.3 += total;
                    }
                    None => {
                        classes.push((parts[0].to_string(), vec![parts[1].to_string()], hit, total))
                    }
                }
            }
            _ => {
                // Unexpected line — skip silently; IRIS might emit warnings
            }
        }
    }

    if total_line.is_none() && classes.is_empty() {
        return err_json(
            "PARSE_ERROR",
            &format!("no coverage data in output: {trimmed}"),
        );
    }

    let (total_hit, total_exec) = if classes.is_empty() {
        total_line.unwrap_or((0, 0))
    } else {
        (
            classes.iter().map(|c| c.2).sum(),
            classes.iter().map(|c| c.3).sum(),
        )
    };
    let class_values: Vec<serde_json::Value> = classes
        .iter()
        .map(|(class, routines, hit, total)| {
            let mut v = serde_json::json!({
                "class": class,
                "routine": routines[0],
                "routines": routines,
                "hit": hit,
                "total": total,
                "pct": pct_or_null(*hit, *total),
            });
            if *total == 0 {
                v["note"] = serde_json::json!(
                    "No lines were counted for this class: it was not loaded during the run, or it compiles to no executable lines. This is not 0% coverage."
                );
            }
            v
        })
        .collect();

    let mut result = serde_json::json!({
        "success": true,
        "total_pct": pct_or_null(total_hit, total_exec),
        "hits": total_hit,
        "total": total_exec,
        "classes": class_values
    });
    if let Some(text) = run_error {
        result["partial"] = serde_json::json!(true);
        result["run_error"] = serde_json::json!(text);
    }
    result
}

/// `hit` of `total` as a percentage with one decimal, or `null` for no lines at all.
fn pct_or_null(hit: i64, total: i64) -> serde_json::Value {
    if total > 0 {
        serde_json::json!(((hit as f64 / total as f64) * 1000.0).round() / 10.0)
    } else {
        serde_json::Value::Null
    }
}

/// `result` (from [`parse_coverage_output`]) restricted to `classes`: the report of a monitor that
/// someone started covers whatever they asked it to, and `mode=report` is asked about some. A class
/// with no routine in the monitor is `COVERAGE_ROUTINE_UNKNOWN`.
pub fn restrict_to_classes(result: serde_json::Value, classes: &[String]) -> serde_json::Value {
    let Some(entries) = result["classes"].as_array() else {
        return result;
    };
    if let Some(missing) = classes
        .iter()
        .find(|c| !entries.iter().any(|e| e["class"] == c.as_str()))
    {
        return err_json(
            "COVERAGE_ROUTINE_UNKNOWN",
            &format!(
                "{missing} has no routine in the line monitor, so there is nothing to report. Check the class name, and that it was in the mode=start that began this monitor."
            ),
        );
    }
    let kept: Vec<serde_json::Value> = entries
        .iter()
        .filter(|e| classes.iter().any(|c| e["class"] == c.as_str()))
        .cloned()
        .collect();
    let hits: i64 = kept.iter().filter_map(|e| e["hit"].as_i64()).sum();
    let total: i64 = kept.iter().filter_map(|e| e["total"].as_i64()).sum();
    let mut out = result;
    out["classes"] = serde_json::Value::Array(kept);
    out["hits"] = serde_json::json!(hits);
    out["total"] = serde_json::json!(total);
    out["total_pct"] = pct_or_null(hits, total);
    out
}

/// Parse package expand output: one class name per line then "DONE|count"
pub fn parse_package_expand_output(output: &str) -> Result<Vec<String>, serde_json::Value> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return Err(err_json("IRIS_EXECUTE_ERROR", "empty response from IRIS"));
    }
    let mut classes = Vec::new();
    for line in trimmed.lines() {
        if line.is_empty() {
            continue;
        }
        if line.starts_with("ERROR|") {
            let parts: Vec<&str> = line.splitn(3, '|').collect();
            let msg = parts.get(2).copied().unwrap_or("unknown SQL error");
            return Err(err_json("SQL_ERROR", msg));
        }
        if line.starts_with("DONE|") {
            break;
        }
        classes.push(line.to_string());
    }
    Ok(classes)
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// Execute ObjectScript code for coverage operations, as `cx`'s identity.
///
/// HTTP first; a failure before the code ran goes to the target's docker leg when it has one
/// (some IRIS community/Lab builds lack the Atelier PUT API). A service account that the Atelier
/// API refuses is reported as such and never run in the container (spec 141, R2).
async fn execute_coverage_code(
    cx: &ExecContext,
    code: &str,
    ns: &str,
) -> Result<String, serde_json::Value> {
    let opts = RunOpts::with_terminal(std::time::Duration::from_secs(300), "IrisDevRun");
    match cx.run(code, ns, &opts).await {
        Ok(out) => match coverage_privilege_denied(&out.output) {
            Some(line) => Err(serde_json::json!({
                "success": false,
                "error_code": "COVERAGE_PRIVILEGE_DENIED",
                "error": format!(
                    "{} cannot run the line-by-line monitor: {line}. The monitor's state is \
                     system-wide and lives in the IRISSYS database, so the account needs write \
                     access to it (%DB_IRISSYS:RW). Nothing ran.",
                    out.identity.user
                ),
                "identity": out.identity.to_json(),
            })),
            None => Ok(out.output),
        },
        Err(e) => Err(e.to_json()),
    }
}

/// The line of `output` that says IRIS refused a monitor call with `<PROTECT>`.
fn coverage_privilege_denied(output: &str) -> Option<&str> {
    output
        .lines()
        .find(|l| l.contains("<PROTECT>") && l.contains("LineByLine"))
        .map(str::trim)
}

/// Build start mode ObjectScript code (single-line form).
///
/// A running monitor is refused and left alone. A class with no routine in the monitor is an error
/// and the monitor this program just started is stopped again.
fn build_coverage_start_code(routines: &[String], namespace: &str) -> String {
    let classes: Vec<String> = routines.iter().map(|r| strip_routine_suffix(r)).collect();
    let mut lines = vec![format!(
        " New $NAMESPACE  Set $NAMESPACE={}",
        crate::objectscript::os_str_expr(namespace)
    )];
    lines.extend(refuse_if_running_lines());
    lines.extend([
        format!(" Set routines={}", build_routine_list_fragment(routines)),
        format!(" Set sc={MONITOR}.Start(routines,{METRIC_LIST},\"\")"),
        r#" If $System.Status.IsError(sc)  Write "ERROR|MONITOR_IN_USE|"_$System.Status.GetErrorText(sc),$C(10)  Quit"#
            .to_string(),
    ]);
    lines.extend(unknown_class_lines(&classes, true));
    lines.push(r#" Write "OK|started",$C(10)"#.to_string());
    lines.join("\n")
}

/// Build stop mode ObjectScript code (single-line form, no curly braces).
pub fn build_coverage_stop_code(namespace: &str) -> String {
    [
        format!(
            " New $NAMESPACE  Set $NAMESPACE={}",
            crate::objectscript::os_str_expr(namespace)
        ),
        " Do ##class(%Monitor.System.LineByLine).Stop()".to_string(),
        r#" Write "OK|stopped",$C(10)"#.to_string(),
    ]
    .join("\n")
}

/// Decide the outcome of stop mode from its output.
///
/// The old code was `Ok(_) => {"success": true, "stopped": true}` — the output was thrown away.
/// `execute_via_generator` reports IRIS-side failure inside that output, so `mode=stop` answered
/// `stopped: true` for a monitor that was still running and still holding the single process-wide
/// LineByLine slot; the next `mode=start` then failed with MONITOR_IN_USE and nothing pointed back
/// at the stop that never happened. `OK|stopped` is only written after `Stop()` returns, so its
/// presence is the proof.
pub fn parse_stop_output(output: &str) -> serde_json::Value {
    if let Some(msg) = crate::iris::connection::generator_error_message(output) {
        return err_json("IRIS_EXECUTE_ERROR", msg.trim());
    }
    if !output.lines().any(|l| l.trim() == "OK|stopped") {
        return err_json(
            "IRIS_EXECUTE_ERROR",
            &format!(
                "%Monitor.System.LineByLine.Stop() did not confirm; the monitor may still be running. Raw output: {}",
                output.trim()
            ),
        );
    }
    serde_json::json!({"success": true, "stopped": true})
}

/// Build report mode ObjectScript code (single-line form): the rows of every routine the running
/// monitor holds, which [`restrict_to_classes`] cuts down to the classes asked for. With no monitor
/// running there is nothing to report.
fn build_coverage_report_code(namespace: &str) -> String {
    let mut lines = vec![
        format!(
            " New $NAMESPACE  Set $NAMESPACE={}",
            crate::objectscript::os_str_expr(namespace)
        ),
        format!(
            r#" If {MONITOR}.GetRoutineCount()=0  Write "ERROR|MONITOR_NOT_RUNNING|no line monitor is running: start one with mode=start, or use mode=run.",$C(10)  Quit"#
        ),
    ];
    lines.extend(collect_lines());
    lines.join("\n")
}

/// Build ObjectScript that checks if TestCoverage.Manager exists in the namespace.
/// Output: "YES" or "NO"
fn build_testcoverage_check_code(namespace: &str) -> String {
    [
        format!(" New $NAMESPACE  Set $NAMESPACE={}", crate::objectscript::os_str_expr(namespace)),
        r#" If ##class(%Dictionary.ClassDefinition).%ExistsId("TestCoverage.Manager")  Write "YES",$C(10)  Quit"#.to_string(),
        r#" Write "NO",$C(10)"#.to_string(),
    ]
    .join("\n")
}

/// Returns true if the TestCoverage IPM package is installed in the given namespace.
pub async fn testcoverage_available(cx: &ExecContext, ns: &str) -> bool {
    let code = build_testcoverage_check_code(ns);
    match execute_coverage_code(cx, &code, ns).await {
        Ok(output) => output.trim().starts_with("YES"),
        Err(_) => false,
    }
}

/// Handle an `iris_coverage` tool call.
/// Returns the JSON response value (caller wraps in CallToolResult).
pub async fn handle_iris_coverage(
    cx: &ExecContext,
    params: &IrisCoverageParams,
) -> serde_json::Value {
    let ns = params
        .namespace
        .clone()
        .unwrap_or_else(|| cx.conn.namespace.clone());

    // Every name below is spliced into generated ObjectScript, and the docker path runs it line by
    // line as terminal input, so each one is checked before any code is built (137).
    if let Err(e) = crate::objectscript::validate_namespace(&ns) {
        return e;
    }
    for name in params.classes.iter().flatten().chain(params.package.iter()) {
        if let Err(e) = crate::objectscript::validate_class_name(name) {
            return e;
        }
    }

    match params.mode.as_str() {
        "check" => {
            let code = build_coverage_check_code(&ns);
            let tc_avail = testcoverage_available(cx, &ns).await;
            match execute_coverage_code(cx, &code, &ns).await {
                Err(e) => e,
                Ok(output) => {
                    let mut result = parse_check_output(&output);
                    if let Some(obj) = result.as_object_mut() {
                        obj.insert(
                            "testcoverage_available".to_string(),
                            serde_json::Value::Bool(tc_avail),
                        );
                        if !tc_avail {
                            obj.insert(
                                "testcoverage_hint".to_string(),
                                serde_json::json!(
                                    "Install TestCoverage: zpm \"install testcoverage\""
                                ),
                            );
                        }
                    }
                    result
                }
            }
        }

        "run" => {
            let test_path =
                match &params.test_path {
                    Some(p) => p.clone(),
                    None => return err_json(
                        "MISSING_PARAM",
                        "mode=run requires test_path (compiled class pattern, e.g. 'MyApp.Tests')",
                    ),
                };

            if let Some(msg) = crate::tools::test_pattern_problem(&test_path) {
                return err_json("INVALID_PARAMS", &format!("test_path: {msg}"));
            }

            // Resolve class list (explicit or via package expansion)
            let classes = match resolve_classes(cx, params, &ns).await {
                Ok(c) => c,
                Err(e) => return e,
            };

            if classes.is_empty() {
                return err_json(
                    "NO_CLASSES",
                    "no concrete classes found — provide classes or a non-empty package",
                );
            }

            let routines: Vec<String> = classes.iter().map(|c| build_routine_name(c)).collect();
            let is_test_production =
                crate::tools::detect_test_production(&cx.conn, &cx.client, &ns, &test_path).await;
            let code = build_coverage_run_code_with(&routines, &test_path, &ns, is_test_production);

            let tc_avail = testcoverage_available(cx, &ns).await;

            match execute_coverage_code(cx, &code, &ns).await {
                Err(e) => e,
                Ok(output) => {
                    let mut result = parse_coverage_output(&output);
                    if let Some(obj) = result.as_object_mut() {
                        // meets_target: null when there is no figure to judge (0 of 0 lines) or the
                        // run threw and the figure is partial.
                        if let (Some(target), true) =
                            (params.target_pct, obj.contains_key("total_pct"))
                        {
                            let partial = obj.get("partial").is_some();
                            let verdict = match obj.get("total_pct").and_then(|v| v.as_f64()) {
                                Some(pct) if !partial => serde_json::Value::Bool(pct >= target),
                                _ => serde_json::Value::Null,
                            };
                            obj.insert("meets_target".to_string(), verdict);
                            obj.insert("target_pct".to_string(), serde_json::json!(target));
                        }
                        // testcoverage_available field
                        obj.insert(
                            "testcoverage_available".to_string(),
                            serde_json::Value::Bool(tc_avail),
                        );
                        // cobertura_skipped when requested but unavailable
                        if params.cobertura_path.is_some() && !tc_avail {
                            obj.insert(
                                "cobertura_skipped".to_string(),
                                serde_json::json!("TestCoverage IPM package not installed; install with: zpm \"install testcoverage\""),
                            );
                        }
                    }
                    result
                }
            }
        }

        "start" => {
            let classes = match resolve_classes(cx, params, &ns).await {
                Ok(c) => c,
                Err(e) => return e,
            };
            if classes.is_empty() {
                return err_json("NO_CLASSES", "no concrete classes found");
            }
            let routines: Vec<String> = classes.iter().map(|c| build_routine_name(c)).collect();
            let code = build_coverage_start_code(&routines, &ns);

            match execute_coverage_code(cx, &code, &ns).await {
                Err(e) => e,
                Ok(output) => {
                    let first = output.trim().lines().next().unwrap_or("").to_string();
                    let parts: Vec<&str> = first.splitn(3, '|').collect();
                    match parts.first().copied() {
                        Some("OK") => serde_json::json!({
                            "success": true,
                            "started": true,
                            "routines": routines
                        }),
                        Some("ERROR") => err_json(
                            parts.get(1).copied().unwrap_or("MONITOR_IN_USE"),
                            parts.get(2).copied().unwrap_or("unknown"),
                        ),
                        _ => err_json("PARSE_ERROR", &format!("unexpected start output: {first}")),
                    }
                }
            }
        }

        "stop" => {
            let code = build_coverage_stop_code(&ns);
            match execute_coverage_code(cx, &code, &ns).await {
                Err(e) => e,
                Ok(output) => parse_stop_output(&output),
            }
        }

        "report" => {
            let classes = match resolve_classes(cx, params, &ns).await {
                Ok(c) => c,
                Err(e) => return e,
            };
            if classes.is_empty() {
                return err_json("NO_CLASSES", "no concrete classes found");
            }
            let code = build_coverage_report_code(&ns);

            match execute_coverage_code(cx, &code, &ns).await {
                Err(e) => e,
                Ok(output) => restrict_to_classes(parse_coverage_output(&output), &classes),
            }
        }

        other => err_json(
            "INVALID_ACTION",
            &format!("unknown mode: {other} (expected: start, stop, report, run, check)"),
        ),
    }
}

/// Resolve the class list from either `classes` or `package` expansion.
async fn resolve_classes(
    cx: &ExecContext,
    params: &IrisCoverageParams,
    ns: &str,
) -> Result<Vec<String>, serde_json::Value> {
    if let Some(ref explicit) = params.classes {
        return Ok(explicit.clone());
    }
    if let Some(ref pkg) = params.package {
        let code = build_package_expand_code(pkg, ns);
        let output = execute_coverage_code(cx, &code, ns).await?;
        return parse_package_expand_output(&output);
    }
    Err(err_json(
        "MISSING_PARAM",
        "provide either 'classes' (list of class names) or 'package' (auto-discover)",
    ))
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// A namespace, class or package with a quote and a newline must not start a new source line or
    /// close the literal it sits in: every builder keeps the line count of its benign output.
    #[test]
    fn hostile_names_leave_the_generated_line_structure_alone() {
        let hostile = "USER\"\n Kill ^IadSentinel141\n Set x=\"";
        let benign = "USER";
        let classes_h = vec![hostile.to_string()];
        let classes_b = vec![benign.to_string()];
        let pairs = [
            (
                build_coverage_check_code(hostile),
                build_coverage_check_code(benign),
            ),
            (
                build_coverage_run_code(&classes_h, "Pkg.Test", hostile),
                build_coverage_run_code(&classes_b, "Pkg.Test", benign),
            ),
            (
                build_package_expand_code(hostile, hostile),
                build_package_expand_code(benign, benign),
            ),
            (
                build_coverage_start_code(&classes_h, hostile),
                build_coverage_start_code(&classes_b, benign),
            ),
            (
                build_coverage_stop_code(hostile),
                build_coverage_stop_code(benign),
            ),
            (
                build_coverage_report_code(hostile),
                build_coverage_report_code(benign),
            ),
            (
                build_testcoverage_check_code(hostile),
                build_testcoverage_check_code(benign),
            ),
        ];
        for (i, (h, b)) in pairs.iter().enumerate() {
            assert_eq!(h.lines().count(), b.lines().count(), "builder {i}:\n{h}");
            assert!(
                h.lines().all(|l| !l.trim_start().starts_with("Kill ^Iad")),
                "builder {i} gave the newline its own line:\n{h}"
            );
            assert!(h.contains("$CHAR(10)"), "builder {i}:\n{h}");
        }
    }

    #[test]
    fn the_package_prefix_is_a_bound_parameter() {
        let code = build_package_expand_code("A' OR Name <> '", "USER");
        assert!(code.contains("%STARTSWITH ? AND"), "{code}");
        assert!(!code.contains("OR Name <>\" "), "{code}");
        assert!(code.contains("%Execute(\"A' OR Name <> '.\")"), "{code}");
    }

    #[test]
    fn a_protect_error_from_the_monitor_is_found_in_the_output() {
        let out = "ERROR: <PROTECT> 32 Stop+2^%Monitor.System.LineByLine.1\n";
        assert_eq!(
            coverage_privilege_denied(out),
            Some("ERROR: <PROTECT> 32 Stop+2^%Monitor.System.LineByLine.1")
        );
        assert_eq!(coverage_privilege_denied("OK|stopped\n"), None);
        // A <PROTECT> from the code under test is not the monitor's.
        assert_eq!(
            coverage_privilege_denied("<PROTECT> ^Mine,/db/\nCOVERAGE_DATA_START\n"),
            None
        );
    }

    // build_coverage_report_code: reads whatever the running monitor holds
    #[test]
    fn build_coverage_report_code_reads_every_monitored_routine() {
        let code = build_coverage_report_code("USER");
        assert!(code.contains("$NAMESPACE"), "must set namespace");
        assert!(code.contains("TOTAL|"), "must emit TOTAL line");
        assert!(
            code.contains("GetRoutineName"),
            "routines come from the monitor"
        );
        assert!(
            code.contains("MONITOR_NOT_RUNNING"),
            "no monitor, nothing to report"
        );
        assert!(
            !code.contains(".Stop()"),
            "a report leaves the monitor running"
        );
    }

    // build_testcoverage_check_code: contains TestCoverage.Manager check
    #[test]
    fn build_testcoverage_check_code_content() {
        let code = build_testcoverage_check_code("USER");
        assert!(
            code.contains("TestCoverage.Manager"),
            "must reference class"
        );
        assert!(code.contains("YES"), "must have YES branch");
        assert!(code.contains("NO"), "must have NO branch");
        assert!(code.contains("USER"), "must include namespace");
    }

    // build_coverage_start_code: starts only when none is running
    #[test]
    fn build_coverage_start_code_content() {
        let code = build_coverage_start_code(&["Demo.A.*".to_string()], "USER");
        assert!(
            code.contains("LineByLine"),
            "must reference LineByLine monitor"
        );
        assert!(code.contains("Start("), "must start monitor");
        assert!(
            code.contains("OK|started"),
            "must emit OK|started on success"
        );
        assert!(
            code.contains("MONITOR_IN_USE"),
            "must emit error on failure"
        );
        // The only Stop is the one that ends a monitor this program started on an unknown class;
        // nothing stops a monitor first (it may be another session's).
        let first_start = code.find("Start(").unwrap();
        assert!(
            !code[..first_start].contains(".Stop()"),
            "a running monitor must not be stopped to make room: {code}"
        );
        assert!(code.contains("COVERAGE_ROUTINE_UNKNOWN"), "{code}");
    }

    // Every mode that would start a monitor refuses one that is running, before it starts.
    #[test]
    fn modes_that_start_a_monitor_refuse_a_running_one_without_stopping_it() {
        let classes = vec!["Demo.A.*".to_string()];
        for (mode, code) in [
            ("check", build_coverage_check_code("USER")),
            ("start", build_coverage_start_code(&classes, "USER")),
            (
                "run",
                build_coverage_run_code(&classes, "Demo.Tests", "USER"),
            ),
        ] {
            let refuse = code
                .find("MONITOR_IN_USE")
                .unwrap_or_else(|| panic!("{mode}: no refusal in:\n{code}"));
            let start = code.find(".Start(").unwrap();
            assert!(
                refuse < start,
                "{mode}: the refusal comes before Start:\n{code}"
            );
            assert!(
                !code[..start].contains(".Stop()"),
                "{mode}: nothing stops a monitor before it is started:\n{code}"
            );
        }
    }

    // Everything between Start and the last Stop is guarded, so the Stop is reached.
    #[test]
    fn the_run_code_guards_every_line_between_start_and_stop() {
        let code = build_coverage_run_code(&["Demo.A.*".to_string()], "Demo/Tests", "USER");
        let lines: Vec<&str> = code.lines().collect();
        let start = lines.iter().position(|l| l.contains(".Start(")).unwrap();
        let stop = lines.iter().rposition(|l| l.contains(".Stop()")).unwrap();
        assert_eq!(
            lines.iter().filter(|l| l.contains(".Stop()")).count(),
            2,
            "one Stop on the unknown-class exit, one at the end: {code}"
        );
        let after_unknown = lines
            .iter()
            .position(|l| l.trim() == "Set tErr=\"\"")
            .expect("tErr is initialised");
        assert!(start < after_unknown && after_unknown < stop);
        for line in &lines[after_unknown + 1..stop] {
            if line.contains("COVERAGE_DATA_START") {
                continue; // a Write that cannot throw
            }
            assert!(
                line.starts_with(" Try {") && line.ends_with("}"),
                "an unguarded line between Start and Stop: {line}"
            );
        }
        assert!(
            lines[stop + 1].contains("RUN_ERROR"),
            "what threw is reported after the Stop: {code}"
        );
    }

    // ── parse_check_output ──────────────────────────────────────────────────

    #[test]
    fn parse_check_output_empty_returns_error() {
        let v = parse_check_output("");
        assert_eq!(v["error_code"], "IRIS_EXECUTE_ERROR");
    }

    #[test]
    fn parse_check_output_ok() {
        let v = parse_check_output("OK|ready");
        assert_eq!(v["ok"], true);
    }

    #[test]
    fn parse_check_output_bbsiz_not_configured() {
        let v = parse_check_output("BBSIZ_NOT_CONFIGURED|increase gmheap");
        assert_eq!(v["error_code"], "BBSIZ_NOT_CONFIGURED");
        assert!(v["fix"].as_str().unwrap_or("").contains("gmheap"));
    }

    #[test]
    fn parse_check_output_error_line() {
        let v = parse_check_output("ERROR|SQL_ERROR");
        assert_eq!(v["error_code"], "SQL_ERROR");
    }

    #[test]
    fn parse_check_output_unknown_returns_parse_error() {
        let v = parse_check_output("XYZZY|something");
        assert_eq!(v["error_code"], "PARSE_ERROR");
    }

    #[test]
    fn parse_check_output_json_passthrough() {
        let v = parse_check_output(r#"{"ok":true,"bbsiz_state":"ready"}"#);
        assert_eq!(v["ok"], true);
    }

    // ── parse_coverage_output ───────────────────────────────────────────────

    #[test]
    fn parse_coverage_output_empty_returns_error() {
        let v = parse_coverage_output("");
        assert_eq!(v["error_code"], "IRIS_EXECUTE_ERROR");
    }

    #[test]
    fn parse_coverage_output_error_line() {
        let v = parse_coverage_output("ERROR|MONITOR_NOT_RUNNING|monitor was not started");
        assert_eq!(v["error_code"], "MONITOR_NOT_RUNNING");
    }

    #[test]
    fn parse_coverage_output_total_zero_exec() {
        let v = parse_coverage_output("TOTAL|0|0");
        assert_eq!(v["success"], true);
        assert!(v["total_pct"].is_null(), "0 of 0 lines is not 0%: {v}");
    }

    #[test]
    fn parse_coverage_output_with_class_lines() {
        let output = "Demo.MyClass|Demo.MyClass.1|8|10\nTOTAL|8|10";
        let v = parse_coverage_output(output);
        assert_eq!(v["success"], true);
        assert_eq!(v["hits"], 8);
        assert_eq!(v["total"], 10);
        let classes = v["classes"].as_array().unwrap();
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0]["class"], "Demo.MyClass");
    }

    #[test]
    fn parse_coverage_output_coverage_data_start_sentinel() {
        let output = "RunTest stdout noise\nCOVERAGE_DATA_START\nDemo.A|Demo.A.1|5|10\nTOTAL|5|10";
        let v = parse_coverage_output(output);
        assert_eq!(v["success"], true);
        assert_eq!(v["hits"], 5);
    }

    #[test]
    fn parse_coverage_output_json_with_error_code_passthrough() {
        let json = r#"{"error_code":"MONITOR_NOT_RUNNING","success":false}"#;
        let v = parse_coverage_output(json);
        assert_eq!(v["error_code"], "MONITOR_NOT_RUNNING");
    }

    #[test]
    fn parse_coverage_output_json_without_error_code_parse_error() {
        // JSON that doesn't have error_code or success → PARSE_ERROR
        let v = parse_coverage_output(r#"{"foo":"bar"}"#);
        assert_eq!(v["error_code"], "PARSE_ERROR");
    }

    #[test]
    fn parse_coverage_output_no_data_no_total() {
        // Output with no valid lines → PARSE_ERROR
        let v = parse_coverage_output("some|garbage");
        assert_eq!(v["error_code"], "PARSE_ERROR");
    }

    // ── parse_package_expand_output ─────────────────────────────────────────

    #[test]
    fn parse_package_expand_output_empty_returns_error() {
        assert!(parse_package_expand_output("").is_err());
    }

    #[test]
    fn parse_package_expand_output_error_line() {
        let result = parse_package_expand_output("ERROR|SQL_ERROR|table not found");
        let err = result.unwrap_err();
        assert_eq!(err["error_code"], "SQL_ERROR");
    }

    #[test]
    fn parse_package_expand_output_done_stops_collection() {
        let output = "Demo.A\nDemo.B\nDONE|2\nDemo.C";
        let classes = parse_package_expand_output(output).unwrap();
        assert_eq!(classes, vec!["Demo.A", "Demo.B"]);
    }

    #[test]
    fn parse_package_expand_output_normal() {
        let output = "Demo.A\nDemo.B\nDemo.C\nDONE|3";
        let classes = parse_package_expand_output(output).unwrap();
        assert_eq!(classes.len(), 3);
    }

    // ── build_routine_name / strip_routine_suffix ───────────────────────────

    #[test]
    fn build_routine_name_dotted_class() {
        assert_eq!(build_routine_name("Demo.MyClass"), "Demo.MyClass.*");
    }

    #[test]
    fn strip_routine_suffix_removes_dot_one() {
        assert_eq!(strip_routine_suffix("Demo.MyClass.1"), "Demo.MyClass");
    }

    #[test]
    fn strip_routine_suffix_no_suffix() {
        assert_eq!(strip_routine_suffix("Demo.MyClass"), "Demo.MyClass");
    }

    // ── build_coverage_check_code ───────────────────────────────────────────

    #[test]
    fn build_coverage_check_code_contains_namespace() {
        let code = build_coverage_check_code("USER");
        assert!(code.contains("USER"));
        assert!(code.contains("$NAMESPACE"));
    }
}
