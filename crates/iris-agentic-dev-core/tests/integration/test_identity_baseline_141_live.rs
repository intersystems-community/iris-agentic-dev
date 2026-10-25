//! T-R2-L00: who the docker path runs as when no service account is set (spec 141, R2).
//!
//! `docker_only` against `iris-dev-iris`, no `IRIS_SERVICE_USERNAME`. The response names the
//! OS-authenticated identity, and what IRIS reports (`$USERNAME`) is what `identity.user` holds.
//! The observed value is recorded in `research-R2-identity.md`.
//!
//! `lockf -k /tmp/iad-141-iris.lock cargo test --features testing --test integration identity_baseline_141 -- --ignored --test-threads=1`

use crate::support_identity_141::Container;
use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::tools::exec::{ExecContext, ExecRoute, IdentitySource, RunOpts};
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
#[ignore]
async fn docker_only_without_a_service_account_reports_the_os_user_it_ran_as() {
    let container = Container::live();
    let conn = Arc::new(IrisConnection::new(
        "http://127.0.0.1:1",
        "USER",
        "_SYSTEM",
        "SYS",
        DiscoverySource::ExplicitFlag,
    ));
    let mut cx = ExecContext::primary(
        conn.clone(),
        Arc::new(IrisConnection::http_client().unwrap()),
    );
    cx.route = ExecRoute::DockerOnly {
        container: container.name.clone(),
        ssh_host: None,
    };

    let out = cx
        .run(
            "Write \"WHO=\",$USERNAME,\"|\",$ROLES,!",
            "USER",
            &RunOpts::with_terminal(Duration::from_secs(60), "IrisDevRun"),
        )
        .await
        .expect("a docker_only run with no service account");

    let who = out
        .output
        .lines()
        .find_map(|l| l.trim().strip_prefix("WHO="))
        .unwrap_or_else(|| panic!("no WHO line in {:?}", out.output));
    let (user, roles) = who.split_once('|').expect("USER|ROLES");
    assert_eq!(
        out.identity.user, user,
        "identity.user is what IRIS reported"
    );
    assert!(out.identity.verified);
    assert_eq!(out.identity.source, IdentitySource::Primary);
    assert_eq!(out.identity.path.as_str(), "docker_exec_local");
    // Recorded in research-R2-identity.md, section 1b: the container's OS user holds %All.
    eprintln!("L00 observed identity: {user} roles: {roles}");
    assert!(
        roles.contains("%All"),
        "the OS-authenticated identity is expected to hold %All, got {roles}"
    );
}
