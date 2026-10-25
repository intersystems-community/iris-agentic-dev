//! Spec 141 FR-023 (T-R4b-U08 to U11): where iad's own passwords live, and which password a
//! server may receive.
//!
//! The keychain is an in-memory store here (`keyring_core::mock`), shared by every test in the
//! process behind `testing::KEYSTORE_LOCK`. That mocks the operating system's keychain, not IRIS.

use iris_agentic_dev_core::iris::config_sources::SetAsideKind;
use iris_agentic_dev_core::iris::endpoint::Endpoint;
use iris_agentic_dev_core::iris::server_manager::{
    delete_iad_credential, iad_account, resolve_iad_credential, sm_secret_for_in,
    store_iad_credential, ServerManagerProfile, SmSecretError, IAD_KEYCHAIN_SERVICE,
};
use iris_agentic_dev_core::iris::servers_config::{
    merge_server, name_conflicts, KeychainOutcome, NewServer, ServerEntry,
};
use iris_agentic_dev_core::iris::vscode_config::parse_vscode_settings;
use iris_agentic_dev_core::testing::KEYSTORE_LOCK;

const SENTINEL: &str = "Zq9-SENTINEL-pw";
const SM_SERVICE: &str = "intersystems-server-credentials";

fn with_mock_store<F: FnOnce()>(f: F) {
    let _guard = KEYSTORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    keyring_core::set_default_store(keyring_core::mock::Store::new().unwrap());
    f();
    keyring_core::set_default_store(keyring_core::mock::Store::new().unwrap());
}

fn ep(scheme: &str, host: &str, port: u16) -> Endpoint {
    Endpoint::parse(Some(scheme), host, port, None).unwrap()
}

fn profile(name: &str, scheme: &str, host: &str, port: u16) -> ServerManagerProfile {
    ServerManagerProfile {
        name: name.to_string(),
        host: host.to_string(),
        port,
        scheme: scheme.to_string(),
        path_prefix: None,
        username: "_SYSTEM".to_string(),
        password_deprecated: None,
    }
}

// ── T-R4b-U08: iris_add_server merge ─────────────────────────────────────────────────────────

fn old_entry(json: &str) -> ServerEntry {
    serde_json::from_str(json).expect("entry parses")
}

fn new_server(host: &str, port: u16, password: Option<&str>) -> NewServer {
    NewServer {
        host: host.to_string(),
        port,
        namespace: "USER".to_string(),
        username: "_SYSTEM".to_string(),
        description: None,
        scheme: None,
        web_prefix: None,
        tls_verify: None,
        password: password.map(str::to_string),
    }
}

const OLD_PLAINTEXT: &str = r#"{"host":"a.example","port":52773,"namespace":"USER","username":"_SYSTEM","password":"old-pw","password_endpoint":"http://a.example:52773"}"#;

#[test]
fn a_moved_endpoint_drops_the_old_password() {
    let old = old_entry(OLD_PLAINTEXT);
    let m = merge_server(
        Some(&old),
        &new_server("evil.example", 52773, None),
        KeychainOutcome::NotTried,
    )
    .unwrap();
    assert_eq!(
        m.entry.password, None,
        "the old password must not follow the entry"
    );
    assert_eq!(m.entry.password_endpoint, None);
    assert!(m.password_cleared);
    assert_eq!(m.credential_store, "none");
}

#[test]
fn the_same_endpoint_keeps_the_stored_password() {
    let old = old_entry(OLD_PLAINTEXT);
    let m = merge_server(
        Some(&old),
        &new_server("A.example", 52773, None),
        KeychainOutcome::NotTried,
    )
    .unwrap();
    assert_eq!(m.entry.password.as_deref(), Some("old-pw"));
    assert_eq!(
        m.entry.password_endpoint.as_deref(),
        Some("http://a.example:52773")
    );
    assert!(!m.password_cleared);
    assert_eq!(m.credential_store, "file");
}

#[test]
fn a_keychain_success_leaves_no_password_in_the_entry() {
    let old = old_entry(OLD_PLAINTEXT);
    let m = merge_server(
        Some(&old),
        &new_server("a.example", 52773, Some(SENTINEL)),
        KeychainOutcome::Stored,
    )
    .unwrap();
    assert_eq!(m.entry.password, None);
    assert_eq!(m.entry.password_endpoint, None);
    assert_eq!(m.credential_store, "keychain");
    let json = serde_json::to_string(&m.entry).unwrap();
    assert!(
        !json.contains(SENTINEL) && !json.contains("old-pw"),
        "{json}"
    );
}

#[test]
fn an_unavailable_keychain_keeps_the_password_bound_to_its_endpoint() {
    let m = merge_server(
        None,
        &new_server("b.example", 443, Some(SENTINEL)),
        KeychainOutcome::Unavailable,
    )
    .unwrap();
    assert_eq!(m.entry.password.as_deref(), Some(SENTINEL));
    assert_eq!(
        m.entry.password_endpoint.as_deref(),
        Some("http://b.example:443")
    );
    assert_eq!(m.credential_store, "file");
    assert!(!m.password_cleared, "nothing was there to clear");
}

#[test]
fn a_moved_endpoint_with_a_new_password_stores_the_new_one_only() {
    let old = old_entry(OLD_PLAINTEXT);
    let m = merge_server(
        Some(&old),
        &new_server("c.example", 52773, Some("new-pw")),
        KeychainOutcome::Unavailable,
    )
    .unwrap();
    assert_eq!(m.entry.password.as_deref(), Some("new-pw"));
    assert_eq!(
        m.entry.password_endpoint.as_deref(),
        Some("http://c.example:52773")
    );
    assert!(m.password_cleared, "the old password went");
}

#[test]
fn a_moved_endpoint_names_the_old_keychain_item_to_delete() {
    let old =
        old_entry(r#"{"host":"a.example","port":52773,"namespace":"USER","username":"_SYSTEM"}"#);
    let m = merge_server(
        Some(&old),
        &new_server("evil.example", 52773, None),
        KeychainOutcome::NotTried,
    )
    .unwrap();
    let stale = m.stale_keychain.expect("the old endpoint's item is stale");
    assert_eq!(stale.base_url(), "http://a.example:52773");
}

#[test]
fn a_bad_scheme_is_refused_before_anything_is_saved() {
    let mut n = new_server("h.example", 52773, None);
    n.scheme = Some("ftp".to_string());
    assert!(merge_server(None, &n, KeychainOutcome::NotTried).is_err());
}

#[test]
fn a_name_in_the_server_manager_settings_is_a_conflict() {
    let profiles = [profile("iris", "http", "localhost", 52773)];
    assert!(name_conflicts("iris", &profiles));
    assert!(
        name_conflicts("IRIS", &profiles),
        "profile names compare without case"
    );
    assert!(!name_conflicts("mine", &profiles));
}

#[test]
fn saved_servers_json_is_owner_only() {
    #[cfg(unix)]
    {
        use iris_agentic_dev_core::iris::servers_config::{save_to_path, ServersConfig};
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("cfg").join("servers.json");
        save_to_path(&ServersConfig::default(), &path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let dir = std::fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir, 0o700);
    }
}

// ── T-R4b-U09: the keychain ──────────────────────────────────────────────────────────────────

fn seed_sm(name: &str, user: &str, pw: &str) {
    keyring_core::Entry::new(
        SM_SERVICE,
        &format!("credentialProvider:{name}/{}", user.to_lowercase()),
    )
    .unwrap()
    .set_password(pw)
    .unwrap();
}

#[test]
fn a_server_manager_secret_goes_only_to_its_profiles_own_endpoint() {
    with_mock_store(|| {
        seed_sm("iris", "_SYSTEM", SENTINEL);
        let profiles = [profile("iris", "http", "localhost", 52773)];

        let evil = ep("http", "evil.example", 52773);
        match sm_secret_for_in(&profiles, "iris", "_SYSTEM", &evil) {
            Err(SmSecretError::Withheld { profile, .. }) => assert_eq!(profile, "iris"),
            other => panic!("expected the secret withheld, got {other:?}"),
        }
        // Another scheme or port on the same host is another endpoint.
        for e in [ep("https", "localhost", 52773), ep("http", "localhost", 80)] {
            assert!(
                matches!(
                    sm_secret_for_in(&profiles, "iris", "_SYSTEM", &e),
                    Err(SmSecretError::Withheld { .. })
                ),
                "{}",
                e.base_url()
            );
        }
        let ok = sm_secret_for_in(
            &profiles,
            "iris",
            "_SYSTEM",
            &ep("http", "127.0.0.1", 52773),
        );
        assert_eq!(ok.unwrap(), SENTINEL, "loopback names are one host");
    });
}

#[test]
fn a_name_with_no_profile_has_no_server_manager_secret() {
    with_mock_store(|| {
        seed_sm("ghost", "_SYSTEM", SENTINEL);
        let r = sm_secret_for_in(&[], "ghost", "_SYSTEM", &ep("http", "localhost", 52773));
        assert!(matches!(r, Err(SmSecretError::NoProfile { .. })), "{r:?}");
    });
}

#[test]
fn iad_stores_its_own_secret_under_its_own_service_and_never_the_server_managers() {
    with_mock_store(|| {
        let e = ep("https", "iris.corp", 443);
        store_iad_credential("prod", "_SYSTEM", &e, SENTINEL).unwrap();

        // Found under iad's service, bound to the endpoint in the account.
        let account = iad_account("prod", "_SYSTEM", &e);
        assert!(account.contains("https://iris.corp:443"), "{account}");
        let direct = keyring_core::Entry::new(IAD_KEYCHAIN_SERVICE, &account).unwrap();
        assert_eq!(direct.get_password().unwrap(), SENTINEL);
        assert_eq!(IAD_KEYCHAIN_SERVICE, "iris-agentic-dev");

        // Nothing under the Server Manager's service.
        let sm = keyring_core::Entry::new(SM_SERVICE, "credentialProvider:prod/_system").unwrap();
        assert!(matches!(
            sm.get_password(),
            Err(keyring_core::Error::NoEntry)
        ));

        assert_eq!(
            resolve_iad_credential("prod", "_SYSTEM", &e).unwrap(),
            SENTINEL
        );
    });
}

#[test]
fn an_iad_secret_cannot_be_found_for_a_moved_endpoint() {
    with_mock_store(|| {
        store_iad_credential("x", "_SYSTEM", &ep("https", "iris.corp", 443), SENTINEL).unwrap();
        let moved = ep("http", "evil.example", 52773);
        assert!(resolve_iad_credential("x", "_SYSTEM", &moved).is_err());
    });
}

#[test]
fn removing_an_iad_server_deletes_iads_item_and_leaves_the_server_managers() {
    with_mock_store(|| {
        let e = ep("http", "localhost", 52773);
        seed_sm("dev", "_SYSTEM", "sm-pw");
        store_iad_credential("dev", "_SYSTEM", &e, "iad-pw").unwrap();

        delete_iad_credential("dev", "_SYSTEM", &e).unwrap();

        assert!(resolve_iad_credential("dev", "_SYSTEM", &e).is_err());
        let sm = keyring_core::Entry::new(SM_SERVICE, "credentialProvider:dev/_system").unwrap();
        assert_eq!(
            sm.get_password().unwrap(),
            "sm-pw",
            "the Server Manager's item is untouched"
        );
    });
}

#[test]
fn a_native_entry_reads_a_server_manager_secret_only_when_imported_and_still_matching() {
    use iris_agentic_dev_core::iris::connection_pool::native_password_in;
    with_mock_store(|| {
        seed_sm("iris", "_SYSTEM", SENTINEL);
        let profiles = [profile("iris", "http", "localhost", 52773)];

        // Imported (the marker iris_import_servers writes), same endpoint: gets it.
        let imported = old_entry(
            r#"{"host":"localhost","port":52773,"namespace":"USER","username":"_SYSTEM","credential":"server-manager"}"#,
        );
        assert_eq!(
            native_password_in("iris", &imported, &profiles).password,
            SENTINEL
        );

        // A native entry of the same name aimed elsewhere, with no marker: nothing.
        let native = old_entry(
            r#"{"host":"evil.example","port":52773,"namespace":"USER","username":"_SYSTEM"}"#,
        );
        let got = native_password_in("iris", &native, &profiles);
        assert_eq!(got.password, "");
        assert!(
            got.withheld.is_none(),
            "it never asked for the profile's secret"
        );

        // Marked as imported but moved since: withheld, and reported.
        let moved = old_entry(
            r#"{"host":"evil.example","port":52773,"namespace":"USER","username":"_SYSTEM","credential":"server-manager"}"#,
        );
        let got = native_password_in("iris", &moved, &profiles);
        assert_eq!(got.password, "");
        let item = got.withheld.expect("a set-aside item");
        assert_eq!(item.kind, SetAsideKind::CredentialWithheld);
        assert_eq!(item.id, "iris:server_manager");
        assert!(!format!("{item:?}").contains(SENTINEL));
    });
}

#[test]
fn a_plaintext_password_is_used_only_at_the_endpoint_it_was_saved_for() {
    use iris_agentic_dev_core::iris::connection_pool::native_password_in;
    with_mock_store(|| {
        let at_home = old_entry(OLD_PLAINTEXT);
        assert_eq!(native_password_in("a", &at_home, &[]).password, "old-pw");

        // The entry was edited to another host and kept the password.
        let moved = old_entry(
            r#"{"host":"evil.example","port":52773,"namespace":"USER","username":"_SYSTEM","password":"old-pw","password_endpoint":"http://a.example:52773"}"#,
        );
        let got = native_password_in("a", &moved, &[]);
        assert_eq!(got.password, "");
        assert_eq!(got.withheld.expect("reported").id, "a:stored_password");
    });
}

// ── T-R4b-U10: .vscode named servers ─────────────────────────────────────────────────────────

fn settings(
    dir: &std::path::Path,
    body: &str,
) -> iris_agentic_dev_core::iris::vscode_config::VsCodeSettings {
    let p = dir.join("settings.json");
    std::fs::write(&p, body).unwrap();
    parse_vscode_settings(&p).unwrap()
}

#[test]
fn a_workspace_named_server_at_another_endpoint_gets_no_keychain_password() {
    with_mock_store(|| {
        seed_sm("iris", "_SYSTEM", SENTINEL);
        let tmp = tempfile::tempdir().unwrap();
        let s = settings(
            tmp.path(),
            r#"{"objectscript.conn":{"active":true,"server":"iris","ns":"USER"},
                "intersystems.servers":{"iris":{"webServer":{"host":"evil.example","port":52773,"scheme":"http"}}}}"#,
        );
        let user_profiles = [profile("iris", "http", "localhost", 52773)];
        let resolved = s
            .connection_with_profiles(&user_profiles)
            .expect("a connection");
        assert!(
            !resolved.conn.password.contains(SENTINEL) && resolved.conn.password != SENTINEL,
            "the keychain password went to the workspace file's host"
        );
        let item = resolved.set_aside.first().expect("a set-aside item");
        assert_eq!(item.kind, SetAsideKind::CredentialWithheld);
        assert_eq!(item.id, "iris:server_manager");

        // The same definition as the user-level profile gets it.
        let s = settings(
            tmp.path(),
            r#"{"objectscript.conn":{"active":true,"server":"iris","ns":"USER"},
                "intersystems.servers":{"iris":{"webServer":{"host":"localhost","port":52773,"scheme":"http"}}}}"#,
        );
        let resolved = s
            .connection_with_profiles(&user_profiles)
            .expect("a connection");
        assert_eq!(resolved.conn.password, SENTINEL);
        assert!(resolved.set_aside.is_empty());
    });
}

#[test]
fn objectscript_conn_honours_https() {
    let tmp = tempfile::tempdir().unwrap();
    let s = settings(
        tmp.path(),
        r#"{"objectscript.conn":{"active":true,"host":"iris.corp","port":443,"https":true,"password":"pw","ns":"USER"}}"#,
    );
    let resolved = s.connection_with_profiles(&[]).expect("a connection");
    assert_eq!(resolved.conn.base_url, "https://iris.corp:443");

    let s = settings(
        tmp.path(),
        r#"{"objectscript.conn":{"active":true,"host":"iris.corp","port":443,"password":"pw","ns":"USER"}}"#,
    );
    assert_eq!(
        s.connection_with_profiles(&[]).unwrap().conn.base_url,
        "http://iris.corp:443"
    );
}
