//! Spec 141 T220: each MCP session has its own log store, elicitation store and call history.
//!
//! In HTTP transport mode the server builds one `IrisTools` per session by cloning a prototype,
//! and a clone shares every `Arc` field. `iris_get_log` with no id lists the whole store, so a
//! second client read the first client's stored results (an `iris_info` document list, compile
//! output, test detail), and an SCM dialog one client left pending could be resumed by the other.
//! `for_new_session` is what the HTTP factory calls instead of `clone`.

use iris_agentic_dev_core::elicitation::ElicitationAction;
use iris_agentic_dev_core::tools::log_store::{new_log_id, LogEntry};
use iris_agentic_dev_core::tools::IrisTools;
use serde_json::json;
use std::sync::Arc;

fn store_a_log(tools: &IrisTools) -> String {
    let id = new_log_id();
    let entry = LogEntry {
        id: id.clone(),
        tool: "iris_info".to_string(),
        created_at: std::time::Instant::now(),
        preview: vec![],
        full_result: json!(["Secret.Client.A.cls"]),
        total_count: 1,
    };
    tools.log_store.lock().unwrap().store(entry);
    id
}

#[test]
fn a_plain_clone_shares_the_stores_which_is_the_bug() {
    // The control: this is what the HTTP factory did, and why the next tests can fail.
    let a = IrisTools::new(None).unwrap();
    let b = a.clone();
    let id = store_a_log(&a);
    assert!(
        b.log_store
            .lock()
            .unwrap()
            .list()
            .iter()
            .any(|l| l.id == id),
        "a clone sees the other's log"
    );
}

#[test]
fn a_new_session_does_not_see_another_sessions_log() {
    let proto = IrisTools::new(None).unwrap();
    let a = proto.for_new_session();
    let b = proto.for_new_session();
    let id = store_a_log(&a);

    assert!(
        a.log_store
            .lock()
            .unwrap()
            .list()
            .iter()
            .any(|l| l.id == id),
        "the session that stored it still reads it"
    );
    assert!(
        b.log_store.lock().unwrap().list().is_empty(),
        "the other session lists nothing"
    );
    assert!(
        matches!(
            b.log_store.lock().unwrap().get(&id),
            iris_agentic_dev_core::tools::log_store::GetResult::NotFound
        ),
        "the other session cannot fetch it by id either"
    );
    assert!(
        proto.log_store.lock().unwrap().list().is_empty(),
        "the prototype is not a session and holds nothing of either"
    );
}

#[test]
fn a_new_session_keeps_the_log_stores_limits() {
    let proto = IrisTools::new(None).unwrap();
    let (max, ttl) = {
        let s = proto.log_store.lock().unwrap();
        (s.max_entries, s.ttl_minutes)
    };
    let s = proto.for_new_session();
    let s = s.log_store.lock().unwrap();
    assert_eq!((s.max_entries, s.ttl_minutes), (max, ttl));
}

#[test]
fn a_new_session_does_not_see_another_sessions_pending_elicitation() {
    let proto = IrisTools::new(None).unwrap();
    let a = proto.for_new_session();
    let b = proto.for_new_session();
    let eid = a.elicitation_store.insert(
        "Secret.Client.A.cls",
        ElicitationAction::Put,
        Some("Class Secret.Client.A {}".to_string()),
        None,
        "USER",
    );
    assert!(a.elicitation_store.lookup(&eid).is_some(), "the control");
    assert!(
        b.elicitation_store.lookup(&eid).is_none(),
        "the other session cannot resume or read a dialog it did not start"
    );
}

#[test]
fn a_new_session_starts_its_own_call_history() {
    let proto = IrisTools::new(None).unwrap();
    let a = proto.for_new_session();
    let b = proto.for_new_session();
    a.history
        .lock()
        .unwrap()
        .push_back(iris_agentic_dev_core::tools::ToolCallEntry::now(
            "iris_info",
            true,
            1,
            a.session.id,
        ));
    assert_eq!(a.history.lock().unwrap().len(), 1);
    assert!(b.history.lock().unwrap().is_empty());
}

#[test]
fn a_new_session_still_shares_what_is_the_servers_not_the_clients() {
    // One iad process is one operator's connections: the connection state, the pool, the HTTP
    // clients, the config and the checkout cache describe the servers and the operator's account,
    // not what one client asked. A session that rebuilt them would load its own pool (and open the
    // keychain again) and miss an `iris_reload_pool` made through another session.
    let proto = IrisTools::new(None).unwrap();
    let s = proto.for_new_session();
    assert!(Arc::ptr_eq(&proto.connection, &s.connection));
    assert!(Arc::ptr_eq(&proto.pool, &s.pool));
    assert!(Arc::ptr_eq(&proto.client, &s.client));
    assert!(Arc::ptr_eq(&proto.checkout_cache, &s.checkout_cache));
    assert!(Arc::ptr_eq(&proto.registry, &s.registry));
}
