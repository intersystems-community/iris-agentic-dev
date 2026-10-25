//! S7: two accounts on one IRIS host never share a CSP session.
//!
//! One cookie jar for every account on a host lets a request carry another account's
//! `CSPSESSIONID`. What IRIS does with such a request depends on the build and the web
//! application, so the test asserts what the client side guarantees on every build:
//!
//! - each call runs as the account that made it (`$USERNAME`, through the Atelier query action);
//! - the cookie store each account's client holds never contains the other account's
//!   `CSPSESSIONID`, so no request of one carries a session the other opened.
//!
//! The control runs the same sequence through clients that share one jar and prints what IRIS did
//! with it, so a run records the behaviour of the build under test. It asserts nothing: on 2026.2
//! IRIS answers each account as itself even then, and the unit test
//! (`test_primary_cookie_store_171`) is what fails without the fix.
//!
//! `lockf -k /tmp/iad-141-iris.lock cargo test --features testing --test integration primary_session_171 -- --ignored --test-threads=1`

use crate::support_identity_141::{Account, Container};
use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::iris::csp_session::{
    all_logout_urls, logout_blocking, store_for_primary,
};
use iris_agentic_dev_core::iris::http::{client_for, ClientKind};
use iris_agentic_dev_core::testing::{ThrowawayIris, THROWAWAY_IRIS_NAME};
use reqwest::cookie::CookieStore;
use std::time::Duration;

const USER_A: &str = "iadsessa";
const PW_A: &str = "Pw171sessA";
const USER_B: &str = "iadsessb";
const PW_B: &str = "Pw171sessB";

/// `$USERNAME` as the server sees the request, through the Atelier query action, and the
/// `CSPSESSIONID` cookie the answer set, if any.
async fn whoami(client: &reqwest::Client, conn: &IrisConnection) -> (String, Option<String>) {
    let resp = client
        .post(conn.versioned_ns_url("USER", "/action/query"))
        .basic_auth(&conn.username, Some(&conn.password))
        .json(&serde_json::json!({"query": "SELECT $USERNAME AS U"}))
        .send()
        .await
        .expect("the query request must reach IRIS");
    let status = resp.status();
    let new_session = resp
        .headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with("CSPSESSIONID"))
        .and_then(|v| v.split(';').next())
        .map(str::to_string);
    let body: serde_json::Value = resp.json().await.unwrap_or_default();
    let user = body["result"]["content"][0]["U"]
        .as_str()
        .unwrap_or_else(|| panic!("no $USERNAME in the answer (HTTP {status}): {body}"))
        .to_string();
    (user, new_session)
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
    eprintln!("one store per account (caller, expected, (ran as, cookie set)): {own:?}");
    for (i, (caller, expected, (ran_as, _))) in own.iter().enumerate() {
        assert_eq!(ran_as, expected, "call {i} by {caller} ran as {ran_as}");
    }

    // Each account's store holds a session cookie of its own, and not the other's.
    let page: reqwest::Url = conn_a.atelier_url("/").parse().unwrap();
    let held = |c: &IrisConnection| {
        store_for_primary(&c.base_url, &c.username)
            .cookies(&page)
            .map(|h| h.to_str().unwrap_or_default().to_string())
            .unwrap_or_default()
    };
    let (jar_a, jar_b) = (held(&conn_a), held(&conn_b));
    eprintln!("jar a: {jar_a}\njar b: {jar_b}");
    assert!(
        jar_a.contains("CSPSESSIONID"),
        "a holds no session: {jar_a}"
    );
    assert!(
        jar_b.contains("CSPSESSIONID"),
        "b holds no session: {jar_b}"
    );
    assert_ne!(jar_a, jar_b, "both accounts hold one session");

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
