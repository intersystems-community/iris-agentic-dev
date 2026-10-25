//! Spec 141 T198 (review item 12): `IRIS_SCM_ALLOW_DISCONNECT` is its own operator opt-in.
//!
//! `IRIS_SCM_ALLOW_CHECKIN` used to enable CheckIn, Disconnect and Reconnect together. Disconnect
//! switches the namespace's source-control integration off for the session, which is a different
//! blast radius from one document's CheckIn, so one switch must not enable both.
//!
//! Every case runs against a connection nothing listens on, so a refusal can only come from the
//! gate in the handler: a call that got past it fails with a connection error.

use iris_agentic_dev_core::elicitation::{CheckoutCache, ElicitationAction, ElicitationStore};
use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::policy::refusal::REFUSAL_CODES;
use iris_agentic_dev_core::testing::{EnvGuard, BEHAVIOR_ENV_VARS};
use iris_agentic_dev_core::tools::scm::{handle_iris_source_control, ScmParams};
use serde_json::{json, Value};

const CHECKIN: &str = "IRIS_SCM_ALLOW_CHECKIN";
const DISCONNECT: &str = "IRIS_SCM_ALLOW_DISCONNECT";

/// The ids an agent might send, with and without `%`, in the cases it might use.
const CHECKIN_IDS: &[&str] = &["CheckIn", "%CheckIn", "checkin", " %CHECKIN "];
const DISCONNECT_IDS: &[&str] = &[
    "Disconnect",
    "%Disconnect",
    "disconnect",
    " %DISCONNECT",
    "Reconnect",
    "%Reconnect",
    "reconnect",
    "RECONNECT ",
];

fn unroutable() -> IrisConnection {
    IrisConnection::new(
        "http://127.0.0.1:1",
        "USER",
        "_SYSTEM",
        "SYS",
        DiscoverySource::ExplicitFlag,
    )
}

async fn run(p: Value, store: &ElicitationStore) -> Value {
    let p: ScmParams = serde_json::from_value(p).expect("ScmParams");
    let client = reqwest::Client::new();
    let result =
        handle_iris_source_control(&unroutable(), &client, p, store, &CheckoutCache::new())
            .await
            .expect("the handler answers");
    result.structured_content.unwrap_or_else(|| json!({}))
}

async fn execute(id: &str, store: &ElicitationStore) -> Value {
    run(
        json!({"action": "execute", "document": "Demo.A", "action_id": id}),
        store,
    )
    .await
}

/// Only the two opt-in variables are touched. `EnvGuard::clean()` would unset every behavior
/// variable, `IAD_NO_KEYCHAIN` included, under a test that reads it without the guard's lock.
fn env_with(vars: &[(&str, &str)]) -> EnvGuard {
    let mut env = EnvGuard::new();
    env.unset(CHECKIN).unset(DISCONNECT);
    for (k, v) in vars {
        env.set(k, v);
    }
    env
}

fn assert_refused(b: &Value, code: &str, what: &str) {
    assert_eq!(b["error_code"], code, "{what}: {b}");
}

fn assert_reached_iris(b: &Value, what: &str) {
    let code = b["error_code"].as_str().unwrap_or("");
    assert!(
        code != "CHECKIN_BLOCKED" && code != "SCM_ADMIN_BLOCKED",
        "{what} was stopped by the opt-in gate: {b}"
    );
    assert!(
        !b["success"].as_bool().unwrap_or(false) && b["error"].is_string(),
        "{what} should have failed on the unreachable connection: {b}"
    );
}

#[test]
fn the_new_variable_is_a_behavior_variable() {
    assert!(BEHAVIOR_ENV_VARS.contains(&DISCONNECT));
    assert!(BEHAVIOR_ENV_VARS.contains(&CHECKIN));
}

#[tokio::test]
async fn with_neither_set_each_action_names_its_own_refusal() {
    let _env = env_with(&[]);
    let store = ElicitationStore::new();
    for id in CHECKIN_IDS {
        assert_refused(&execute(id, &store).await, "CHECKIN_BLOCKED", id);
    }
    for id in DISCONNECT_IDS {
        assert_refused(&execute(id, &store).await, "SCM_ADMIN_BLOCKED", id);
    }
}

#[tokio::test]
async fn the_checkin_opt_in_does_not_enable_disconnect_or_reconnect() {
    let _env = env_with(&[(CHECKIN, "1")]);
    let store = ElicitationStore::new();
    for id in CHECKIN_IDS {
        assert_reached_iris(&execute(id, &store).await, id);
    }
    for id in DISCONNECT_IDS {
        assert_refused(&execute(id, &store).await, "SCM_ADMIN_BLOCKED", id);
    }
}

#[tokio::test]
async fn the_disconnect_opt_in_does_not_enable_checkin() {
    let _env = env_with(&[(DISCONNECT, "1")]);
    let store = ElicitationStore::new();
    for id in DISCONNECT_IDS {
        assert_reached_iris(&execute(id, &store).await, id);
    }
    for id in CHECKIN_IDS {
        assert_refused(&execute(id, &store).await, "CHECKIN_BLOCKED", id);
    }
}

#[tokio::test]
async fn with_both_set_neither_is_blocked() {
    let _env = env_with(&[(CHECKIN, "true"), (DISCONNECT, "YES")]);
    let store = ElicitationStore::new();
    for id in CHECKIN_IDS.iter().chain(DISCONNECT_IDS) {
        assert_reached_iris(&execute(id, &store).await, id);
    }
}

#[tokio::test]
async fn only_one_true_spelling_enables_disconnect() {
    let store = ElicitationStore::new();
    for v in ["1", "true", "TRUE", "yes", "Yes"] {
        let _env = env_with(&[(DISCONNECT, v)]);
        assert_reached_iris(&execute("%Disconnect", &store).await, v);
    }
    for v in ["0", "", "no", "false", "on", "2", "enabled"] {
        let _env = env_with(&[(DISCONNECT, v)]);
        assert_refused(
            &execute("%Disconnect", &store).await,
            "SCM_ADMIN_BLOCKED",
            v,
        );
    }
}

/// An answer to a pending prompt runs the action the prompt was opened for. It meets the same gate,
/// and the gate of that action: a Disconnect prompt is not unlocked by the CheckIn opt-in.
#[tokio::test]
async fn a_resumed_prompt_meets_the_gate_of_its_own_action() {
    let _env = env_with(&[(CHECKIN, "1")]);
    let store = ElicitationStore::new();
    let pending = |id: &str| {
        store.insert_from(
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
        )
    };
    for id in ["%Disconnect", "Reconnect"] {
        let eid = pending(id);
        let b = run(
            json!({"action": "execute", "elicitation_id": eid, "answer": "yes"}),
            &store,
        )
        .await;
        assert_refused(&b, "SCM_ADMIN_BLOCKED", id);
    }
    let eid = pending("%CheckIn");
    let b = run(
        json!({"action": "execute", "elicitation_id": eid, "answer": "yes"}),
        &store,
    )
    .await;
    assert_reached_iris(&b, "a resumed CheckIn with its own opt-in");
}

#[test]
fn the_new_code_is_a_refusal() {
    assert!(REFUSAL_CODES.contains(&"SCM_ADMIN_BLOCKED"));
    assert!(REFUSAL_CODES.contains(&"CHECKIN_BLOCKED"));
}
