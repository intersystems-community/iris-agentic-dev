//! Every CLI subcommand is classified, and the gated ones are gated (E3).
//!
//! `cmd::COMMAND_GATES` declares, for each executable subcommand, whether it is `Gated` (every
//! step that reaches IRIS goes through the pipeline) or `Local` with a reason. This file does two
//! things with that table:
//!
//! 1. It reads the command tree off the built binary's `--help` (never off a list kept here) and
//!    compares the leaves with the table in both directions. A subcommand added without a row
//!    fails; so does a row whose command is gone.
//! 2. It runs each `Gated` command against a loopback listener that counts connections and answers
//!    404, under three operator files: `read_only = "strict"`, a `live` template that allows only
//!    `search`, and closed writes. A call the limit forbids has to end in a gate's refusal before
//!    any connection arrives. The listener stands for no IRIS behaviour.
//!
//! The invocations below are the only hand-kept part, and a `Gated` command with none fails. What
//! each invocation is judged by is the tool it dispatches to, read from the same classification
//! table the server uses.

#![cfg(unix)]

use iris_agentic_dev::cmd::{CommandGate, COMMAND_GATES};
use iris_agentic_dev_core::iris::workspace_config::ToolCategory;
use iris_agentic_dev_core::tools::write_gate::{
    classify, effective_category, tool_class, TargetKind, WriteClass,
};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Output;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_iris-agentic-dev"))
}

// ── the command tree, read off the binary ───────────────────────────────────────────────────────

fn help(path: &[String]) -> String {
    let mut c = iris_agentic_dev_core::testing::clean_command(&bin());
    c.args(path).arg("--help");
    let o = c.output().expect("spawn --help");
    assert!(o.status.success(), "{path:?} --help failed");
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// The names under a `Commands:` heading, without clap's own `help`.
fn subcommands(help_text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in help_text.lines() {
        if line.starts_with("Commands:") {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        if line.trim().is_empty() {
            break;
        }
        if let Some(name) = line.split_whitespace().next() {
            if name != "help" {
                out.push(name.to_string());
            }
        }
    }
    out
}

/// Every leaf of the tree, as a space-joined path.
fn leaves() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut stack: Vec<Vec<String>> = vec![Vec::new()];
    while let Some(path) = stack.pop() {
        let children = subcommands(&help(&path));
        if children.is_empty() {
            out.insert(path.join(" "));
        }
        for child in children {
            let mut next = path.clone();
            next.push(child);
            stack.push(next);
        }
    }
    out
}

fn declared() -> BTreeSet<String> {
    COMMAND_GATES
        .iter()
        .map(|(p, _)| p.to_string())
        .filter(|p| !p.starts_with('<'))
        .collect()
}

#[test]
fn every_subcommand_has_a_declared_gate() {
    let found = leaves();
    assert!(found.len() >= 10, "the tree read as {found:?}");
    let declared = declared();
    let missing: Vec<_> = found.difference(&declared).collect();
    assert!(
        missing.is_empty(),
        "these subcommands are not in cmd::COMMAND_GATES: {missing:?}. Declare each `Gated` (every \
         step that reaches IRIS goes through call_gated) or `Local` with the reason it reaches no \
         server."
    );
    let stale: Vec<_> = declared.difference(&found).collect();
    assert!(
        stale.is_empty(),
        "cmd::COMMAND_GATES names subcommands the binary does not have: {stale:?}"
    );
}

#[test]
fn local_commands_say_why() {
    for (path, gate) in COMMAND_GATES {
        if let CommandGate::Local(reason) = gate {
            assert!(
                reason.trim().len() >= 20,
                "{path} is Local and its reason is {reason:?}"
            );
        }
    }
    let mut seen = BTreeSet::new();
    for (path, _) in COMMAND_GATES {
        assert!(seen.insert(*path), "{path} is declared twice");
    }
}

// ── the listener ────────────────────────────────────────────────────────────────────────────────

struct Listener {
    port: u16,
    /// Requests that are not connection setup: the call itself.
    calls: Arc<AtomicUsize>,
    /// Every connection, setup included.
    all: Arc<AtomicUsize>,
}

/// Connection setup, which the CLI does before it can judge anything: `GET /api/atelier/` reads
/// the version, and the `SystemMode` query reads the mode that the environment template depends
/// on. Both are reads that precede the pipeline by design (the template cannot be judged without
/// the mode), so they are not what this test counts.
///
/// The rule: this test counts only requests past connection setup. Both reads go to the target the
/// operator configured, and the system mode is needed to pick the template, so they come first.
fn is_setup(request: &str) -> bool {
    let line = request.lines().next().unwrap_or("");
    line.starts_with("GET /api/atelier/ ") || request.contains("SystemMode")
}

impl Listener {
    fn bind() -> Listener {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let calls = Arc::new(AtomicUsize::new(0));
        let all = Arc::new(AtomicUsize::new(0));
        let (c, a) = (calls.clone(), all.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let (c, a) = (c.clone(), a.clone());
                std::thread::spawn(move || {
                    a.fetch_add(1, Ordering::SeqCst);
                    let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(500)));
                    let mut buf = [0u8; 8192];
                    let n = stream.read(&mut buf).unwrap_or(0);
                    if !is_setup(&String::from_utf8_lossy(&buf[..n])) {
                        c.fetch_add(1, Ordering::SeqCst);
                    }
                    let _ = stream.write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                });
            }
        });
        Listener { port, calls, all }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn all(&self) -> usize {
        self.all.load(Ordering::SeqCst)
    }
}

// ── the limits ──────────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
enum Limit {
    Strict,
    LiveNarrowAllow,
    WritesOff,
}

impl Limit {
    const ALL: [Limit; 3] = [Limit::Strict, Limit::LiveNarrowAllow, Limit::WritesOff];

    fn operator_file(self) -> &'static str {
        match self {
            Limit::Strict => {
                "read_only = \"strict\"\nwrite_tools_enabled = false\ndestructive_tools_enabled = false\n"
            }
            Limit::LiveNarrowAllow => concat!(
                "write_tools_enabled = false\ndestructive_tools_enabled = false\n",
                "[policy.default]\nmcpTemplate = \"live\"\nallow = [\"search\"]\n"
            ),
            Limit::WritesOff => "write_tools_enabled = false\ndestructive_tools_enabled = false\n",
        }
    }
}

/// A command line, and the tool call it comes to.
struct Invocation {
    /// The `COMMAND_GATES` path this exercises.
    path: &'static str,
    argv: Vec<String>,
    /// The tool and arguments the command dispatches to, for the class the limits judge it by.
    tool: &'static str,
    args: Value,
}

fn inv(path: &'static str, argv: &[&str], tool: &'static str, args: Value) -> Invocation {
    Invocation {
        path,
        argv: argv.iter().map(|s| s.to_string()).collect(),
        tool,
        args,
    }
}

fn invocations() -> Vec<Invocation> {
    vec![
        inv(
            "compile",
            &["compile", "Foo.cls"],
            "iris_compile",
            json!({"target": "Foo.cls"}),
        ),
        inv(
            "exec",
            &["exec", "Write 1"],
            "iris_execute",
            json!({"code": "Write 1"}),
        ),
        inv(
            "query",
            &["query", "SELECT 1"],
            "iris_query",
            json!({"query": "SELECT 1"}),
        ),
        inv(
            "doc get",
            &["doc", "get", "Foo.Bar"],
            "iris_doc",
            json!({"mode": "get", "name": "Foo.Bar.cls"}),
        ),
        inv(
            "doc put",
            &["doc", "put", "Foo.Bar", "-f", "Foo.cls"],
            "iris_doc",
            json!({"mode": "put", "name": "Foo.Bar.cls"}),
        ),
        inv(
            "tool",
            &["tool", "iris_compile", "--args", "{\"target\":\"Foo.cls\"}"],
            "iris_compile",
            json!({"target": "Foo.cls"}),
        ),
        inv(
            "tool",
            &["tool", "iris_query", "--args", "{\"query\":\"SELECT 1\"}"],
            "iris_query",
            json!({"query": "SELECT 1"}),
        ),
        inv(
            "batch",
            &["batch", "-f", "script.json"],
            "iris_execute",
            json!({"code": "Write 1"}),
        ),
        inv(
            "capability-matrix",
            &["capability-matrix"],
            "capability_matrix",
            json!({}),
        ),
        // The benchmark compiles generated classes on the server and runs them.
        inv(
            "benchmark",
            &["benchmark", "--skill", "skill.md"],
            "iris_compile",
            json!({"target": "Bench.cls"}),
        ),
    ]
}

/// Whether the limit lets the invocation's call reach the server (the rule of core's
/// `test_enum_no_leaks`): a read, not a scratch-class tool under strict, in a category `allow`
/// lists.
fn may_connect(limit: Limit, i: &Invocation) -> bool {
    let args: Map<String, Value> = i.args.as_object().cloned().unwrap_or_default();
    let Some(row) = tool_class(i.tool) else {
        return false;
    };
    if row.kind == TargetKind::Local {
        return false;
    }
    let class = classify(i.tool, Some(&args)).unwrap_or(WriteClass::Write);
    if class != WriteClass::ReadOnly {
        return false;
    }
    match limit {
        Limit::Strict => !row.scratch,
        Limit::LiveNarrowAllow => {
            let cats = effective_category(i.tool, Some(&args), class);
            !cats.is_empty() && cats.iter().all(|c| *c == ToolCategory::Search)
        }
        Limit::WritesOff => true,
    }
}

const REFUSALS: &[&str] = &[
    "WRITE_TOOLS_DISABLED",
    "DESTRUCTIVE_TOOLS_DISABLED",
    "DESTRUCTIVE_REQUIRES_WRITES",
    "READ_ONLY_STRICT",
    "POLICY_GATE",
    "ENV_GATE_BLOCKED",
    "DATA_POLICY_BLOCKED",
];

struct Env {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    proj: PathBuf,
}

impl Env {
    fn new(operator: &str) -> Env {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let home = root.join("home");
        let proj = root.join("proj");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(home.join(".iris-agentic-dev.toml"), operator).unwrap();
        std::fs::write(proj.join("skill.md"), "# a skill\n").unwrap();
        std::fs::write(proj.join("Foo.cls"), "Class Foo.Bar {}\n").unwrap();
        std::fs::write(
            proj.join("script.json"),
            r#"[{"tool": "iris_execute", "args": {"code": "Write 1"}}]"#,
        )
        .unwrap();
        Env {
            _tmp: tmp,
            home,
            proj,
        }
    }

    fn run(&self, argv: &[String], port: u16) -> Output {
        let mut c = iris_agentic_dev_core::testing::clean_command(&bin());
        c.env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .current_dir(&self.proj)
            .args(argv)
            .args(["--host", "127.0.0.1", "--web-port", &port.to_string()]);
        c.output().unwrap()
    }
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
fn every_gated_command_has_an_invocation_and_every_invocation_a_gated_command() {
    let invocations = invocations();
    for (path, gate) in COMMAND_GATES {
        if *gate == CommandGate::Gated {
            assert!(
                invocations.iter().any(|i| i.path == *path),
                "{path} is Gated and the sweep has no invocation for it"
            );
        }
    }
    for i in &invocations {
        assert!(
            COMMAND_GATES
                .iter()
                .any(|(p, g)| *p == i.path && *g == CommandGate::Gated),
            "an invocation exercises {}, which is not a Gated command",
            i.path
        );
    }
}

#[test]
fn no_gated_command_reaches_the_server_past_a_limit() {
    let mut failures = Vec::new();
    let (mut refused, mut connected) = (0, 0);
    for limit in Limit::ALL {
        let env = Env::new(limit.operator_file());
        let listener = Listener::bind();
        for i in invocations() {
            let (before, before_all) = (listener.calls(), listener.all());
            let o = env.run(&i.argv, listener.port);
            let opened = listener.calls() - before;
            let asked = listener.all() - before_all;
            let t = text(&o);
            let what = format!("{limit:?} `{}`", i.argv.join(" "));
            let allowed = may_connect(limit, &i);
            if opened > 0 {
                connected += 1;
                if !allowed {
                    failures.push(format!(
                        "{what}: sent {opened} request(s) past connection setup ({asked} connection(s) in all); the \
                         limit forbids a call to {} ({})",
                        i.tool,
                        t.trim().chars().take(200).collect::<String>()
                    ));
                }
            }
            if !allowed {
                if REFUSALS.iter().any(|code| t.contains(code)) {
                    refused += 1;
                } else {
                    failures.push(format!(
                        "{what}: the limit forbids a call to {} and no gate refused it ({})",
                        i.tool,
                        t.trim().chars().take(200).collect::<String>()
                    ));
                }
            }
        }
    }
    assert!(
        refused >= 10,
        "only {refused} refusals: the sweep did not bite"
    );
    assert!(
        connected >= 1,
        "no command reached the listener: a leak could not show"
    );
    assert!(
        failures.is_empty(),
        "{} command(s) crossed a limit:\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}
