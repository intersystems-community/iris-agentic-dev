//! Layer 2 for spec 141 T211: over MCP stdio, `iris_generate_class` with no model configured says
//! `LLM_UNAVAILABLE` and touches nothing, and `read_only = "strict"` refuses it before the model
//! check, since it stores and compiles a class.
//!
//! Nothing listens on the default connection. A connection error is never an accepted answer.

use iris_agentic_dev_core::testing::McpSession;
use serde_json::{json, Value};

const NOTHING_LISTENS: [(&str, &str); 2] = [("IRIS_HOST", "127.0.0.1"), ("IRIS_WEB_PORT", "1")];

fn session(project: Option<&str>) -> (McpSession, tempfile::TempDir) {
    let env: Vec<(String, String)> = NOTHING_LISTENS
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let dir = tempfile::tempdir().unwrap();
    if let Some(body) = project {
        std::fs::write(dir.path().join(".iris-agentic-dev.toml"), body).unwrap();
    }
    let cwd = std::fs::canonicalize(dir.path()).unwrap();
    (McpSession::start_in(Some(&cwd), &env), dir)
}

fn text_of(answer: &Value) -> String {
    answer.to_string()
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn with_no_model_the_tool_says_llm_unavailable_and_sends_nothing() {
    let (mut s, _d) = session(None);
    let answer = s.call("iris_generate_class", &json!({"description": "a class"}));
    let text = text_of(&answer);
    assert!(text.contains("LLM_UNAVAILABLE"), "{text}");
    assert!(
        !text.contains("IRIS_UNREACHABLE") && !text.contains("error sending request"),
        "no request may reach IRIS before the model is asked: {text}"
    );
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn strict_read_only_refuses_the_tool_before_the_model_check() {
    let (mut s, _d) = session(Some("read_only = \"strict\"\n"));
    let answer = s.call("iris_generate_class", &json!({"description": "a class"}));
    let text = text_of(&answer);
    assert!(text.contains("READ_ONLY_STRICT"), "{text}");
    assert!(!text.contains("LLM_UNAVAILABLE"), "{text}");
    assert!(
        !s.list_tools().iter().any(|t| t == "iris_generate_class"),
        "a tool that stores and compiles a class is hidden under strict"
    );
}
