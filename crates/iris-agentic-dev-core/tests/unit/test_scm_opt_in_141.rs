//! Spec 141 R5 C24 (PLATFORM-32): `iris_source_control` Disconnect and Reconnect need an operator
//! opt-in, as CheckIn does.
//!
//! Disconnect and Reconnect switch a document's source-control state: Disconnect drops the link
//! between the document and the repository, and Reconnect restores one. With the opt-ins unset,
//! `execute` refused CheckIn and ran the other two, and `menu` listed them. (T198 gave Disconnect
//! and Reconnect their own variable; `test_scm_disconnect_opt_in_141` covers the split.)
//!
//! Every case runs against a connection nothing listens on, so a refusal can only come from the
//! gate in the handler: a call that got past it fails with a connection error.

use iris_agentic_dev_core::elicitation::{CheckoutCache, ElicitationAction, ElicitationStore};
use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::testing::EnvGuard;
use iris_agentic_dev_core::tools::scm::{
    handle_iris_source_control, menu_actions, needs_operator_opt_in, ScmOptIns, ScmParams,
};
use serde_json::{json, Value};

fn unroutable() -> IrisConnection {
    IrisConnection::new(
        "http://127.0.0.1:1",
        "USER",
        "_SYSTEM",
        "SYS",
        DiscoverySource::ExplicitFlag,
    )
}

fn params(v: Value) -> ScmParams {
    serde_json::from_value(v).expect("ScmParams")
}

async fn run(p: ScmParams, store: &ElicitationStore) -> Value {
    let client = reqwest::Client::new();
    let result =
        handle_iris_source_control(&unroutable(), &client, p, store, &CheckoutCache::new())
            .await
            .expect("the handler answers");
    result.structured_content.unwrap_or_else(|| json!({}))
}

/// Both opt-in variables unset for the test. Not `EnvGuard::clean()`: that also unsets
/// `IAD_NO_KEYCHAIN`, which a keystore test reads without the guard's lock.
fn scm_env() -> EnvGuard {
    let mut env = EnvGuard::new();
    env.unset("IRIS_SCM_ALLOW_CHECKIN")
        .unset("IRIS_SCM_ALLOW_DISCONNECT");
    env
}

/// The refusal code for an id: CheckIn has its own, Disconnect and Reconnect share another.
fn blocked_code(id: &str) -> &'static str {
    if id.to_ascii_lowercase().contains("checkin") {
        "CHECKIN_BLOCKED"
    } else {
        "SCM_ADMIN_BLOCKED"
    }
}

/// Both spellings of the id, with and without `%`, and the case an agent might use.
const SPELLINGS: &[&str] = &[
    "CheckIn",
    "%CheckIn",
    "checkin",
    " %CHECKIN ",
    "Disconnect",
    "%Disconnect",
    "disconnect",
    " %DISCONNECT",
    "Reconnect",
    "%Reconnect",
    "reconnect",
    "RECONNECT ",
];

#[test]
fn the_three_actions_need_the_opt_in_in_every_spelling() {
    for id in SPELLINGS {
        assert!(needs_operator_opt_in(id), "{id:?}");
    }
    for id in [
        "CheckOut",
        "%CheckOut",
        "UndoCheckout",
        "GetLatest",
        "AddToSourceControl",
        "Diff",
        "",
        "Disconnected",
    ] {
        assert!(!needs_operator_opt_in(id), "{id:?} is not one of the three");
    }
}

#[tokio::test]
async fn execute_is_refused_without_the_opt_in() {
    let _env = scm_env();
    let store = ElicitationStore::new();
    for id in SPELLINGS {
        let b = run(
            params(json!({"action": "execute", "document": "Demo.A", "action_id": id})),
            &store,
        )
        .await;
        assert_eq!(b["error_code"], blocked_code(id), "{id:?}: {b}");
    }
}

#[tokio::test]
async fn an_action_outside_the_three_reaches_iris() {
    let _env = scm_env();
    let store = ElicitationStore::new();
    let b = run(
        params(json!({"action": "execute", "document": "Demo.A", "action_id": "%GetLatest"})),
        &store,
    )
    .await;
    assert_ne!(b["error_code"], "CHECKIN_BLOCKED", "{b}");
    assert_ne!(b["error_code"], "SCM_ADMIN_BLOCKED", "{b}");
}

/// An answer to a pending prompt runs the action the prompt was opened for. It meets the same gate.
#[tokio::test]
async fn a_resumed_prompt_meets_the_gate_too() {
    let _env = scm_env();
    let store = ElicitationStore::new();
    for id in ["%Disconnect", "Reconnect", "%CheckIn"] {
        let eid = store.insert_from(
            iris_agentic_dev_core::elicitation::Origin {
                tool: "iris_source_control".to_string(),
                target_key: None,
                class: iris_agentic_dev_core::tools::write_gate::WriteClass::Write,
            },
            "Demo.A.cls",
            ElicitationAction::ScmExecute,
            None,
            Some(id.to_string()),
            "USER",
        );
        let b = run(
            params(json!({"action": "execute", "elicitation_id": eid, "answer": "yes"})),
            &store,
        )
        .await;
        assert_eq!(b["error_code"], blocked_code(id), "{id}: {b}");
    }
}

#[test]
fn the_menu_lists_them_only_with_the_opt_in() {
    let raw = "CheckOut|1\nCheckIn|1\nDisconnect|1\nReconnect|1\n%Reconnect|1\nGetLatest|1\nUndoCheckout|0\n";
    let ids = |checkin: bool, disconnect: bool| -> Vec<String> {
        menu_actions(
            raw,
            ScmOptIns {
                checkin,
                disconnect,
            },
        )
        .iter()
        .map(|a| a["id"].as_str().unwrap().to_string())
        .collect()
    };
    assert_eq!(ids(false, false), ["CheckOut", "GetLatest"]);
    assert_eq!(ids(true, false), ["CheckOut", "CheckIn", "GetLatest"]);
    assert_eq!(
        ids(false, true),
        [
            "CheckOut",
            "Disconnect",
            "Reconnect",
            "%Reconnect",
            "GetLatest"
        ]
    );
    assert_eq!(
        ids(true, true),
        [
            "CheckOut",
            "CheckIn",
            "Disconnect",
            "Reconnect",
            "%Reconnect",
            "GetLatest"
        ]
    );
}
