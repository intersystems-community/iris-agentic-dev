//! S9 follow-up: the per-session metadata cache must not hand one server's, account's or limit's
//! answer to another call. Each dictionary tool's cache key names the endpoint and the account it
//! read, plus every argument that changes the answer.

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::tools::dict::{
    dispatch_cache_key, message_map_cache_key, subclass_cache_key, ExtractMessageMapParams,
    FindSubclassImplementationsParams, ResolveDynamicDispatchParams,
};

fn conn(url: &str, user: &str) -> IrisConnection {
    IrisConnection::new(url, "USER", user, "SYS", DiscoverySource::ExplicitFlag)
}

fn dispatch(limit: Option<usize>) -> ResolveDynamicDispatchParams {
    serde_json::from_value(serde_json::json!({
        "method_name": "OnRequest",
        "package_prefix": "Demo",
        "limit": limit,
    }))
    .unwrap()
}

fn message_map() -> ExtractMessageMapParams {
    serde_json::from_value(serde_json::json!({"class_name": "Demo.Router"})).unwrap()
}

fn subclass() -> FindSubclassImplementationsParams {
    serde_json::from_value(serde_json::json!({
        "base_classes": ["Ens.BusinessOperation"],
        "method_name": "OnMessage",
    }))
    .unwrap()
}

#[test]
fn two_servers_with_one_namespace_get_different_keys() {
    let a = conn("http://localhost:52780", "_SYSTEM");
    let b = conn("http://localhost:39780", "_SYSTEM");
    assert_ne!(
        dispatch_cache_key(&a, &dispatch(None)),
        dispatch_cache_key(&b, &dispatch(None))
    );
    assert_ne!(
        message_map_cache_key(&a, &message_map()),
        message_map_cache_key(&b, &message_map())
    );
    assert_ne!(
        subclass_cache_key(&a, &subclass()),
        subclass_cache_key(&b, &subclass())
    );
}

#[test]
fn two_accounts_on_one_server_get_different_keys() {
    let admin = conn("http://localhost:52780", "_SYSTEM");
    let reader = conn("http://localhost:52780", "reader");
    assert_ne!(
        dispatch_cache_key(&admin, &dispatch(None)),
        dispatch_cache_key(&reader, &dispatch(None))
    );
    assert_ne!(
        message_map_cache_key(&admin, &message_map()),
        message_map_cache_key(&reader, &message_map())
    );
    assert_ne!(
        subclass_cache_key(&admin, &subclass()),
        subclass_cache_key(&reader, &subclass())
    );
}

#[test]
fn the_account_name_is_compared_without_case() {
    let a = conn("http://localhost:52780", "_SYSTEM");
    let b = conn("http://localhost:52780", "_system");
    assert_eq!(
        dispatch_cache_key(&a, &dispatch(None)),
        dispatch_cache_key(&b, &dispatch(None))
    );
}

#[test]
fn a_dispatch_limit_belongs_in_the_key() {
    let a = conn("http://localhost:52780", "_SYSTEM");
    assert_ne!(
        dispatch_cache_key(&a, &dispatch(Some(5))),
        dispatch_cache_key(&a, &dispatch(None))
    );
}

#[test]
fn a_repeated_call_hits_its_own_entry() {
    let a = conn("http://localhost:52780", "_SYSTEM");
    assert_eq!(
        dispatch_cache_key(&a, &dispatch(Some(5))),
        dispatch_cache_key(&a, &dispatch(Some(5)))
    );
    assert_eq!(
        message_map_cache_key(&a, &message_map()),
        message_map_cache_key(&a, &message_map())
    );
    assert_eq!(
        subclass_cache_key(&a, &subclass()),
        subclass_cache_key(&a, &subclass())
    );
}
