//! Spec 140 (iris-interop-dev parity), first item: their #434. `docs/connecting.md` orders the
//! sources as CLI flags, then `.iris-agentic-dev.toml`, then `IRIS_*`. Before 1.6.3, clap filled
//! `--host` from `IRIS_HOST`, so any `IRIS_HOST` made the connection explicit and the workspace
//! file was ignored at startup, while a live reload let the file win. Each test runs the binary
//! in a scratch directory whose toml names the live server and whose `IRIS_HOST` names a host
//! that does not exist, or the other way round.

use iris_agentic_dev_core::testing::{answer_text, clean_command, require_iad_binary, McpSession};
use std::path::{Path, PathBuf};

/// The live server, from the test environment.
fn live() -> (String, String, String, String) {
    let get = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
    (
        get("IRIS_HOST", "localhost"),
        get("IRIS_WEB_PORT", "52780"),
        get("IRIS_USERNAME", "_SYSTEM"),
        get("IRIS_PASSWORD", "SYS"),
    )
}

const NOWHERE: &str = "iad-precedence-140.invalid";

fn scratch(name: &str, toml: Option<String>) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("iad-140-{name}"));
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = dir.join(".iris-agentic-dev.toml");
    match toml {
        Some(t) => std::fs::write(&cfg, t).unwrap(),
        None => {
            let _ = std::fs::remove_file(&cfg);
        }
    }
    dir
}

fn toml_for(host: &str, port: &str, user: &str, pass: &str) -> String {
    format!("host = \"{host}\"\nweb_port = {port}\nusername = \"{user}\"\npassword = \"{pass}\"\nnamespace = \"USER\"\n")
}

/// `IRIS_*` for the child; `IRIS_CONTAINER` empty so no docker-exec path can stand in for HTTP.
fn env(host: &str, port: &str, user: &str, pass: &str) -> Vec<(String, String)> {
    [
        ("IRIS_HOST", host),
        ("IRIS_WEB_PORT", port),
        ("IRIS_USERNAME", user),
        ("IRIS_PASSWORD", pass),
        ("IRIS_NAMESPACE", "USER"),
        ("IRIS_CONTAINER", ""),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

fn mcp_executes(dir: &Path, env: &[(String, String)]) -> String {
    let mut s = McpSession::start_in(Some(dir), env);
    answer_text(&s.call(
        "iris_execute",
        &serde_json::json!({"code": "Write \"iad140:\",$ZVersion", "namespace": "USER"}),
    ))
}

#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn the_workspace_file_outranks_iris_host_at_mcp_startup() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let (h, p, u, w) = live();
    let dir = scratch("mcp-file", Some(toml_for(&h, &p, &u, &w)));
    let out = mcp_executes(&dir, &env(NOWHERE, "1", &u, &w));
    assert!(
        out.contains("iad140:"),
        "the toml names the live server: {out}"
    );
}

#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn the_workspace_file_outranks_iris_host_for_a_cli_command() {
    let Some(bin) = require_iad_binary() else {
        return;
    };
    let (h, p, u, w) = live();
    let dir = scratch("cli-file", Some(toml_for(&h, &p, &u, &w)));
    let mut cmd = clean_command(&bin);
    cmd.current_dir(&dir)
        .args(["exec", "Write \"iad140:\",$ZVersion"]);
    for (k, v) in env(NOWHERE, "1", &u, &w) {
        cmd.env(k, v);
    }
    let out = cmd.output().unwrap();
    let text =
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    assert!(
        text.contains("iad140:"),
        "the toml names the live server: {text}"
    );
}

#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn iris_host_alone_still_connects_without_a_workspace_file() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let (h, p, u, w) = live();
    let dir = scratch("env-only", None);
    let out = mcp_executes(&dir, &env(&h, &p, &u, &w));
    assert!(
        out.contains("iad140:"),
        "env alone reaches the server: {out}"
    );
}

#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn a_host_flag_outranks_the_workspace_file() {
    let Some(bin) = require_iad_binary() else {
        return;
    };
    let (h, p, u, w) = live();
    let dir = scratch("flag", Some(toml_for(NOWHERE, "1", &u, &w)));
    let mut cmd = clean_command(&bin);
    cmd.current_dir(&dir).args([
        "exec",
        "--host",
        &h,
        "--web-port",
        &p,
        "Write \"iad140:\",$ZVersion",
    ]);
    for (k, v) in env(NOWHERE, "1", &u, &w) {
        cmd.env(k, v);
    }
    let out = cmd.output().unwrap();
    let text =
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    assert!(
        text.contains("iad140:"),
        "--host names the live server: {text}"
    );
}

/// A `~/.iris-agentic-dev.toml` is a personal default, found by the walk up from any project
/// under the home directory that has no file of its own. It ranks below `IRIS_*`: a project's own
/// file wins over the environment, the home file does not (decided 2026-10-22).
fn home_with_toml(name: &str, toml: &str) -> (PathBuf, PathBuf) {
    let home = std::env::temp_dir().join(format!("iad-140-home-{name}"));
    let project = home.join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(home.join(".iris-agentic-dev.toml"), toml).unwrap();
    let _ = std::fs::remove_file(project.join(".iris-agentic-dev.toml"));
    (home, project)
}

fn exec_in(bin: &Path, home: &Path, project: &Path, iris_host: Option<&str>) -> String {
    let (_, p, u, w) = live();
    let mut cmd = clean_command(bin);
    cmd.current_dir(project)
        .env("HOME", home)
        .env_remove("OBJECTSCRIPT_WORKSPACE")
        .env("IRIS_CONTAINER", "")
        .env("IRIS_USERNAME", &u)
        .env("IRIS_PASSWORD", &w)
        .args(["exec", "Write \"iad140:\",$ZVersion"]);
    match iris_host {
        Some(h) => cmd.env("IRIS_HOST", h).env("IRIS_WEB_PORT", &p),
        None => cmd.env_remove("IRIS_HOST").env_remove("IRIS_WEB_PORT"),
    };
    let out = cmd.output().unwrap();
    String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr)
}

#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn a_home_directory_toml_yields_to_iris_host() {
    let Some(bin) = require_iad_binary() else {
        return;
    };
    let (h, _, u, w) = live();
    let (home, project) = home_with_toml("yields", &toml_for(NOWHERE, "1", &u, &w));
    let text = exec_in(&bin, &home, &project, Some(&h));
    assert!(
        text.contains("iad140:"),
        "IRIS_HOST names the live server: {text}"
    );
}

#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn a_home_directory_toml_is_used_when_iris_host_is_unset() {
    let Some(bin) = require_iad_binary() else {
        return;
    };
    let (h, p, u, w) = live();
    let (home, project) = home_with_toml("used", &toml_for(&h, &p, &u, &w));
    let text = exec_in(&bin, &home, &project, None);
    assert!(
        text.contains("iad140:"),
        "the home file names the live server: {text}"
    );
}

/// The home file yields its connection to `IRIS_HOST`, not its tool settings: `disabled_tools`
/// still applies.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn a_home_directory_toml_keeps_its_tool_list_when_it_yields() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let (h, p, u, w) = live();
    let toml = format!(
        "{}disabled_tools = [\"iris_query\"]\n",
        toml_for(NOWHERE, "1", &u, &w)
    );
    let (home, project) = home_with_toml("tools", &toml);
    let mut env = env(&h, &p, &u, &w);
    env.push(("HOME".into(), home.to_string_lossy().into_owned()));
    let mut s = McpSession::start_in(Some(&project), &env);
    let ran = answer_text(&s.call(
        "iris_execute",
        &serde_json::json!({"code": "Write \"iad140:\",$ZVersion", "namespace": "USER"}),
    ));
    assert!(
        ran.contains("iad140:"),
        "IRIS_HOST names the live server: {ran}"
    );
    let refused = answer_text(&s.call(
        "iris_query",
        &serde_json::json!({"query": "SELECT 1", "namespace": "USER"}),
    ));
    assert!(
        !refused.contains("\"success\":true"),
        "iris_query is disabled by the home file: {refused}"
    );
}
