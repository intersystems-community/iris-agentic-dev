//! Spec 129: the hint replay corpus against live IRIS.
//!
//! `tests/e2e/tasks/hints/replay.jsonl` holds bad calls with the error IRIS gave for each, and the
//! rule that should fire (or `none`). This file replays every `live` item through the real tool
//! handler and checks the response still carries that rule's hint, and that each positive's `fix`
//! prepares (SQL) or runs (ObjectScript) cleanly. `IAD_REGEN_HINTS=1` rewrites the captures instead
//! of comparing them. Mined items are skipped: their original calls cannot be replayed.
//!
//! Run with:
//!   IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS \
//!   cargo test --features testing --test integration test_hints_replay_129_live -- \
//!     --ignored --test-threads=1

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::testing::EnvGuard;
use iris_agentic_dev_core::tools::error_hints::{runtime_error_hint, sql_error_hint};
use iris_agentic_dev_core::tools::IrisTools;
use std::path::PathBuf;

fn corpus_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/e2e/tasks/hints/replay.jsonl")
}

fn setup_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/e2e/tasks/hints/setup")
}

fn tools() -> Option<(IrisTools, IrisConnection)> {
    let host = std::env::var("IRIS_HOST").unwrap_or_default();
    if host.is_empty() {
        if std::env::var("IAD_ALLOW_SKIP").is_ok() {
            eprintln!("SKIP (IAD_ALLOW_SKIP set): IRIS_HOST unset");
            return None;
        }
        panic!(
            "IRIS_HOST unset. This test replays captured IRIS errors; without IRIS it asserts \
             nothing. Set IRIS_HOST=localhost IRIS_WEB_PORT=52780, or IAD_ALLOW_SKIP=1."
        );
    }
    let port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".into());
    let user = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into());
    let pass = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into());
    let conn = IrisConnection::new(
        format!("http://{host}:{port}"),
        "USER",
        user,
        pass,
        DiscoverySource::EnvVar,
    );
    let t = IrisTools::new(Some(conn.clone())).expect("IrisTools::new");
    Some((t, conn))
}

fn parse(r: Result<rmcp::model::CallToolResult, String>) -> serde_json::Value {
    let r = r.expect("call_for_test returned Err");
    let text = r.content[0].as_text().unwrap().text.clone();
    serde_json::from_str(&text).unwrap_or_else(|_| serde_json::json!({"raw": text}))
}

/// `IrisDevRun<hash>` changes on every run; the rest of the error is stable.
fn normalise(s: &str) -> String {
    regex::Regex::new(r"IrisDevRun[0-9a-f]+")
        .unwrap()
        .replace_all(s, "IrisDevRun")
        .into_owned()
}

/// Put and compile the two setup classes if USER lacks them. They stay: the corpus needs them.
async fn ensure_setup(t: &IrisTools) {
    for (doc, file) in [
        ("Test129.Hint.cls", "Test129.Hint.cls"),
        ("Test129.Sub.Deep.cls", "Test129.Sub.Deep.cls"),
    ] {
        let class = doc.trim_end_matches(".cls");
        let v = parse(
            t.call_for_test(
                "iris_execute",
                serde_json::json!({"code": format!(
                    "Write ##class(%Dictionary.CompiledClass).%ExistsId(\"{class}\")"),
                    "namespace": "USER"}),
            )
            .await,
        );
        if v["output"] == "1" {
            continue;
        }
        let src = std::fs::read_to_string(setup_dir().join(file)).expect("setup class");
        let v = parse(
            t.call_for_test(
                "iris_doc",
                serde_json::json!({"mode": "put", "name": doc, "content": src,
                                   "compile": true, "namespace": "USER"}),
            )
            .await,
        );
        assert_ne!(v["success"], false, "setup {doc}: {v}");
    }
}

fn query_of(args: &serde_json::Value) -> String {
    match (args["table"].as_str(), args["query"].as_str()) {
        (Some(t), q) => format!("{t} {}", q.unwrap_or("")),
        (None, Some(q)) => q.to_string(),
        _ => String::new(),
    }
}

/// What the matchers make of an item's capture: `(rule, text, placeholder values)`.
fn expected(
    item: &serde_json::Value,
    captured: &str,
) -> Option<(String, String, serde_json::Value)> {
    let ns = item["namespace"].as_str().unwrap();
    let h = if item["tool"] == "iris_execute" {
        runtime_error_hint(captured, ns)
    } else {
        sql_error_hint(captured, &query_of(&item["args"]), ns)
    }?;
    let vars: serde_json::Map<String, serde_json::Value> = h
        .vars()
        .iter()
        .map(|(k, v)| (k.to_string(), serde_json::Value::String(v.clone())))
        .collect();
    Some((h.rule().to_string(), h.text().to_string(), vars.into()))
}

/// Prepare (never execute) a fix's SQL, or run a fix's ObjectScript, and report success.
async fn fix_works(t: &IrisTools, fix: &serde_json::Value) -> Result<(), String> {
    let ns = fix["namespace"].as_str().unwrap_or("USER");
    let code = if let Some(q) = fix["query"].as_str() {
        format!(
            "Set st=##class(%SQL.Statement).%New() Set sc=st.%Prepare(\"{}\") \
             If $$$ISERR(sc) {{ Write \"PREPARE_FAILED:\",$System.Status.GetErrorText(sc) }} \
             Else {{ Write \"PREPARED\" }}",
            q.replace('"', "\"\"")
        )
    } else {
        fix["code"].as_str().unwrap().to_string()
    };
    let v = parse(
        t.call_for_test(
            "iris_execute",
            serde_json::json!({"code": code, "namespace": ns}),
        )
        .await,
    );
    let out = v["output"].as_str().unwrap_or_default();
    if v["success"] == true && !out.contains("PREPARE_FAILED") {
        Ok(())
    } else {
        Err(v.to_string())
    }
}

#[tokio::test]
#[ignore]
async fn replay_corpus_still_matches_live_iris() {
    let Some((t, _conn)) = tools() else { return };
    ensure_setup(&t).await;
    let regen = std::env::var("IAD_REGEN_HINTS").is_ok_and(|v| v == "1");
    let text = std::fs::read_to_string(corpus_path()).expect("replay.jsonl");
    let mut out = Vec::new();
    let mut problems = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let mut item: serde_json::Value = serde_json::from_str(line).expect("json line");
        let id = item["id"].as_str().unwrap().to_string();
        if item["source"] != "live" {
            out.push(item);
            continue;
        }
        let v = parse(
            t.call_for_test(item["tool"].as_str().unwrap(), item["args"].clone())
                .await,
        );
        let raw = if item["tool"] == "iris_execute" {
            v["output"].as_str().unwrap_or_default()
        } else {
            v["error"].as_str().unwrap_or_default()
        };
        let captured = normalise(raw);
        if v["success"] == true || captured.is_empty() {
            problems.push(format!("{id}: the call no longer fails: {v}"));
        }
        let rule = item["rule"].as_str().unwrap().to_string();
        match (expected(&item, &captured), rule.as_str()) {
            (None, "none") => assert!(v.get("hint").is_none(), "{id}: {v}"),
            (Some((got, _, _)), "none") => problems.push(format!("{id}: expected none, got {got}")),
            (None, want) => problems.push(format!("{id}: expected {want}, got none: {captured}")),
            (Some((got, text, vars)), want) => {
                if regen {
                    item["vars"] = vars;
                }
                if got != want {
                    problems.push(format!("{id}: expected {want}, got {got}"));
                }
                if v["hint"] != text.as_str() {
                    problems.push(format!("{id}: response hint differs: {v}"));
                }
                if let Err(e) = fix_works(&t, &item["fix"]).await {
                    problems.push(format!("{id}: fix fails: {e}"));
                }
            }
        }
        if regen {
            item["captured"] = serde_json::Value::String(captured);
        } else if item["captured"].as_str() != Some(captured.as_str()) {
            problems.push(format!(
                "{id}: capture changed (rerun with IAD_REGEN_HINTS=1):\n  was {}\n  now {captured}",
                item["captured"]
            ));
        }
        out.push(item);
    }
    if regen {
        let body: String = out.iter().map(|i| format!("{i}\n")).collect();
        std::fs::write(corpus_path(), body).expect("write replay.jsonl");
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// FR-002 live: the pack switch drops `hint_ref` from a real response and keeps the text.
#[tokio::test]
#[ignore]
async fn coding_pack_off_drops_hint_ref_on_a_live_error() {
    let mut env_guard = EnvGuard::new();
    let Some((t, _conn)) = tools() else { return };
    let args =
        serde_json::json!({"query": "SELECT TOP 1 Name FROM Security.Users", "namespace": "USER"});
    let on = parse(t.call_for_test("iris_query", args.clone()).await);
    env_guard.set("IAD_CODING_PACK", "off");
    let off = parse(t.call_for_test("iris_query", args).await);
    env_guard.unset("IAD_CODING_PACK");
    assert_eq!(on["hint_ref"]["skill"], "iris-agentic-dev", "{on}");
    assert!(off.get("hint_ref").is_none(), "{off}");
    assert_eq!(on["hint"], off["hint"]);
    assert!(off["hint"].is_string(), "{off}");
}
