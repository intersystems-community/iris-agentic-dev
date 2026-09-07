//! Layer 2 for #127's TLS trust settings.
//!
//! The unit tests cover the resolver's truth table and the TOML round-trip in-process. These spawn
//! the shipped binary and ask it, over JSON-RPC, whether it is validating certificates — the #111
//! pattern, where a setting parses and is never read by the running server.
//!
//! `check_config` is the only place an operator can see this. It makes no network calls, so a
//! connection with validation switched off looks exactly like a healthy one until a cert goes bad
//! and nothing complains.
//!
//! No IRIS needed.

use iris_agentic_dev_core::testing::{answer_text, require_iad_binary, McpSession};

fn body(answer: &serde_json::Value) -> serde_json::Value {
    let result = answer
        .get("result")
        .unwrap_or_else(|| panic!("expected a result, got: {}", answer_text(answer)));
    if let Some(sc) = result.get("structuredContent") {
        return sc.clone();
    }
    let text = result
        .pointer("/content/0/text")
        .and_then(|t| t.as_str())
        .unwrap_or_else(|| panic!("no structuredContent and no text: {}", answer_text(answer)));
    serde_json::from_str(text).unwrap_or_else(|e| panic!("content is not JSON ({e}): {text}"))
}

/// `check_config`'s `tls_verify`, from a server started with the given env.
fn reported_tls_verify(env: &[(String, String)]) -> serde_json::Value {
    let answer = McpSession::start(env).call("check_config", &serde_json::json!({}));
    let body = body(&answer);
    body.get("tls_verify")
        .cloned()
        .unwrap_or_else(|| panic!("check_config reported no tls_verify: {body}"))
}

fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn a_server_with_nothing_declared_reports_validation_on() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    assert_eq!(
        reported_tls_verify(&[]),
        serde_json::json!(true),
        "validating is the default; anything else has to be asked for"
    );
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn iris_tls_verify_false_is_visible_in_check_config() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    assert_eq!(
        reported_tls_verify(&env(&[("IRIS_TLS_VERIFY", "false")])),
        serde_json::json!(false),
    );
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn iris_insecure_is_visible_in_check_config_too() {
    // The blunter of the two variables. Reported through the same resolver, so an operator who set
    // one variable and forgot cannot be told validation is on when it is not.
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    assert_eq!(
        reported_tls_verify(&env(&[("IRIS_INSECURE", "true")])),
        serde_json::json!(false),
    );
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn a_workspace_config_declaring_tls_verify_false_reaches_the_running_server() {
    // The #127 ask: express this per project instead of in the process environment. The config is
    // read from the watched workspace root, so point the server at a temp dir holding one.
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let dir = tempfile::TempDir::new().expect("tempdir");
    std::fs::write(
        dir.path().join(".iris-agentic-dev.toml"),
        "host = \"localhost\"\nweb_port = 9443\nscheme = \"https\"\ntls_verify = false\n",
    )
    .expect("write toml");

    let got = reported_tls_verify(&env(&[(
        "OBJECTSCRIPT_WORKSPACE",
        dir.path().to_str().unwrap(),
    )]));
    assert_eq!(
        got,
        serde_json::json!(false),
        "tls_verify = false in .iris-agentic-dev.toml must reach the running server, not just parse"
    );
}
