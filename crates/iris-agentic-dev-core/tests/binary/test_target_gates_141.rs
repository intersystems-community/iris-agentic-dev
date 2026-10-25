//! Layer 2 for spec 141 FR-001 to FR-003, R1 section 6.2: over MCP stdio, a call that names
//! `prod` is judged by `prod`'s policy and write gates, in every spelling of its arguments.
//!
//! The project file declares two servers: the default connection (nothing listens) and `prod`,
//! a Live subject server at `127.0.0.1:1` that allows only search. A refusal has to come from a
//! gate on `prod`; a connection error means the call went past the gates.

use iris_agentic_dev_core::testing::McpSession;
use serde_json::{json, Value};

const PROJECT: &str = r#"
mode = "operate"

[instance.prod]
host = "127.0.0.1"
web_port = 1
role = "subject"

[policy.prod]
mcpTemplate = "live"
allow = ["search"]
dataPolicy = "block"
"#;

fn session() -> (McpSession, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".iris-agentic-dev.toml"), PROJECT).unwrap();
    let cwd = std::fs::canonicalize(dir.path()).unwrap();
    let s = McpSession::start_in(Some(&cwd), &[]);
    (s, dir)
}

/// The JSON body of a tool answer.
fn body(answer: &Value) -> Value {
    if let Some(v) = answer.pointer("/result/structuredContent") {
        if !v.is_null() {
            return v.clone();
        }
    }
    answer
        .pointer("/result/content/0/text")
        .and_then(Value::as_str)
        .and_then(|t| serde_json::from_str(t).ok())
        .unwrap_or_else(|| json!({"rpc": answer}))
}

/// Asks `tool` for `args`, checks the answer is a gate refusal naming `prod`, and returns its body
/// so the test can assert on what it was refused for.
fn refused_naming_prod(tool: &str, args: Value) -> Value {
    let (mut s, _d) = session();
    let answer = s.call(tool, &args);
    let b = body(&answer);
    let code = b["error_code"].as_str().unwrap_or("");
    assert!(
        iris_agentic_dev_core::policy::refusal::REFUSAL_CODES.contains(&code),
        "{tool} {args}: {code:?} is not a gate refusal. Answer: {answer}"
    );
    assert!(
        b.to_string().contains("prod"),
        "{tool} {args}: the refusal does not name prod: {b}"
    );
    b
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn iris_execute_on_prod_is_refused_by_prod() {
    let plain = refused_naming_prod("iris_execute", json!({"code": "Write 1", "server": "prod"}));
    assert_eq!(plain["success"], false, "{plain}");
    let padded = refused_naming_prod(
        "iris_execute",
        json!({"code": "Write 1", "server": " prod ", "confirmed": true}),
    );
    assert_eq!(padded["success"], false, "{padded}");
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn iris_doc_put_on_prod_is_refused_by_prod_in_both_spellings_of_mode() {
    for key in ["mode", "action"] {
        let mut args =
            json!({"name": "Sentinel141.cls", "content": "Class Sentinel141 {}", "server": "prod"});
        args[key] = json!("put");
        // `action` is not a parameter of iris_doc; the gate has to read it anyway.
        let b = refused_naming_prod("iris_doc", args);
        assert_eq!(b["success"], false, "{key}=put: {b}");
    }
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn iris_global_kill_on_prod_is_refused_in_every_spelling() {
    for action_key in ["action", "mode"] {
        for global_key in ["global_name", "globalName", "global"] {
            for confirm_key in ["confirm", "confirmed"] {
                let mut args = json!({"server": "prod"});
                args[action_key] = json!("kill");
                args[global_key] = json!("^Sentinel141");
                args[confirm_key] = json!(true);
                let b = refused_naming_prod("iris_global", args.clone());
                assert_eq!(
                    b["success"], false,
                    "{action_key}/{global_key}/{confirm_key}: {args} {b}"
                );
            }
        }
    }
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn global_kill_on_prod_is_refused() {
    for global_key in ["global", "global_name", "globalName"] {
        let mut args = json!({"server": "prod"});
        args[global_key] = json!("^Sentinel141");
        let b = refused_naming_prod("global_kill", args);
        assert_eq!(b["success"], false, "{global_key}: {b}");
    }
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn a_caller_cannot_loosen_prods_data_policy_with_an_argument() {
    for value in ["allow", "Allow", "redact"] {
        let key = "dataPolicy";
        let mut args = json!({"server": "prod", "acknowledgePhi": true});
        args[key] = json!(value);
        let (mut s, _d) = session();
        let answer = s.call("iris_message_body", &args);
        let b = body(&answer);
        assert_eq!(
            b["error_code"], "DATA_POLICY_BLOCKED",
            "{key}={value}: prod blocks bulk-PHI reads and the caller's argument can only narrow: {answer}"
        );
        assert!(b.to_string().contains("prod"), "{b}");
    }
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn check_config_lists_prods_effective_policy_gates_and_errors() {
    let (mut s, _d) = session();
    let answer = s.call("check_config", &json!({}));
    let b = body(&answer);
    let prod = &b["effective_policy"]["prod"];
    assert!(prod.is_object(), "no effective_policy.prod in {b}");
    assert_eq!(prod["mcpTemplate"]["value"], "live", "{prod}");
    assert_eq!(prod["allow"]["value"], json!(["search"]), "{prod}");
    assert_eq!(prod["dataPolicy"]["value"], "block", "{prod}");
    assert_eq!(prod["role"]["value"], "subject", "{prod}");
    // Nothing declared writeTools and nothing has probed prod: write-closed, and the report says why.
    assert_eq!(prod["writeTools"]["value"], false, "{prod}");
    assert_eq!(prod["writeTools"]["from"], "system_mode", "{prod}");
    assert_eq!(prod["destructiveTools"]["value"], false, "{prod}");
    assert!(b["config_errors"].is_array(), "config_errors missing: {b}");
}
