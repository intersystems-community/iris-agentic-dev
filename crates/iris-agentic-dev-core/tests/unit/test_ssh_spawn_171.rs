//! S12: the SSH leg of the terminal path starts `ssh` with `BatchMode=yes` and a host key check.
//!
//! `test_exec_route_141` checks the argument list `ssh_args` builds. This test checks the process
//! that is actually started: a stand-in `ssh` on `PATH` writes the argv it was given to a file, so
//! a call site that built its own command line, or dropped the `-o` options, fails here. The
//! stand-in is not IRIS and answers nothing; the call's result is not what is asserted.

#![cfg(unix)]

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection, TerminalTarget};
use iris_agentic_dev_core::testing::EnvGuard;
use std::os::unix::fs::PermissionsExt;

/// Run one SSH terminal call through a stand-in `ssh` and return the argv it recorded.
async fn argv_for(strict: bool) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("argv.txt");
    let ssh = dir.path().join("ssh");
    std::fs::write(
        &ssh,
        "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"$a\"; done > \"$IAD_FAKE_SSH_OUT\"\ncat > /dev/null\n",
    )
    .unwrap();
    std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o755)).unwrap();

    let mut env = EnvGuard::new();
    let path = format!(
        "{}:{}",
        dir.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    env.set("PATH", &path)
        .set("IAD_FAKE_SSH_OUT", out.to_str().unwrap());

    let conn = IrisConnection::new(
        "http://127.0.0.1:1",
        "USER",
        "u",
        "p",
        DiscoverySource::ExplicitFlag,
    );
    let target = TerminalTarget {
        container: "iris171".to_string(),
        ssh_host: Some("ssh171.example".to_string()),
        login: None,
        ssh_strict: strict,
    };
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        conn.execute_terminal("Write 1", "USER", &target),
    )
    .await;
    std::fs::read_to_string(&out)
        .expect("the stand-in ssh was never started")
        .lines()
        .map(str::to_string)
        .collect()
}

#[tokio::test]
async fn the_ssh_process_gets_batch_mode_and_accept_new_by_default() {
    let argv = argv_for(false).await;
    let joined = argv.join(" ");
    assert!(joined.contains("-o BatchMode=yes"), "{argv:?}");
    assert!(
        joined.contains("-o StrictHostKeyChecking=accept-new"),
        "{argv:?}"
    );
    assert!(!joined.contains("StrictHostKeyChecking=no"), "{argv:?}");
    let host_at = argv.iter().position(|a| a == "ssh171.example").unwrap();
    assert!(
        argv[host_at + 1].contains("docker exec"),
        "the remote command follows the host: {argv:?}"
    );
}

#[tokio::test]
async fn the_ssh_process_requires_a_known_host_when_the_server_is_strict() {
    let argv = argv_for(true).await;
    let joined = argv.join(" ");
    assert!(joined.contains("-o BatchMode=yes"), "{argv:?}");
    assert!(joined.contains("-o StrictHostKeyChecking=yes"), "{argv:?}");
    assert!(!joined.contains("accept-new"), "{argv:?}");
}
