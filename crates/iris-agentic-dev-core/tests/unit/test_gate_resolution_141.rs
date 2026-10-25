//! Spec 141 R1 section 6.1: every gate reads the policy of the server the call names.
//!
//! The table walks every IRIS-kind tool in the router with `server = "prod"`, a Live subject
//! server that allows only search and blocks data reads, while the default connection `dev` has no
//! policy at all. Before 1.7.0 each handler ran its gates against the default connection, so
//! `server = "prod"` was judged by `dev`'s policy and every one of these calls went through.
//!
//! Each call goes down both tails of the pipeline: `call_gated` (the CLI and the tests) and the MCP
//! server over an in-memory pipe.
//!
//! The refusal has to come from a gate, never from the network: `prod` is at `127.0.0.1:1`, so a
//! call that got past the gates fails with a connection error, and a connection error is not an
//! accepted answer.

use crate::support_gate_141::*;
use iris_agentic_dev_core::policy::refusal::REFUSAL_CODES;
use iris_agentic_dev_core::tools::write_gate::{
    classify, TargetKind, TargetSource, WriteClass, CLASSIFICATION,
};
use iris_agentic_dev_core::tools::{IrisTools, Toolset};
use serde_json::{json, Map, Value};

/// The argument that carries a dispatcher's action: `mode` for `iris_doc` and `iris_query`,
/// `action` for the rest. Read off the advertised schema, so a new tool is covered by the same
/// rule.
fn action_key(props: &[String]) -> Option<&'static str> {
    if props.iter().any(|p| p == "action") {
        Some("action")
    } else if props.iter().any(|p| p == "mode") {
        Some("mode")
    } else {
        None
    }
}

type Advertised = std::collections::BTreeMap<String, Vec<String>>;

/// The arguments for a call that classifies as a write where the tool has one, and as its only
/// class where it has not. The gates read these before the handler does, so they are the content
/// the gates look at, not a complete valid call.
fn write_args(adv: &Advertised, tool: &str, server_arg: &str, server: &str) -> Map<String, Value> {
    let row = CLASSIFICATION.iter().find(|r| r.tool == tool).unwrap();
    let mut args = Map::new();
    if !row.actions.is_empty() {
        let write = row
            .actions
            .iter()
            .find(|(_, c)| *c != WriteClass::ReadOnly)
            .map(|(a, _)| a.to_string());
        let props = adv.get(tool).cloned().unwrap_or_default();
        let (read_default_is_write, key) =
            (row.default != WriteClass::ReadOnly, action_key(&props));
        let action = match (write, read_default_is_write) {
            (Some(a), _) => a,
            // Every named action reads and the default writes: an action the table does not name.
            (None, true) => "an_action_the_table_does_not_name".to_string(),
            (None, false) => row.actions[0].0.to_string(),
        };
        if let Some(key) = key {
            args.insert(key.to_string(), json!(action));
        }
    }
    // Content the gates read.
    args.insert("code".into(), json!("Write 1"));
    args.insert("query".into(), json!("SELECT 1"));
    args.insert("target".into(), json!("Sentinel141.cls"));
    args.insert("name".into(), json!("Sentinel141.cls"));
    args.insert("global_name".into(), json!("^Sentinel141"));
    args.insert("global".into(), json!("^Sentinel141"));
    args.insert(server_arg.to_string(), json!(server));
    // The unknown-parameter check runs before the policy gates, so a read-only call must carry only
    // what the tool advertises; the write gate before it reads whatever is there.
    if let Some(props) = adv.get(tool) {
        args.retain(|k, _| props.contains(k) || k == server_arg);
    }
    args
}

/// The tools the table covers: IRIS-kind, named by a `server` argument (or two), present in the
/// toolset.
fn table_tools(tools: &IrisTools) -> Vec<(&'static str, &'static str)> {
    let names = tools.registered_tool_names();
    CLASSIFICATION
        .iter()
        .filter(|r| r.kind == TargetKind::Iris && names.contains(r.tool))
        .filter_map(|r| match r.target {
            TargetSource::Arg("server") | TargetSource::ArgOrPending("server") => {
                Some((r.tool, "server"))
            }
            TargetSource::Args(a) => Some((r.tool, a[0])),
            TargetSource::Name(n) => Some((r.tool, n)),
            _ => None,
        })
        .collect()
}

fn names_prod(body: &Value) -> bool {
    body.to_string().contains("prod")
}

/// A tool whose category the allow list names passes the gates; everything else must be refused.
fn passes_allow_search(tool: &str) -> bool {
    matches!(tool, "iris_search" | "iris_symbols")
}

fn assert_refused_by_prod(tool: &str, body: &Value, tail: &str, writes_declared: bool) {
    let code = code_of(body);
    assert!(
        REFUSAL_CODES.contains(&code.as_str()),
        "{tail}: {tool} with server=prod answered {code:?}, not a gate refusal. Body: {body}"
    );
    assert!(
        names_prod(body),
        "{tail}: the {code} refusal for {tool} does not name prod, so it was not judged on prod. \
         Body: {body}"
    );
    if writes_declared {
        assert!(
            code != "WRITE_TOOLS_DISABLED" && code != "DESTRUCTIVE_TOOLS_DISABLED",
            "{tail}: {tool} was refused by the write gate although [policy.prod] declares \
             writeTools and destructiveTools. Body: {body}"
        );
    }
}

async fn run_table_gated(toml: &str, toolset: Toolset, writes_declared: bool) -> usize {
    let rig = rig_with(toml, toolset, &[("prod", UNROUTABLE)]);
    let tools = &rig.tools;
    let adv = advertised(tools);
    let list = table_tools(tools);
    assert!(
        list.len() > 20,
        "the table found only {} tools; the classification columns are empty",
        list.len()
    );
    let mut refused = 0;
    for (tool, server_arg) in list {
        let args = write_args(&adv, tool, server_arg, "prod");
        let class = classify(tool, Some(&args)).unwrap();
        let answer = tools.call_gated(tool, Value::Object(args)).await;
        let body = match &answer {
            Ok(r) => body_of(r),
            Err(e) => json!({"error": e}),
        };
        if passes_allow_search(tool) {
            assert!(
                !REFUSAL_CODES.contains(&code_of(&body).as_str()),
                "call_gated: {tool} is in the allow list and was refused. Body: {body}"
            );
            continue;
        }
        assert_refused_by_prod(tool, &body, "call_gated", writes_declared);
        if !writes_declared && class != WriteClass::ReadOnly {
            assert_eq!(
                code_of(&body),
                "WRITE_TOOLS_DISABLED",
                "call_gated: {tool} writes on a member nobody declared and nobody could probe; \
                 the write gate answers first. Body: {body}"
            );
        }
        refused += 1;
    }
    refused
}

#[tokio::test]
async fn call_gated_judges_every_tool_by_the_server_it_names_gates_closed() {
    let a = run_table_gated(PROD_LOCKED, Toolset::Merged, false).await;
    let b = run_table_gated(PROD_LOCKED, Toolset::Baseline, false).await;
    assert!(a > 30 && b > 20, "refused {a} and {b}");
}

#[tokio::test]
async fn call_gated_judges_every_tool_by_the_server_it_names_gates_declared_open() {
    let refused = run_table_gated(PROD_LOCKED_WRITES_DECLARED, Toolset::Merged, true).await;
    assert!(refused > 20, "refused only {refused} tools");
}

async fn run_table_mcp(toml: &str, writes_declared: bool) -> usize {
    let rig = rig_with(toml, Toolset::Merged, &[("prod", UNROUTABLE)]);
    let list = table_tools(&rig.tools);
    let adv = advertised(&rig.tools);
    let mut client = McpClient::start(rig.tools.clone()).await;
    let mut refused = 0;
    for (tool, server_arg) in list {
        let args = write_args(&adv, tool, server_arg, "prod");
        let body = client.call(tool, Value::Object(args)).await;
        if passes_allow_search(tool) {
            assert!(
                !REFUSAL_CODES.contains(&code_of(&body).as_str()),
                "mcp: {tool} is in the allow list and was refused. Body: {body}"
            );
            continue;
        }
        assert_refused_by_prod(tool, &body, "mcp", writes_declared);
        refused += 1;
    }
    refused
}

#[tokio::test]
async fn the_mcp_tail_judges_every_tool_by_the_server_it_names() {
    // The gates-declared-open table runs through `call_gated` only: each call over the pipe costs
    // a few hundred milliseconds of telemetry bookkeeping in a current-thread runtime.
    let refused = run_table_mcp(PROD_LOCKED, false).await;
    assert!(
        refused > 20,
        "refused only {refused} tools over the MCP tail"
    );
}

// ── T033: the same table with no `server` ───────────────────────────────────────────────────────

const DEFAULT_ALLOWS_SEARCH: &str = r#"
[policy.default]
allow = ["search"]
"#;

#[tokio::test]
async fn with_no_server_every_tool_outside_search_answers_policy_gate() {
    let rig = rig_with(DEFAULT_ALLOWS_SEARCH, Toolset::Merged, &[]);
    let tools = &rig.tools;
    let adv = advertised(tools);
    let names = tools.registered_tool_names();
    let mut checked = 0;
    for row in CLASSIFICATION
        .iter()
        .filter(|r| r.kind == TargetKind::Iris && names.contains(r.tool))
    {
        if matches!(row.target, TargetSource::Session | TargetSource::Args(_)) {
            continue;
        }
        let mut args = write_args(&adv, row.tool, "server", "dev");
        args.remove("server");
        args.remove("name");
        if row.tool == "iris_test_server" {
            continue;
        }
        let args_for_class = args.clone();
        let answer = tools.call_gated(row.tool, Value::Object(args)).await;
        let body = match &answer {
            Ok(r) => body_of(r),
            Err(e) => json!({"error": e}),
        };
        let code = code_of(&body);
        if passes_allow_search(row.tool) {
            assert!(
                !REFUSAL_CODES.contains(&code.as_str()),
                "{} is a search tool and was refused: {body}",
                row.tool
            );
            continue;
        }
        // The destructive tier is off on the default, and its gate comes first; the bulk-PHI gate
        // comes before `allow` and answers for its two tools.
        let class = classify(row.tool, Some(&args_for_class)).unwrap();
        let expected: &[&str] = if class == WriteClass::Destructive {
            &["DESTRUCTIVE_TOOLS_DISABLED"]
        } else if matches!(row.tool, "journal_search" | "iris_message_body") {
            &["DATA_POLICY_BLOCKED"]
        } else {
            &["POLICY_GATE"]
        };
        assert!(
            expected.contains(&code.as_str()),
            "{} with no server and [policy.default] allow = [\"search\"] answered {code:?}, \
             expected one of {expected:?}. Body: {body}",
            row.tool
        );
        checked += 1;
    }
    assert!(checked > 30, "only {checked} tools were checked");
}

// ── aliasing and spelling of `server` ───────────────────────────────────────────────────────────

const DEFAULT_LIVE: &str = r#"
[policy.default]
mcpTemplate = "live"
"#;

#[tokio::test]
async fn a_server_that_names_the_default_url_is_judged_at_least_as_strictly_as_the_default() {
    // `_env` is the pool name of the IRIS_HOST connection. It is the default connection under
    // another name, and `[policy._env]` does not exist. It must not read as the strict-but-dev
    // default and run what the default's live template refuses.
    let rig = rig_with(DEFAULT_LIVE, Toolset::Merged, &[("_env", DEFAULT_URL)]);
    for server in [None, Some("_env")] {
        let mut args = json!({"code": "Write 1"});
        if let Some(s) = server {
            args["server"] = json!(s);
        }
        let r = rig.tools.call_gated("iris_execute", args).await.unwrap();
        let body = body_of(&r);
        assert_eq!(
            code_of(&body),
            "ENV_GATE_BLOCKED",
            "server={server:?}: the default's live template must hold for the same instance \
             under another name. Body: {body}"
        );
    }
}

#[tokio::test]
async fn server_padded_or_in_another_case_resolves_to_the_same_policy_or_is_not_found() {
    let rig = rig(PROD_LOCKED_WRITES_DECLARED);
    for spelling in ["prod", " prod ", "\tprod\n", "PROD", "Prod"] {
        let r = rig
            .tools
            .call_gated(
                "iris_execute",
                json!({"code": "Write 1", "server": spelling}),
            )
            .await;
        let body = match &r {
            Ok(r) => body_of(r),
            Err(e) => json!({"error": e}),
        };
        let code = code_of(&body);
        let refused_by_prod = REFUSAL_CODES.contains(&code.as_str()) && names_prod(&body);
        let not_found = body.to_string().contains("SERVER_NOT_FOUND");
        assert!(
            refused_by_prod || not_found,
            "server={spelling:?} must be judged on prod or not found, never on a default policy. \
             Body: {body}"
        );
        assert!(
            code != "IRIS_UNREACHABLE" && !body.to_string().contains("error sending request"),
            "server={spelling:?} reached the network: {body}"
        );
    }
}

#[tokio::test]
async fn an_unknown_server_is_refused_before_any_gate_and_any_request() {
    // Writes are off on the default, so a gate that ran first would answer WRITE_TOOLS_DISABLED.
    let rig = rig("write_tools_enabled = false\n");
    for server in ["nope", "  nope  "] {
        let r = rig
            .tools
            .call_gated("iris_execute", json!({"code": "Write 1", "server": server}))
            .await;
        let text = match &r {
            Ok(r) => body_of(r).to_string(),
            Err(e) => e.clone(),
        };
        assert!(
            text.contains("SERVER_NOT_FOUND"),
            "server={server:?}: an unknown server must answer SERVER_NOT_FOUND before any gate, \
             got {text}"
        );
    }
}

// ── the default instance under another name ─────────────────────────────────────────────────────

#[tokio::test]
async fn an_alias_of_the_default_follows_the_defaults_write_gate_and_can_only_close_it_further() {
    // `_env` is the IRIS_HOST connection. The operator's launch settings and the default's own
    // resolution apply to it: a write that is open on the default is open through `_env`.
    let open = rig_with("", Toolset::Merged, &[("_env", DEFAULT_URL)]);
    let r = open
        .tools
        .call_gated("iris_execute", json!({"code": "Write 1", "server": "_env"}))
        .await;
    let body = match &r {
        Ok(r) => body_of(r),
        Err(e) => json!({"error": e}),
    };
    assert!(
        !REFUSAL_CODES.contains(&code_of(&body).as_str()),
        "the default instance's writes are open through its other name: {body}"
    );

    // The member's own declaration can close it.
    let closed = rig_with(
        "[policy._env]\nwriteTools = false\n",
        Toolset::Merged,
        &[("_env", DEFAULT_URL)],
    );
    let r = closed
        .tools
        .call_gated("iris_execute", json!({"code": "Write 1", "server": "_env"}))
        .await
        .unwrap();
    assert_eq!(code_of(&body_of(&r)), "WRITE_TOOLS_DISABLED");

    // And the default's own closed gate holds through the alias.
    let off = rig_with(
        "write_tools_enabled = false\n[policy._env]\nwriteTools = true\n",
        Toolset::Merged,
        &[("_env", DEFAULT_URL)],
    );
    let r = off
        .tools
        .call_gated("iris_execute", json!({"code": "Write 1", "server": "_env"}))
        .await
        .unwrap();
    assert_eq!(code_of(&body_of(&r)), "WRITE_TOOLS_DISABLED");
}

// ── what was learned about a member does not outlive the files that decided what to trust ───────

#[tokio::test]
async fn a_configuration_reload_forgets_the_cached_system_modes() {
    use iris_agentic_dev_core::iris::config_sources::{
        ConfigSnapshot, LoadInputs, CONFIG_FILE_NAME,
    };
    use iris_agentic_dev_core::iris::connection::SystemMode;
    use iris_agentic_dev_core::tools::ConfigWatcher;
    use std::sync::Arc;

    let rig = rig_with("", Toolset::Merged, &[("prod", UNROUTABLE)]);
    let proj = rig.dir.path().join("proj");
    let project_file = proj.join(CONFIG_FILE_NAME);
    std::fs::write(&project_file, "[policy.prod]\nallow = [\"search\"]\n").unwrap();
    let inputs = LoadInputs {
        cwd: std::fs::canonicalize(&proj).unwrap(),
        home: Some(std::fs::canonicalize(rig.dir.path().join("home")).unwrap()),
        ..Default::default()
    };
    let snapshot = ConfigSnapshot::load(&inputs);
    *rig.tools.config_watcher.lock().unwrap() = ConfigWatcher::for_snapshot(&snapshot);
    rig.tools.set_config(inputs, snapshot);

    let pool = rig.tools.pool_ref();
    pool.record_mode_for_test("prod", SystemMode::Development);
    assert_eq!(
        pool.cached_system_mode("prod"),
        Some(SystemMode::Development)
    );

    // Change the operator file, which the project file's connection does not depend on, so the
    // reload keeps the pool and only the files' meaning moves.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(
        rig.dir.path().join("home").join(CONFIG_FILE_NAME),
        "[policy.prod]\nallow = [\"query\"]\n",
    )
    .unwrap();
    let _ = rig
        .tools
        .call_gated("check_config", json!({}))
        .await
        .unwrap();

    let pool_after = rig.tools.pool_ref();
    assert!(
        Arc::ptr_eq(&pool, &pool_after),
        "this test is about a reload that keeps the pool; a swapped pool forgets for free"
    );
    let pool = pool_after;
    assert_eq!(
        pool.cached_system_mode("prod"),
        None,
        "a Development answer cached under the old files must not survive a reload"
    );
}
