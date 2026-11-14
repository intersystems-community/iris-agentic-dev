//! `iris-agentic-dev skill install` by tier, run as a subprocess against a temporary HOME.
//!
//! The binary fetches the manifest and each `SKILL.md` from `GITHUB_RAW_BASE_URL`. A small HTTP
//! server on a local port serves this checkout's own files under the `HEAD/` path, so the test
//! installs the real skills with their real tiers. No IRIS is involved.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// Serve `GET /intersystems-community/iris-agentic-dev/HEAD/<path>` from the checkout, until the
/// test process exits. Returns the base URL.
fn serve_repo() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let root = repo();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let root = root.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(&stream);
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    return;
                }
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                        break;
                    }
                }
                let path = line.split_whitespace().nth(1).unwrap_or("");
                let rel = path.strip_prefix("/intersystems-community/iris-agentic-dev/HEAD/");
                let body = rel
                    .filter(|r| !r.contains(".."))
                    .and_then(|r| std::fs::read(root.join(r)).ok());
                let mut out = &stream;
                let _ = match body {
                    Some(b) => write!(
                        out,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        b.len()
                    )
                    .and_then(|_| out.write_all(&b)),
                    None => write!(
                        out,
                        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    ),
                };
            });
        }
    });
    format!("http://127.0.0.1:{port}")
}

struct Run {
    stdout: String,
    stderr: String,
    ok: bool,
}

fn install(home: &Path, args: &[&str]) -> Run {
    let out = Command::new(env!("CARGO_BIN_EXE_iris-agentic-dev"))
        .args(["skill", "install", "--agent", "claude-code"])
        .args(args)
        .env("HOME", home)
        .env_remove("XDG_CONFIG_HOME")
        .env("GITHUB_RAW_BASE_URL", serve_repo())
        .output()
        .expect("spawn iris-agentic-dev");
    Run {
        stdout: String::from_utf8_lossy(&out.stdout).into(),
        stderr: String::from_utf8_lossy(&out.stderr).into(),
        ok: out.status.success(),
    }
}

fn installed(home: &Path) -> Vec<String> {
    let base = home.join(".claude/skills");
    let mut v: Vec<String> = std::fs::read_dir(&base)
        .map(|it| {
            it.flatten()
                .filter(|e| e.path().join("SKILL.md").is_file())
                .map(|e| e.file_name().to_string_lossy().into())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

const CORE: &[&str] = &[
    "iris-agentic-dev",
    "iris-connectivity",
    "iris-sql",
    "objectscript-debugging",
    "objectscript-guardrails",
    "objectscript-list-patterns",
    "objectscript-review",
    "objectscript-sql-patterns",
    "objectscript-tdd",
    "objectscript-unit-test",
];

#[test]
fn bare_install_writes_the_core_skills_only() {
    let home = tempfile::TempDir::new().unwrap();
    let r = install(home.path(), &[]);
    assert!(r.ok, "{}\n{}", r.stdout, r.stderr);
    assert_eq!(installed(home.path()), CORE, "{}", r.stdout);
    assert!(r.stdout.contains("10 written"), "{}", r.stdout);
}

#[test]
fn dry_run_writes_nothing() {
    let home = tempfile::TempDir::new().unwrap();
    let r = install(home.path(), &["--dry-run"]);
    assert!(r.ok, "{}\n{}", r.stdout, r.stderr);
    assert!(installed(home.path()).is_empty());
    assert!(r.stdout.contains("10 written"), "{}", r.stdout);
}

#[test]
fn all_installs_core_and_extra_but_not_internal() {
    let home = tempfile::TempDir::new().unwrap();
    let r = install(home.path(), &["--all"]);
    assert!(r.ok, "{}\n{}", r.stdout, r.stderr);
    let got = installed(home.path());
    for c in CORE {
        assert!(got.contains(&c.to_string()), "{c} missing: {got:?}");
    }
    assert!(got.contains(&"iris-query-plans".to_string()), "{got:?}");
    assert!(!got.contains(&"opencode-introspect".to_string()), "{got:?}");
    assert_eq!(got.len(), 35, "{got:?}");
}

#[test]
fn a_named_internal_skill_installs() {
    let home = tempfile::TempDir::new().unwrap();
    let r = install(home.path(), &["opencode-introspect"]);
    assert!(r.ok, "{}\n{}", r.stdout, r.stderr);
    assert_eq!(installed(home.path()), ["opencode-introspect"]);
}

/// An install from before tiers left extra and internal skills behind. A bare install names them
/// and the flag that removes them, and removes nothing itself. `--prune` removes the managed copies
/// and leaves a user-authored file alone.
#[test]
fn leftovers_get_a_hint_and_prune_removes_only_managed_copies() {
    let home = tempfile::TempDir::new().unwrap();
    let r = install(home.path(), &["--all"]);
    assert!(r.ok, "{}", r.stderr);
    let r = install(home.path(), &["opencode-introspect"]);
    assert!(r.ok, "{}", r.stderr);
    let mine = home.path().join(".claude/skills/iris-vector-ai/SKILL.md");
    std::fs::write(&mine, "---\nname: iris-vector-ai\n---\nmy notes\n").unwrap();

    let r = install(home.path(), &[]);
    assert!(r.ok, "{}", r.stderr);
    assert!(r.stdout.contains("iris-query-plans"), "{}", r.stdout);
    assert!(r.stdout.contains("opencode-introspect"), "{}", r.stdout);
    assert!(r.stdout.contains("skill install --prune"), "{}", r.stdout);
    assert!(
        !r.stdout.contains("iris-vector-ai"),
        "user file is no leftover: {}",
        r.stdout
    );
    assert_eq!(installed(home.path()).len(), 36, "hint deletes nothing");

    let r = install(home.path(), &["--prune", "--dry-run"]);
    assert!(r.ok, "{}", r.stderr);
    assert!(r.stdout.contains("would remove"), "{}", r.stdout);
    assert_eq!(installed(home.path()).len(), 36, "dry run deletes nothing");

    let r = install(home.path(), &["--prune"]);
    assert!(r.ok, "{}", r.stderr);
    let mut want: Vec<String> = CORE.iter().map(|s| s.to_string()).collect();
    want.push("iris-vector-ai".into());
    want.sort();
    assert_eq!(installed(home.path()), want, "{}", r.stdout);
    assert!(mine.exists());
}

#[test]
fn prune_with_named_skills_is_refused() {
    let home = tempfile::TempDir::new().unwrap();
    let r = install(home.path(), &["--prune", "iris-sql"]);
    assert!(!r.ok, "{}", r.stdout);
    assert!(r.stderr.contains("--prune"), "{}", r.stderr);
}

#[test]
fn list_marks_each_skill_with_its_tier() {
    let home = tempfile::TempDir::new().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_iris-agentic-dev"))
        .args(["skill", "list", "--agent", "claude-code"])
        .env("HOME", home.path())
        .env_remove("XDG_CONFIG_HOME")
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{s}");
    let line = |n: &str| {
        s.lines()
            .find(|l| l.split_whitespace().next() == Some(n))
            .unwrap_or_else(|| panic!("{n} not listed:\n{s}"))
            .to_string()
    };
    assert!(line("objectscript-guardrails").contains("core"));
    assert!(line("iris-query-plans").contains("extra"));
    assert!(!s.contains("opencode-introspect"), "{s}");
    assert!(!s.contains("iris-vector-graph"), "stale name: {s}");
}
