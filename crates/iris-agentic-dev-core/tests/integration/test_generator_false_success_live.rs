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
use iris_agentic_dev_core::testing::EnvGuard;
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

/// A name with a space used to compile and then fail at runtime inside `$Order(@gRef@(key))`, and
/// before the generator-output fix that returned an empty preview *and* a valid token, which
/// `global_kill` would then accept for a global nobody had read. Since 137 the name is refused
/// before any IRIS call. What this test holds is the property both fixes protect: no token for a
/// global that was not read.
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
        Some("INVALID_GLOBAL_NAME"),
        "the name is refused before any IRIS call: {v}"
    );
    assert!(
        v["global_name"]
            .as_str()
            .unwrap_or("")
            .contains("IAD Bad Name"),
        "the message must name what was refused: {v}"
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
    let cx = iris_agentic_dev_core::tools::exec::ExecContext::primary(
        std::sync::Arc::new(conn.clone()),
        std::sync::Arc::new(client.clone()),
    );
    let v = handle_iris_coverage(&cx, &params).await;
    assert_eq!(
        v["success"].as_bool(),
        Some(true),
        "stop must still succeed against a live monitor: {v}"
    );
    assert_eq!(v["stopped"].as_bool(), Some(true), "got {v}");
}

// ── skill_forget (docker exec path) ───────────────────────────────────────────

/// The tools, and the guard that puts `IRIS_CONTAINER` back: keep it alive for the whole test.
/// The docker path reads the variable at call time, so it has to stay set while the tools are in
/// use, and not a moment after.
fn make_tools() -> Option<(iris_agentic_dev_core::tools::IrisTools, EnvGuard)> {
    let (conn, _) = make_conn()?;
    let mut env_guard = EnvGuard::new();
    if std::env::var_os("IRIS_CONTAINER").is_none() {
        env_guard.set(
            "IRIS_CONTAINER",
            iris_agentic_dev_core::testing::LiveTarget::from_env().container,
        );
    }
    let tools = iris_agentic_dev_core::tools::IrisTools::for_test_gates_open(
        Some(conn),
        iris_agentic_dev_core::tools::Toolset::Baseline,
    )
    .expect("IrisTools::for_test_gates_open");
    Some((tools, env_guard))
}

/// A name holding a double quote used to produce `Kill ^SKILLS("a\"b")`, which the terminal rejects
/// with `<SYNTAX>` and still exits 0, so `skill_forget` answered `success: true` for a skill it had
/// not touched. Since 137 the quote is doubled, so the Kill runs on `^SKILLS("iad""probe")`: a
/// stored skill with that name is really removed, and the answer says so only after it is gone.
#[tokio::test]
#[ignore]
async fn live_skill_forget_refuses_an_unconfirmed_kill() {
    let (tools, _container_env) = match make_tools() {
        Some(t) => t,
        None => {
            eprintln!("IRIS_HOST not set — skipping live_skill_forget_refuses_an_unconfirmed_kill");
            return;
        }
    };
    let (conn, client) = make_conn().expect("connection");
    conn.execute_via_generator(
        r#"Set ^SKILLS("iad""probe")="probe|body|0|now""#,
        "USER",
        &client,
    )
    .await
    .expect("seeding ^SKILLS failed");
    let result = tools
        .call_for_test("skill_forget", serde_json::json!({"name": "iad\"probe"}))
        .await
        .expect("call_for_test");
    let v = parse_json(result);
    let check = conn
        .execute_via_generator(r#"Write $Data(^SKILLS("iad""probe"))"#, "USER", &client)
        .await
        .expect("checking ^SKILLS failed");
    assert_eq!(v["success"].as_bool(), Some(true), "got {v}");
    assert_eq!(
        check.trim(),
        "0",
        "success must mean the skill with the quote in its name is gone: {check:?}"
    );
}

/// The happy path, end to end: seed `^SKILLS`, forget it, and check IRIS agrees it is gone.
#[tokio::test]
#[ignore]
async fn live_skill_forget_removes_a_seeded_skill() {
    let (tools, _container_env) = match make_tools() {
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

// ── execute_via_generator: output that happens to be JSON must survive ────────

/// Atelier's `/action/query` does not always hand back the column value as a JSON *string*. When
/// the value parses as JSON in its own right, IRIS emits it structurally — `Write "[{""a"":1}]"`
/// comes back as `"result": [{"a": 1}]`, an array. `as_str()` on that returns `None`, so the old
/// `.unwrap_or("")` turned every JSON-emitting tool's output into an empty string and reported
/// `Ok("")`. `find_subclass_implementations` builds its whole payload that way, which is why it
/// returned zero implementations for every input regardless of the hierarchy.
#[tokio::test]
#[ignore]
async fn live_json_shaped_output_is_not_swallowed() {
    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping live_json_shaped_output_is_not_swallowed");
            return;
        }
    };

    // An array of objects: the exact shape find_subclass_implementations writes.
    let out = conn
        .execute_via_generator(
            r#"Set a=[] Set o={} Do o.%Set("class","A.B") Do a.%Push(o) Write a.%ToJSON(),!"#,
            "USER",
            &client,
        )
        .await
        .expect("generator call failed");
    let v: serde_json::Value = serde_json::from_str(out.trim())
        .unwrap_or_else(|e| panic!("output {out:?} did not parse as JSON: {e}"));
    assert_eq!(v[0]["class"], "A.B", "got {v}");

    // A bare object, and a bare number — both are valid JSON and both took the same path.
    let obj = conn
        .execute_via_generator(r#"Write "{""k"":1}",!"#, "USER", &client)
        .await
        .expect("generator call failed");
    assert!(
        obj.contains("\"k\""),
        "object-shaped output was swallowed: {obj:?}"
    );
    let num = conn
        .execute_via_generator("Write 42,!", "USER", &client)
        .await
        .expect("generator call failed");
    assert_eq!(num.trim(), "42", "numeric output was swallowed: {num:?}");
}

/// Atelier hands a result column that looks like JSON back as JSON, so output that began with `[`
/// or `{` and was not valid JSON failed with ERROR #5035 "Parsing error" and no result; before
/// 1.6.3 that read as an empty success, which is how resolve_dynamic_dispatch returned no
/// candidates when a FormalSpec held a quote. The generator now marks its result so Atelier
/// always returns it as text, valid JSON or not.
#[tokio::test]
#[ignore]
async fn output_that_looks_like_json_comes_back_as_written() {
    let Some((c, client)) = make_conn() else {
        panic!("IRIS_HOST unset; set IRIS_HOST=localhost IRIS_WEB_PORT=52780");
    };
    for (code, want) in [
        (r#"Write "[1,2""#, "[1,2"),
        (r#"Write "{""a"":1}""#, r#"{"a":1}"#),
        (r#"Write "[ 1 , 2 ]""#, "[ 1 , 2 ]"),
    ] {
        let out = c
            .execute_via_generator(code, "USER", &client)
            .await
            .unwrap_or_else(|e| panic!("{code}: {e}"));
        assert_eq!(out.trim_end(), want, "{code}");
    }
}

/// 141 T184: `make_tools` set `IRIS_CONTAINER` for the whole process and never put it back, so
/// every later test in the integration binary took the docker path. The identity-paths test
/// `l06_iris_test_runs_the_tests_as_the_service_account_with_and_without_a_container` read
/// `docker_exec_local` where it expected `atelier`, in a full run only.
#[tokio::test]
#[ignore]
async fn making_the_tools_leaves_iris_container_as_it_was() {
    let mut env_guard = EnvGuard::new();
    env_guard.unset("IRIS_CONTAINER");
    let made = make_tools().is_some();
    let after = std::env::var("IRIS_CONTAINER").ok();
    assert!(
        made,
        "IRIS_HOST unset; set IRIS_HOST=localhost IRIS_WEB_PORT=52780"
    );
    assert_eq!(
        after, None,
        "make_tools left IRIS_CONTAINER set for the rest of the process"
    );
}
