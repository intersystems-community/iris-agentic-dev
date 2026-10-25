//! Spec 141 T202: a configuration file's `container`, `namespace`, `username` and `password` reach
//! the connection as data, and loading the file leaves the process environment alone.
//!
//! Before, `workspace_config_to_connection` wrote `IRIS_CONTAINER`, `IRIS_NAMESPACE`,
//! `IRIS_USERNAME` and `IRIS_PASSWORD` into the environment. A file's credentials then looked like
//! the launch settings to every later reader, and a value outlived a reload that removed the key.
//!
//! Configs are parsed from TOML text, so a field that drops out of the struct fails here.

use iris_agentic_dev_core::iris::workspace_config::{
    container_discovery_hints, workspace_config_to_connection, WorkspaceConfig,
};
use iris_agentic_dev_core::testing::EnvGuard;
use iris_agentic_dev_core::tools::exec::{route_for_connection, ExecRoute, RouteTarget};

const NAMED: [&str; 4] = [
    "IRIS_CONTAINER",
    "IRIS_NAMESPACE",
    "IRIS_USERNAME",
    "IRIS_PASSWORD",
];

fn parse(text: &str) -> WorkspaceConfig {
    toml::from_str(text).expect("config must parse")
}

fn env_now() -> Vec<(&'static str, Option<String>)> {
    NAMED.iter().map(|k| (*k, std::env::var(k).ok())).collect()
}

/// Every shape of file that names a container, with every field it can carry.
const SHAPES: &[(&str, &str)] = &[
    (
        "container only",
        "container = \"file-iris\"\nnamespace = \"FILENS\"\nusername = \"fileuser\"\npassword = \"filepw\"\n",
    ),
    (
        "host and container",
        "host = \"remote.example\"\ncontainer = \"file-iris\"\nnamespace = \"FILENS\"\nusername = \"fileuser\"\npassword = \"filepw\"\n",
    ),
    (
        "docker only",
        "container = \"file-iris\"\ndocker_only = true\nnamespace = \"FILENS\"\nusername = \"fileuser\"\npassword = \"filepw\"\n",
    ),
    (
        "nopws",
        "container = \"file-iris\"\nnopws = true\nnamespace = \"FILENS\"\nusername = \"fileuser\"\npassword = \"filepw\"\n",
    ),
];

#[test]
fn loading_a_file_leaves_the_four_variables_as_they_were() {
    // Unset: the four stay unset.
    {
        let _clean = EnvGuard::clean();
        for (label, text) in SHAPES {
            let before = env_now();
            let cfg = parse(text);
            let _ = workspace_config_to_connection(&cfg, "USER");
            assert_eq!(env_now(), before, "{label}: the environment changed");
            for k in NAMED {
                assert!(
                    std::env::var(k).is_err(),
                    "{label}: {k} was written from the file"
                );
            }
        }
    }
    // Set by the launch: the launch's values stay, not the file's.
    {
        let mut env = EnvGuard::clean();
        env.set("IRIS_CONTAINER", "launch-iris");
        env.set("IRIS_NAMESPACE", "LAUNCHNS");
        env.set("IRIS_USERNAME", "launchuser");
        env.set("IRIS_PASSWORD", "launchpw");
        for (label, text) in SHAPES {
            let before = env_now();
            let cfg = parse(text);
            let _ = workspace_config_to_connection(&cfg, "USER");
            assert_eq!(
                env_now(),
                before,
                "{label}: the launch settings were overwritten"
            );
        }
    }
}

#[test]
fn a_container_only_file_hands_discovery_its_container_and_account() {
    let _clean = EnvGuard::clean();
    let cfg = parse(SHAPES[0].1);
    assert!(
        workspace_config_to_connection(&cfg, "USER").is_none(),
        "a container-only file builds no connection of its own"
    );
    let hints = container_discovery_hints(&cfg);
    assert_eq!(hints.container.as_deref(), Some("file-iris"));
    assert_eq!(hints.namespace.as_deref(), Some("FILENS"));
    assert_eq!(hints.username.as_deref(), Some("fileuser"));
    assert_eq!(hints.password.as_deref(), Some("filepw"));
    let shown = format!("{hints:?}");
    assert!(
        !shown.contains("filepw"),
        "the password must not print: {shown}"
    );
}

#[test]
fn a_file_that_builds_its_own_connection_leaves_discovery_nothing_to_find() {
    let _clean = EnvGuard::clean();
    for (label, text) in &SHAPES[1..] {
        let hints = container_discovery_hints(&parse(text));
        assert!(
            hints.container.is_none() && hints.username.is_none() && hints.password.is_none(),
            "{label}: {hints:?}"
        );
    }
    let none = container_discovery_hints(&parse("namespace = \"APP\"\n"));
    assert!(none.container.is_none(), "{none:?}");
}

#[test]
fn the_files_container_is_on_the_connection_and_outranks_the_launch_environment() {
    let mut env = EnvGuard::clean();
    env.set("IRIS_CONTAINER", "launch-iris");
    for (label, text) in &SHAPES[1..] {
        let conn = workspace_config_to_connection(&parse(text), "USER")
            .unwrap_or_else(|| panic!("{label}: a connection"));
        assert_eq!(
            conn.default_container().as_deref(),
            Some("file-iris"),
            "{label}"
        );
        match route_for_connection(&conn, RouteTarget::DefaultOrEnv) {
            ExecRoute::HttpThenDocker { container, .. }
            | ExecRoute::DockerOnly { container, .. } => {
                assert_eq!(
                    container, "file-iris",
                    "{label}: the docker leg runs in the file's container"
                )
            }
            other => panic!("{label}: expected a docker leg, got {other:?}"),
        }
    }
}

#[test]
fn without_a_file_container_the_launch_environment_still_names_it() {
    let mut env = EnvGuard::clean();
    env.set("IRIS_CONTAINER", "launch-iris");
    let conn = workspace_config_to_connection(&parse("host = \"remote.example\"\n"), "USER")
        .expect("a host file builds a connection");
    assert_eq!(conn.default_container().as_deref(), Some("launch-iris"));
}
