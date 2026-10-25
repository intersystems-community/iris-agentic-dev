//! Spec 141 R2 and FR-023 together: where a server's service-account password lives, and what a
//! re-registration does to it.
//!
//! The keychain is an in-memory store here (`keyring_core::mock`), shared behind
//! `testing::KEYSTORE_LOCK`. That mocks the operating system's keychain, not IRIS.

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::iris::connection_pool::apply_service_account;
use iris_agentic_dev_core::iris::endpoint::Endpoint;
use iris_agentic_dev_core::iris::server_manager::{resolve_iad_credential, store_iad_credential};
use iris_agentic_dev_core::iris::servers_config::{
    carry_service_account, merge_server, KeychainOutcome, NewServer, ServerEntry, ServersConfig,
};
use iris_agentic_dev_core::testing::KEYSTORE_LOCK;

fn with_mock_store<F: FnOnce()>(f: F) {
    let _guard = KEYSTORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    keyring_core::set_default_store(keyring_core::mock::Store::new().unwrap());
    f();
    keyring_core::set_default_store(keyring_core::mock::Store::new().unwrap());
}

fn entry(json: &str) -> ServerEntry {
    serde_json::from_str(json).expect("entry parses")
}

fn new_server(host: &str) -> NewServer {
    NewServer {
        host: host.to_string(),
        port: 52773,
        namespace: "USER".to_string(),
        username: "_SYSTEM".to_string(),
        ..Default::default()
    }
}

const WITH_SERVICE: &str = r#"{"host":"a.example","port":52773,"namespace":"USER","username":"_SYSTEM","password_endpoint":"http://a.example:52773","service_username":"iad_svc","service_password":"svc-pw","container":"db1"}"#;

#[test]
fn servers_json_round_trips_the_service_account_fields() {
    let cfg: ServersConfig = serde_json::from_str(&format!(
        r#"{{"version":1,"servers":{{"a":{WITH_SERVICE}}}}}"#
    ))
    .unwrap();
    let e = &cfg.servers["a"];
    assert_eq!(e.service_username.as_deref(), Some("iad_svc"));
    assert_eq!(e.service_password.as_deref(), Some("svc-pw"));
    assert_eq!(e.container.as_deref(), Some("db1"));
    let back = serde_json::to_value(e).unwrap();
    assert_eq!(back["service_username"], "iad_svc");
    assert_eq!(back["container"], "db1");
}

#[test]
fn the_same_account_at_the_same_endpoint_keeps_its_plaintext_password() {
    let old = entry(WITH_SERVICE);
    let mut m = merge_server(
        Some(&old),
        &new_server("a.example"),
        KeychainOutcome::NotTried,
    )
    .unwrap();
    carry_service_account(&mut m, Some(&old), Some("iad_svc"), Some("db1"));
    assert!(!m.moved);
    assert_eq!(m.entry.service_password.as_deref(), Some("svc-pw"));
    assert_eq!(
        m.entry.password_endpoint.as_deref(),
        Some("http://a.example:52773"),
        "the kept password stays bound to its endpoint"
    );
}

#[test]
fn a_moved_endpoint_drops_the_plaintext_service_password() {
    let old = entry(WITH_SERVICE);
    let mut m = merge_server(
        Some(&old),
        &new_server("b.example"),
        KeychainOutcome::NotTried,
    )
    .unwrap();
    carry_service_account(&mut m, Some(&old), Some("iad_svc"), Some("db1"));
    assert!(m.moved);
    assert_eq!(m.entry.service_username.as_deref(), Some("iad_svc"));
    assert_eq!(m.entry.service_password, None);
}

#[test]
fn another_account_does_not_inherit_the_old_accounts_password() {
    let old = entry(WITH_SERVICE);
    let mut m = merge_server(
        Some(&old),
        &new_server("a.example"),
        KeychainOutcome::NotTried,
    )
    .unwrap();
    carry_service_account(&mut m, Some(&old), Some("other_svc"), None);
    assert_eq!(m.entry.service_username.as_deref(), Some("other_svc"));
    assert_eq!(m.entry.service_password, None);
    assert_eq!(m.entry.container, None);
}

fn conn_at(host: &str) -> IrisConnection {
    IrisConnection::new(
        format!("http://{host}:52773"),
        "USER",
        "_SYSTEM",
        "pw",
        DiscoverySource::EnvVar,
    )
}

#[test]
fn the_service_password_comes_from_iads_own_keychain_item_for_this_endpoint() {
    with_mock_store(|| {
        let at = Endpoint::parse(Some("http"), "a.example", 52773, None).unwrap();
        store_iad_credential("a", "iad_svc", &at, "from-keychain").unwrap();
        let mut conn = conn_at("a.example");
        apply_service_account(&mut conn, "a", Some("iad_svc"), Some("plain"), Some("db1"));
        assert_eq!(conn.member.service_username.as_deref(), Some("iad_svc"));
        assert_eq!(
            conn.member.service_password.as_deref(),
            Some("from-keychain"),
            "the keychain wins over the file"
        );
        assert_eq!(conn.member.container.as_deref(), Some("db1"));

        // The same server name at another endpoint has no item: it gets the plaintext it was given.
        let mut other = conn_at("b.example");
        apply_service_account(&mut other, "a", Some("iad_svc"), None, None);
        assert_eq!(other.member.service_password.as_deref(), Some(""));
        assert!(resolve_iad_credential(
            "a",
            "iad_svc",
            &Endpoint::parse(Some("http"), "b.example", 52773, None).unwrap()
        )
        .is_err());
    });
}

#[test]
fn a_blank_account_leaves_the_member_without_one() {
    with_mock_store(|| {
        let mut conn = conn_at("a.example");
        apply_service_account(&mut conn, "a", Some("  "), Some("plain"), None);
        assert_eq!(conn.member.service_username, None);
        assert_eq!(conn.member.service_password, None);
    });
}
