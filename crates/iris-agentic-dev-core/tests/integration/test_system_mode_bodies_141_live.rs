//! T-R4a-L02: what the `SystemMode` query really answers, on `iris-dev-iris`.
//!
//! `_SYSTEM` reads an instance whose `^%SYS("SystemMode")` was never set and gets one row with an
//! empty value: `Unset`. An account that cannot log in to the Atelier API gets HTTP 401 and no
//! body: `Unknown`, which the write gates must not read as "development". The answers are saved
//! under `tests/fixtures/system_mode_141/` and `test_template_roles_141.rs` parses those files.
//!
//! The scratch account is created by the test and dropped by it, pass or fail.
//! `IAD_REGEN_FIXTURES=1` rewrites the fixtures instead of comparing against them.

use iris_agentic_dev_core::iris::connection::{parse_system_mode_response, SystemMode};
use std::path::PathBuf;

const SCRATCH_USER: &str = "iad141scratch";
const SCRATCH_PW: &str = "Scratch141pw";
const QUERY: &str = "SELECT Value FROM %Library.Global_Get('%SYS', '^%SYS(\"SystemMode\")')";

fn base() -> String {
    let host = std::env::var("IRIS_HOST").unwrap_or_else(|_| "localhost".into());
    let port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".into());
    format!("http://{host}:{port}")
}

fn admin() -> (String, String) {
    (
        std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into()),
        std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into()),
    )
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/system_mode_141")
        .join(name)
}

async fn ask(user: &str, pw: &str, sql: &str) -> (u16, String) {
    let r = reqwest::Client::new()
        .post(format!("{}/api/atelier/v8/%25SYS/action/query", base()))
        .basic_auth(user, Some(pw))
        .json(&serde_json::json!({ "query": sql }))
        .send()
        .await
        .expect("the live IRIS must answer");
    (r.status().as_u16(), r.text().await.unwrap_or_default())
}

/// Drops the scratch account when it goes out of scope, so a failed assertion leaves nothing.
struct Scratch;
impl Drop for Scratch {
    fn drop(&mut self) {
        let (u, p) = admin();
        let url = format!("{}/api/atelier/v8/%25SYS/action/query", base());
        let sql = format!("DROP USER {SCRATCH_USER}");
        let _ = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                let _ = reqwest::Client::new()
                    .post(url)
                    .basic_auth(u, Some(p))
                    .json(&serde_json::json!({ "query": sql }))
                    .send()
                    .await;
            });
        })
        .join();
    }
}

fn check_fixture(name: &str, status: u16, body: &str) {
    let path = fixture(name);
    let captured = serde_json::json!({ "status": status, "body": body });
    if std::env::var("IAD_REGEN_FIXTURES").is_ok_and(|v| v == "1") {
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&captured).unwrap() + "\n",
        )
        .unwrap();
        return;
    }
    let saved: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())),
    )
    .unwrap();
    assert_eq!(
        saved, captured,
        "{name} no longer matches what IRIS answers; rerun with IAD_REGEN_FIXTURES=1 and review"
    );
}

#[tokio::test]
#[ignore = "requires live IRIS (iris-dev-iris) and creates, then drops, a scratch account"]
async fn the_system_mode_query_answers_unset_for_the_admin_and_unknown_for_an_account_with_no_roles(
) {
    let (u, p) = admin();

    // Make sure a leftover from an aborted run cannot colour the result.
    let _ = ask(&u, &p, &format!("DROP USER {SCRATCH_USER}")).await;
    let _guard = Scratch;
    let (st, body) = ask(
        &u,
        &p,
        &format!("CREATE USER {SCRATCH_USER} IDENTIFY BY '{SCRATCH_PW}'"),
    )
    .await;
    assert_eq!(st, 200, "creating the scratch account failed: {body}");

    let (st_sys, body_sys) = ask(&u, &p, QUERY).await;
    assert_eq!(
        parse_system_mode_response(st_sys, &body_sys),
        SystemMode::Unset
    );
    check_fixture("as_system.json", st_sys, &body_sys);

    let (st_no, body_no) = ask(SCRATCH_USER, SCRATCH_PW, QUERY).await;
    assert!(
        !body_no.contains("\"result\""),
        "an account that cannot log in to the API gets no answer: {st_no} {body_no}"
    );
    assert_eq!(
        parse_system_mode_response(st_no, &body_no),
        SystemMode::Unknown,
        "an account that cannot read the answer must not read as an unset (development) instance"
    );
    // The committed capture is what `test_template_roles_141` parses. It is rewritten, not compared,
    // because the status for this account (401 or an empty 200) depends on the web application's
    // authentication settings, which other live tests change.
    if std::env::var("IAD_REGEN_FIXTURES").is_ok_and(|v| v == "1") {
        check_fixture("as_no_roles.json", st_no, &body_no);
    }
}
