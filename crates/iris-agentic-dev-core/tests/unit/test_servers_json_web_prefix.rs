//! `web_prefix` in the iad-native server registry (116, issue #129).
//!
//! `servers.json` could not describe an instance served under a gateway path prefix, so
//! `iris_add_server` wrote an entry that resolved to the gateway root. The same deployment was
//! reachable when discovered through VS Code Server Manager, whose profiles carry `pathPrefix`.
//!
//! Every URL here is resolved from a parsed `servers.json` string rather than a struct literal —
//! a field the struct has but serde never fills is the #110 pattern, and a struct literal cannot
//! catch it.

use iris_agentic_dev_core::iris::connection_pool::{native_base_url, web_prefix_path_part};
use iris_agentic_dev_core::iris::servers_config::{
    load_from_path, save_to_path, validate_web_prefix, ServerEntry, ServersConfig,
};
use iris_agentic_dev_core::testing::{params_type, struct_fields};

/// Parse one `servers.json` string and return the named entry.
fn entry_from_json(json: &str, name: &str) -> ServerEntry {
    let cfg: ServersConfig = serde_json::from_str(json).expect("servers.json must parse");
    cfg.servers
        .get(name)
        .unwrap_or_else(|| panic!("entry `{name}` missing from parsed config: {json}"))
        .clone()
}

/// The reporter's layout: two HealthShare instances on one host and port, told apart by prefix.
const SHARED_GATEWAY_JSON: &str = r#"{
  "version": 1,
  "servers": {
    "ucr": {
      "host": "hs.example.com", "port": 8080, "namespace": "HSCUSTOM",
      "username": "_SYSTEM", "web_prefix": "/hs20261"
    },
    "viewer": {
      "host": "hs.example.com", "port": 8080, "namespace": "HSCUSTOM",
      "username": "_SYSTEM", "web_prefix": "/hscv20261"
    }
  }
}"#;

/// FR-001, FR-002: the field is read from the file and reaches the URL.
#[test]
fn a_prefixed_entry_resolves_to_a_prefixed_url() {
    let ucr = entry_from_json(SHARED_GATEWAY_JSON, "ucr");
    assert_eq!(ucr.web_prefix.as_deref(), Some("/hs20261"));
    assert_eq!(
        native_base_url(&ucr),
        "http://hs.example.com:8080/hs20261",
        "the prefix is what distinguishes this instance from the other one on the same port"
    );
}

/// SC-001: the two entries are distinguishable, which is the whole point of the report.
#[test]
fn two_instances_on_one_host_and_port_resolve_to_different_urls() {
    let ucr = native_base_url(&entry_from_json(SHARED_GATEWAY_JSON, "ucr"));
    let viewer = native_base_url(&entry_from_json(SHARED_GATEWAY_JSON, "viewer"));
    assert_ne!(
        ucr, viewer,
        "both entries resolved to the same URL, so per-call `server` routing still cannot tell \
         them apart"
    );
    assert_eq!(viewer, "http://hs.example.com:8080/hscv20261");
}

/// FR-005: an entry with no prefix resolves to exactly what it resolves to today.
#[test]
fn an_entry_without_the_key_resolves_unchanged() {
    let json = r#"{"version":1,"servers":{"dev":{"host":"localhost","port":52780,
        "namespace":"USER","username":"_SYSTEM","scheme":"http"}}}"#;
    let dev = entry_from_json(json, "dev");
    assert!(dev.web_prefix.is_none(), "absent key must deserialize None");
    assert_eq!(native_base_url(&dev), "http://localhost:52780");
}

/// `scheme` still drives the URL when a prefix is present — the two are independent.
#[test]
fn https_and_a_prefix_compose() {
    let json = r#"{"version":1,"servers":{"prod":{"host":"prod.example.com","port":443,
        "namespace":"PROD","username":"admin","scheme":"https","web_prefix":"csp/healthshare/hs"}}}"#;
    let prod = entry_from_json(json, "prod");
    assert_eq!(
        native_base_url(&prod),
        "https://prod.example.com:443/csp/healthshare/hs",
        "a multi-segment prefix must survive intact"
    );
}

/// The VS Code spelling, in a hand-edited file. Accepting it costs one serde alias and saves the
/// next person from a silently-ignored key, since `ServerEntry` does not deny unknown fields.
#[test]
fn the_vscode_spelling_pathprefix_is_accepted_as_an_alias() {
    let json = r#"{"version":1,"servers":{"hs":{"host":"h","port":8080,"namespace":"USER",
        "username":"u","pathPrefix":"/hs20261"}}}"#;
    let hs = entry_from_json(json, "hs");
    assert_eq!(
        hs.web_prefix.as_deref(),
        Some("/hs20261"),
        "pathPrefix is what VS Code Server Manager calls this, so a hand-edited file will use it"
    );
}

/// SC-003: all four spellings are one URL. The VS Code branch normalises with `trim_matches('/')`
/// and the native branch must use that same rule, not a second one.
#[test]
fn the_four_prefix_spellings_resolve_to_one_url() {
    let mut seen: Vec<String> = Vec::new();
    for spelling in ["hs20261", "/hs20261", "hs20261/", "/hs20261/"] {
        let json = format!(
            r#"{{"version":1,"servers":{{"hs":{{"host":"h","port":8080,"namespace":"USER",
            "username":"u","web_prefix":"{spelling}"}}}}}}"#
        );
        seen.push(native_base_url(&entry_from_json(&json, "hs")));
    }
    assert!(
        seen.windows(2).all(|w| w[0] == w[1]),
        "the four spellings must resolve to one URL, got: {seen:?}"
    );
    assert_eq!(seen[0], "http://h:8080/hs20261");
}

/// FR-006: nothing that means "no prefix" may add a slash. A doubled slash in the Atelier path is
/// the bug 089 fixed for Server Manager paths.
#[test]
fn an_empty_or_slashes_only_prefix_means_no_prefix() {
    for spelling in ["", "/", "//", "   "] {
        let json = format!(
            r#"{{"version":1,"servers":{{"hs":{{"host":"h","port":8080,"namespace":"USER",
            "username":"u","web_prefix":"{spelling}"}}}}}}"#
        );
        assert_eq!(
            native_base_url(&entry_from_json(&json, "hs")),
            "http://h:8080",
            "web_prefix {spelling:?} must resolve as no prefix, with no trailing slash"
        );
    }
}

/// FR-002, stated as the guard the inconsistency needed.
///
/// Both branches of `load_pool` call `web_prefix_path_part`, so one host/port/prefix triple is one
/// URL no matter which source described it. Asserting the shared helper against each branch's own
/// format string is what fails if someone re-inlines either one.
#[test]
fn the_native_and_vscode_branches_agree_on_one_url() {
    for prefix in [
        None,
        Some("hs20261"),
        Some("/hs20261/"),
        Some(""),
        Some(" "),
    ] {
        let json = match prefix {
            None => r#"{"version":1,"servers":{"hs":{"host":"h","port":8080,
                "namespace":"USER","username":"u","scheme":"https"}}}"#
                .to_string(),
            Some(p) => format!(
                r#"{{"version":1,"servers":{{"hs":{{"host":"h","port":8080,"namespace":"USER",
                "username":"u","scheme":"https","web_prefix":"{p}"}}}}}}"#
            ),
        };
        let native = native_base_url(&entry_from_json(&json, "hs"));
        // The VS Code branch's assembly, verbatim.
        let vscode = format!(
            "{}://{}:{}{}",
            "https",
            "h",
            8080,
            web_prefix_path_part(prefix)
        );
        assert_eq!(
            native, vscode,
            "prefix {prefix:?}: servers.json and a Server Manager profile describing the same \
             deployment must resolve to the same URL"
        );
    }
}

/// The third pool source. `[instance.*]` in `.iris-agentic-dev.toml` had `web_prefix` all along and
/// normalised it with its own inline `trim_matches`, which is how the registry ended up with no
/// prefix support and two spellings of the same logic. One host/port/prefix triple has to be one
/// URL whichever of the three files described it.
#[test]
fn the_toml_branch_agrees_with_the_registry_on_one_url() {
    use iris_agentic_dev_core::iris::workspace_config::{instance_base_url, FleetConfig};

    for prefix in ["hs20261", "/hs20261", "hs20261/", "/hs20261/", "", " "] {
        let toml = format!(
            r#"
            [instance.hs]
            host = "h"
            port = 8080
            scheme = "https"
            namespace = "USER"
            web_prefix = "{prefix}"
            "#
        );
        let cfg: FleetConfig = toml::from_str(&toml).expect("TOML must parse");
        let inst = cfg
            .instance
            .get("hs")
            .expect("[instance.hs] must be present");

        let json = format!(
            r#"{{"version":1,"servers":{{"hs":{{"host":"h","port":8080,"namespace":"USER",
            "username":"u","scheme":"https","web_prefix":"{prefix}"}}}}}}"#
        );

        assert_eq!(
            instance_base_url(inst),
            native_base_url(&entry_from_json(&json, "hs")),
            "prefix {prefix:?}: a fleet TOML instance and a registry entry describing the same \
             deployment must resolve to the same URL"
        );
    }
}

/// FR-001: a registry file that never had the key round-trips without gaining it, so upgrading iad
/// does not rewrite everyone's `servers.json`.
#[test]
fn saving_an_entry_without_a_prefix_writes_no_key() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("servers.json");
    let json = r#"{"version":1,"servers":{"dev":{"host":"localhost","port":52780,
        "namespace":"USER","username":"_SYSTEM"}}}"#;
    let cfg: ServersConfig = serde_json::from_str(json).expect("parse");
    save_to_path(&cfg, &path).expect("save");
    let written = std::fs::read_to_string(&path).expect("read back");
    assert!(
        !written.contains("web_prefix"),
        "a prefix-less entry must not gain the key on save: {written}"
    );
    assert_eq!(load_from_path(&path), cfg, "and must load back identical");
}

/// FR-009 / forward compatibility: an older binary reading a newer file. `ServerEntry` has no
/// `deny_unknown_fields` and must keep it that way.
#[test]
fn an_unknown_key_does_not_break_the_load() {
    let json = r#"{"version":1,"servers":{"dev":{"host":"localhost","port":52780,
        "namespace":"USER","username":"_SYSTEM","somethingFromTheFuture":true}}}"#;
    let dev = entry_from_json(json, "dev");
    assert_eq!(dev.host, "localhost");
}

/// FR-007: a prefix carrying a scheme or host cannot be concatenated into a working URL, so it is
/// refused where it is entered rather than 404-ing later from the gateway root.
#[test]
fn a_prefix_containing_a_url_is_refused() {
    for bad in [
        "http://other.example.com/hs",
        "https://other/hs",
        "//other/hs",
    ] {
        let err = validate_web_prefix(bad).expect_err(&format!(
            "web_prefix {bad:?} must be refused, not concatenated"
        ));
        assert!(
            err.contains(bad),
            "the message must quote what was passed, got: {err}"
        );
    }
}

/// The spellings that must stay accepted, so the check does not become the new bug.
#[test]
fn ordinary_prefixes_pass_validation() {
    for good in [
        "",
        "/",
        "hs20261",
        "/hs20261",
        "/hs20261/",
        "csp/healthshare/hs20261",
    ] {
        assert!(
            validate_web_prefix(good).is_ok(),
            "web_prefix {good:?} must be accepted"
        );
    }
}

/// FR-003: declared in the tool's input schema, not accepted as an untyped passthrough (113).
#[test]
fn add_server_declares_web_prefix() {
    let fields = struct_fields(&params_type("iris_add_server"));
    assert!(
        fields.contains("web_prefix"),
        "iris_add_server must declare web_prefix — with deny_unknown_fields an undeclared \
         parameter is rejected outright. Declared: {fields:?}"
    );
}
