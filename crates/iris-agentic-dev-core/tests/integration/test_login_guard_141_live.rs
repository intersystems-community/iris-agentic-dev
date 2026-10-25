//! T-R2-L02: the login guard against a real IRIS (spec 141, R2 section 1b).
//!
//! `docker exec` into `iris session` runs as the container's OS-authenticated user, which holds
//! `%All`. The first stdin line drops to the service account. These tests measure that on
//! `iris-dev-iris`: the roles after the drop, whether the dropped process can climb back, what a
//! class save does, and what a disabled account leaves behind.
//!
//! Serial, `#[ignore]`:
//! `lockf -k /tmp/iad-141-iris.lock cargo test --features testing --test integration login_guard_141 -- --ignored --test-threads=1`

use crate::support_identity_141::{Account, Container};
use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection, TerminalTarget};
use iris_agentic_dev_core::tools::exec::MarkerError;

const USER: &str = "iadT141L02";
const PW: &str = "Pw141L02x";
/// `%Development:USE` is what the terminal needs after the login (measured on 2026.2): without it
/// the session ends with `Access Denied`.
const RESOURCES: &str = "%DB_USER:R,%Development:U";

fn conn() -> IrisConnection {
    IrisConnection::new(
        "http://127.0.0.1:1",
        "USER",
        "_SYSTEM",
        "SYS",
        DiscoverySource::ExplicitFlag,
    )
}

fn target(container: &Container, login: Option<&str>) -> TerminalTarget {
    TerminalTarget {
        container: container.name.clone(),
        ssh_host: None,
        login: login.map(str::to_string),
        ssh_strict: false,
    }
}

/// The guard on `container`, which must be running. `optional` containers (the AI Hub build) are
/// measured when present and named in the output when not.
async fn check_guard(container: &Container) -> String {
    let acct = Account::create(container, USER, PW, RESOURCES);
    let c = conn();

    // 1. Who runs, with which roles.
    let out = c
        .execute_terminal(
            "Write \"WHO=\",$USERNAME,\"|\",$ROLES,!",
            "USER",
            &target(container, Some(USER)),
        )
        .await
        .expect("the guard must let a working account through");
    assert_eq!(out.ran_as.as_deref(), Some(USER), "marker: {out:?}");
    let who = out
        .text
        .lines()
        .find_map(|l| l.trim().strip_prefix("WHO="))
        .unwrap_or_else(|| panic!("no WHO line in {:?}", out.text));
    assert!(who.starts_with(&format!("{USER}|")), "{who}");
    assert!(
        !who.contains("%All"),
        "the dropped process still holds %All: {who}"
    );

    // 2. It cannot climb back.
    let out = c
        .execute_terminal(
            "Write \"CLIMB=\",$SYSTEM.Security.Login(\"irisowner\"),\"|\",$USERNAME,!",
            "USER",
            &target(container, Some(USER)),
        )
        .await
        .unwrap();
    let climb = out
        .text
        .lines()
        .find_map(|l| l.trim().strip_prefix("CLIMB="))
        .unwrap_or_else(|| panic!("no CLIMB line in {:?}", out.text));
    assert!(
        climb.starts_with("0|"),
        "Login(\"irisowner\") must return 0: {climb}"
    );
    assert!(
        climb.ends_with(&format!("|{USER}")),
        "the user changed after a refused login: {climb}"
    );

    // 3. A class save is refused.
    let out = c
        .execute_terminal(
            "Set c=##class(%Dictionary.ClassDefinition).%New(\"IadT141.L02Class\") Write \"SAVE=\",$SYSTEM.Status.GetErrorText(c.%Save()),!",
            "USER",
            &target(container, Some(USER)),
        )
        .await
        .unwrap();
    assert!(
        out.text.contains("PROTECT"),
        "a class save must be refused: {}",
        out.text
    );
    let exists = container.session(
        "USER",
        "Write \"EX=\",##class(%Dictionary.ClassDefinition).%ExistsId(\"IadT141.L02Class\"),!",
    );
    assert!(
        exists.contains("EX=0"),
        "the refused save left a class: {exists}"
    );

    // 4. A disabled account ends the process before the next line.
    acct.disable();
    container.session("USER", "Kill ^IadT141L02");
    let err = c
        .execute_terminal("Set ^IadT141L02=1", "USER", &target(container, Some(USER)))
        .await
        .expect_err("a disabled account must not run code");
    let marker = err
        .downcast_ref::<MarkerError>()
        .unwrap_or_else(|| panic!("expected a MarkerError, got {err}"));
    assert_eq!(marker.code(), "SERVICE_ACCOUNT_LOGIN_FAILED", "{marker}");
    assert_eq!(
        container.global("USER", "^IadT141L02"),
        "UNSET",
        "the sentinel was written, so the code ran after a failed login"
    );
    who.to_string()
}

#[tokio::test]
#[ignore]
async fn the_login_guard_drops_privilege_and_cannot_be_undone_on_iris_dev_iris() {
    let who = check_guard(&Container::live()).await;
    assert!(who.starts_with(USER) && !who.contains("%All"), "{who}");
}

#[tokio::test]
#[ignore]
async fn the_login_guard_on_the_ai_hub_container_when_it_is_running() {
    let hub = Container::named("iad-aihub-iris");
    if !hub.running() {
        eprintln!("iad-aihub-iris is not running; the guard was measured on iris-dev-iris only");
        return;
    }
    let who = check_guard(&hub).await;
    assert!(who.starts_with(USER) && !who.contains("%All"), "{who}");
}

#[tokio::test]
#[ignore]
async fn an_account_without_development_use_prints_the_marker_and_runs_nothing() {
    let container = Container::live();
    let user = "iadT141L02b";
    let _acct = Account::create(&container, user, PW, "%DB_USER:RW");
    container.session("USER", "Kill ^IadT141L02b");
    let err = conn()
        .execute_terminal(
            "Set ^IadT141L02b=$USERNAME",
            "USER",
            &target(&container, Some(user)),
        )
        .await
        .expect_err("the terminal ends the session after the login");
    let marker = err.downcast_ref::<MarkerError>().expect("a MarkerError");
    assert_eq!(marker.code(), "SERVICE_ACCOUNT_TERMINAL_DENIED", "{marker}");
    assert_eq!(container.global("USER", "^IadT141L02b"), "UNSET");
}

#[tokio::test]
#[ignore]
async fn without_a_service_account_the_terminal_reports_the_os_user() {
    let container = Container::live();
    let out = conn()
        .execute_terminal(
            "Write \"WHO=\",$USERNAME,\"|\",$ROLES,!",
            "USER",
            &target(&container, None),
        )
        .await
        .unwrap();
    let ran_as = out.ran_as.expect("the probe line names the OS user");
    let who = out
        .text
        .lines()
        .find_map(|l| l.trim().strip_prefix("WHO="))
        .unwrap();
    assert!(who.starts_with(&format!("{ran_as}|")), "{who} vs {ran_as}");
}
