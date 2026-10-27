//! S16 and S17, through `call_gated`.
//!
//! S16: a gate that decided to refuse and then failed while building the refusal used to let the
//! call through. It refuses now.
//!
//! S17: the tools that change where later calls go (`iris_select_container`, `iris_start_sandbox`,
//! `iris_containers`, `iris_add_server` and their three siblings) meet `allow` and the environment
//! template, judged by the default server's policy. Nothing listens at the rig's addresses, so a
//! call that got past a gate would fail with a connection error or a not-found, never with a
//! refusal code.

use crate::support_gate_141::*;
use iris_agentic_dev_core::testing::fail_refusal_builds;
use iris_agentic_dev_core::tools::Toolset;
use serde_json::{json, Value};

/// The tools a connection change goes through. Every parameter of them is optional, so `{}` passes
/// the schema check and the gates are what answers.
const CONNECTION_TOOLS: &[&str] = &[
    "iris_select_container",
    "iris_start_sandbox",
    "iris_containers",
    "iris_add_server",
    "iris_remove_server",
    "iris_import_servers",
    "iris_reload_pool",
];

async fn answer(rig: &Rig, tool: &str, args: Value) -> Result<Value, String> {
    rig.tools.call_gated(tool, args).await.map(|r| {
        let mut body = body_of(&r);
        body["is_error"] = json!(r.is_error.unwrap_or(false));
        body
    })
}

// ── S16 ─────────────────────────────────────────────────────────────────────────────────────────

/// `prod` is a live subject server that allows only search: an `iris_compile` against it meets the
/// environment template, then `allow`, before it meets anything else.
#[tokio::test]
async fn a_refusal_that_cannot_be_built_is_still_a_refusal() {
    let rig = rig(PROD_LOCKED_WRITES_DECLARED);
    let args = json!({"target": "Pkg.Cls.cls", "server": "prod"});

    // The control: with the refusal buildable, the call is refused by the environment template.
    let body = answer(&rig, "iris_compile", args.clone()).await.unwrap();
    assert_eq!(body["error_code"], "ENV_GATE_BLOCKED", "{body}");

    // The refusal fails to build. The call must still not run: no result that is not an error,
    // and no connection error, which would mean the handler was reached.
    fail_refusal_builds(true);
    let out = answer(&rig, "iris_compile", args).await;
    fail_refusal_builds(false);
    match out {
        Err(e) => assert!(
            !e.to_lowercase().contains("connect") && e.contains("refus"),
            "the call errored for another reason: {e}"
        ),
        Ok(body) => panic!("the call went on after a gate refused it: {body}"),
    }
}

/// `prod` is a subject server with every gate open and no `allow`: the only thing between an
/// `iris_doc` put and the network is the role gate's `confirm`.
const PROD_SUBJECT_OPEN: &str = r#"
mode = "operate"

[instance.prod]
host = "127.0.0.1"
web_port = 1
role = "subject"

[policy.prod]
mcpTemplate = "dev"
writeTools = true
destructiveTools = true
"#;

#[tokio::test]
async fn the_role_gate_refuses_when_its_refusal_cannot_be_built() {
    let rig = rig(PROD_SUBJECT_OPEN);
    let args = json!({"name": "Pkg.Cls.cls", "mode": "put", "content": "Class Pkg.Cls {}", "server": "prod"});
    let body = answer(&rig, "iris_doc", args.clone()).await.unwrap();
    assert_eq!(
        body["error_code"], "SUBJECT_CONFIRM_REQUIRED",
        "control: {body}"
    );
    fail_refusal_builds(true);
    let out = answer(&rig, "iris_doc", args).await;
    fail_refusal_builds(false);
    match out {
        Err(e) => assert!(e.contains("refus"), "{e}"),
        Ok(body) => panic!("the call went on after the role gate refused it: {body}"),
    }
}

// ── S17 ─────────────────────────────────────────────────────────────────────────────────────────

/// Every write and destructive gate open, so the refusal under test is the one that is left.
const GATES_OPEN: &str = "write_tools_enabled = true\ndestructive_tools_enabled = true\n";

/// True when a call got past the policy gates. `call_gated` cannot dispatch the container tools
/// (they are MCP-only), so a call that passed ends in "unknown tool", an error and not a body.
fn passed(out: &Result<Value, String>) -> bool {
    match out {
        Err(e) => e.starts_with("unknown tool"),
        Ok(body) => !matches!(
            body["error_code"].as_str(),
            Some("POLICY_GATE") | Some("ENV_GATE_BLOCKED")
        ),
    }
}

/// A rig for each toolset that registers the tool: the consolidated `iris_containers` is in Merged
/// only, `iris_select_container` and `iris_start_sandbox` in Baseline only.
fn rigs_for(toml: &str, tool: &str) -> Vec<Rig> {
    let mut out = Vec::new();
    for toolset in [Toolset::Baseline, Toolset::Merged] {
        let rig = rig_with(toml, toolset, &[("prod", UNROUTABLE)]);
        if rig.tools.registered_tool_names().contains(tool) {
            out.push(rig);
        }
    }
    assert!(!out.is_empty(), "{tool} is registered in no toolset");
    out
}

#[tokio::test]
async fn the_connection_tools_meet_allow() {
    for tool in CONNECTION_TOOLS {
        for rig in rigs_for(
            &format!("{GATES_OPEN}[policy.default]\nallow = [\"query\", \"search\"]\n"),
            tool,
        ) {
            let body = answer(&rig, tool, json!({})).await.unwrap();
            assert_eq!(body["error_code"], "POLICY_GATE", "{tool}: {body}");
            assert_eq!(body["blocked_category"], "admin", "{tool}: {body}");
            assert_eq!(body["refusal"]["kind"], "operator_limit", "{tool}: {body}");
        }
    }
}

#[tokio::test]
async fn the_connection_tools_meet_the_environment_template() {
    for template in ["live", "test"] {
        for tool in CONNECTION_TOOLS {
            let toml = format!("{GATES_OPEN}[policy.default]\nmcpTemplate = \"{template}\"\n");
            for rig in rigs_for(&toml, tool) {
                let body = answer(&rig, tool, json!({})).await.unwrap();
                assert_eq!(
                    body["error_code"], "ENV_GATE_BLOCKED",
                    "{template} {tool}: {body}"
                );
            }
        }
    }
}

#[tokio::test]
async fn the_default_servers_policy_is_the_one_that_judges_them() {
    // `prod` locks itself down; the default server's policy is open. The call names no server, and
    // the tool is judged by the default's.
    let rig = rig_with(
        &format!("{GATES_OPEN}{PROD_LOCKED}"),
        Toolset::Baseline,
        &[("prod", UNROUTABLE)],
    );
    let out = answer(
        &rig,
        "iris_select_container",
        json!({"name": "no-such-container"}),
    )
    .await;
    assert!(
        passed(&out),
        "prod's policy judged a call that names no server: {out:?}"
    );
}

#[tokio::test]
async fn an_operator_who_allows_admin_keeps_the_connection_tools() {
    let rig = rig_with(
        &format!("{GATES_OPEN}[policy.default]\nallow = [\"admin\"]\n"),
        Toolset::Baseline,
        &[("prod", UNROUTABLE)],
    );
    let out = answer(
        &rig,
        "iris_select_container",
        json!({"name": "no-such-container"}),
    )
    .await;
    assert!(passed(&out), "{out:?}");
}

#[tokio::test]
async fn the_tools_that_only_read_local_state_are_not_gated_by_allow() {
    // Skills, the knowledge base and the server list stay available under a narrow `allow`.
    let rig = rig("[policy.default]\nallow = [\"query\"]\n");
    for tool in ["skill_list", "kb_recall", "check_config", "iris_servers"] {
        let args = if tool == "kb_recall" {
            json!({"query": "x"})
        } else {
            json!({})
        };
        let out = answer(&rig, tool, args).await;
        if let Ok(body) = out {
            assert_ne!(body["error_code"], "POLICY_GATE", "{tool}: {body}");
        }
    }
}

// ── R6 ──────────────────────────────────────────────────────────────────────────────────────────

/// `iris_query` `mode="write"` refuses a DML statement it cannot count, whatever `max_rows_affected`
/// the caller sends, before any request reaches IRIS. Nothing listens at the rig's addresses, so a
/// statement that got past the refusal would fail with a connection error instead.
#[tokio::test]
async fn an_uncountable_statement_is_refused_in_every_default_run() {
    let rig = rig(GATES_OPEN);
    let mut client = McpClient::start(rig.tools.clone()).await;
    for query in [
        "TRUNCATE t",
        "TRUNCATE TABLE a, b",
        "DELETE FROM t1, t2",
        "DELETE FROM t JOIN u ON 1 = 1",
        "UPDATE t1, t2 SET a = 1",
        "UPDATE t (a) VALUES (1)",
        "DELETE %BOGUS FROM t",
        "DELETE %NOUNIQCHECK FROM t",
        "DELETE %STARTTABLE FROM t",
        "TRUNCATE TABLE %NOCHECK t",
    ] {
        // No cap sent, a cap, and `force`, which does not lift it.
        for extra in [
            json!({}),
            json!({"max_rows_affected": 5}),
            json!({"max_rows_affected": 5, "force": true}),
        ] {
            let mut args = json!({"query": query, "mode": "write"});
            for (k, v) in extra.as_object().unwrap() {
                args[k] = v.clone();
            }
            let body = client.call("iris_query", args).await;
            assert_eq!(body["error_code"], "ROWS_NOT_COUNTABLE", "{query}: {body}");
            assert_eq!(body["refusal"]["kind"], "fixed_rule", "{query}: {body}");
        }
    }
    // The control: a statement the counter can read gets past the refusal and fails on the
    // connection instead.
    let body = client
        .call(
            "iris_query",
            json!({"query": "DELETE FROM t WHERE a = 1", "mode": "write"}),
        )
        .await;
    assert_ne!(body["error_code"], "ROWS_NOT_COUNTABLE", "{body}");
}

// ── R9 ──────────────────────────────────────────────────────────────────────────────────────────

/// A tool that cannot build the HTTP client for its server answers with that error. It does not
/// fall back to the client built at startup, whose certificate setting and CSP session belong to
/// the default connection and not to the server the call named.
#[tokio::test]
async fn a_call_that_cannot_build_its_client_errors() {
    let rig = rig(GATES_OPEN);
    let calls = [
        ("iris_table_info", json!({"table": "A.B"})),
        ("iris_info", json!({"what": "namespace"})),
        ("iris_query", json!({"query": "SELECT 1"})),
        (
            "iris_query",
            json!({"query": "DELETE FROM t WHERE a = 1", "mode": "write"}),
        ),
    ];
    for (tool, args) in calls {
        iris_agentic_dev_core::testing::fail_client_builds(true);
        let out = answer(&rig, tool, args.clone()).await;
        iris_agentic_dev_core::testing::fail_client_builds(false);
        let text = match out {
            Err(e) => e,
            Ok(body) => body.to_string(),
        };
        assert!(
            text.contains("could not build the HTTP client"),
            "{tool}: the call went on with another client: {text}"
        );
        // The control: with client builds working, the same call reaches the (absent) server.
        let text = match answer(&rig, tool, args).await {
            Err(e) => e,
            Ok(body) => body.to_string(),
        };
        assert!(
            !text.contains("could not build the HTTP client"),
            "{tool}: {text}"
        );
    }
}
