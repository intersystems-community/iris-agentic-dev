//! S9: in HTTP mode each session has its own WebSocket terminal pool, confirmation tokens and
//! checkout cache, and its elicitation store is swept.
//!
//! `test_session_stores_141` covers the log store, the pending SCM dialogs and the call history.
//! These three were still shared by `for_new_session`: a confirmation token one client earned with
//! `global_preview` was good for another client's `global_kill`, a terminal session opened by one
//! client could be driven by another that learned its token, and one client's SCM checkout let
//! another client's write skip its own pre-write probe.

use iris_agentic_dev_core::elicitation::ElicitationAction;
use iris_agentic_dev_core::tools::admin_tools::ConfirmEntry;
use iris_agentic_dev_core::tools::IrisTools;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[test]
fn a_new_session_does_not_share_the_checkout_cache() {
    let proto = IrisTools::new(None).unwrap();
    let a = proto.for_new_session();
    let b = proto.for_new_session();
    a.checkout_cache.mark("USER", "My.Class.cls");
    assert!(a.checkout_cache.is_checked_out("USER", "My.Class.cls"));
    assert!(
        !b.checkout_cache.is_checked_out("USER", "My.Class.cls"),
        "another session's checkout must not let this session skip its own probe"
    );
    assert!(!proto.checkout_cache.is_checked_out("USER", "My.Class.cls"));
}

#[tokio::test]
async fn a_new_session_does_not_share_confirmation_tokens() {
    let proto = IrisTools::new(None).unwrap();
    let a = proto.for_new_session();
    let b = proto.for_new_session();
    a.confirm_tokens.lock().await.insert(
        "token-a".to_string(),
        ConfirmEntry {
            global: "IadS9".to_string(),
            server: None,
            issued_at: Instant::now(),
        },
    );
    assert!(a.confirm_tokens.lock().await.contains_key("token-a"));
    assert!(
        b.confirm_tokens.lock().await.is_empty(),
        "a token earned by one session must not confirm another session's kill"
    );
}

#[test]
fn a_new_session_does_not_share_the_terminal_pool() {
    let proto = IrisTools::new(None).unwrap();
    let a = proto.for_new_session();
    let b = proto.for_new_session();
    assert!(!Arc::ptr_eq(&a.ws_pool, &b.ws_pool));
    assert!(!Arc::ptr_eq(&a.ws_pool, &proto.ws_pool));
    a.ws_pool.record_target("ws:default:USER:1", None);
    assert_eq!(a.ws_pool.target_of("ws:default:USER:1"), Some(None));
    assert_eq!(
        b.ws_pool.target_of("ws:default:USER:1"),
        None,
        "the other session's pool has never heard of that token"
    );
}

#[test]
fn what_is_one_operators_stays_shared_and_the_reason_is_documented() {
    // The connection state, pool, clients, registry and config are the operator's account on the
    // operator's servers; a reload through one session must reach the others.
    // (`for_new_session`'s doc comment says so.)
    let proto = IrisTools::new(None).unwrap();
    let s = proto.for_new_session();
    assert!(Arc::ptr_eq(&proto.connection, &s.connection));
    assert!(Arc::ptr_eq(&proto.pool, &s.pool));
    assert!(Arc::ptr_eq(&proto.client, &s.client));
    assert!(Arc::ptr_eq(&proto.registry, &s.registry));
}

#[test]
fn a_new_session_does_not_share_the_metadata_cache() {
    let proto = IrisTools::new(None).unwrap();
    let a = proto.for_new_session();
    let b = proto.for_new_session();
    a.metadata_cache.lock().unwrap().insert(
        "find_subclass:x".to_string(),
        (serde_json::json!({"k": 1}), Instant::now()),
    );
    assert!(b.metadata_cache.lock().unwrap().is_empty());
    assert!(proto.metadata_cache.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_session_sweeps_its_own_elicitation_store() {
    let proto = IrisTools::new(None).unwrap();
    let session = proto.for_new_session_sweeping(Duration::from_millis(30));
    let id = session.elicitation_store.insert_expiring_at(
        "Old.Dialog.cls",
        ElicitationAction::Put,
        Instant::now() - Duration::from_secs(1),
    );
    assert_eq!(
        session.elicitation_store.len(),
        1,
        "the expired entry sits in the store until a sweep ({id})"
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while !session.elicitation_store.is_empty() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        session.elicitation_store.len(),
        0,
        "the session's own sweeper removed the expired entry"
    );
}

#[tokio::test]
async fn the_sweeper_ends_with_its_session() {
    let proto = IrisTools::new(None).unwrap();
    let session = proto.for_new_session_sweeping(Duration::from_millis(10));
    let weak = session.elicitation_store.weak_handle();
    drop(session);
    let deadline = Instant::now() + Duration::from_secs(5);
    while weak.strong_count() > 0 && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        weak.strong_count(),
        0,
        "a sweeper that outlived its session would keep the store alive for the life of the process"
    );
}
