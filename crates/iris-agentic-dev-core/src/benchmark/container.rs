//! Confirms an `IrisConnection` is reachable for a benchmark run, and provides the
//! HTTP-only compile/write/run-test primitives the benchmark loop needs. Fully HTTP —
//! no Docker exec required, per research.md.
//!
//! **Architecture note (supersedes an earlier draft of this module)**: the ported
//! `jira_bugs` task suite's `test_code` classes extend `%RegisteredObject` with
//! `ClassMethod`/`Method`s named `TestXxx`, NOT `%UnitTest.TestCase` — mirroring
//! objectscript-coder's own `IrisLight.BenchRunner` mechanism (introspect the compiled
//! class's methods via `%Dictionary.ClassDefinition`, invoke each `Test*` method
//! directly via `$ClassMethod`/`$Method`, catch exceptions to detect failure). This
//! needs no `%UnitTest.Manager`/`^UnitTestRoot`/Docker at all — it runs correctly over
//! the plain `execute_via_generator` HTTP path, verified live and deterministic across
//! repeated runs. An earlier draft of this module attempted to run these test classes
//! via `%UnitTest.Manager.RunTest` (which requires the class to extend
//! `%UnitTest.TestCase`, which these do not, and which — separately — was found to
//! silently no-op test execution under `execute_via_generator` regardless). Neither
//! problem applies to the `$ClassMethod`/`$Method` introspection approach used here.

use crate::iris::connection::IrisConnection;
use std::sync::Arc;

/// Uploads a `.cls` source file via Atelier PUT (`ignoreConflict=1`, matching the pattern
/// already used by `iris_compile`'s local-file-path branch), then compiles it.
///
/// **Verified live**: Atelier's PUT auto-prefixes a bare (no-package) class name's
/// document with `User.` — e.g. `Class TestFoo {...}` compiles to `User.TestFoo`, not
/// `TestFoo`. Callers that need to reference the class by name after compiling it must
/// account for this (see `resolve_class_name`).
pub async fn write_and_compile(
    iris: &IrisConnection,
    client: &reqwest::Client,
    namespace: &str,
    doc_name: &str,
    content: &str,
) -> anyhow::Result<Vec<String>> {
    let put_url = iris.versioned_ns_url(
        namespace,
        &format!("/doc/{}?ignoreConflict=1", urlencoding::encode(doc_name)),
    );
    let lines: Vec<&str> = content.lines().collect();
    let put_resp = client
        .put(&put_url)
        .basic_auth(&iris.username, Some(&iris.password))
        .json(&serde_json::json!({"enc": false, "content": lines}))
        .send()
        .await?;
    if !put_resp.status().is_success() {
        anyhow::bail!("PUT {doc_name} returned HTTP {}", put_resp.status());
    }
    let put_body: serde_json::Value = put_resp.json().await.unwrap_or_default();
    if let Some(errs) = put_body["status"]["errors"].as_array() {
        if !errs.is_empty() {
            let msg = errs[0]["error"].as_str().unwrap_or("Upload failed");
            anyhow::bail!("PUT {doc_name} failed: {msg}");
        }
    }
    let cr = iris
        .compile_document(doc_name, namespace, "cuk", client)
        .await?;
    Ok(cr.errors)
}

/// A class name with no `.` package separator compiles to `User.<name>` via Atelier PUT
/// (verified live) — this predicts the actual compiled class name from the source-derived
/// name, without a round-trip query.
pub fn resolve_class_name(class_name: &str) -> String {
    if class_name.contains('.') {
        class_name.to_string()
    } else {
        format!("User.{class_name}")
    }
}

/// Runs every `Test*` method on `class_name` (resolved via `resolve_class_name`) and
/// reports `(all_passed, detail)`; `detail` lists failing methods
/// (`"TestFoo: <exception message>"`, semicolon-separated) when `all_passed` is `false`.
/// Mirrors objectscript-coder's `IrisLight.BenchRunner.RunAll` mechanism exactly —
/// introspect `%Dictionary.ClassDefinition`'s `Methods`, invoke each `Test*`-named
/// method via `$ClassMethod`/`$Method`, catch exceptions to detect failure.
pub async fn run_class_tests(
    iris: &IrisConnection,
    client: &reqwest::Client,
    namespace: &str,
    class_name: &str,
) -> anyhow::Result<(bool, String)> {
    let resolved = resolve_class_name(class_name);
    let safe = objectscript_string_literal(&resolved);
    let code = format!(
        r#"set passed = 0
set failed = 0
set failMsg = ""
set classDef = ##class(%Dictionary.ClassDefinition).%OpenId({safe})
if '$IsObject(classDef) {{
  write "0_1|CLASS_NOT_FOUND: ",{safe},!
}} else {{
  for i=1:1:classDef.Methods.Count() {{
    set method = classDef.Methods.GetAt(i)
    set methodName = method.Name
    if ($Extract(methodName,1,4) '= "Test") {{ continue }}
    try {{
      if method.ClassMethod {{
        do $ClassMethod({safe}, methodName)
      }} else {{
        set obj = $ClassMethod({safe},"%New")
        do $Method(obj, methodName)
      }}
      set passed = passed + 1
    }} catch ex {{
      set failed = failed + 1
      if failMsg '= "" {{ set failMsg = failMsg _ "; " }}
      set failMsg = failMsg _ methodName _ ": " _ ex.DisplayString()
    }}
  }}
  write passed,"_",failed,"|",failMsg,!
}}
"#
    );
    let output = iris.execute_via_generator(&code, namespace, client).await?;
    Ok(parse_run_all_output(&output))
}

fn objectscript_string_literal(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// Parses the `passed_failed|failMsg` line this module's generated code writes, mirroring
/// `objectscript_mcp/benchmark/runner.py`'s `_run_tests` parser.
fn parse_run_all_output(output: &str) -> (bool, String) {
    for line in output.lines() {
        let line = line.trim();
        if let Some((counts, msg)) = line.split_once('|') {
            if let Some((p, f)) = counts.split_once('_') {
                if let (Ok(passed), Ok(failed)) = (p.trim().parse::<u32>(), f.trim().parse::<u32>())
                {
                    return (passed > 0 && failed == 0, msg.to_string());
                }
            }
        }
    }
    (false, format!("no result: {output}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_class_name_prefixes_bare_names_with_user() {
        assert_eq!(resolve_class_name("TestFoo"), "User.TestFoo");
    }

    #[test]
    fn resolve_class_name_leaves_packaged_names_unchanged() {
        assert_eq!(resolve_class_name("MyApp.TestFoo"), "MyApp.TestFoo");
    }

    #[test]
    fn parse_run_all_output_all_passed() {
        let (passed, detail) = parse_run_all_output("2_0|");
        assert!(passed);
        assert!(detail.is_empty());
    }

    #[test]
    fn parse_run_all_output_some_failed() {
        let (passed, detail) = parse_run_all_output("1_1|TestFails: deliberate fail");
        assert!(!passed);
        assert!(detail.contains("TestFails"));
    }

    #[test]
    fn parse_run_all_output_class_not_found() {
        let (passed, detail) = parse_run_all_output("0_1|CLASS_NOT_FOUND: User.Bogus");
        assert!(!passed);
        assert!(detail.contains("CLASS_NOT_FOUND"));
    }

    #[test]
    fn parse_run_all_output_no_result_line() {
        let (passed, detail) = parse_run_all_output("garbage output with no pipe");
        assert!(!passed);
        assert!(detail.contains("no result"));
    }
}

/// Confirms an `IrisConnection` is reachable — used by the CLI subcommand before
/// starting a run, mirroring the connection-check step every other subcommand performs.
pub async fn confirm_reachable(
    iris: &Arc<IrisConnection>,
    client: &reqwest::Client,
) -> anyhow::Result<()> {
    iris.execute_via_generator("write 1", "USER", client)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("IRIS_UNREACHABLE: {e}"))
}
