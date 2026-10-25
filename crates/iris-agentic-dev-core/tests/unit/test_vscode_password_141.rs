//! Spec 141 T204: a `.vscode/settings.json` entry whose password cannot be read builds no
//! connection, and the missing password is reported. It never becomes `SYS`.
//!
//! A repository file can name any host. Before this, a named server with no password in the file,
//! no keychain entry (or no keychain at all) and a direct `objectscript.conn` host with no password
//! both connected with the guess `SYS`: a credential nobody supplied, sent to an address the
//! repository chose, and a lockout waiting to happen on an account that has a different password.
//!
//! The keychain is the in-memory store from `keyring_core::mock` behind `testing::KEYSTORE_LOCK`;
//! it stands in for the operating system's keychain, not for IRIS.

use iris_agentic_dev_core::iris::config_sources::SetAsideKind;
use iris_agentic_dev_core::iris::discovery::{discover_vscode_settings_in, discovery_set_aside};
use iris_agentic_dev_core::iris::server_manager::ServerManagerProfile;
use iris_agentic_dev_core::iris::vscode_config::{parse_vscode_settings, VsCodeResolution};
use iris_agentic_dev_core::testing::KEYSTORE_LOCK;

const SM_SERVICE: &str = "intersystems-server-credentials";
const SENTINEL: &str = "Zq9-T204-pw";

enum Keychain {
    Empty,
    Seeded(&'static str, &'static str),
    Absent,
}

fn with_keychain<F: FnOnce()>(kc: Keychain, f: F) {
    let _guard = KEYSTORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    match kc {
        Keychain::Empty | Keychain::Seeded(..) => {
            keyring_core::set_default_store(keyring_core::mock::Store::new().unwrap());
        }
        Keychain::Absent => {
            keyring_core::unset_default_store();
        }
    }
    if let Keychain::Seeded(name, user) = kc {
        keyring_core::Entry::new(
            SM_SERVICE,
            &format!("credentialProvider:{name}/{}", user.to_lowercase()),
        )
        .unwrap()
        .set_password(SENTINEL)
        .unwrap();
    }
    f();
    keyring_core::set_default_store(keyring_core::mock::Store::new().unwrap());
}

fn profile(name: &str, host: &str, port: u16) -> ServerManagerProfile {
    ServerManagerProfile {
        name: name.to_string(),
        host: host.to_string(),
        port,
        scheme: "http".to_string(),
        path_prefix: None,
        username: "_SYSTEM".to_string(),
        password_deprecated: None,
    }
}

fn settings(
    dir: &std::path::Path,
    body: &str,
) -> iris_agentic_dev_core::iris::vscode_config::VsCodeSettings {
    let p = dir.join("settings.json");
    std::fs::write(&p, body).unwrap();
    parse_vscode_settings(&p).unwrap()
}

/// A named server with no `password` field, at the address of the user-level profile of that name.
const NAMED: &str = r#"{"objectscript.conn":{"active":true,"server":"dev-iris","ns":"USER"},
    "intersystems.servers":{"dev-iris":{"webServer":{"host":"localhost","port":52773},
    "username":"_SYSTEM"}}}"#;

/// A direct host with no `password` field.
const DIRECT: &str = r#"{"objectscript.conn":{"active":true,"host":"iris.corp","port":52773,
    "username":"_SYSTEM","ns":"USER"}}"#;

fn missing(r: VsCodeResolution) -> iris_agentic_dev_core::iris::config_sources::SetAsideItem {
    match r {
        VsCodeResolution::PasswordMissing(item) => item,
        VsCodeResolution::Connection(c) => {
            panic!(
                "expected no connection; got one with password {:?}",
                c.conn.password
            )
        }
        VsCodeResolution::NotConfigured => panic!("expected the missing password to be reported"),
    }
}

#[test]
fn a_named_server_with_no_keychain_entry_builds_no_connection() {
    with_keychain(Keychain::Empty, || {
        let tmp = tempfile::tempdir().unwrap();
        let s = settings(tmp.path(), NAMED);
        let profiles = [profile("dev-iris", "localhost", 52773)];
        assert!(
            s.connection_with_profiles(&profiles).is_none(),
            "no password was supplied, so there is no connection to build"
        );
        let item = missing(s.resolve_with_profiles(&profiles));
        assert_eq!(item.kind, SetAsideKind::CredentialWithheld);
        assert_eq!(item.id, "dev-iris:password_missing");
        assert_eq!(item.server.as_deref(), Some("dev-iris"));
        assert!(item.reason.contains("password"), "{item:?}");
        assert!(
            !item.reason.contains("SYS"),
            "the report must not suggest a guessed password: {item:?}"
        );
        assert!(!item.lift.is_empty(), "it says how to fix it: {item:?}");
    });
}

#[test]
fn a_named_server_with_no_keychain_at_all_builds_no_connection() {
    with_keychain(Keychain::Absent, || {
        let tmp = tempfile::tempdir().unwrap();
        let s = settings(tmp.path(), NAMED);
        let profiles = [profile("dev-iris", "localhost", 52773)];
        let item = missing(s.resolve_with_profiles(&profiles));
        assert_eq!(item.id, "dev-iris:password_missing");
        assert!(
            item.reason.contains("keychain"),
            "it says the keychain could not be read: {item:?}"
        );
    });
}

#[test]
fn a_named_server_with_no_user_level_profile_builds_no_connection() {
    with_keychain(Keychain::Seeded("dev-iris", "_SYSTEM"), || {
        let tmp = tempfile::tempdir().unwrap();
        let s = settings(tmp.path(), NAMED);
        // The secret exists, but no profile of that name vouches for the address.
        let item = missing(s.resolve_with_profiles(&[]));
        assert_eq!(item.id, "dev-iris:password_missing");
        assert!(
            !format!("{item:?}").contains(SENTINEL),
            "a report never carries a secret"
        );
    });
}

#[test]
fn a_direct_host_with_no_password_builds_no_connection() {
    with_keychain(Keychain::Empty, || {
        let tmp = tempfile::tempdir().unwrap();
        let s = settings(tmp.path(), DIRECT);
        assert!(s.connection_with_profiles(&[]).is_none());
        let item = missing(s.resolve_with_profiles(&[]));
        assert_eq!(item.id, "default:password_missing");
        assert!(item.reason.contains("iris.corp"), "{item:?}");
    });
}

// ── the controls: a password that was supplied still connects ───────────────────────────────────

#[test]
fn a_password_in_the_file_still_connects() {
    with_keychain(Keychain::Empty, || {
        let tmp = tempfile::tempdir().unwrap();
        let s = settings(
            tmp.path(),
            r#"{"objectscript.conn":{"active":true,"host":"iris.corp","port":52773,
                "username":"_SYSTEM","password":"from-file","ns":"USER"}}"#,
        );
        let c = s.connection_with_profiles(&[]).expect("a connection");
        assert_eq!(c.conn.password, "from-file");
    });
}

#[test]
fn a_keychain_entry_at_the_profiles_own_address_still_connects() {
    with_keychain(Keychain::Seeded("dev-iris", "_SYSTEM"), || {
        let tmp = tempfile::tempdir().unwrap();
        let s = settings(tmp.path(), NAMED);
        let c = s
            .connection_with_profiles(&[profile("dev-iris", "localhost", 52773)])
            .expect("a connection");
        assert_eq!(c.conn.password, SENTINEL);
        assert!(c.set_aside.is_empty());
    });
}

// ── discovery reports it, and check_config shows the report ────────────────────────────────────

// A `current_thread` test: holding the std lock across the await blocks nothing else.
#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn discovery_builds_nothing_and_keeps_the_report_for_check_config() {
    // The settings file is read from a directory, not the process's working directory.
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".vscode")).unwrap();
    std::fs::write(tmp.path().join(".vscode/settings.json"), DIRECT).unwrap();

    let found = {
        let _guard = KEYSTORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        keyring_core::set_default_store(keyring_core::mock::Store::new().unwrap());
        discover_vscode_settings_in(tmp.path()).await
    };
    assert!(found.is_none(), "{found:?}");
    let items = discovery_set_aside();
    let item = items
        .iter()
        .find(|i| i.id == "default:password_missing")
        .unwrap_or_else(|| panic!("the missing password is reported: {items:?}"));
    assert_eq!(item.kind, SetAsideKind::CredentialWithheld);

    // `check_config` lists it under `set_aside`.
    let rig = crate::support_gate_141::rig("");
    let cfg = crate::support_gate_141::body_of(
        &rig.tools
            .call_for_test("check_config", serde_json::json!({}))
            .await
            .unwrap(),
    );
    let listed: Vec<&str> = cfg["set_aside"]
        .as_array()
        .expect("set_aside is a list")
        .iter()
        .filter_map(|i| i["id"].as_str())
        .collect();
    assert!(
        listed.contains(&"default:password_missing"),
        "check_config must report the missing password: {cfg}"
    );

    // A second discovery over a file that supplies a password replaces the report.
    std::fs::write(
        tmp.path().join(".vscode/settings.json"),
        r#"{"objectscript.conn":{"active":true,"host":"iris.corp","port":52773,
            "username":"_SYSTEM","password":"pw","ns":"USER"}}"#,
    )
    .unwrap();
    let found = discover_vscode_settings_in(tmp.path()).await;
    assert_eq!(found.expect("a connection").password, "pw");
    assert!(
        discovery_set_aside()
            .iter()
            .all(|i| i.id != "default:password_missing"),
        "the old report is gone"
    );
}
