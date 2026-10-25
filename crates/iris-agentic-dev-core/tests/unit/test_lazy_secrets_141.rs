//! Spec 141 T189: a pool member's keychain secrets are read the first time that member's
//! connection is used for a request, and not when the pool loads.
//!
//! Loading used to read every member's password (iad's own item, the Server Manager's item, the
//! service account's item). With many `servers.json` entries the first call that named a server
//! raised one macOS approval prompt per entry. The keychain is an in-memory store here
//! (`keyring_core::mock`) shared behind `testing::KEYSTORE_LOCK`; it stands in for the operating
//! system's keychain, not for IRIS. `take_secret_reads` is the read counter.

use iris_agentic_dev_core::iris::connection_pool::{load_pool_from, ConnectionPool};
use iris_agentic_dev_core::iris::endpoint::Endpoint;
use iris_agentic_dev_core::iris::server_manager::{
    iad_account, store_iad_credential, take_secret_reads, ServerManagerProfile,
    IAD_KEYCHAIN_SERVICE,
};
use iris_agentic_dev_core::iris::servers_config::ServersConfig;
use iris_agentic_dev_core::testing::{EnvGuard, KEYSTORE_LOCK};
use iris_agentic_dev_core::tools::{IrisTools, Toolset};
use serde_json::json;
use std::sync::Arc;

const SM_SERVICE: &str = "intersystems-server-credentials";

fn with_mock_store<F: FnOnce()>(f: F) {
    let _guard = KEYSTORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // `load_pool_from` adds an `_env` member when `IRIS_HOST` is set, as it is for a live run, and
    // that sixth member broke the pool-of-five count. Start from no behavior variables at all.
    let _env = EnvGuard::clean();
    keyring_core::set_default_store(keyring_core::mock::Store::new().unwrap());
    let _ = take_secret_reads();
    f();
    keyring_core::set_default_store(keyring_core::mock::Store::new().unwrap());
}

fn ep(port: u16) -> Endpoint {
    Endpoint::parse(Some("http"), "127.0.0.1", port, None).unwrap()
}

/// The `service/account` string a read of iad's own item for `name` at port `port` is logged as.
fn iad_read(name: &str, user: &str, port: u16) -> String {
    format!(
        "{IAD_KEYCHAIN_SERVICE}/{}",
        iad_account(name, user, &ep(port))
    )
}

fn sm_read(name: &str) -> String {
    format!("{SM_SERVICE}/credentialProvider:{name}/_system")
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v
}

/// Five entries on 127.0.0.1, ports 1 to 5 (nothing listens there, so a call fails fast):
/// `dev`, `qa` and `prod` keep iad's own password, `prod` also has a service account, `legacy` was
/// imported from the Server Manager, `plain` keeps a plaintext password for its own endpoint.
const SERVERS_JSON: &str = r#"{"version":1,"servers":{
  "dev":{"host":"127.0.0.1","port":1,"namespace":"USER","username":"_SYSTEM"},
  "qa":{"host":"127.0.0.1","port":2,"namespace":"USER","username":"_SYSTEM"},
  "prod":{"host":"127.0.0.1","port":3,"namespace":"USER","username":"_SYSTEM",
          "service_username":"iad_svc","container":"prod-db"},
  "legacy":{"host":"127.0.0.1","port":4,"namespace":"USER","username":"_SYSTEM",
            "credential":"server-manager"},
  "plain":{"host":"127.0.0.1","port":5,"namespace":"USER","username":"_SYSTEM",
           "password":"plain-pw","password_endpoint":"http://127.0.0.1:5"}
}}"#;

fn profile(name: &str, port: u16) -> ServerManagerProfile {
    ServerManagerProfile {
        name: name.to_string(),
        host: "127.0.0.1".to_string(),
        port,
        scheme: "http".to_string(),
        path_prefix: None,
        username: "_SYSTEM".to_string(),
        password_deprecated: None,
    }
}

fn seed_keychain() {
    store_iad_credential("dev", "_SYSTEM", &ep(1), "dev-pw").unwrap();
    store_iad_credential("qa", "_SYSTEM", &ep(2), "qa-pw").unwrap();
    store_iad_credential("prod", "_SYSTEM", &ep(3), "prod-pw").unwrap();
    store_iad_credential("prod", "iad_svc", &ep(3), "prod-svc-pw").unwrap();
    keyring_core::Entry::new(SM_SERVICE, "credentialProvider:legacy/_system")
        .unwrap()
        .set_password("legacy-pw")
        .unwrap();
}

fn five_member_pool() -> ConnectionPool {
    let cfg: ServersConfig = serde_json::from_str(SERVERS_JSON).expect("servers.json parses");
    load_pool_from(cfg, vec![profile("legacy", 4)], None)
}

fn tools_over(pool: ConnectionPool) -> IrisTools {
    let tools = IrisTools::new_with_toolset(None, Toolset::Merged).unwrap();
    *tools.pool.write().unwrap() = Arc::new(pool);
    tools
}

#[test]
fn loading_a_pool_of_five_reads_no_secret() {
    with_mock_store(|| {
        seed_keychain();
        let pool = five_member_pool();
        assert_eq!(pool.len(), 5);
        assert_eq!(
            take_secret_reads(),
            Vec::<String>::new(),
            "loading the pool must not touch the keychain"
        );
        // Listing a member (names, source, default) is not a request either.
        let _ = (pool.names(), pool.source_of("prod"), pool.default_name());
        assert_eq!(take_secret_reads(), Vec::<String>::new());
    });
}

#[test]
fn the_first_use_of_a_member_reads_that_members_secrets_and_no_others() {
    with_mock_store(|| {
        seed_keychain();
        let pool = five_member_pool();
        let _ = take_secret_reads();

        let prod = pool.get(Some("prod")).expect("prod is in the pool");
        assert_eq!(prod.password, "prod-pw");
        assert_eq!(prod.member.service_username.as_deref(), Some("iad_svc"));
        assert_eq!(prod.member.service_password.as_deref(), Some("prod-svc-pw"));
        assert_eq!(prod.member.container.as_deref(), Some("prod-db"));
        assert_eq!(
            sorted(take_secret_reads()),
            sorted(vec![
                iad_read("prod", "_SYSTEM", 3),
                iad_read("prod", "iad_svc", 3)
            ]),
            "exactly prod's own password and prod's service password"
        );

        // Cached per member: a second request reads nothing, and is the same connection.
        let again = pool.get(Some("prod")).unwrap();
        assert!(Arc::ptr_eq(&prod, &again));
        assert_eq!(take_secret_reads(), Vec::<String>::new());
    });
}

#[test]
fn an_imported_member_reads_iads_item_then_the_server_managers_and_only_its_own() {
    with_mock_store(|| {
        seed_keychain();
        let pool = five_member_pool();
        let _ = take_secret_reads();

        let legacy = pool.get(Some("legacy")).unwrap();
        assert_eq!(legacy.password, "legacy-pw");
        assert_eq!(
            sorted(take_secret_reads()),
            sorted(vec![iad_read("legacy", "_SYSTEM", 4), sm_read("legacy")])
        );
    });
}

#[test]
fn a_plaintext_member_still_tries_iads_item_first_and_reads_nothing_of_the_others() {
    with_mock_store(|| {
        seed_keychain();
        let pool = five_member_pool();
        let _ = take_secret_reads();

        let plain = pool.get(Some("plain")).unwrap();
        assert_eq!(plain.password, "plain-pw");
        assert_eq!(
            take_secret_reads(),
            vec![iad_read("plain", "_SYSTEM", 5)],
            "iad's own item has precedence over the file's password"
        );
    });
}

/// Runs `body` on a current-thread runtime, so the read log (per thread) sees every read.
fn block_on<F: std::future::Future<Output = ()>>(body: F) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(body);
}

#[test]
fn a_tool_call_naming_one_member_reads_only_that_members_secrets() {
    with_mock_store(|| {
        seed_keychain();
        let tools = tools_over(five_member_pool());
        let _ = take_secret_reads();

        // The call fails (nothing listens on port 3); what matters is whose secrets it read.
        block_on(async {
            let _ = tools
                .call_gated("iris_query", json!({"query": "SELECT 1", "server": "prod"}))
                .await;
        });

        let reads = sorted(take_secret_reads());
        assert!(!reads.is_empty(), "the call must have resolved prod");
        for read in &reads {
            assert!(
                read.contains("prod/"),
                "a call on prod read someone else's secret: {read} (all reads: {reads:?})"
            );
        }
    });
}

#[test]
fn listing_servers_and_checking_config_read_no_secret() {
    with_mock_store(|| {
        seed_keychain();
        let tools = tools_over(five_member_pool());
        let _ = take_secret_reads();

        block_on(async {
            let listing = tools
                .call_gated("iris_servers", json!({}))
                .await
                .expect("iris_servers answers");
            let listing = format!("{listing:?}");
            for name in ["dev", "qa", "prod", "legacy", "plain"] {
                assert!(listing.contains(name), "{name} missing from {listing}");
            }
            let _ = tools.call_gated("check_config", json!({})).await;
        });

        assert_eq!(
            take_secret_reads(),
            Vec::<String>::new(),
            "iris_servers and check_config only list members"
        );
    });
}

// ── The US4 rules hold when the secret is read late ─────────────────────────────────────────

#[test]
fn a_moved_endpoint_never_gets_the_old_password_when_resolved_late() {
    with_mock_store(|| {
        seed_keychain();
        // `legacy` was imported from the Server Manager profile at port 4; the entry now points at
        // port 9. `plain` keeps a password saved for port 5 and now points at port 8.
        let cfg: ServersConfig = serde_json::from_str(
            r#"{"version":1,"servers":{
  "legacy":{"host":"127.0.0.1","port":9,"namespace":"USER","username":"_SYSTEM",
            "credential":"server-manager"},
  "plain":{"host":"127.0.0.1","port":8,"namespace":"USER","username":"_SYSTEM",
           "password":"plain-pw","password_endpoint":"http://127.0.0.1:5"}
}}"#,
        )
        .unwrap();
        let pool = load_pool_from(cfg, vec![profile("legacy", 4)], None);
        let _ = take_secret_reads();
        assert!(
            pool.withheld_credentials().is_empty(),
            "nothing is judged before the member is used"
        );

        let legacy = pool.get(Some("legacy")).unwrap();
        let plain = pool.get(Some("plain")).unwrap();
        assert_eq!(legacy.password, "", "the profile's secret stays home");
        assert_eq!(
            plain.password, "",
            "a password saved for port 5 is not sent to 8"
        );
        assert!(
            !take_secret_reads().contains(&sm_read("legacy")),
            "the Server Manager item was never read for a moved endpoint"
        );

        let ids: Vec<String> = pool
            .withheld_credentials()
            .iter()
            .map(|i| i.id.clone())
            .collect();
        assert_eq!(
            sorted(ids),
            vec!["legacy:server_manager", "plain:stored_password"]
        );
    });
}

#[test]
fn a_server_manager_profile_member_reads_its_secret_on_first_use() {
    with_mock_store(|| {
        seed_keychain();
        // A profile no servers.json entry shadows.
        keyring_core::Entry::new(SM_SERVICE, "credentialProvider:vsonly/_system")
            .unwrap()
            .set_password("vsonly-pw")
            .unwrap();
        let cfg: ServersConfig = serde_json::from_str(r#"{"version":1,"servers":{}}"#).unwrap();
        let pool = load_pool_from(cfg, vec![profile("vsonly", 6), profile("other", 7)], None);
        assert_eq!(pool.source_of("vsonly"), "vscode");
        assert_eq!(take_secret_reads(), Vec::<String>::new());

        assert_eq!(pool.get(Some("vsonly")).unwrap().password, "vsonly-pw");
        assert_eq!(take_secret_reads(), vec![sm_read("vsonly")]);
    });
}
