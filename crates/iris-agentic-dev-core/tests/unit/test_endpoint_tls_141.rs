//! Spec 141 FR-021, FR-022 and the credential half of FR-023 (T-R4b-U04 to U07): where a server
//! is, whether its certificate is checked, and which credentials may go there.
//!
//! The scans read the source tree; the rest parse TOML and JSON strings and call the same
//! functions the server calls, none of them with a struct literal standing in for a config file.
//! Nothing here touches IRIS.

use iris_agentic_dev_core::iris::config_sources::{
    loosening_settings, ConfigSnapshot, LoadInputs, SetAsideKind, CONFIG_FILE_NAME,
};
use iris_agentic_dev_core::iris::connection_pool::{
    fleet_connection, native_connection, server_manager_connection,
};
use iris_agentic_dev_core::iris::endpoint::{Endpoint, Scheme};
use iris_agentic_dev_core::iris::http::{TlsSetting, TlsSource};
use iris_agentic_dev_core::iris::server_manager::ServerManagerProfile;
use iris_agentic_dev_core::iris::servers_config::ServerEntry;
use iris_agentic_dev_core::iris::workspace_config::apply_snapshot;
use std::path::{Path, PathBuf};

// ── T-R4b-U04: one place can turn certificate checks off ────────────────────────────────────

/// Every `.rs` file under both crates' `src`, with its path relative to `crates/`.
fn sources() -> Vec<(String, String)> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut out = Vec::new();
    let mut stack: Vec<PathBuf> = ["iris-agentic-dev-core", "iris-agentic-dev-bin"]
        .iter()
        .map(|c| crates.join(c).join("src"))
        .collect();
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)
            .expect("src must be readable")
            .flatten()
        {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let rel = path
                    .strip_prefix(&crates)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((rel, std::fs::read_to_string(&path).unwrap()));
            }
        }
    }
    assert!(out.len() > 50, "only {} source files found", out.len());
    out
}

fn files_containing(needle: &str) -> Vec<String> {
    sources()
        .into_iter()
        .filter(|(_, text)| text.contains(needle))
        .map(|(rel, _)| rel)
        .collect()
}

#[test]
fn danger_accept_invalid_certs_appears_once_and_in_the_builder() {
    let hits = files_containing("danger_accept_invalid_certs");
    assert_eq!(
        hits,
        vec!["iris-agentic-dev-core/src/iris/http.rs".to_string()],
        "the only place certificate checks can be switched off is `client_for`"
    );
    let text = sources()
        .into_iter()
        .find(|(rel, _)| rel.ends_with("iris/http.rs"))
        .expect("iris/http.rs exists")
        .1;
    assert_eq!(
        text.matches("danger_accept_invalid_certs(").count(),
        1,
        "exactly one builder call"
    );
}

#[test]
fn reqwest_clients_are_built_in_the_one_module_or_for_a_non_iris_host() {
    // The three allowlisted files talk to the model APIs and to GitHub, never to an IRIS server.
    let allowed = [
        "iris-agentic-dev-core/src/iris/http.rs",
        "iris-agentic-dev-core/src/generate.rs",
        "iris-agentic-dev-core/src/manifest/resolve.rs",
        "iris-agentic-dev-core/src/skill_install/mod.rs",
        "iris-agentic-dev-core/src/skills/mod.rs",
    ];
    let offenders: Vec<String> = files_containing("Client::builder()")
        .into_iter()
        .filter(|rel| !allowed.contains(&rel.as_str()))
        .collect();
    assert!(
        offenders.is_empty(),
        "these files build their own client for an IRIS server, so they can decide certificate \
         checks for themselves: {offenders:?}"
    );
}

#[test]
fn nothing_writes_the_tls_variables_into_the_environment() {
    for name in ["IRIS_TLS_VERIFY", "IRIS_INSECURE"] {
        let hits = files_containing(&format!("set_var(\"{name}\""));
        assert!(
            hits.is_empty(),
            "{name} is set in the process environment by {hits:?}: one file's setting would \
             reach every server"
        );
    }
}

#[test]
fn the_old_client_constructor_that_took_a_bare_insecure_flag_is_gone() {
    let hits = files_containing("fn iris_http_client");
    assert!(hits.is_empty(), "{hits:?}");
}

// ── T-R4b-U05: Endpoint ────────────────────────────────────────────────────────────────────

fn ep(scheme: Option<&str>, host: &str, port: u16, prefix: Option<&str>) -> Endpoint {
    Endpoint::parse(scheme, host, port, prefix)
        .unwrap_or_else(|e| panic!("{scheme:?} {host:?} {port} {prefix:?}: {e}"))
}

#[test]
fn endpoint_accepts_http_and_https_in_any_case() {
    assert_eq!(ep(Some("http"), "h", 80, None).scheme(), Scheme::Http);
    assert_eq!(ep(Some("HTTPS"), "h", 443, None).scheme(), Scheme::Https);
    assert_eq!(ep(Some("Https:"), "h", 443, None).scheme(), Scheme::Https);
    assert_eq!(
        ep(None, "h", 80, None).scheme(),
        Scheme::Http,
        "http by default"
    );
}

#[test]
fn endpoint_rejects_a_scheme_that_moves_the_url() {
    for scheme in ["http://evil.example/#", "ftp", "javascript", "", "http@x"] {
        let r = Endpoint::parse(Some(scheme), "localhost", 52773, None);
        // An empty scheme is "not given", the one non-error among them.
        if scheme.is_empty() {
            assert!(r.is_ok(), "{scheme:?} means no scheme given");
        } else {
            assert!(r.is_err(), "{scheme:?} must be refused");
        }
    }
}

#[test]
fn endpoint_rejects_an_empty_or_url_shaped_host() {
    for host in [
        "",
        "  ",
        "a b",
        "evil.example/x",
        "evil.example#",
        "u@evil.example",
        "h?x",
    ] {
        assert!(
            Endpoint::parse(None, host, 52773, None).is_err(),
            "{host:?} is not a host name"
        );
    }
}

#[test]
fn endpoint_rejects_a_prefix_that_carries_a_scheme_or_host() {
    assert!(Endpoint::parse(None, "h", 80, Some("http://other/hs")).is_err());
    assert!(Endpoint::parse(None, "h", 80, Some("a b")).is_err());
    assert_eq!(
        ep(None, "h", 80, Some("//myprefix//")).base_url(),
        "http://h:80/myprefix",
        "surrounding slashes are not part of the prefix"
    );
}

#[test]
fn endpoint_builds_one_base_url() {
    assert_eq!(ep(None, "h", 52773, None).base_url(), "http://h:52773");
    assert_eq!(
        ep(Some("https"), "h", 443, Some("/hs20261/")).base_url(),
        "https://h:443/hs20261"
    );
    assert_eq!(
        ep(None, "h", 80, Some("")).base_url(),
        "http://h:80",
        "an empty prefix is no prefix"
    );
    assert_eq!(
        ep(None, "::1", 52773, None).base_url(),
        "http://[::1]:52773"
    );
    assert_eq!(
        ep(None, "[::1]", 52773, None).base_url(),
        "http://[::1]:52773"
    );
}

#[test]
fn endpoint_round_trips_through_a_base_url() {
    for url in [
        "http://localhost:52773",
        "https://iris.corp:443/prefix/sub",
        "http://[::1]:8080",
    ] {
        let e = Endpoint::from_base_url(url).unwrap_or_else(|e| panic!("{url}: {e}"));
        assert_eq!(e.base_url(), url);
    }
    assert!(Endpoint::from_base_url("ftp://h:1").is_err());
    assert!(Endpoint::from_base_url("not a url").is_err());
}

#[test]
fn loopback_names_are_one_host() {
    let a = ep(None, "localhost", 52773, None);
    for host in ["127.0.0.1", "LOCALHOST", "::1", "[::1]", "127.4.5.6"] {
        let b = ep(None, host, 52773, None);
        assert!(a.same_server(&b), "{host} is this machine");
        assert!(b.is_loopback());
    }
    assert!(!ep(None, "localhost.evil.example", 52773, None).is_loopback());
    assert!(!ep(None, "127.0.0.1.evil.example", 52773, None).is_loopback());
}

#[test]
fn same_server_compares_host_without_case_and_everything_else_exactly() {
    let base = ep(Some("https"), "Iris.Corp", 443, Some("hs"));
    assert!(base.same_server(&ep(Some("https"), "iris.corp", 443, Some("/hs/"))));
    assert!(
        !base.same_server(&ep(Some("http"), "iris.corp", 443, Some("hs"))),
        "scheme"
    );
    assert!(
        !base.same_server(&ep(Some("https"), "iris.corp", 80, Some("hs"))),
        "port"
    );
    assert!(
        !base.same_server(&ep(Some("https"), "iris.corp", 443, None)),
        "prefix"
    );
    assert!(
        !base.same_server(&ep(Some("https"), "other.corp", 443, Some("hs"))),
        "host"
    );
    assert!(
        !ep(None, "iris.corp", 52773, None).same_server(&ep(None, "localhost", 52773, None)),
        "a remote host is not this machine"
    );
}

// ── T-R4b-U06: a connection carries its own certificate setting ──────────────────────────────

fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, body).unwrap();
    p
}

fn inputs(home: &Path, cwd: &Path) -> LoadInputs {
    LoadInputs {
        cwd: std::fs::canonicalize(cwd).unwrap(),
        home: Some(std::fs::canonicalize(home).unwrap()),
        ..Default::default()
    }
}

/// The project file used by the next tests: its own connection and one instance turn validation
/// off, another instance says nothing.
const PROJECT_FILE: &str = "mode = \"operate\"\n\
    host = \"ws.example\"\nweb_port = 9443\nscheme = \"https\"\ntls_verify = false\n\
    [instance.a]\nhost = \"a.example\"\nweb_port = 9443\nscheme = \"https\"\ntls_verify = false\n\
    [instance.b]\nhost = \"b.example\"\nweb_port = 9443\nscheme = \"https\"\n";

struct Loaded {
    snap: ConfigSnapshot,
    _tmp: tempfile::TempDir,
}

fn load_project(trusted: bool) -> Loaded {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let proj = tmp.path().join("proj");
    std::fs::create_dir_all(&home).unwrap();
    let ws = write(&proj, CONFIG_FILE_NAME, PROJECT_FILE);
    if trusted {
        let table: toml::Table = PROJECT_FILE.parse().unwrap();
        let settings = loosening_settings(&table);
        assert!(
            settings.iter().any(|s| s.contains("tls_verify = false")),
            "the project file's loosening settings must include the certificate switch: {settings:?}"
        );
        write(
            &home,
            CONFIG_FILE_NAME,
            &format!(
                "[[trust]]\npath = {:?}\nformat = 1\nsettings = {:?}\n",
                std::fs::canonicalize(&ws).unwrap().to_string_lossy(),
                settings
            ),
        );
    }
    let snap = ConfigSnapshot::load(&inputs(&home, &proj));
    assert_eq!(snap.trust().is_trusted(), trusted, "{:?}", snap.trust());
    Loaded { snap, _tmp: tmp }
}

fn default_conn_tls(snap: &ConfigSnapshot) -> TlsSetting {
    let (conn, _, _) = apply_snapshot(None, snap, "USER");
    conn.expect("the project file names a host").tls
}

fn instance_tls(snap: &ConfigSnapshot, name: &str) -> TlsSetting {
    let inst = snap
        .fleet
        .instance
        .get(name)
        .unwrap_or_else(|| panic!("instance {name} missing"));
    fleet_connection(name, inst).expect("a valid instance").tls
}

#[test]
fn a_trusted_project_file_turns_validation_off_for_its_own_connection_and_instance_only() {
    let env_before = (
        std::env::var_os("IRIS_TLS_VERIFY"),
        std::env::var_os("IRIS_INSECURE"),
    );
    let loaded = load_project(true);

    assert!(
        !default_conn_tls(&loaded.snap).verify,
        "the file's own connection"
    );
    assert!(
        !instance_tls(&loaded.snap, "a").verify,
        "instance a says so"
    );
    assert!(
        instance_tls(&loaded.snap, "b").verify,
        "instance b says nothing"
    );

    // A servers.json entry, an IRIS_HOST connection and a Server Manager profile are not the file's.
    let entry: ServerEntry = serde_json::from_str(
        r#"{"host":"s.example","port":443,"namespace":"USER","username":"u","scheme":"https"}"#,
    )
    .unwrap();
    assert!(native_connection("s", &entry).unwrap().tls.verify);
    assert!(
        server_manager_connection(&sm_profile("p"), "pw")
            .unwrap()
            .tls
            .verify,
        "profiles always validate"
    );

    assert_eq!(
        env_before,
        (
            std::env::var_os("IRIS_TLS_VERIFY"),
            std::env::var_os("IRIS_INSECURE")
        ),
        "loading a file must not change the process environment"
    );
}

#[test]
fn an_untrusted_project_file_cannot_turn_validation_off_anywhere() {
    let loaded = load_project(false);
    assert!(default_conn_tls(&loaded.snap).verify);
    assert!(
        instance_tls(&loaded.snap, "a").verify,
        "set aside for instance a"
    );
    assert!(instance_tls(&loaded.snap, "b").verify);
    let ids: Vec<&str> = loaded
        .snap
        .set_aside
        .iter()
        .filter(|i| i.kind == SetAsideKind::UntrustedSetting)
        .map(|i| i.id.as_str())
        .collect();
    assert!(ids.contains(&"workspace:tls_verify"), "{ids:?}");
    assert!(ids.contains(&"workspace:instance.a.tls_verify"), "{ids:?}");
}

#[test]
fn the_operator_file_turns_validation_off_without_trust() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    write(&home, CONFIG_FILE_NAME, PROJECT_FILE);
    let snap = ConfigSnapshot::load(&inputs(&home, &home));
    let tls = default_conn_tls(&snap);
    assert!(!tls.verify);
    assert_eq!(tls.source, TlsSource::ConfigFile);
    assert!(!instance_tls(&snap, "a").verify);
}

#[test]
fn a_servers_json_entry_has_its_own_setting_and_the_operators_file_is_honoured() {
    let off: ServerEntry = serde_json::from_str(
        r#"{"host":"s.example","port":443,"namespace":"USER","username":"u","scheme":"https","tls_verify":false}"#,
    )
    .unwrap();
    assert_eq!(
        off.tls_verify,
        Some(false),
        "the key must not be silently dropped"
    );
    let conn = native_connection("s", &off).unwrap();
    assert!(!conn.tls.verify);
    assert_eq!(conn.tls.source, TlsSource::ServersJson);

    let silent: ServerEntry = serde_json::from_str(
        r#"{"host":"s.example","port":443,"namespace":"USER","username":"u","scheme":"https"}"#,
    )
    .unwrap();
    let conn = native_connection("s", &silent).unwrap();
    assert!(conn.tls.verify);
    assert_eq!(conn.tls.source, TlsSource::Default);
}

#[test]
fn only_the_launch_settings_read_the_two_environment_variables() {
    let off = TlsSetting::from_launch_values(None, Some("false"));
    assert!(!off.verify);
    assert_eq!(off.source, TlsSource::LaunchEnv);
    assert!(!TlsSetting::from_launch_values(Some("true"), None).verify);
    let on = TlsSetting::from_launch_values(None, None);
    assert!(on.verify);
    assert_eq!(on.source, TlsSource::Default);
    // A typo is not consent (the truth table of `tls_insecure`).
    assert!(TlsSetting::from_launch_values(Some("yes"), None).verify);
}

fn sm_profile(name: &str) -> ServerManagerProfile {
    ServerManagerProfile {
        name: name.to_string(),
        host: "sm.example".to_string(),
        port: 443,
        scheme: "https".to_string(),
        path_prefix: None,
        username: "_SYSTEM".to_string(),
        password_deprecated: None,
    }
}

// ── T-R4b-U07: environment credentials go only to the endpoint launch settings name ───────────

fn launch_inputs(home: &Path, cwd: &Path, launch: &Endpoint) -> LoadInputs {
    LoadInputs {
        host_env: Some(launch.host().to_string()),
        launch_endpoint: Some(launch.clone()),
        env_credentials_set: Some(vec!["IRIS_USERNAME".into(), "IRIS_PASSWORD".into()]),
        ..inputs(home, cwd)
    }
}

fn withheld_ids(file_body: &str, launch: &Endpoint) -> Vec<String> {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let proj = tmp.path().join("proj");
    std::fs::create_dir_all(&home).unwrap();
    write(&proj, CONFIG_FILE_NAME, file_body);
    let snap = ConfigSnapshot::load(&launch_inputs(&home, &proj, launch));
    snap.set_aside
        .iter()
        .filter(|i| i.kind == SetAsideKind::CredentialWithheld)
        .map(|i| i.id.clone())
        .collect()
}

#[test]
fn a_file_naming_iris_host_on_another_scheme_or_port_gets_no_environment_password() {
    // IRIS_HOST names https://iris.corp:443.
    let launch = ep(Some("https"), "iris.corp", 443, None);

    for (label, body) in [
        (
            "another scheme",
            "host = \"iris.corp\"\nscheme = \"http\"\nweb_port = 443\n",
        ),
        (
            "another port",
            "host = \"iris.corp\"\nscheme = \"https\"\nweb_port = 80\n",
        ),
        (
            "another prefix",
            "host = \"iris.corp\"\nscheme = \"https\"\nweb_port = 443\nweb_prefix = \"other\"\n",
        ),
    ] {
        assert_eq!(
            withheld_ids(body, &launch),
            vec!["default:credentials".to_string()],
            "{label}"
        );
    }

    // The same server, spelled with other case: it gets them.
    assert_eq!(
        withheld_ids(
            "host = \"IRIS.corp\"\nscheme = \"https\"\nweb_port = 443\n",
            &launch
        ),
        Vec::<String>::new(),
        "the endpoint launch settings name"
    );
}
