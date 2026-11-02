//! S9: in HTTP transport mode a session's confirmation tokens and terminal sessions belong to that
//! session.
//!
//! Two clients open sessions on one server. The first earns a `global_kill` confirmation token with
//! `global_preview` and opens a WebSocket terminal. The second presents both. They used to work,
//! because every session was a copy of one `IrisTools` that shared the token map and the terminal
//! pool.
//!
//! Live, on the throwaway container (`IAD_THROWAWAY_IMAGE` picks its version). Run it behind the
//! shared lock:
//! `lockf -k /tmp/iad-141-iris.lock cargo test --features testing --test bin_integration
//! test_session_stores_171 -- --ignored --test-threads=1`.

use crate::test_session_stores_141::{start_at, Session};
use iris_agentic_dev_core::testing::ThrowawayIris;
use serde_json::json;

const GLOBAL: &str = "IadS9Probe";

#[tokio::test]
#[ignore = "live: starts the throwaway IRIS; run behind /tmp/iad-141-iris.lock"]
async fn a_second_http_session_cannot_use_the_first_sessions_token_or_terminal() {
    let Some(iris) = ThrowawayIris::start() else {
        return;
    };
    let home = tempfile::tempdir().unwrap();
    let server = start_at(
        home.path(),
        (iris.host, iris.web_port, iris.username, iris.password),
        &[
            ("IRIS_WRITE_TOOLS_ENABLED", "true"),
            ("IRIS_DESTRUCTIVE_TOOLS_ENABLED", "true"),
        ],
    );
    let mut a = Session::open(server.port, "client-a").await;
    let mut b = Session::open(server.port, "client-b").await;
    assert_ne!(a.id, b.id, "two sessions");

    // Data worth keeping for the length of the test.
    let set = a
        .call(
            "iris_global",
            json!({"action": "set", "global_name": format!("^{GLOBAL}"), "subscripts": ["k"], "value": "kept"}),
        )
        .await;
    assert_eq!(set["success"], true, "{set}");

    // A earns a kill token.
    let preview = a
        .call("global_preview", json!({"global": format!("^{GLOBAL}")}))
        .await;
    let token = preview["confirm_token"]
        .as_str()
        .unwrap_or_else(|| panic!("no confirm_token: {preview}"))
        .to_string();

    // B presents it and is refused; the data stays.
    let killed = b
        .call(
            "global_kill",
            json!({"global": format!("^{GLOBAL}"), "confirm_token": token}),
        )
        .await;
    assert_eq!(
        killed["error_code"], "CONFIRM_REQUIRED",
        "session B used session A's token: {killed}"
    );
    let still = a
        .call(
            "iris_global",
            json!({"action": "get", "global_name": format!("^{GLOBAL}"), "subscripts": ["k"]}),
        )
        .await;
    assert!(still.to_string().contains("kept"), "{still}");

    // A opens a terminal. B cannot drive it, and A still can.
    let opened = a.call("iris_ws_open", json!({})).await;
    let session = opened["session"]
        .as_str()
        .unwrap_or_else(|| panic!("no terminal session: {opened}"))
        .to_string();
    let hijack = b
        .call(
            "iris_ws_exec",
            json!({"session": session, "code": "Write \"hijacked\""}),
        )
        .await;
    assert!(
        !hijack.to_string().contains("hijacked"),
        "session B ran code in session A's terminal: {hijack}"
    );
    let own = a
        .call(
            "iris_ws_exec",
            json!({"session": session, "code": "Write \"mine\""}),
        )
        .await;
    assert!(own.to_string().contains("mine"), "{own}");
    a.call("iris_ws_close", json!({"session": session})).await;

    // A's own token still works.
    let killed = a
        .call(
            "global_kill",
            json!({"global": format!("^{GLOBAL}"), "confirm_token": token}),
        )
        .await;
    assert_eq!(killed["success"], true, "{killed}");
}
