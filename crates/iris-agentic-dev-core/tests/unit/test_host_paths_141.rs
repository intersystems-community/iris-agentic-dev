//! Spec 141 T212, T216, T218: the tools that read the host's disk stay inside the project, and
//! `iris_test` no longer builds a directory on the IRIS host.
//!
//! These run the tools through the pipeline (`call_gated`) against a project directory the rig
//! builds, with a server nothing listens on: a refusal has to come from the path rule, before any
//! request, and a call that is allowed fails later with a connection error, which these tests do not
//! accept as a refusal.
//!
//! The path rule itself is in `test_host_fs_141`.

use crate::support_gate_141::*;
use iris_agentic_dev_core::testing::EnvGuard;
use iris_agentic_dev_core::tools::Toolset;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const OPERATOR: &str = "mode = \"operate\"\nwrite_tools_enabled = true\n";

const CLASS: &str = "Class Host141.Sample Extends %RegisteredObject\n{\n}\n";

struct Project {
    rig: Rig,
    /// The project directory: the rig's working directory, canonical.
    proj: PathBuf,
    /// A directory beside it, outside the project.
    outside: PathBuf,
}

fn project() -> Project {
    let rig = rig_with(OPERATOR, Toolset::Merged, &[]);
    let proj = std::fs::canonicalize(rig.dir.path().join("proj")).unwrap();
    let outside = std::fs::canonicalize(rig.dir.path())
        .unwrap()
        .join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    Project { rig, proj, outside }
}

async fn call(p: &Project, tool: &str, args: Value) -> Value {
    match p.rig.tools.call_gated(tool, args).await {
        Ok(r) => body_of(&r),
        Err(e) => json!({"error_code": "PIPELINE_ERROR", "error": e.to_string()}),
    }
}

fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, text).unwrap();
    p
}

fn assert_blocked(body: &Value, reason: &str) {
    assert_eq!(body["error_code"], "HOST_PATH_BLOCKED", "{body}");
    assert_eq!(body["reason"], reason, "{body}");
    assert_eq!(body["refusal"]["kind"], "fixed_rule", "{body}");
}

// ── T212: iris_compile ────────────────────────────────────────────────────────

#[tokio::test]
async fn compile_refuses_a_file_outside_the_project_before_reading_it() {
    let p = project();
    let secret = write(&p.outside, "Secret.cls", CLASS);
    let body = call(
        &p,
        "iris_compile",
        json!({"target": secret.to_string_lossy()}),
    )
    .await;
    assert_blocked(&body, "outside_workspace");
}

#[tokio::test]
async fn compile_refuses_a_path_with_a_parent_reference_even_when_it_lands_inside() {
    let p = project();
    write(&p.proj, "Inside.cls", CLASS);
    let sneaky = format!("{}/sub/../Inside.cls", p.proj.display());
    let body = call(&p, "iris_compile", json!({"target": sneaky})).await;
    assert_blocked(&body, "parent_reference");
}

#[tokio::test]
async fn compile_refuses_a_file_that_is_not_source() {
    let p = project();
    let notes = write(&p.proj, "notes.txt", "not source");
    let body = call(
        &p,
        "iris_compile",
        json!({"target": notes.to_string_lossy()}),
    )
    .await;
    assert_blocked(&body, "extension");
    let hosts = call(&p, "iris_compile", json!({"target": "/etc/hosts"})).await;
    assert_blocked(&hosts, "extension");
}

#[cfg(unix)]
#[tokio::test]
async fn compile_refuses_a_symlink_that_leaves_the_project() {
    let p = project();
    let secret = write(&p.outside, "Secret.cls", CLASS);
    let link = p.proj.join("Link.cls");
    std::os::unix::fs::symlink(&secret, &link).unwrap();
    let body = call(
        &p,
        "iris_compile",
        json!({"target": link.to_string_lossy()}),
    )
    .await;
    assert_blocked(&body, "outside_workspace");
}

#[cfg(unix)]
#[tokio::test]
async fn compile_refuses_a_directory_symlink_that_leaves_the_project() {
    let p = project();
    write(&p.outside, "Deep.cls", CLASS);
    std::os::unix::fs::symlink(&p.outside, p.proj.join("out")).unwrap();
    let via = p.proj.join("out").join("Deep.cls");
    let body = call(&p, "iris_compile", json!({"target": via.to_string_lossy()})).await;
    assert_blocked(&body, "outside_workspace");
}

#[tokio::test]
async fn compile_refuses_a_missing_file_outside_the_project_the_same_way() {
    let p = project();
    let ghost = p.outside.join("Ghost.cls");
    let body = call(
        &p,
        "iris_compile",
        json!({"target": ghost.to_string_lossy()}),
    )
    .await;
    assert_blocked(&body, "outside_workspace");
}

#[tokio::test]
async fn compile_lets_a_file_inside_the_project_through_the_rule() {
    let p = project();
    for name in ["Ok.cls", "Ok.mac", "Ok.inc", "Ok.int", "UPPER.CLS"] {
        let file = write(&p.proj, name, CLASS);
        let body = call(
            &p,
            "iris_compile",
            json!({"target": file.to_string_lossy()}),
        )
        .await;
        assert_ne!(
            body["error_code"], "HOST_PATH_BLOCKED",
            "{name} is inside the project and source: {body}"
        );
    }
    let missing = p.proj.join("Missing.cls");
    let body = call(
        &p,
        "iris_compile",
        json!({"target": missing.to_string_lossy()}),
    )
    .await;
    assert_eq!(body["error_code"], "FILE_NOT_FOUND", "{body}");
}

// ── T216: kb index and iris_symbols_local ─────────────────────────────────────

#[tokio::test]
async fn kb_index_refuses_a_directory_outside_the_project() {
    let _learning = EnvGuard::with("OBJECTSCRIPT_LEARNING", "true");
    let p = project();
    write(&p.outside, "private.md", "# private\n");
    let body = call(
        &p,
        "kb",
        json!({"action": "index", "path": p.outside.to_string_lossy()}),
    )
    .await;
    assert_blocked(&body, "outside_workspace");
    let body = call(&p, "kb_index", json!({"workspace_path": "/etc"})).await;
    assert_blocked(&body, "outside_workspace");
}

#[cfg(unix)]
#[tokio::test]
async fn kb_index_skips_a_file_whose_link_leaves_the_project() {
    let _learning = EnvGuard::with("OBJECTSCRIPT_LEARNING", "true");
    let p = project();
    let secret = write(&p.outside, "secret.md", "# secret\n");
    std::os::unix::fs::symlink(&secret, p.proj.join("notes.md")).unwrap();
    let body = call(
        &p,
        "kb",
        json!({"action": "index", "path": p.proj.to_string_lossy()}),
    )
    .await;
    assert_eq!(body["success"], true, "the linked file is skipped: {body}");
    assert_eq!(body["indexed"], 0, "{body}");
    assert_eq!(body["skipped_outside"], 1, "{body}");
}

#[tokio::test]
async fn symbols_local_refuses_a_workspace_path_outside_the_project() {
    let p = project();
    write(&p.outside, "Secret.cls", CLASS);
    for path in [p.outside.to_string_lossy().to_string(), "/".to_string()] {
        let body = call(
            &p,
            "iris_symbols_local",
            json!({"query": "*", "workspace_path": path}),
        )
        .await;
        assert_blocked(&body, "outside_workspace");
    }
    let body = call(
        &p,
        "iris_symbols_local",
        json!({"query": "*", "workspace_path": format!("{}/..", p.proj.display())}),
    )
    .await;
    assert_blocked(&body, "parent_reference");
}

#[tokio::test]
async fn symbols_local_reads_the_project_and_a_subdirectory_of_it() {
    let p = project();
    std::fs::create_dir_all(p.proj.join("src")).unwrap();
    write(&p.proj.join("src"), "Sample.cls", CLASS);
    let default = call(&p, "iris_symbols_local", json!({"query": "Host141.*"})).await;
    assert!(
        default["symbols"].as_array().is_some_and(|s| !s.is_empty()),
        "the default workspace is the project: {default}"
    );
    let sub = call(
        &p,
        "iris_symbols_local",
        json!({"query": "Host141.*", "workspace_path": p.proj.join("src").to_string_lossy()}),
    )
    .await;
    assert!(
        sub["symbols"].as_array().is_some_and(|s| !s.is_empty()),
        "a subdirectory of the project is allowed: {sub}"
    );
    assert_eq!(sub["truncated"], false, "{sub}");
}

// ── T218: iris_test and iris_coverage patterns ────────────────────────────────

#[tokio::test]
async fn iris_test_refuses_a_pattern_that_is_a_path() {
    let p = project();
    for pattern in [
        "../etc",
        "MyApp/../..",
        "MyApp/Tests",
        "/etc",
        "MyApp\\Tests",
        "C:\\tests",
        "MyApp..Tests",
        "",
        "My\nApp",
    ] {
        let body = call(&p, "iris_test", json!({"pattern": pattern})).await;
        assert_eq!(body["error_code"], "INVALID_PARAMS", "{pattern:?}: {body}");
        assert!(
            body["error"].as_str().unwrap_or("").contains("pattern"),
            "{pattern:?}: {body}"
        );
    }
}

#[tokio::test]
async fn iris_coverage_applies_the_same_pattern_rule() {
    let p = project();
    for test_path in ["../etc", "MyApp/Tests", "MyApp\\Tests"] {
        let body = call(
            &p,
            "iris_coverage",
            json!({"mode": "run", "package": "MyApp", "test_path": test_path}),
        )
        .await;
        assert_eq!(
            body["error_code"], "INVALID_PARAMS",
            "{test_path:?}: {body}"
        );
    }
}

#[tokio::test]
async fn iris_test_lets_a_class_or_wildcard_pattern_past_the_rule() {
    let p = project();
    for pattern in ["MyApp.Tests", "MyApp.Tests.*", "MyApp", "%UnitTest.Foo"] {
        let body = call(&p, "iris_test", json!({"pattern": pattern})).await;
        assert_ne!(
            body["error_code"], "INVALID_PARAMS",
            "{pattern} is a class pattern: {body}"
        );
    }
}
