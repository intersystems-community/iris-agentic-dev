//! S7: two accounts on one IRIS host never share a CSP session.
//!
//! One cookie jar for every account on a host lets a request carry another account's
//! `CSPSESSIONID`. What IRIS does with it depends on the build and the web application, so the test
//! asserts the two things that hold either way and that a shared jar can break:
//!
//! - each call runs as the account that made it (`$USERNAME`, through the Atelier query action);
//! - each account keeps its own session: after its first call IRIS issues it no new
//!   `CSPSESSIONID`, so no call has been answered by opening a session in place of another
//!   account's. A shared jar that IRIS answers with a fresh session on every switch (and so
//!   orphans the old one) fails this.
//!
//! The control runs the same sequence through clients that share one jar and prints what IRIS did,
//! so a run records the behaviour of the build under test. It asserts nothing: whichever way a
//! build answers, the second part is what the fix guarantees.
//!
//! `lockf -k /tmp/iad-141-iris.lock cargo test --features testing --test integration primary_session_171 -- --ignored --test-threads=1`

use crate::support_identity_141::{Account, Container};
use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::iris::csp_session::{all_logout_urls, logout_blocking};
use iris_agentic_dev_core::iris::http::{client_for, ClientKind};
use iris_agentic_dev_core::testing::{ThrowawayIris, THROWAWAY_IRIS_NAME};
use std::time::Duration;

const USER_A: &str = "iadsessa";
const PW_A: &str = "Pw171sessA";
const USER_B: &str = "iadsessb";
const PW_B: &str = "Pw171sessB";

/// `$USERNAME` as the server sees the request, through the Atelier query action, and how many
/// `CSPSESSIONID` cookies the answer set.
async fn whoami(client: &reqwest::Client, conn: &IrisConnection) -> (String, usize) {
    let resp = client
        .post(conn.versioned_ns_url("USER", "/action/query"))
        .basic_auth(&conn.username, Some(&conn.password))
        .json(&serde_json::json!({"query": "SELECT $USERNAME AS U"}))
        .send()
        .await
        .expect("the query request must reach IRIS");
    let status = resp.status();
    let new_sessions = resp
        .headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter(|v| v.to_str().is_ok_and(|v| v.starts_with("CSPSESSIONID")))
        .count();
    let body: serde_json::Value = resp.json().await.unwrap_or_default();
    let user = body["result"]["content"][0]["U"]
        .as_str()
        .unwrap_or_else(|| panic!("no $USERNAME in the answer (HTTP {status}): {body}"))
        .to_string();
    (user, new_sessions)
}

#[tokio::test]
#[ignore]
async fn two_accounts_on_one_host_never_share_a_csp_session() {
    let Some(iris) = ThrowawayIris::start() else {
        return;
    };
    let c = Container::named(THROWAWAY_IRIS_NAME);
    c.wait_stable();
    let _a = Account::create(&c, USER_A, PW_A, "%DB_USER:RW,%Development:U");
    let _b = Account::create(&c, USER_B, PW_B, "%DB_USER:RW,%Development:U");

    let url = format!("http://{}:{}", iris.host, iris.web_port);
    let conn = |user: &str, pw: &str| {
        IrisConnection::new(url.clone(), "USER", user, pw, DiscoverySource::ExplicitFlag)
    };
    let (conn_a, conn_b) = (conn(USER_A, PW_A), conn(USER_B, PW_B));

    // The control: two clients on one cookie store, alternating. Printed, not asserted.
    let tls = Default::default();
    let (shared_a, shared_b) = (
        client_for(&tls, ClientKind::Call).unwrap(),
        client_for(&tls, ClientKind::Call).unwrap(),
    );
    let mut shared = Vec::new();
    for _ in 0..3 {
        shared.push(("a", whoami(&shared_a, &conn_a).await));
        shared.push(("b", whoami(&shared_b, &conn_b).await));
    }
    eprintln!("one shared cookie store (caller, ran as, new sessions): {shared:?}");

    // The fix: each connection's client holds its own account's store.
    let (client_a, client_b) = (
        conn_a.client(ClientKind::Call).unwrap(),
        conn_b.client(ClientKind::Call).unwrap(),
    );
    let mut own = Vec::new();
    for _ in 0..3 {
        own.push(("a", USER_A, whoami(&client_a, &conn_a).await));
        own.push(("b", USER_B, whoami(&client_b, &conn_b).await));
    }
    eprintln!("one store per account (caller, expected, (ran as, new sessions)): {own:?}");
    for (i, (caller, expected, (ran_as, new_sessions))) in own.iter().enumerate() {
        assert_eq!(ran_as, expected, "call {i} by {caller} ran as {ran_as}");
        // Each account's first call opens its session; no later call opens another.
        if i >= 2 {
            assert_eq!(
                *new_sessions, 0,
                "call {i} by {caller} was given a new CSP session instead of reusing its own: {own:?}"
            );
        }
    }

    // The exit logout still reaches the sessions.
    let urls = all_logout_urls();
    assert!(
        urls.iter().any(|u| u.starts_with(&format!(
            "http://{}:{}/api/atelier/",
            iris.host, iris.web_port
        ))),
        "{urls:?}"
    );
    logout_blocking(Duration::from_secs(10));
}
