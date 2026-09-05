//! Live-IRIS half of the "generator failure reported as success" regressions.
//!
//! The unit tests in `tests/unit/test_generator_false_success.rs` pin the decision each site makes.
//! These run the sites against `iris-dev-iris` so the failure shapes are the ones IRIS actually
//! produces, and so the fixes are shown not to have broken the happy path.
//!
//! Run with:
//!   IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS \
//!     IRIS_CONTAINER=iris-dev-iris cargo test -p iris-agentic-dev-core --features testing \
//!     --test test_generator_false_success_live -- --include-ignored --test-threads=1 --nocapture

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use std::sync::Arc;

fn make_conn() -> Option<(IrisConnection, reqwest::Client)> {
    let iris_host = std::env::var("IRIS_HOST").unwrap_or_default();
    if iris_host.is_empty() {
        return None;
    }
    let web_port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".to_string());
    let username = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string());
    let password = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".to_string());
    let conn = IrisConnection::new(
        format!("http://{iris_host}:{web_port}"),
        "USER",
        username,
        password,
        DiscoverySource::EnvVar,
    );
    Some((conn, reqwest::Client::new()))
}

fn parse_json(r: rmcp::model::CallToolResult) -> serde_json::Value {
    let text = r
        .content
        .first()
        .map(|c| c.as_text().unwrap().text.clone())
        .expect("no text content");
    serde_json::from_str(&text).expect("json parse failed")
}

// ── global_preview: a failed preview must not mint a confirm_token ────────────

/// `global_preview` never validates the global name, so a name with a space compiles fine and then
/// fails at runtime inside `$Order(@gRef@(key))`. That is a real IRIS failure delivered the way the
/// bug required: `Ok(out)` holding `ERROR: <SYNTAX>...`. Before the fix this returned an empty
/// preview *and* a valid token, which `global_kill` would then accept for a global nobody had read.
#[tokio::test]
#[ignore]
async fn live_failed_preview_mints_no_confirm_token() {
    use iris_agentic_dev_core::tools::admin_tools::{global_preview_impl, GlobalPreviewParams};
    use tokio::sync::Mutex;

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping live_failed_preview_mints_no_confirm_token");
            return;
        }
    };
    let confirm_tokens = Mutex::new(std::collections::HashMap::new());

    let result = global_preview_impl(
        GlobalPreviewParams {
            global: "IAD Bad Name".to_string(),
            server: None,
            count: 5,
            iris: Arc::new(conn),
            client: Arc::new(client),
        },
        &confirm_tokens,
    )
    .await
    .expect("global_preview_impl must return a result, not an McpError");
    let v = parse_json(result);

    assert_eq!(
        v["success"].as_bool(),
        Some(false),
        "a preview that failed in IRIS must not report success: {v}"
    );
    assert_eq!(
        v["error_code"].as_str(),
        Some("IRIS_EXECUTE_ERROR"),
        "the refusal must come from the generator-output check, not from some earlier bail: {v}"
    );
    assert!(
        v["error"].as_str().unwrap_or("").contains("IAD Bad Name"),
        "the message must name the global that could not be read: {v}"
    );
    assert!(
        v["confirm_token"].is_null(),
        "no confirm_token may be issued for a global that was never read: {v}"
    );
    assert!(
        confirm_tokens.lock().await.is_empty(),
        "no token may be stored either — global_kill checks the map, not the response"
    );
}

/// The same call on a global that exists must still work, token and all.
#[tokio::test]
#[ignore]
async fn live_successful_preview_still_mints_a_token() {
    use iris_agentic_dev_core::tools::admin_tools::{global_preview_impl, GlobalPreviewParams};
    use tokio::sync::Mutex;

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping live_successful_preview_still_mints_a_token");
            return;
        }
    };
    let iris = Arc::new(conn);
    let client = Arc::new(client);
    let confirm_tokens = Mutex::new(std::collections::HashMap::new());

    let seed = r#"Set ^IADPreviewProbe("row1")="value1""#;
    iris.execute_via_generator(seed, &iris.namespace, &client)
        .await
        .expect("seeding ^IADPreviewProbe failed");

    let result = global_preview_impl(
        GlobalPreviewParams {
            global: "IADPreviewProbe".to_string(),
            server: None,
            count: 5,
            iris: Arc::clone(&iris),
            client: Arc::clone(&client),
        },
        &confirm_tokens,
    )
    .await
    .expect("global_preview_impl failed");
    let v = parse_json(result);

    let _ = iris
        .execute_via_generator(
            r#"Kill ^IADPreviewProbe Write "cleaned""#,
            &iris.namespace,
            &client,
        )
        .await;

    assert_eq!(v["success"].as_bool(), Some(true), "got {v}");
    assert!(
        v["confirm_token"].as_str().is_some_and(|t| !t.is_empty()),
        "a preview that read the global must still issue a token: {v}"
    );
    assert_eq!(v["total_subscripts"].as_u64(), Some(1), "got {v}");
}

// ── my_access / capability_matrix: the $USERNAME read ─────────────────────────

#[tokio::test]
#[ignore]
async fn live_my_access_reports_the_real_username() {
    use iris_agentic_dev_core::tools::admin_tools::my_access_impl;

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping live_my_access_reports_the_real_username");
            return;
        }
    };
    let v = parse_json(
        my_access_impl(&conn, &client)
            .await
            .expect("my_access_impl"),
    );
    assert_eq!(v["success"].as_bool(), Some(true), "got {v}");
    let expected = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string());
    assert_eq!(
        v["username"].as_str().map(str::to_uppercase),
        Some(expected.to_uppercase()),
        "the username must come from IRIS, not from a failure string: {v}"
    );
    assert!(
        !v["roles"].as_array().expect("roles array").is_empty(),
        "an empty role set for _SYSTEM is the shape the bug produced: {v}"
    );
}

#[tokio::test]
#[ignore]
async fn live_capability_matrix_reports_the_real_username() {
    use iris_agentic_dev_core::tools::admin_tools::capability_matrix_impl;

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!(
                "IRIS_HOST not set — skipping live_capability_matrix_reports_the_real_username"
            );
            return;
        }
    };
    let v = parse_json(
        capability_matrix_impl(&conn, &client, None)
            .await
            .expect("capability_matrix_impl"),
    );
    assert_eq!(v["success"].as_bool(), Some(true), "got {v}");
    assert!(
        !v["roles"].as_array().expect("roles array").is_empty(),
        "resolved user must come back with real roles: {v}"
    );
}

// ── iris_admin list_user_roles ────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn live_list_user_roles_decodes_real_roles() {
    use iris_agentic_dev_core::tools::admin::admin_list_user_roles_impl;

    let (conn, _client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping live_list_user_roles_decodes_real_roles");
            return;
        }
    };
    let user = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string());
    let v = parse_json(
        admin_list_user_roles_impl(Some(&conn), &user)
            .await
            .expect("admin_list_user_roles_impl"),
    );
    assert_eq!(v["success"].as_bool(), Some(true), "got {v}");
    let roles: Vec<&str> = v["roles"]
        .as_array()
        .expect("roles array")
        .iter()
        .filter_map(|r| r.as_str())
        .collect();
    assert!(
        roles.iter().any(|r| r.starts_with('%')),
        "real IRIS roles start with %: {roles:?}"
    );
    assert!(
        !roles.iter().any(|r| r.contains('<')),
        "an IRIS error signature in the roles list is the bug: {roles:?}"
    );

    // A user that does not exist keeps its specific code rather than the generic one.
    let v = parse_json(
        admin_list_user_roles_impl(Some(&conn), "iad-no-such-user")
            .await
            .expect("admin_list_user_roles_impl"),
    );
    assert_eq!(v["error_code"].as_str(), Some("USER_NOT_FOUND"), "got {v}");
}

// ── iris_coverage mode=stop ───────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn live_coverage_stop_confirms_the_stop() {
    use iris_agentic_dev_core::tools::coverage::{handle_iris_coverage, IrisCoverageParams};

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping live_coverage_stop_confirms_the_stop");
            return;
        }
    };
    let params: IrisCoverageParams = serde_json::from_value(serde_json::json!({
        "mode": "stop",
        "namespace": "USER",
    }))
    .expect("params");
    let v = handle_iris_coverage(&conn, &client, &params).await;
    assert_eq!(
        v["success"].as_bool(),
        Some(true),
        "stop must still succeed against a live monitor: {v}"
    );
    assert_eq!(v["stopped"].as_bool(), Some(true), "got {v}");
}

// ── skill_forget (docker exec path) ───────────────────────────────────────────

fn make_tools() -> Option<iris_agentic_dev_core::tools::IrisTools> {
    let (conn, _) = make_conn()?;
    if std::env::var("IRIS_CONTAINER").is_err() {
        std::env::set_var("IRIS_CONTAINER", "iris-dev-iris");
    }
    Some(iris_agentic_dev_core::tools::IrisTools::new(Some(conn)).expect("IrisTools::new"))
}

/// A name holding a double quote produces `Kill ^SKILLS("a\"b")`, which the terminal rejects with
/// `<SYNTAX>` — and still exits 0, so `iris.execute` returns `Ok`. Before the fix `skill_forget`
/// only checked `is_ok()` and answered `success: true` for a skill it had not touched.
#[tokio::test]
#[ignore]
async fn live_skill_forget_refuses_an_unconfirmed_kill() {
    let tools = match make_tools() {
        Some(t) => t,
        None => {
            eprintln!("IRIS_HOST not set — skipping live_skill_forget_refuses_an_unconfirmed_kill");
            return;
        }
    };
    let result = tools
        .call_for_test("skill_forget", serde_json::json!({"name": "iad\"probe"}))
        .await
        .expect("call_for_test");
    let v = parse_json(result);
    assert_eq!(
        v["success"].as_bool(),
        Some(false),
        "a Kill IRIS refused must not be reported as a removal: {v}"
    );
    assert_eq!(
        v["error_code"].as_str(),
        Some("IRIS_EXECUTE_ERROR"),
        "got {v}"
    );
}

/// The happy path, end to end: seed `^SKILLS`, forget it, and check IRIS agrees it is gone.
#[tokio::test]
#[ignore]
async fn live_skill_forget_removes_a_seeded_skill() {
    let tools = match make_tools() {
        Some(t) => t,
        None => {
            eprintln!("IRIS_HOST not set — skipping live_skill_forget_removes_a_seeded_skill");
            return;
        }
    };
    let (conn, client) = make_conn().expect("connection");
    let seed = r#"Set ^SKILLS("iad-forget-probe")="probe|body|0|now""#;
    conn.execute_via_generator(seed, "USER", &client)
        .await
        .expect("seeding ^SKILLS failed");

    let result = tools
        .call_for_test(
            "skill_forget",
            serde_json::json!({"name": "iad-forget-probe"}),
        )
        .await
        .expect("call_for_test");
    let v = parse_json(result);
    assert_eq!(v["success"].as_bool(), Some(true), "got {v}");

    let check = conn
        .execute_via_generator(
            r#"Write $Data(^SKILLS("iad-forget-probe"))"#,
            "USER",
            &client,
        )
        .await
        .expect("checking ^SKILLS failed");
    assert_eq!(
        check.trim(),
        "0",
        "the skill must actually be gone from ^SKILLS, got: {check:?}"
    );
}
