//! Spec 140: which source chooses the connection. A project's `.iris-agentic-dev.toml` outranks
//! `IRIS_*`; the same file in the home directory is a personal default and yields to `IRIS_HOST`.
//! The binary tests are in `bin/tests/integration/binary_140_config_precedence.rs`.

use iris_agentic_dev_core::iris::workspace_config::file_chooses_connection;
use std::path::Path;

#[test]
fn a_project_file_chooses_over_iris_host() {
    let home = Path::new("/home/me");
    let cfg = Path::new("/home/me/work/app/.iris-agentic-dev.toml");
    assert!(file_chooses_connection(cfg, Some(home), true));
    assert!(file_chooses_connection(cfg, Some(home), false));
}

#[test]
fn a_home_directory_file_yields_to_iris_host_only() {
    let home = Path::new("/home/me");
    let cfg = Path::new("/home/me/.iris-agentic-dev.toml");
    assert!(!file_chooses_connection(cfg, Some(home), true));
    assert!(file_chooses_connection(cfg, Some(home), false));
}

#[test]
fn with_no_known_home_every_file_is_a_project_file() {
    let cfg = Path::new("/home/me/.iris-agentic-dev.toml");
    assert!(file_chooses_connection(cfg, None, true));
}

/// On macOS `/var` is a link to `/private/var`, so the walk up from the working directory and
/// `$HOME` can name the same directory two ways. They still count as one.
#[cfg(unix)]
#[test]
fn a_home_reached_through_a_symlink_is_still_home() {
    let real = tempfile::tempdir().unwrap();
    let link_parent = tempfile::tempdir().unwrap();
    let link = link_parent.path().join("home-link");
    std::os::unix::fs::symlink(real.path(), &link).unwrap();
    let cfg = real.path().join(".iris-agentic-dev.toml");
    std::fs::write(&cfg, "host = \"x\"\n").unwrap();
    assert!(!file_chooses_connection(&cfg, Some(&link), true));
}

/// A workspace file is checked in with the repository, so its `host` is the repository's choice.
/// The operator's `IRIS_USERNAME`/`IRIS_PASSWORD` go to it only when it is this machine or the
/// server `IRIS_HOST` already names. Otherwise a cloned repository could send the operator's
/// password anywhere, and from 1.6.3 the file outranks `IRIS_HOST` (AVA scan, 2026-10-22).
#[test]
fn env_credentials_go_only_to_loopback_or_the_iris_host_server() {
    use iris_agentic_dev_core::iris::workspace_config::env_credentials_may_go_to;
    for host in [
        "localhost",
        "LOCALHOST",
        "127.0.0.1",
        "127.4.5.6",
        "::1",
        "[::1]",
    ] {
        assert!(env_credentials_may_go_to(host, None), "{host} is loopback");
    }
    assert!(env_credentials_may_go_to("iris.corp", Some("iris.corp")));
    assert!(env_credentials_may_go_to("IRIS.corp ", Some("iris.CORP")));
    assert!(!env_credentials_may_go_to("evil.example", None));
    assert!(!env_credentials_may_go_to(
        "evil.example",
        Some("iris.corp")
    ));
    assert!(!env_credentials_may_go_to("localhost.evil.example", None));
    assert!(!env_credentials_may_go_to("127.0.0.1.evil.example", None));
}
