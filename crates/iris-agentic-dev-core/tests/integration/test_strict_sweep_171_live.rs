//! S17, against `iris-dev-iris`: under `read_only = "strict"` no call that strict lets through puts
//! a scratch class on the server or asks it to compile anything, and strict with `irisAudit = true`
//! is refused without writing the audit class.
//!
//! The requests are read from iad's own request watch (`testing::take_requests`), which every IRIS
//! HTTP client of a test build reports to. A scratch class is deleted before the call returns, so
//! its URLs are the only place it shows: any request under `/doc/IrisDevTmp` (its PUT, GET or
//! DELETE) or to `/action/compile` fails the sweep. The watch sees URLs, not methods.
//!
//! The three skill readers' `^SKILLS` read goes through the container's terminal, not HTTP, so this
//! file does not see it; the unit tests in `test_skill_kb_gate_171.rs` check that strict skips it.
//! No test here opens a socket or drives a terminal.
//!
//! Serial, `#[ignore]`: run with
//! `IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo test --features testing --test integration test_strict_sweep_171_live -- --include-ignored --test-threads=1`.

use super::test_enum_tools_work_live::cases;
use iris_agentic_dev_core::iris::config_sources::{ConfigSnapshot, LoadInputs, CONFIG_FILE_NAME};
use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::skills::SkillRegistry;
use iris_agentic_dev_core::testing::{clear_requests, take_requests, LiveTarget};
use iris_agentic_dev_core::tools::{IrisTools, Toolset};
use serde_json::{json, Value};

/// A URL that writes, reads or deletes a scratch class, or compiles.
fn compiles(line: &str) -> bool {
    line.contains("/doc/IrisDevTmp") || line.contains("/action/compile")
}

struct Rig {
    tools: IrisTools,
    /// `host:port` of the server, the prefix of every watched URL this rig sends.
    target: String,
    _dir: tempfile::TempDir,
}

impl Rig {
    /// The requests this rig's server received since the last call, and nothing else's.
    fn requests(&self) -> Vec<String> {
        take_requests()
            .into_iter()
            .filter(|l| l.starts_with(&self.target))
            .collect()
    }
}

async fn rig(operator: &str, toolset: Toolset) -> Rig {
    let live = LiveTarget::from_env();
    live.assert_running();
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let home = root.join("home");
    let proj = root.join("proj");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(home.join(CONFIG_FILE_NAME), operator).unwrap();
    let inputs = LoadInputs {
        cwd: proj,
        home: Some(home),
        ..Default::default()
    };
    let snapshot = ConfigSnapshot::load(&inputs);
    let declared = snapshot.declared_gates();
    let mut conn = IrisConnection::new(
        format!("http://{}:{}", live.host, live.web_port),
        &live.namespace,
        &live.username,
        &live.password,
        DiscoverySource::ExplicitFlag,
    );
    conn.probe().await;
    let tools = IrisTools::with_registry_and_toolset(
        Some(conn),
        SkillRegistry::new(),
        toolset,
        None,
        None,
        false,
        declared,
    )
    .unwrap();
    tools.set_config(inputs, snapshot);
    clear_requests(); // the probe's requests are not a call's
    Rig {
        tools,
        target: format!("{}:{}", live.host, live.web_port),
        _dir: dir,
    }
}

async fn call(r: &Rig, tool: &str, args: Value) -> Value {
    match r.tools.call_gated(tool, args).await {
        Ok(res) => res
            .structured_content
            .clone()
            .or_else(|| {
                res.content
                    .iter()
                    .find_map(|c| c.as_text())
                    .and_then(|t| serde_json::from_str(&t.text).ok())
            })
            .unwrap_or(json!({})),
        Err(e) => json!({"error": e}),
    }
}

const OPEN: &str = "write_tools_enabled = true\ndestructive_tools_enabled = true\n";
const STRICT: &str =
    "write_tools_enabled = true\ndestructive_tools_enabled = true\nread_only = \"strict\"\n";
const AUDIT: &str = "[policy.default]\nirisAudit = true\n";

/// The audit emission runs in a spawned task after the call answers.
async fn settle() {
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
}

#[tokio::test]
#[ignore = "requires live IRIS (iris-dev-iris)"]
async fn the_control_compiles_a_scratch_class_with_strict_off() {
    let r = rig(OPEN, Toolset::Baseline).await;
    let b = call(&r, "skill", json!({"action": "list"})).await;
    assert_eq!(b["success"], true, "{b}");
    let lines = r.requests();
    assert!(
        lines.iter().any(|l| compiles(l)),
        "the control sent no scratch class or compile, so the sweep below proves nothing: \
         {lines:?}"
    );
}

#[tokio::test]
#[ignore = "requires live IRIS (iris-dev-iris)"]
async fn the_skill_readers_answer_under_strict_without_compiling() {
    let r = rig(STRICT, Toolset::Baseline).await;
    for (tool, args) in [
        ("skill_list", json!({})),
        ("skill_search", json!({"query": "guardrails"})),
        ("skill_describe", json!({"name": "objectscript-guardrails"})),
    ] {
        let b = call(&r, tool, args).await;
        assert!(
            b["error_code"].is_null() || b["error_code"] == "NOT_FOUND",
            "{tool}: {b}"
        );
        if b["sources"].is_object() {
            assert_eq!(
                b["sources"]["synthesized"]["refused"]["error_code"], "READ_ONLY_STRICT",
                "{tool}: {b}"
            );
        }
    }
    let bad: Vec<String> = r.requests().into_iter().filter(|l| compiles(l)).collect();
    assert!(bad.is_empty(), "the readers compiled under strict: {bad:?}");
}

#[tokio::test]
#[ignore = "requires live IRIS (iris-dev-iris)"]
async fn no_read_only_call_compiles_anything_under_strict() {
    let mut swept = 0;
    let mut refused = 0;
    let mut failures = vec![];
    for toolset in [Toolset::Baseline, Toolset::Merged] {
        let r = rig(STRICT, toolset).await;
        let names = r.tools.registered_tool_names();
        for case in cases() {
            if !names.contains(case.tool) {
                continue;
            }
            let b = call(&r, case.tool, case.args.clone()).await;
            let lines = r.requests();
            swept += 1;
            if b["error_code"] == "READ_ONLY_STRICT" {
                refused += 1;
            }
            let bad: Vec<&String> = lines.iter().filter(|l| compiles(l)).collect();
            if !bad.is_empty() {
                failures.push(format!(
                    "{toolset:?} {} {}: {bad:?} (answer code {})",
                    case.tool, case.args, b["error_code"]
                ));
            }
        }
    }
    assert!(swept > 100, "only {swept} calls were swept");
    assert!(refused > 20, "only {refused} calls were refused by strict");
    assert!(
        failures.is_empty(),
        "under strict these calls put a scratch class on the server or compiled:\n{}",
        failures.join("\n")
    );
}

#[tokio::test]
#[ignore = "requires live IRIS (iris-dev-iris)"]
async fn the_control_audit_compiles_a_class_with_strict_off() {
    let r = rig(&format!("{OPEN}{AUDIT}"), Toolset::Baseline).await;
    let b = call(&r, "iris_query", json!({"query": "SELECT 1 AS one"})).await;
    assert!(b["error_code"].is_null(), "{b}");
    settle().await;
    let lines = r.requests();
    assert!(
        lines.iter().any(|l| compiles(l)),
        "the audit emission sent no scratch class or compile, so the test below proves nothing: \
         {lines:?}"
    );
}

#[tokio::test]
#[ignore = "requires live IRIS (iris-dev-iris)"]
async fn strict_with_audit_is_refused_and_compiles_nothing() {
    for toolset in [Toolset::Baseline, Toolset::Merged] {
        let r = rig(&format!("{STRICT}{AUDIT}"), toolset).await;
        for (tool, args) in [
            ("iris_query", json!({"query": "SELECT 1 AS one"})),
            ("iris_execute", json!({"code": "Write 1"})),
            ("iris_namespace_list", json!({})),
        ] {
            let b = call(&r, tool, args).await;
            assert_eq!(
                b["error_code"], "READ_ONLY_STRICT_WITH_AUDIT",
                "{toolset:?} {tool}: {b}"
            );
        }
        // The tools that answer whatever the configuration says still answer, and write no audit
        // class while they do.
        let b = call(&r, "check_config", json!({})).await;
        assert!(
            b["config_errors"]
                .to_string()
                .contains("READ_ONLY_STRICT_WITH_AUDIT"),
            "{toolset:?}: {b}"
        );
        let b = call(&r, "skill_list", json!({})).await;
        assert_eq!(
            b["sources"]["synthesized"]["refused"]["error_code"], "READ_ONLY_STRICT_WITH_AUDIT",
            "{toolset:?}: {b}"
        );
        let _ = call(&r, "iris_servers", json!({})).await;
        settle().await;
        let bad: Vec<String> = r.requests().into_iter().filter(|l| compiles(l)).collect();
        assert!(
            bad.is_empty(),
            "{toolset:?}: compiled under strict: {bad:?}"
        );
    }
}
