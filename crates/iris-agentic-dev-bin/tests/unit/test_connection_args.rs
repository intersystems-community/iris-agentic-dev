use clap::Parser;
use iris_agentic_dev::cmd::connection_args::ConnectionArgs;
use iris_agentic_dev_core::testing::EnvGuard;

// Minimal CLI wrapper used by these tests to exercise ConnectionArgs parsing.
#[derive(Parser)]
struct TestCli {
    #[command(flatten)]
    conn: ConnectionArgs,
}

fn parse(args: &[&str]) -> ConnectionArgs {
    let mut argv = vec!["test-cli"];
    argv.extend_from_slice(args);
    TestCli::parse_from(argv).conn
}

#[test]
fn test_defaults_are_none_except_namespace_and_web_port() {
    let mut env_guard = EnvGuard::new();
    // Remove env vars so clap env-defaults don't interfere; the guard puts them back.
    for k in [
        "IRIS_HOST",
        "IRIS_WEB_PORT",
        "IRIS_USERNAME",
        "IRIS_PASSWORD",
        "IRIS_NAMESPACE",
        "IRIS_CONTAINER",
    ] {
        env_guard.unset(k);
    }

    let c = parse(&[]);
    assert!(c.host.is_none());
    assert_eq!(c.web_port, 52773);
    assert_eq!(c.namespace, "USER");
    assert!(c.username.is_none());
    assert!(c.password.is_none());
    assert!(c.container.is_none());
}

#[test]
fn test_host_flag_parsed() {
    let c = parse(&["--host", "myserver"]);
    assert_eq!(c.host.as_deref(), Some("myserver"));
}

#[test]
fn test_port_flag_parsed() {
    let c = parse(&["--web-port", "52780"]);
    assert_eq!(c.web_port, 52780);
}

#[test]
fn test_namespace_flag_short() {
    let c = parse(&["-n", "MYNS"]);
    assert_eq!(c.namespace, "MYNS");
}

#[test]
fn test_namespace_flag_long() {
    let c = parse(&["--namespace", "PROD"]);
    assert_eq!(c.namespace, "PROD");
}

#[test]
fn test_username_flag() {
    let c = parse(&["--username", "admin"]);
    assert_eq!(c.username.as_deref(), Some("admin"));
}

#[test]
fn test_username_short_flag() {
    let c = parse(&["-u", "admin"]);
    assert_eq!(c.username.as_deref(), Some("admin"));
}

#[test]
fn test_password_flag() {
    let c = parse(&["--password", "secret"]);
    assert_eq!(c.password.as_deref(), Some("secret"));
}

#[test]
fn test_password_short_flag() {
    let c = parse(&["-p", "secret"]);
    assert_eq!(c.password.as_deref(), Some("secret"));
}

#[test]
fn test_container_flag() {
    let c = parse(&["--container", "iris-dev-iris"]);
    assert_eq!(c.container.as_deref(), Some("iris-dev-iris"));
}

#[test]
fn test_all_flags_together() {
    let c = parse(&[
        "--host",
        "192.168.1.1",
        "--web-port",
        "52773",
        "--namespace",
        "APP",
        "--username",
        "SuperUser",
        "--password",
        "pass",
        "--container",
        "my-iris",
    ]);
    assert_eq!(c.host.as_deref(), Some("192.168.1.1"));
    assert_eq!(c.web_port, 52773);
    assert_eq!(c.namespace, "APP");
    assert_eq!(c.username.as_deref(), Some("SuperUser"));
    assert_eq!(c.password.as_deref(), Some("pass"));
    assert_eq!(c.container.as_deref(), Some("my-iris"));
}

#[test]
fn test_namespace_percent_sys() {
    // Namespaces starting with % (like %SYS) must be accepted verbatim
    let c = parse(&["--namespace", "%SYS"]);
    assert_eq!(c.namespace, "%SYS");
}
