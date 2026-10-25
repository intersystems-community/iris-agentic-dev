//! Layer 2 for S19: `capability-matrix` and `benchmark` honour `read_only = "strict"`.
//!
//! Both compile a scratch class on the server, which strict read-only turns off. They used to
//! call the implementation directly and never met the pipeline, so `iris-agentic-dev
//! capability-matrix` compiled a class on a server whose operator had said iad writes nothing
//! there. A refusal has to come before any request: the connection is `127.0.0.1:1`, where nothing
//! listens, so a call that got past the check fails with a connection error.

#![cfg(unix)]

use std::path::PathBuf;
use std::process::{Command, Output};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_iris-agentic-dev"))
}

struct Env {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    proj: PathBuf,
}

impl Env {
    fn new(operator: &str, project: Option<&str>) -> Env {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let home = root.join("home");
        let proj = root.join("proj");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(home.join(".iris-agentic-dev.toml"), operator).unwrap();
        if let Some(p) = project {
            std::fs::write(proj.join(".iris-agentic-dev.toml"), p).unwrap();
        }
        std::fs::write(proj.join("skill.md"), "# a skill\n").unwrap();
        Env {
            _tmp: tmp,
            home,
            proj,
        }
    }

    fn run(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut c = iris_agentic_dev_core::testing::clean_command(&bin());
        c.env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .current_dir(&self.proj)
            .args(args)
            .args(["--host", "127.0.0.1", "--web-port", "1"]);
        for (k, v) in env {
            c.env(k, v);
        }
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

fn assert_strict_refusal(what: &str, o: &Output) {
    let t = text(o);
    assert_eq!(o.status.code(), Some(1), "{what}: {t}");
    assert!(t.contains("READ_ONLY_STRICT"), "{what}: {t}");
    assert!(t.contains("operator_limit"), "{what}: {t}");
    assert!(
        !t.contains("IRIS_UNREACHABLE") && !t.contains("error sending request"),
        "{what}: the command reached the network: {t}"
    );
}

const STRICT: &str = "read_only = \"strict\"\n";

fn commands() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        ("capability-matrix", vec!["capability-matrix"]),
        (
            "capability-matrix --json",
            vec!["capability-matrix", "--json"],
        ),
        ("benchmark", vec!["benchmark", "--skill", "skill.md"]),
        (
            "benchmark --mode cli-dispatch",
            vec!["benchmark", "--skill", "skill.md", "--mode", "cli-dispatch"],
        ),
    ]
}

#[test]
fn strict_in_the_operator_file_refuses_both() {
    let e = Env::new(STRICT, None);
    for (what, args) in commands() {
        assert_strict_refusal(what, &e.run(&args, &[]));
    }
}

#[test]
fn strict_in_a_project_file_refuses_both() {
    // A project file can tighten: strict is a restriction.
    let e = Env::new("", Some(STRICT));
    for (what, args) in commands() {
        assert_strict_refusal(what, &e.run(&args, &[]));
    }
}

#[test]
fn strict_in_the_launch_environment_refuses_both() {
    let e = Env::new("", None);
    for (what, args) in commands() {
        assert_strict_refusal(what, &e.run(&args, &[("IAD_READ_ONLY", "strict")]));
    }
}

#[test]
fn without_strict_both_run() {
    // The control: the refusals above are strict's doing and not a command that never ran.
    // `capability-matrix` fails on the connection; `benchmark` runs its tasks and reports every
    // one of them as errored.
    let e = Env::new("", None);
    let o = e.run(&["capability-matrix"], &[]);
    let t = text(&o);
    assert!(!t.contains("READ_ONLY_STRICT"), "{t}");
    assert_eq!(o.status.code(), Some(1), "no IRIS, so a failed probe: {t}");
    assert!(t.contains("IRIS_UNREACHABLE"), "{t}");

    let o = e.run(&["benchmark", "--skill", "skill.md"], &[]);
    let t = text(&o);
    assert!(!t.contains("READ_ONLY_STRICT"), "{t}");
    assert!(t.contains("pass_rate"), "the benchmark did not run: {t}");
}

#[test]
fn read_only_off_is_not_strict() {
    let e = Env::new("read_only = \"off\"\n", None);
    let o = e.run(&["capability-matrix"], &[]);
    assert!(!text(&o).contains("READ_ONLY_STRICT"), "{}", text(&o));
}
