//! Layer 2 for S19: `capability-matrix` and `benchmark` honour `read_only = "strict"`.
//!
//! Both compile a scratch class on the server, which strict read-only turns off. They used to
//! call the implementation directly and never met the pipeline, so `iris-agentic-dev
//! capability-matrix` compiled a class on a server whose operator had said iad writes nothing
//! there. A refusal has to come before any request: the connection is `127.0.0.1:1`, where nothing
//! listens, so a call that got past the check fails with a connection error.

#![cfg(unix)]

use std::path::PathBuf;
use std::process::Output;

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

// ── R3: the template, `allow` and the write gate ───────────────────────────────────────────────
//
// `benchmark` compiles classes on the server and runs code in it, so it meets the same pipeline
// as `capability-matrix`: the operator file's template, `allow` and write gate answer for every
// step, not only strict. Nothing listens at `127.0.0.1:1`, so a refusal is the gate's answer and a
// run that got past it reports the connection or a table of errored tasks.

fn assert_refused(what: &str, code: &str, o: &Output) {
    let t = text(o);
    assert_eq!(o.status.code(), Some(1), "{what}: {t}");
    assert!(t.contains(code), "{what}: expected {code}: {t}");
    assert!(t.contains("\"refusal\""), "{what}: no refusal object: {t}");
    assert!(
        !t.contains("pass_rate"),
        "{what}: the benchmark ran and reported a result: {t}"
    );
}

fn benchmarks() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        ("benchmark", vec!["benchmark", "--skill", "skill.md"]),
        (
            "benchmark --mode cli-dispatch",
            vec!["benchmark", "--skill", "skill.md", "--mode", "cli-dispatch"],
        ),
        (
            "benchmark --baseline",
            vec!["benchmark", "--skill", "skill.md", "--baseline"],
        ),
    ]
}

#[test]
fn the_live_template_refuses_benchmark_before_it_compiles() {
    let e = Env::new("[policy.default]\nmcpTemplate = \"live\"\n", None);
    for (what, args) in benchmarks() {
        assert_refused(what, "ENV_GATE_BLOCKED", &e.run(&args, &[]));
    }
}

#[test]
fn the_test_template_refuses_it_too() {
    let e = Env::new("[policy.default]\nmcpTemplate = \"test\"\n", None);
    assert_refused(
        "benchmark",
        "ENV_GATE_BLOCKED",
        &e.run(&["benchmark", "--skill", "skill.md"], &[]),
    );
}

#[test]
fn a_narrow_allow_refuses_benchmark() {
    let e = Env::new("[policy.default]\nallow = [\"query\", \"search\"]\n", None);
    for (what, args) in benchmarks() {
        assert_refused(what, "POLICY_GATE", &e.run(&args, &[]));
    }
}

#[test]
fn a_project_file_that_narrows_allow_refuses_it() {
    // A project file can tighten.
    let e = Env::new("", Some("[policy.default]\nallow = [\"query\"]\n"));
    assert_refused(
        "benchmark",
        "POLICY_GATE",
        &e.run(&["benchmark", "--skill", "skill.md"], &[]),
    );
}

#[test]
fn closed_write_tools_refuse_benchmark() {
    let e = Env::new("write_tools_enabled = false\n", None);
    for (what, args) in benchmarks() {
        assert_refused(what, "WRITE_TOOLS_DISABLED", &e.run(&args, &[]));
    }
}

#[test]
fn an_allow_that_stops_at_the_compile_stops_the_run_at_the_compile() {
    // `execute` is allowed, so the lock and the version query pass; the first class the run would
    // store is a `compile` call and is refused. The run stops there: exit 1, the refusal on
    // stderr, and no table of errored tasks that would read as a benchmark result.
    let e = Env::new("[policy.default]\nallow = [\"execute\"]\n", None);
    let o = e.run(&["benchmark", "--skill", "skill.md"], &[]);
    assert_refused("benchmark", "POLICY_GATE", &o);
    assert!(
        text(&o).contains("\"blocked_category\":\"compile\""),
        "{}",
        text(&o)
    );
}

#[test]
fn an_allow_that_lists_what_the_run_needs_lets_it_start() {
    // The control: `execute` and `compile` are what the run does, and a run that lists them is not
    // refused. (It fails on the connection, not on a gate.)
    let e = Env::new(
        "[policy.default]\nallow = [\"execute\", \"compile\"]\n",
        None,
    );
    let o = e.run(&["benchmark", "--skill", "skill.md"], &[]);
    let t = text(&o);
    assert!(
        !t.contains("POLICY_GATE") && !t.contains("ENV_GATE_BLOCKED"),
        "{t}"
    );
    assert!(t.contains("pass_rate"), "the benchmark did not run: {t}");
}

// ── live ────────────────────────────────────────────────────────────────────────────────────────

fn live(args: &[&str], strict: bool) -> Output {
    let tmp = tempfile::tempdir().unwrap();
    let env_or = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
    let mut c = iris_agentic_dev_core::testing::clean_command(&bin());
    crate::support_141::isolate(&mut c);
    c.current_dir(tmp.path())
        .env("IRIS_HOST", env_or("IRIS_HOST", "localhost"))
        .env("IRIS_WEB_PORT", env_or("IRIS_WEB_PORT", "52780"))
        .env("IRIS_USERNAME", env_or("IRIS_USERNAME", "_SYSTEM"))
        .env("IRIS_PASSWORD", env_or("IRIS_PASSWORD", "SYS"))
        .args(args);
    if strict {
        c.env("IAD_READ_ONLY", "strict");
    }
    c.output().unwrap()
}

/// `capability-matrix` now goes through the pipeline. Against a live server it still answers, and
/// under strict it compiles nothing: the refusal is the answer, not a probe that ran.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn capability_matrix_still_answers_live_and_is_refused_under_strict() {
    let o = live(&["capability-matrix", "--json"], false);
    let t = text(&o);
    assert_eq!(o.status.code(), Some(0), "{t}");
    let body: serde_json::Value = serde_json::from_str(String::from_utf8_lossy(&o.stdout).trim())
        .unwrap_or_else(|e| panic!("not JSON ({e}): {t}"));
    assert_ne!(body["success"], false, "{t}");

    let o = live(&["capability-matrix", "--json"], true);
    assert_strict_refusal("live, strict", &o);
}

// ── R3, live ────────────────────────────────────────────────────────────────────────────────────

/// `benchmark` against the live server with an operator file of its own.
fn live_benchmark(operator: &str) -> Output {
    let tmp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(tmp.path()).unwrap();
    let home = root.join("home");
    let proj = root.join("proj");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(home.join(".iris-agentic-dev.toml"), operator).unwrap();
    std::fs::write(proj.join("skill.md"), "# a skill\n").unwrap();
    let env_or = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
    iris_agentic_dev_core::testing::clean_command(&bin())
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .current_dir(&proj)
        .env("IRIS_HOST", env_or("IRIS_HOST", "localhost"))
        .env("IRIS_WEB_PORT", env_or("IRIS_WEB_PORT", "52780"))
        .env("IRIS_USERNAME", env_or("IRIS_USERNAME", "_SYSTEM"))
        .env("IRIS_PASSWORD", env_or("IRIS_PASSWORD", "SYS"))
        .args(["benchmark", "--skill", "skill.md"])
        .output()
        .unwrap()
}

/// The classes the first step of every task stores, as Atelier document names.
fn fixture_docs() -> Vec<String> {
    use iris_agentic_dev_core::benchmark::{extract_class_name, load_embedded_tasks};
    let mut docs: Vec<String> = load_embedded_tasks()
        .unwrap()
        .iter()
        .flat_map(|t| t.initial_code.files.iter())
        .filter_map(|f| extract_class_name(&f.content))
        .map(|c| format!("{c}.cls"))
        .collect();
    docs.sort();
    docs.dedup();
    docs
}

struct LiveIris {
    rt: tokio::runtime::Runtime,
    conn: iris_agentic_dev_core::iris::connection::IrisConnection,
    client: reqwest::Client,
}

impl LiveIris {
    fn new() -> Self {
        let t = iris_agentic_dev_core::testing::LiveTarget::from_env();
        t.assert_running();
        LiveIris {
            rt: tokio::runtime::Runtime::new().unwrap(),
            conn: iris_agentic_dev_core::iris::connection::IrisConnection::new(
                t.base_url(),
                &t.namespace,
                &t.username,
                &t.password,
                iris_agentic_dev_core::iris::connection::DiscoverySource::ExplicitFlag,
            ),
            client: reqwest::Client::new(),
        }
    }

    fn doc_url(&self, name: &str) -> String {
        self.conn.versioned_ns_url("USER", &format!("/doc/{name}"))
    }

    fn stored(&self, name: &str) -> bool {
        self.rt.block_on(async {
            self.client
                .get(self.doc_url(name))
                .basic_auth(&self.conn.username, Some(&self.conn.password))
                .send()
                .await
                .unwrap()
                .status()
                .is_success()
        })
    }

    fn delete(&self, name: &str) {
        self.rt.block_on(async {
            let _ = self
                .client
                .delete(self.doc_url(name))
                .basic_auth(&self.conn.username, Some(&self.conn.password))
                .send()
                .await;
        });
    }

    /// The benchmark lock the command takes against `localhost`.
    fn lock(&self) -> String {
        self.rt
            .block_on(self.conn.execute_via_generator(
                "write $Get(^IRISDEV(\"telemetry\",\"benchmark_lock\",\"localhost\"))",
                "USER",
                &self.client,
            ))
            .unwrap()
            .trim()
            .to_string()
    }
}

/// A `live` template refuses `benchmark` against a real server before it stores a class or takes
/// the lock; the dev control stores the fixtures (the run then errors on the missing model) and
/// gives the lock back.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn benchmark_under_a_live_template_stores_nothing_on_the_server() {
    let iris = LiveIris::new();
    let docs = fixture_docs();
    assert!(docs.len() > 3, "the task suite has no fixtures: {docs:?}");
    for d in &docs {
        iris.delete(d);
    }
    let lock_before = iris.lock();

    let o = live_benchmark("[policy.default]\nmcpTemplate = \"live\"\n");
    assert_refused("live template, live server", "ENV_GATE_BLOCKED", &o);
    let stored: Vec<&String> = docs.iter().filter(|d| iris.stored(d)).collect();
    assert!(stored.is_empty(), "the refused run stored {stored:?}");
    assert_eq!(iris.lock(), lock_before, "the refused run took the lock");

    // The control: the same server and command with the dev template gets as far as the fixtures.
    let o = live_benchmark("");
    let t = text(&o);
    let stored: Vec<&String> = docs.iter().filter(|d| iris.stored(d)).collect();
    for d in &docs {
        iris.delete(d);
    }
    assert!(t.contains("pass_rate"), "the control did not run: {t}");
    assert!(
        !stored.is_empty(),
        "the control stored no fixture, so the refusal above proves nothing: {t}"
    );
    assert_eq!(iris.lock(), lock_before, "the control left the lock held");
}
