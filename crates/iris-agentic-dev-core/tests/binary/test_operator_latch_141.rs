//! Layer 2 for spec 141 FR-039 and FR-043 (T192, T193): a refusal tells the agent which setting
//! stands in the way and nothing else, and an edit to the operator file during a session applies
//! only where it tightens.
//!
//! Each test spawns `iris-agentic-dev mcp` with a temporary `HOME` holding an operator file and a
//! project directory with no file of its own. No IRIS is needed: a refused call never reaches one,
//! and a call that gets past the gate fails on the unreachable server with a different code.

use crate::support_mcp_141::Env;
use iris_agentic_dev_core::iris::config_sources::loosening_settings;
use iris_agentic_dev_core::testing::{clean_command, iad_binary_path};
use serde_json::{json, Value};

fn refused(r: &Value) -> bool {
    r["error_code"] == "WRITE_TOOLS_DISABLED"
}

/// What a refusal returned to the agent must not contain.
fn assert_no_recipe(r: &Value) {
    let text = r.to_string();
    for needle in [
        ".iris-agentic-dev.toml",
        "operator file",
        "iris-agentic-dev trust",
        "write_tools_enabled = ",
    ] {
        assert!(!text.contains(needle), "{needle:?} in {text}");
    }
}

fn held_ids(cfg: &Value) -> Vec<String> {
    cfg["set_aside"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["kind"] == "held_for_restart")
        .map(|i| i["id"].as_str().unwrap().to_string())
        .collect()
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn a_refusal_gives_the_agent_the_setting_and_the_operator_the_edit_on_stderr() {
    let env = Env::new("write_tools_enabled = false\n");
    let mut s = env.start();

    let r = s.put();
    assert!(refused(&r), "{r}");
    assert_eq!(r["refusal"]["kind"], "operator_limit", "{r}");
    assert_eq!(
        r["refusal"]["lift"],
        json!({"setting": "write_tools_enabled"})
    );
    assert_no_recipe(&r);

    // A second refusal of the same setting adds no second line.
    let r = s.put();
    assert!(refused(&r), "{r}");
    assert_no_recipe(&r);

    let stderr = s.stderr();
    let lines: Vec<&str> = stderr
        .lines()
        .filter(|l| l.contains("WRITE_TOOLS_DISABLED"))
        .collect();
    assert_eq!(lines.len(), 1, "once per setting per process: {stderr}");
    let op = std::fs::canonicalize(env.operator_file()).unwrap();
    assert!(lines[0].contains(&op.display().to_string()), "{}", lines[0]);
    assert!(
        lines[0].contains("write_tools_enabled = true"),
        "{}",
        lines[0]
    );
    assert!(lines[0].contains("restart"), "{}", lines[0]);
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn trust_show_lists_the_edit_that_lifts_each_limit() {
    let env = Env::new("write_tools_enabled = false\n");
    std::fs::write(
        env.proj.join(".iris-agentic-dev.toml"),
        "write_tools_enabled = true\nnamespace = \"PROD\"\n",
    )
    .unwrap();
    let out = clean_command(&iad_binary_path())
        .env("HOME", &env.home)
        .env("USERPROFILE", &env.home)
        .current_dir(&env.proj)
        .args(["trust", "--show"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8_lossy(&out.stdout);
    for needle in [
        "WRITE_TOOLS_DISABLED",
        "write_tools_enabled = true",
        "destructive_tools_enabled = true",
        ".iris-agentic-dev.toml",
        "restart",
    ] {
        assert!(text.contains(needle), "{needle:?} missing from:\n{text}");
    }
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn a_gate_loosened_in_the_operator_file_during_a_session_waits_for_the_restart() {
    let env = Env::new("write_tools_enabled = false\n");
    let mut s = env.start();
    assert!(refused(&s.put()), "writes start off");

    env.edit_operator("write_tools_enabled = true\n");
    let r = s.put();
    assert!(refused(&r), "the loosening edit must not apply: {r}");
    assert_no_recipe(&r);

    let cfg = s.body("check_config", json!({}));
    assert_eq!(cfg["write_tools_enabled"], false, "{cfg}");
    assert_eq!(
        held_ids(&cfg),
        vec!["operator:write_tools_enabled"],
        "{cfg}"
    );
    let item = cfg["set_aside"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == "operator:write_tools_enabled")
        .unwrap();
    assert_eq!(item["lift"], "restart iad", "{item}");
    assert_eq!(item["setting"], "write_tools_enabled = true", "{item}");

    // One stderr line for the held setting, however many calls follow.
    s.put();
    s.body("check_config", json!({}));
    let stderr = s.stderr();
    let held_lines = stderr
        .lines()
        .filter(|l| l.contains("held until") && l.contains("write_tools_enabled = true"))
        .count();
    assert_eq!(held_lines, 1, "{stderr}");
    drop(s);

    // The next start takes the file as it is.
    let mut s = env.start();
    let r = s.put();
    assert!(!refused(&r), "after a restart the edit applies: {r}");
    let cfg = s.body("check_config", json!({}));
    assert_eq!(cfg["write_tools_enabled"], true, "{cfg}");
    assert!(held_ids(&cfg).is_empty(), "{cfg}");
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn a_gate_tightened_in_the_operator_file_applies_at_once_and_loosening_it_back_waits() {
    let env = Env::new("write_tools_enabled = true\n");
    let mut s = env.start();
    assert!(!refused(&s.put()), "writes start on");

    env.edit_operator("write_tools_enabled = false\n");
    assert!(refused(&s.put()), "a tightening edit applies at once");
    let cfg = s.body("check_config", json!({}));
    assert!(held_ids(&cfg).is_empty(), "{cfg}");

    // Back to true: that loosens what the server now runs on.
    env.edit_operator("write_tools_enabled = true\n");
    assert!(refused(&s.put()), "loosening again waits for a restart");
    let cfg = s.body("check_config", json!({}));
    assert_eq!(
        held_ids(&cfg),
        vec!["operator:write_tools_enabled"],
        "{cfg}"
    );
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn dropping_strict_read_only_or_deleting_the_file_waits_for_the_restart() {
    let env = Env::new("read_only = \"strict\"\n");
    let mut s = env.start();
    assert_eq!(s.body("check_config", json!({}))["read_only"], "strict");

    env.edit_operator("namespace = \"USER\"\n");
    let cfg = s.body("check_config", json!({}));
    assert_eq!(cfg["read_only"], "strict", "{cfg}");
    assert_eq!(held_ids(&cfg), vec!["operator:read_only"], "{cfg}");

    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::remove_file(env.operator_file()).unwrap();
    let cfg = s.body("check_config", json!({}));
    assert_eq!(
        cfg["read_only"], "strict",
        "a deleted file loosens too: {cfg}"
    );
    drop(s);

    let mut s = env.start();
    assert_eq!(s.body("check_config", json!({}))["read_only"], "off");
}

/// A trust entry written during the session (what `iris-agentic-dev trust` does) loosens, so it is
/// held until the next start: the workspace file stays untrusted and its write gate stays set aside.
/// Decision for T193: the stricter reading, because an agent can write the entry as easily as the
/// command can.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn a_trust_entry_written_during_the_session_applies_at_the_next_start() {
    let env = Env::new("write_tools_enabled = false\n");
    let ws_file = env.proj.join(".iris-agentic-dev.toml");
    std::fs::write(
        &ws_file,
        "write_tools_enabled = true\nnamespace = \"PROD\"\n",
    )
    .unwrap();
    let ws_canon = std::fs::canonicalize(&ws_file).unwrap();

    let mut s = env.start();
    let r = s.put();
    assert!(refused(&r), "{r}");
    assert_eq!(r["refusal"]["kind"], "untrusted_setting", "{r}");

    let table: toml::Table = std::fs::read_to_string(&ws_file).unwrap().parse().unwrap();
    let settings = loosening_settings(&table)
        .iter()
        .map(|l| format!("{:?}", l))
        .collect::<Vec<_>>()
        .join(", ");
    env.edit_operator(&format!(
        "write_tools_enabled = false\n[[trust]]\npath = {:?}\nformat = 1\nsettings = [{settings}]\n",
        ws_canon.to_string_lossy()
    ));
    let r = s.put();
    assert!(refused(&r), "the entry must not apply yet: {r}");
    assert_eq!(r["refusal"]["kind"], "untrusted_setting", "{r}");
    let cfg = s.body("check_config", json!({}));
    let held = held_ids(&cfg);
    assert_eq!(held.len(), 1, "{cfg}");
    assert!(held[0].starts_with("operator:trust:"), "{held:?}");
    drop(s);

    let mut s = env.start();
    let r = s.put();
    assert!(
        !refused(&r),
        "the entry trusts the file after a restart: {r}"
    );
}
