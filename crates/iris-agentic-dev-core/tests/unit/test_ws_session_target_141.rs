//! Spec 141 R1 D3: `iris_ws_exec` is judged on the server its session was opened against.
//!
//! The token is text the caller sends back. Gating on the server it names, or on the default
//! connection, lets a session opened on one server run code under the policy of another.

use crate::support_gate_141::*;
use serde_json::json;

async fn exec(rig: &Rig, token: &str) -> serde_json::Value {
    match rig
        .tools
        .call_gated("iris_ws_exec", json!({"session": token, "code": "Write 1"}))
        .await
    {
        Ok(r) => body_of(&r),
        Err(e) => json!({"error": e}),
    }
}

#[tokio::test]
async fn a_token_that_names_dev_but_was_opened_on_prod_is_gated_on_prod() {
    let rig = rig(PROD_LOCKED);
    // The text says `dev`, whose writes are open. The entry says `prod`, whose are not.
    let token = "ws:dev:USER:0000-forged-text";
    rig.tools.ws_pool.record_target(token, Some("prod"));
    let body = exec(&rig, token).await;
    assert_eq!(code_of(&body), "WRITE_TOOLS_DISABLED", "{body}");
    assert!(body.to_string().contains("prod"), "{body}");
}

#[tokio::test]
async fn a_token_that_names_prod_but_was_opened_on_the_default_is_gated_on_the_default() {
    let rig = rig(PROD_LOCKED);
    let token = "ws:prod:USER:0000-forged-text";
    rig.tools.ws_pool.record_target(token, None);
    let body = exec(&rig, token).await;
    assert_ne!(
        code_of(&body),
        "WRITE_TOOLS_DISABLED",
        "the session belongs to the default connection, whose writes are open: {body}"
    );
    assert!(
        !matches!(code_of(&body).as_str(), "POLICY_GATE" | "ENV_GATE_BLOCKED"),
        "prod's policy must not apply to a session on the default: {body}"
    );
}

#[tokio::test]
async fn close_is_judged_on_the_session_entry_too() {
    let rig = rig("[policy.prod]\nallow = [\"search\"]\n");
    let token = "ws:dev:USER:0000-forged-text";
    rig.tools.ws_pool.record_target(token, Some("prod"));
    let body = match rig
        .tools
        .call_gated("iris_ws_close", json!({"session": token}))
        .await
    {
        Ok(r) => body_of(&r),
        Err(e) => json!({"error": e}),
    };
    assert_eq!(code_of(&body), "POLICY_GATE", "{body}");
    assert!(body.to_string().contains("prod"), "{body}");
}

#[tokio::test]
async fn a_token_no_session_holds_is_gated_on_the_default() {
    let rig = rig("write_tools_enabled = false\n");
    let body = exec(&rig, "ws:prod:USER:never-opened").await;
    assert_eq!(code_of(&body), "WRITE_TOOLS_DISABLED", "{body}");
}
