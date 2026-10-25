//! T181 over the wire: a process spawned through `clean_command` finds no workspace file, wherever
//! the test itself runs. A worktree under the main checkout would otherwise reach the main
//! checkout's `.iris-agentic-dev.toml` by the walk up from the cwd, and that file outranks `IRIS_*`.

use iris_agentic_dev_core::testing::{clean_command, iad_binary_path};
use serde_json::Value;

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn a_spawned_check_config_reports_no_workspace_file() {
    let bin = iad_binary_path();
    assert!(
        bin.exists(),
        "no binary at {}; cargo build first",
        bin.display()
    );
    let out = clean_command(&bin)
        .args(["tool", "check_config"])
        .output()
        .expect("spawn iris-agentic-dev tool check_config");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let body: Value = serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!(
            "check_config printed no JSON ({e}); stdout: {stdout}; stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        )
    });
    assert!(
        body["config_sources"]["workspace_file"].is_null(),
        "a clean spawn must not find a project file: {}",
        body["config_sources"]
    );
}
