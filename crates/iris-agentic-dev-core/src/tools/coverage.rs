//! iris_coverage — ObjectScript line coverage via %Monitor.System.LineByLine.

use schemars::JsonSchema;
use serde::Deserialize;

fn err_json(code: &str, msg: &str) -> serde_json::Value {
    serde_json::json!({"success": false, "error_code": code, "message": msg})
}

// ── Params ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct IrisCoverageParams {
    /// Mode: "start" | "stop" | "report" | "run" | "check"
    pub mode: String,
    /// Explicit list of class names (without .1) — mutually exclusive with package
    pub classes: Option<Vec<String>>,
    /// Auto-discover all concrete classes in package via %Dictionary.ClassDefinition
    pub package: Option<String>,
    /// Compiled class pattern for mode=run (e.g. "MyApp.Tests"); /noload always used
    pub test_path: Option<String>,
    /// Coverage target percentage (default 90.0)
    pub target_pct: Option<f64>,
    /// IRIS namespace (defaults to connection default)
    pub namespace: Option<String>,
    /// Write Cobertura XML to this path (requires TestCoverage IPM package)
    pub cobertura_path: Option<String>,
}

// ── Routine name helpers ──────────────────────────────────────────────────────

/// `"MyApp.MyClass"` → `"MyApp.MyClass.1"` (INT routine name for %Monitor.System.LineByLine)
pub fn build_routine_name(class: &str) -> String {
    format!("{}.1", class)
}

/// `"MyApp.MyClass.1"` → `"MyApp.MyClass"` (strip the INT suffix for display)
pub fn strip_routine_suffix(routine: &str) -> String {
    routine.strip_suffix(".1").unwrap_or(routine).to_string()
}

// ── ObjectScript code builders ────────────────────────────────────────────────

// execute_via_generator cannot return content containing `{` characters.
// All output uses pipe-delimited format; JSON is assembled in Rust.
//
// Coverage output protocol:
//   CHECK mode:  "OK|bbsiz_state" or "BBSIZ_NOT_CONFIGURED|<message>"
//   RUN mode:    Lines of "CLASS|routine|hit|total" then "TOTAL|hit|total|truncated"
//   ERROR:       "ERROR|<code>|<message>"

/// Build ObjectScript that pre-flight checks the monitor by attempting a dry Start.
/// Output: "OK|ready" or "BBSIZ_NOT_CONFIGURED|<msg>"
///
/// Uses single-line commands (no curly-brace blocks) so the code works via both
/// execute_via_generator (HTTP/Atelier) and execute (docker exec terminal).
pub fn build_coverage_check_code(namespace: &str) -> String {
    [
        format!(" New $NAMESPACE  Set $NAMESPACE=\"{}\"", namespace),
        " Do ##class(%Monitor.System.LineByLine).Stop()".to_string(),
        " Set sc=##class(%Monitor.System.LineByLine).Start($lb(\"%Library.RegisteredObject.1\"),\"\",\"\")".to_string(),
        r#" If $System.Status.IsError(sc)  Write "BBSIZ_NOT_CONFIGURED|Start() failed: "_$System.Status.GetErrorText(sc)_" — increase gmheap (Management Portal > System Administration > Configuration > Additional Settings > Advanced Memory > gmheap, set to 256+, restart IRIS)",$C(10)  Quit"#.to_string(),
        " Do ##class(%Monitor.System.LineByLine).Stop()".to_string(),
        r#" Write "OK|ready",$C(10)"#.to_string(),
    ]
    .join("\n")
}

/// Build the routine list fragment for ObjectScript ($lb("R1","R2",...))
fn build_routine_list_fragment(routines: &[String]) -> String {
    let quoted: Vec<String> = routines.iter().map(|r| format!("\"{}\"", r)).collect();
    format!("$lb({})", quoted.join(","))
}

/// Build ObjectScript that runs start→RunTest→stop→collect in one call.
/// Output lines: "CLASS|routine|hit|total" then "TOTAL|hit|total"
/// On error: first line is "ERROR|<code>|<msg>"
///
/// All ObjectScript uses single-line form (no curly-brace blocks, no dot-continuation),
/// so the generated code executes correctly in both compiled ClassMethod context
/// (execute_via_generator/HTTP) and interactive terminal context (docker exec).
pub fn build_coverage_run_code(routines: &[String], test_path: &str, namespace: &str) -> String {
    let routine_list = build_routine_list_fragment(routines);

    // Build per-routine collection lines: one flat block per routine (unrolled).
    // Avoids nested loops — the For/While inner loop uses postfix Quit and $Select,
    // which is valid in both terminal mode (docker exec) and compiled ClassMethod context.
    let mut lines = vec![
        format!(" New $NAMESPACE  Set $NAMESPACE=\"{}\"", namespace),
        " Do ##class(%Monitor.System.LineByLine).Stop()".to_string(),
        format!(" Set routines={}", routine_list),
        " Set sc=##class(%Monitor.System.LineByLine).Start(routines,\"\",\"\")".to_string(),
        format!(
            r#" If $System.Status.IsError(sc)  Write "ERROR|MONITOR_IN_USE|"_$System.Status.GetErrorText(sc),$C(10)  Quit"#
        ),
        format!(
            " Do ##class(%UnitTest.Manager).RunTest(\"{}\",\"/noload/nodelete\")",
            test_path
        ),
        " Do ##class(%Monitor.System.LineByLine).Stop()".to_string(),
        r#" Write $C(10),"COVERAGE_DATA_START",$C(10)"#.to_string(),
        " Set totalHit=0  Set totalExec=0".to_string(),
    ];

    // Emit one block per routine (unrolled — no outer For loop needed)
    for (idx, rtn) in routines.iter().enumerate() {
        let rtn_var = format!("rtn{}", idx);
        lines.push(format!(" Set {}=\"{}\"", rtn_var, rtn));
        lines.push(format!(
            " Set rset{}=##class(%ResultSet).%New(\"%Monitor.System.LineByLine:Result\")",
            idx
        ));
        lines.push(format!(" Do rset{}.Execute({})", idx, rtn_var));
        lines.push(format!(" Set hit{}=0  Set execTotal{}=0", idx, idx));
        // Inner ResultSet loop: skip non-executable lines (execCount<0) by only
        // counting when execCount>=0. Uses $Select to avoid Continue/If blocks.
        // $Select(cond:val,1:0) is a ternary that works in all execution contexts.
        lines.push(format!(
            " For  Quit:'rset{}.Next()  Set data{}=rset{}.GetData(1)  Set ec{}=$ListGet(data{},2)  Set execTotal{}=execTotal{}+$Select(ec{}>=0:1,1:0)  Set hit{}=hit{}+$Select(ec{}>0:1,1:0)",
            idx, idx, idx, idx, idx, idx, idx, idx, idx, idx, idx
        ));
        let class_name = rtn.strip_suffix(".1").unwrap_or(rtn);
        lines.push(format!(
            r#" Write "{}|{}|"_hit{}_"|"_execTotal{},$C(10)"#,
            class_name, rtn, idx, idx
        ));
        lines.push(format!(
            " Set totalHit=totalHit+hit{}  Set totalExec=totalExec+execTotal{}",
            idx, idx
        ));
    }

    lines.push(r#" Write "TOTAL|"_totalHit_"|"_totalExec,$C(10)"#.to_string());
    lines.join("\n")
}

/// Build ObjectScript that expands a package to its concrete class names.
/// Output lines: one class name per line, then "DONE|count"
///
/// Uses single-line form (no curly-brace blocks) for cross-context compatibility.
pub fn build_package_expand_code(package: &str, namespace: &str) -> String {
    let prefix = format!("{}.", package);
    let sql = format!(
        "SELECT Name FROM %Dictionary.ClassDefinition WHERE Name %STARTSWITH '{}' AND Abstract = 0",
        prefix.replace('\'', "''")
    );
    [
        format!(" New $NAMESPACE  Set $NAMESPACE=\"{}\"", namespace),
        " Set count=0".to_string(),
        format!(" Set stmt=##class(%SQL.Statement).%New()  Set sc=stmt.%Prepare(\"{}\")", sql),
        r#" If $System.Status.IsError(sc)  Write "ERROR|SQL_ERROR|"_$System.Status.GetErrorText(sc),$C(10)  Quit"#.to_string(),
        " Set rs=stmt.%Execute()".to_string(),
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
            let code = parts.get(1).copied().unwrap_or("IRIS_EXECUTE_ERROR");
            serde_json::json!({
                "success": false,
                "error_code": code
            })
        }
        _ => err_json(
            "PARSE_ERROR",
            &format!("unexpected check output: {first_line}"),
        ),
    }
}

/// Parse coverage run output (pipe-delimited lines).
/// Input: lines of "CLASS|routine|hit|total" then "TOTAL|hit|total"
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

    // Skip RunTest stdout that precedes the sentinel line
    let data_section = if let Some(pos) = trimmed.find("COVERAGE_DATA_START") {
        let after = &trimmed[pos + "COVERAGE_DATA_START".len()..];
        after.trim_start_matches('\n').trim_start_matches('\r')
    } else {
        trimmed
    };

    let mut classes: Vec<serde_json::Value> = Vec::new();
    let mut total_hit: i64 = 0;
    let mut total_exec: i64 = 0;
    let mut found_total = false;

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
            Some("TOTAL") => {
                total_hit = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
                total_exec = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
                found_total = true;
            }
            _ if parts.len() >= 4 => {
                let class = parts[0];
                let routine = parts[1];
                let hit: i64 = parts[2].parse().unwrap_or(0);
                let total: i64 = parts[3].parse().unwrap_or(0);
                let pct = if total > 0 {
                    (hit as f64 / total as f64) * 100.0
                } else {
                    0.0
                };
                classes.push(serde_json::json!({
                    "class": class,
                    "routine": routine,
                    "hit": hit,
                    "total": total,
                    "pct": (pct * 10.0).round() / 10.0
                }));
            }
            _ => {
                // Unexpected line — skip silently; IRIS might emit warnings
            }
        }
    }

    if !found_total && classes.is_empty() {
        return err_json(
            "PARSE_ERROR",
            &format!("no coverage data in output: {trimmed}"),
        );
    }

    let total_pct = if total_exec > 0 {
        (total_hit as f64 / total_exec as f64) * 100.0
    } else {
        0.0
    };
    let total_pct_rounded = (total_pct * 10.0).round() / 10.0;

    serde_json::json!({
        "success": true,
        "total_pct": total_pct_rounded,
        "hits": total_hit,
        "total": total_exec,
        "classes": classes
    })
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

/// Execute ObjectScript code for coverage operations.
/// Tries execute_via_generator (HTTP/Atelier) first; falls back to docker exec
/// when the Atelier PUT API is unavailable (e.g. some IRIS community/Lab builds).
async fn execute_coverage_code(
    iris: &crate::iris::connection::IrisConnection,
    client: &reqwest::Client,
    code: &str,
    ns: &str,
) -> Result<String, serde_json::Value> {
    match iris.execute_via_generator(code, ns, client).await {
        Ok(output) => Ok(output),
        Err(_) => {
            // HTTP path unavailable — try docker exec fallback
            iris.execute(code, ns)
                .await
                .map_err(|e| err_json("IRIS_UNREACHABLE", &e.to_string()))
        }
    }
}

/// Build start mode ObjectScript code (single-line form, no curly braces).
fn build_coverage_start_code(routine_list: &str, namespace: &str) -> String {
    [
        format!(" New $NAMESPACE  Set $NAMESPACE=\"{}\"", namespace),
        " Do ##class(%Monitor.System.LineByLine).Stop()".to_string(),
        format!(" Set routines={}", routine_list),
        " Set sc=##class(%Monitor.System.LineByLine).Start(routines,\"\",\"\")".to_string(),
        r#" If $System.Status.IsError(sc)  Write "ERROR|MONITOR_IN_USE|"_$System.Status.GetErrorText(sc),$C(10)  Quit"#.to_string(),
        r#" Write "OK|started",$C(10)"#.to_string(),
    ]
    .join("\n")
}

/// Build report mode ObjectScript code for the given routines (single-line form).
fn build_coverage_report_code(routines: &[String], namespace: &str) -> String {
    let mut lines = vec![
        format!(" New $NAMESPACE  Set $NAMESPACE=\"{}\"", namespace),
        " Set totalHit=0  Set totalExec=0".to_string(),
    ];
    for (idx, rtn) in routines.iter().enumerate() {
        lines.push(format!(" Set rtn{}=\"{}\"", idx, rtn));
        lines.push(format!(
            " Set rset{}=##class(%ResultSet).%New(\"%Monitor.System.LineByLine:Result\")",
            idx
        ));
        lines.push(format!(" Do rset{}.Execute(rtn{})", idx, idx));
        lines.push(format!(" Set hit{}=0  Set execTotal{}=0", idx, idx));
        lines.push(format!(
            " For  Quit:'rset{}.Next()  Set data{}=rset{}.GetData(1)  Set ec{}=$ListGet(data{},2)  Set execTotal{}=execTotal{}+$Select(ec{}>=0:1,1:0)  Set hit{}=hit{}+$Select(ec{}>0:1,1:0)",
            idx, idx, idx, idx, idx, idx, idx, idx, idx, idx, idx
        ));
        let class_name = rtn.strip_suffix(".1").unwrap_or(rtn);
        lines.push(format!(
            r#" Write "{}|{}|"_hit{}_"|"_execTotal{},$C(10)"#,
            class_name, rtn, idx, idx
        ));
        lines.push(format!(
            " Set totalHit=totalHit+hit{}  Set totalExec=totalExec+execTotal{}",
            idx, idx
        ));
    }
    lines.push(r#" Write "TOTAL|"_totalHit_"|"_totalExec,$C(10)"#.to_string());
    lines.join("\n")
}

/// Build ObjectScript that checks if TestCoverage.Manager exists in the namespace.
/// Output: "YES" or "NO"
fn build_testcoverage_check_code(namespace: &str) -> String {
    [
        format!(" New $NAMESPACE  Set $NAMESPACE=\"{}\"", namespace),
        r#" If ##class(%Dictionary.ClassDefinition).%ExistsId("TestCoverage.Manager")  Write "YES",$C(10)  Quit"#.to_string(),
        r#" Write "NO",$C(10)"#.to_string(),
    ]
    .join("\n")
}

/// Returns true if the TestCoverage IPM package is installed in the given namespace.
pub async fn testcoverage_available(
    iris: &crate::iris::connection::IrisConnection,
    client: &reqwest::Client,
    ns: &str,
) -> bool {
    let code = build_testcoverage_check_code(ns);
    match execute_coverage_code(iris, client, &code, ns).await {
        Ok(output) => output.trim().starts_with("YES"),
        Err(_) => false,
    }
}

/// Handle an `iris_coverage` tool call.
/// Returns the JSON response value (caller wraps in CallToolResult).
pub async fn handle_iris_coverage(
    iris: &crate::iris::connection::IrisConnection,
    client: &reqwest::Client,
    params: &IrisCoverageParams,
) -> serde_json::Value {
    let ns = params
        .namespace
        .clone()
        .unwrap_or_else(|| iris.namespace.clone());

    match params.mode.as_str() {
        "check" => {
            let code = build_coverage_check_code(&ns);
            let tc_avail = testcoverage_available(iris, client, &ns).await;
            match execute_coverage_code(iris, client, &code, &ns).await {
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

            // Resolve class list (explicit or via package expansion)
            let classes = match resolve_classes(iris, client, params, &ns).await {
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
            let code = build_coverage_run_code(&routines, &test_path, &ns);

            let tc_avail = testcoverage_available(iris, client, &ns).await;

            match execute_coverage_code(iris, client, &code, &ns).await {
                Err(e) => e,
                Ok(output) => {
                    let mut result = parse_coverage_output(&output);
                    if let Some(obj) = result.as_object_mut() {
                        // meets_target field
                        if let Some(target) = params.target_pct {
                            let total_pct =
                                obj.get("total_pct").and_then(|v| v.as_f64()).unwrap_or(0.0);
                            obj.insert(
                                "meets_target".to_string(),
                                serde_json::Value::Bool(total_pct >= target),
                            );
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
            let classes = match resolve_classes(iris, client, params, &ns).await {
                Ok(c) => c,
                Err(e) => return e,
            };
            if classes.is_empty() {
                return err_json("NO_CLASSES", "no concrete classes found");
            }
            let routines: Vec<String> = classes.iter().map(|c| build_routine_name(c)).collect();
            let routine_list = build_routine_list_fragment(&routines);
            let code = build_coverage_start_code(&routine_list, &ns);

            match execute_coverage_code(iris, client, &code, &ns).await {
                Err(e) => e,
                Ok(output) => {
                    let first = output.trim().lines().next().unwrap_or("").to_string();
                    let parts: Vec<&str> = first.splitn(2, '|').collect();
                    match parts.first().copied() {
                        Some("OK") => serde_json::json!({
                            "success": true,
                            "started": true,
                            "routines": routines
                        }),
                        Some("ERROR") => {
                            let msg = parts.get(1).copied().unwrap_or("unknown");
                            err_json("MONITOR_IN_USE", msg)
                        }
                        _ => err_json("PARSE_ERROR", &format!("unexpected start output: {first}")),
                    }
                }
            }
        }

        "stop" => {
            let code = [
                format!(" New $NAMESPACE  Set $NAMESPACE=\"{}\"", ns),
                " Do ##class(%Monitor.System.LineByLine).Stop()".to_string(),
                r#" Write "OK|stopped",$C(10)"#.to_string(),
            ]
            .join("\n");
            match execute_coverage_code(iris, client, &code, &ns).await {
                Err(e) => e,
                Ok(_) => serde_json::json!({"success": true, "stopped": true}),
            }
        }

        "report" => {
            let classes = match resolve_classes(iris, client, params, &ns).await {
                Ok(c) => c,
                Err(e) => return e,
            };
            if classes.is_empty() {
                return err_json("NO_CLASSES", "no concrete classes found");
            }
            let routines: Vec<String> = classes.iter().map(|c| build_routine_name(c)).collect();
            let code = build_coverage_report_code(&routines, &ns);

            match execute_coverage_code(iris, client, &code, &ns).await {
                Err(e) => e,
                Ok(output) => parse_coverage_output(&output),
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
    iris: &crate::iris::connection::IrisConnection,
    client: &reqwest::Client,
    params: &IrisCoverageParams,
    ns: &str,
) -> Result<Vec<String>, serde_json::Value> {
    if let Some(ref explicit) = params.classes {
        return Ok(explicit.clone());
    }
    if let Some(ref pkg) = params.package {
        let code = build_package_expand_code(pkg, ns);
        let output = execute_coverage_code(iris, client, &code, ns).await?;
        return parse_package_expand_output(&output);
    }
    Err(err_json(
        "MISSING_PARAM",
        "provide either 'classes' (list of class names) or 'package' (auto-discover)",
    ))
}
