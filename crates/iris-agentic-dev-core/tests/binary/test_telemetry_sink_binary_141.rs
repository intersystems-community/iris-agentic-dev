//! Spec 141 R5 C16 (binary layer): `check_config` reports the telemetry sink, only the operator
//! can turn the IRIS sink on, and an edit to the operator file during a session that turns it on
//! waits for the next start.
//!
//! Each test spawns `iris-agentic-dev mcp` with a temporary `HOME` and its own project directory.
//! No IRIS is involved.
//!
//! Run: `IAD_BINARY=$PWD/target/debug/iris-agentic-dev cargo test --features testing --test binary
//! test_telemetry_sink_binary_141 -- --ignored --test-threads=1`

use crate::support_mcp_141::Env;
use serde_json::{json, Value};

fn aside_ids(cfg: &Value, kind: &str) -> Vec<String> {
    cfg["set_aside"]
        .as_array()
        .expect("set_aside array")
        .iter()
        .filter(|i| i["kind"] == kind)
        .map(|i| i["id"].as_str().unwrap().to_string())
        .collect()
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn the_sink_is_local_by_default() {
    let env = Env::new("write_tools_enabled = true\n");
    let cfg = env.start().body("check_config", json!({}));
    assert_eq!(cfg["telemetry_sink"], "local", "{cfg}");
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn the_operator_file_turns_the_iris_sink_on() {
    let env = Env::new("telemetry_sink = \"iris\"\n");
    let cfg = env.start().body("check_config", json!({}));
    assert_eq!(cfg["telemetry_sink"], "iris", "{cfg}");
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn the_launch_setting_turns_the_iris_sink_on() {
    let env = Env::new("");
    let cfg = env
        .start_with(&[("IAD_TELEMETRY_SINK", "iris")])
        .body("check_config", json!({}));
    assert_eq!(cfg["telemetry_sink"], "iris", "{cfg}");
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn a_workspace_file_cannot_turn_it_on_and_the_server_says_it_set_it_aside() {
    let env = Env::new("");
    std::fs::write(
        env.proj.join(".iris-agentic-dev.toml"),
        "telemetry_sink = \"iris\"\n",
    )
    .unwrap();
    let cfg = env.start().body("check_config", json!({}));
    assert_eq!(cfg["telemetry_sink"], "local", "{cfg}");
    assert!(
        aside_ids(&cfg, "operator_only").contains(&"workspace:telemetry_sink".to_string()),
        "telemetry_sink from a workspace file must be set aside: {:?}",
        cfg["set_aside"]
    );
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn turning_it_on_in_the_operator_file_mid_session_is_held_until_restart() {
    let env = Env::new("write_tools_enabled = true\n");
    let mut s = env.start();
    assert_eq!(s.body("check_config", json!({}))["telemetry_sink"], "local");

    env.edit_operator("write_tools_enabled = true\ntelemetry_sink = \"iris\"\n");
    let cfg = s.body("check_config", json!({}));
    assert_eq!(cfg["telemetry_sink"], "local", "{cfg}");
    assert!(
        aside_ids(&cfg, "held_for_restart").contains(&"operator:telemetry_sink".to_string()),
        "the edit waits for the next start: {:?}",
        cfg["set_aside"]
    );

    let restarted = env.start().body("check_config", json!({}));
    assert_eq!(restarted["telemetry_sink"], "iris", "{restarted}");
}
