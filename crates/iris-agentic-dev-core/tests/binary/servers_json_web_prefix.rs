//! Layer 2 for `web_prefix` in the iad-native registry (116, issue #129).
//!
//! The unit tests prove the URL resolves. These prove the running server advertises the parameter
//! and reports the stored prefix back, which is what the reporter could not get: they had to read
//! the Rust source to learn the field did not exist.
//!
//! No IRIS and no keychain. `HOME` points at a temp directory holding a hand-written
//! `servers.json`, so the registry under test is the file this test wrote and nothing else.

use iris_agentic_dev_core::testing::{
    advertised_schemas, answer_text, require_iad_binary, McpSession,
};

/// A registry with one prefixed and one prefix-less entry, in a temp `HOME`.
///
/// Returns the temp dir — hold it for the lifetime of the session, or the file is gone before the
/// server reads it.
fn registry_home() -> tempfile::TempDir {
    let home = tempfile::tempdir().expect("tempdir");
    let dir = home.path().join(".config").join("iris-agentic-dev");
    std::fs::create_dir_all(&dir).expect("create config dir");
    std::fs::write(
        dir.join("servers.json"),
        r#"{
  "version": 1,
  "servers": {
    "gw-prefixed": {
      "host": "gateway.example.com", "port": 8080, "namespace": "HSCUSTOM",
      "username": "_SYSTEM", "password": "x", "web_prefix": "/hs20261"
    },
    "gw-plain": {
      "host": "gateway.example.com", "port": 8080, "namespace": "USER",
      "username": "_SYSTEM", "password": "x"
    }
  }
}"#,
    )
    .expect("write servers.json");
    home
}

fn env_for(home: &tempfile::TempDir) -> Vec<(String, String)> {
    vec![(
        "HOME".to_string(),
        home.path().to_string_lossy().to_string(),
    )]
}

/// The entry named `name` from an `iris_servers` answer.
fn server_entry(answer: &serde_json::Value, name: &str) -> serde_json::Value {
    let text = answer
        .pointer("/result/content/0/text")
        .and_then(|t| t.as_str())
        .unwrap_or_else(|| panic!("iris_servers gave no text answer: {}", answer_text(answer)));
    let body: serde_json::Value =
        serde_json::from_str(text).unwrap_or_else(|e| panic!("not JSON ({e}): {text}"));
    body["servers"]
        .as_array()
        .unwrap_or_else(|| panic!("no servers array: {body}"))
        .iter()
        .find(|s| s["name"] == name)
        .cloned()
        .unwrap_or_else(|| panic!("`{name}` missing from the listing: {body}"))
}

/// FR-003: the parameter is declared in the schema the server actually advertises.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn iris_add_server_advertises_web_prefix() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let schemas = advertised_schemas();
    let schema = schemas
        .get("iris_add_server")
        .expect("iris_add_server must be advertised");
    let props = schema["properties"]
        .as_object()
        .unwrap_or_else(|| panic!("no properties in the advertised schema: {schema}"));
    assert!(
        props.contains_key("web_prefix"),
        "iris_add_server must advertise web_prefix — the schema sets additionalProperties:false, \
         so an undeclared parameter is refused outright. Advertised: {:?}",
        props.keys().collect::<Vec<_>>()
    );
}

/// FR-004: the prefix comes back in the listing, so an unreachable entry is diagnosable from a tool
/// call instead of from the source.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn iris_servers_reports_the_prefix() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let home = registry_home();
    let answer = McpSession::start(&env_for(&home)).call("iris_servers", &serde_json::json!({}));
    let entry = server_entry(&answer, "gw-prefixed");
    assert_eq!(
        entry["web_prefix"].as_str(),
        Some("/hs20261"),
        "the listing must report the prefix: {entry}"
    );
}

/// FR-004 second half: absent, not an empty string pretending to be a value.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn a_prefix_less_entry_reports_no_prefix() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let home = registry_home();
    let answer = McpSession::start(&env_for(&home)).call("iris_servers", &serde_json::json!({}));
    let entry = server_entry(&answer, "gw-plain");
    assert!(
        entry.get("web_prefix").is_none() || entry["web_prefix"].is_null(),
        "a prefix-less entry must report no prefix rather than \"\": {entry}"
    );
}

/// FR-001, FR-003, FR-008: the prefix survives the write, and re-registering the same name with a
/// new prefix updates the entry instead of duplicating or dropping it.
///
/// `HOME` is temp, so this writes to a throwaway registry. The keychain is not reachable for a
/// `HOME`-isolated server on CI and may be on a developer machine — either way the assertion is on
/// what landed in `servers.json`, which is where the prefix lives.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn iris_add_server_persists_and_updates_the_prefix() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let home = tempfile::tempdir().expect("tempdir");
    let registry = home
        .path()
        .join(".config")
        .join("iris-agentic-dev")
        .join("servers.json");
    let mut session = McpSession::start(&[(
        "HOME".to_string(),
        home.path().to_string_lossy().to_string(),
    )]);

    let mut args = serde_json::json!({
        "name": "iad116-tmp",
        "host": "gateway.example.com",
        "port": 8080,
        "namespace": "HSCUSTOM",
        "username": "_SYSTEM",
        "password": "not-a-real-credential",
        "web_prefix": "hs20261/"
    });
    let answer = session.call("iris_add_server", &args);
    let written = std::fs::read_to_string(&registry).unwrap_or_else(|e| {
        panic!(
            "no registry written at {} ({e}): {}",
            registry.display(),
            answer_text(&answer)
        )
    });
    let cfg: serde_json::Value = serde_json::from_str(&written).expect("registry is JSON");
    assert_eq!(
        cfg["servers"]["iad116-tmp"]["web_prefix"].as_str(),
        Some("hs20261/"),
        "the prefix must persist as given; normalisation happens when the URL is built, so the \
         file keeps what the operator typed: {written}"
    );

    // Same name, different prefix — the reporter's stale `hs20252` entry, corrected in place.
    args["web_prefix"] = serde_json::json!("/hscv20261");
    session.call("iris_add_server", &args);
    let written = std::fs::read_to_string(&registry).expect("registry still there");
    let cfg: serde_json::Value = serde_json::from_str(&written).expect("registry is JSON");
    assert_eq!(
        cfg["servers"]["iad116-tmp"]["web_prefix"].as_str(),
        Some("/hscv20261"),
        "re-registering the name must update the prefix: {written}"
    );
    assert_eq!(
        cfg["servers"].as_object().map(|s| s.len()),
        Some(1),
        "and must not add a second entry: {written}"
    );

    // `HOME` is temp but the OS keychain is not — on a developer machine the credential above went
    // into the real keychain. `iris_remove_server` deletes it, and it needs the entry in the pool,
    // so reload first.
    session.call("iris_reload_pool", &serde_json::json!({}));
    session.call(
        "iris_remove_server",
        &serde_json::json!({"name": "iad116-tmp"}),
    );
}

/// FR-007: a prefix carrying a scheme is refused where it is entered. Concatenating it produces
/// `http://host:8080http://other/hs`, which fails later as a connection error with no hint that the
/// prefix was the problem.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn a_prefix_containing_a_url_is_refused_at_registration() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let home = tempfile::tempdir().expect("tempdir");
    let registry = home
        .path()
        .join(".config")
        .join("iris-agentic-dev")
        .join("servers.json");
    let answer = McpSession::start(&[(
        "HOME".to_string(),
        home.path().to_string_lossy().to_string(),
    )])
    .call(
        "iris_add_server",
        &serde_json::json!({
            "name": "iad116-bad",
            "host": "gateway.example.com",
            "port": 8080,
            "namespace": "USER",
            "username": "_SYSTEM",
            "password": "not-a-real-credential",
            "web_prefix": "http://other.example.com/hs"
        }),
    );
    let text = answer_text(&answer);
    assert!(
        text.contains("INVALID_PARAMS"),
        "expected INVALID_PARAMS, got: {text}"
    );
    assert!(
        text.contains("web_prefix"),
        "the message must name the parameter: {text}"
    );
    assert!(
        !registry.exists(),
        "a refused registration must not write an entry — the file exists at {}",
        registry.display()
    );
}
