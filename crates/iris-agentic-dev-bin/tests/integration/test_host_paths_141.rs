//! Layer 2 for spec 141 T212, T216 and T218: the CLI and the compile hook hold host paths to the
//! project directory, and `iris_test` refuses a pattern that is a path.
//!
//! No IRIS is needed. The server is `127.0.0.1:3`, where nothing listens: a refusal has to come
//! before any request, and a call the rule lets through fails later with a connection error, which
//! these tests read as "got past the rule".

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_iris-agentic-dev"))
}

struct Env {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    proj: PathBuf,
    outside: PathBuf,
}

impl Env {
    fn new() -> Env {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let (home, proj, outside) = (root.join("home"), root.join("proj"), root.join("outside"));
        for d in [&home, &proj, &outside] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(
            home.join(".iris-agentic-dev.toml"),
            "write_tools_enabled = true\n",
        )
        .unwrap();
        Env {
            _tmp: tmp,
            home,
            proj,
            outside,
        }
    }

    fn command(&self, program: &Path) -> Command {
        let mut c = iris_agentic_dev_core::testing::clean_command(program);
        c.env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .current_dir(&self.proj);
        c
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(&bin())
            .args(args)
            .args(["--host", "127.0.0.1", "--web-port", "3"])
            .output()
            .unwrap()
    }
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn assert_blocked(what: &str, o: &Output, reason: &str) {
    let t = text(o);
    assert_eq!(o.status.code(), Some(1), "{what}: {t}");
    assert!(t.contains("HOST_PATH_BLOCKED"), "{what}: {t}");
    assert!(t.contains(reason), "{what}: expected reason {reason}: {t}");
    assert!(
        !t.contains("error sending request") && !t.contains("IRIS_UNREACHABLE"),
        "{what}: the call reached the network instead of being refused: {t}"
    );
}

fn assert_past_the_rule(what: &str, o: &Output) {
    let t = text(o);
    assert!(
        !t.contains("HOST_PATH_BLOCKED"),
        "{what}: refused by the path rule: {t}"
    );
}

#[test]
fn compile_of_a_file_outside_the_project_is_refused_before_any_request() {
    let e = Env::new();
    let secret = e.outside.join("Secret.cls");
    std::fs::write(&secret, "Class Secret {}\n").unwrap();
    let o = e.run(&["compile", secret.to_str().unwrap()]);
    assert_blocked("compile outside", &o, "outside_workspace");

    let o = e.run(&["compile", "../outside/Secret.cls"]);
    assert_blocked("compile ../", &o, "parent_reference");

    let hosts = e.run(&[
        "tool",
        "iris_compile",
        "--args",
        r#"{"target":"/etc/hosts"}"#,
    ]);
    assert_blocked("tool iris_compile /etc/hosts", &hosts, "extension");
}

#[test]
fn compile_of_a_file_inside_the_project_gets_past_the_rule() {
    let e = Env::new();
    let sub = e.proj.join("src");
    std::fs::create_dir_all(&sub).unwrap();
    let file = sub.join("Foo.cls");
    std::fs::write(&file, "Class Foo {}\n").unwrap();
    let absolute = e.run(&["compile", file.to_str().unwrap()]);
    assert_past_the_rule("absolute", &absolute);
    assert!(
        text(&absolute).contains("Foo.cls"),
        "the compile ran and named the file: {}",
        text(&absolute)
    );
    let relative = e.run(&["compile", "src/Foo.cls"]);
    assert_past_the_rule("relative", &relative);
}

#[test]
fn a_symlink_out_of_the_project_is_refused() {
    let e = Env::new();
    let secret = e.outside.join("Secret.cls");
    std::fs::write(&secret, "Class Secret {}\n").unwrap();
    std::os::unix::fs::symlink(&secret, e.proj.join("Link.cls")).unwrap();
    let o = e.run(&["compile", e.proj.join("Link.cls").to_str().unwrap()]);
    assert_blocked("compile link", &o, "outside_workspace");
}

/// The compile hook runs the real binary from the project directory with an absolute path inside
/// it, as Claude Code does, and that must still compile. A path outside is refused.
#[test]
fn the_compile_hook_still_compiles_an_absolute_path_inside_the_project() {
    let e = Env::new();
    let stubs = e.outside.join("bin");
    std::fs::create_dir_all(&stubs).unwrap();
    std::os::unix::fs::symlink(bin(), stubs.join("iris-agentic-dev")).unwrap();
    let hook = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/compile-hook.sh");
    let inside = e.proj.join("Foo.cls");
    std::fs::write(&inside, "Class Foo {}\n").unwrap();
    let outside = e.outside.join("Bar.cls");
    std::fs::write(&outside, "Class Bar {}\n").unwrap();

    let run_hook = |file: &Path| -> Output {
        let path = format!(
            "{}:{}",
            stubs.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut child = e
            .command(Path::new("bash"))
            .arg(&hook)
            .env("PATH", path)
            .env("IRIS_HOST", "127.0.0.1")
            .env("IRIS_WEB_PORT", "3")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        {
            use std::io::Write;
            let input = serde_json::json!({
                "hook_event_name": "PostToolUse",
                "tool_input": {"file_path": file.to_string_lossy()},
            });
            child
                .stdin
                .take()
                .unwrap()
                .write_all(input.to_string().as_bytes())
                .unwrap();
        }
        child.wait_with_output().unwrap()
    };

    let ok = run_hook(&inside);
    assert!(ok.status.success(), "{}", text(&ok));
    assert_past_the_rule("hook inside", &ok);
    assert!(
        text(&ok).contains("Foo.cls"),
        "the hook reached the compile: {}",
        text(&ok)
    );

    let refused = run_hook(&outside);
    assert!(refused.status.success(), "a hook never fails the edit");
    assert!(
        text(&refused).contains("HOST_PATH_BLOCKED"),
        "{}",
        text(&refused)
    );
}

#[test]
fn symbols_local_and_kb_index_refuse_a_directory_outside_the_project() {
    let e = Env::new();
    std::fs::write(e.outside.join("Secret.cls"), "Class Secret {}\n").unwrap();
    let args = format!(
        r#"{{"query":"*","workspace_path":"{}"}}"#,
        e.outside.display()
    );
    let o = e.run(&["tool", "iris_symbols_local", "--args", &args]);
    assert_blocked("symbols_local outside", &o, "outside_workspace");

    let o = e.run(&[
        "tool",
        "iris_symbols_local",
        "--args",
        r#"{"query":"*","workspace_path":"/"}"#,
    ]);
    assert_blocked("symbols_local /", &o, "outside_workspace");

    let args = format!(r#"{{"action":"index","path":"{}"}}"#, e.outside.display());
    let o = e.run(&["tool", "kb", "--args", &args]);
    assert_blocked("kb index outside", &o, "outside_workspace");
}

#[test]
fn symbols_local_reads_the_project_when_no_path_is_given() {
    let e = Env::new();
    std::fs::write(e.proj.join("Mine.cls"), "Class Host141.Mine {\n}\n").unwrap();
    let o = e.run(&[
        "tool",
        "iris_symbols_local",
        "--args",
        r#"{"query":"Host141.*"}"#,
    ]);
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    assert!(t.contains("Host141.Mine"), "{t}");
    assert!(t.contains("\"truncated\":false"), "{t}");
}

#[test]
fn iris_test_refuses_a_pattern_that_is_a_path() {
    let e = Env::new();
    for pattern in ["../x", "a/b", "a\\\\b"] {
        let args = format!(r#"{{"pattern":"{pattern}"}}"#);
        let o = e.run(&["tool", "iris_test", "--args", &args]);
        let t = text(&o);
        assert!(t.contains("INVALID_PARAMS"), "{pattern}: {t}");
        assert!(!t.contains("error sending request"), "{pattern}: {t}");
    }
}
