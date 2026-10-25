//! Layer 2 for spec 141 FR-030 to FR-036, FR-042: a workspace file cannot raise a gate until the
//! operator trusts it, and the server says what it set aside.
//!
//! Each test spawns `iris-agentic-dev mcp` with a temporary `HOME` and `USERPROFILE` (set here, so
//! the test does not depend on `clean_command` isolating them), a project directory of its own and a
//! `namespace = "PROD"` project file. With no IRIS to ask and a production-looking namespace the
//! inferred write gate is off, so a gate that is on can only have come from the file.

use iris_agentic_dev_core::testing::{clean_mcp_command, iad_binary_path, stop_server};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};

struct Session {
    child: Child,
    reader: BufReader<std::process::ChildStdout>,
    next_id: u64,
    stderr_path: PathBuf,
}

impl Session {
    fn start(home: &Path, cwd: &Path, env: &[(&str, &str)]) -> Session {
        let bin = iad_binary_path();
        assert!(
            bin.exists(),
            "no binary at {}; cargo build first",
            bin.display()
        );
        let stderr_path = home.join("stderr.log");
        let stderr = std::fs::File::create(&stderr_path).unwrap();
        let mut cmd = clean_mcp_command(&bin);
        cmd.env("HOME", home)
            .env("USERPROFILE", home)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(stderr));
        for (k, v) in env {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().expect("spawn iris-agentic-dev mcp");
        let reader = BufReader::new(child.stdout.take().unwrap());
        let mut s = Session {
            child,
            reader,
            next_id: 1,
            stderr_path,
        };
        let init = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t141","version":"0"}}});
        writeln!(s.child.stdin.as_mut().unwrap(), "{init}").unwrap();
        s.read(1);
        writeln!(
            s.child.stdin.as_mut().unwrap(),
            "{}",
            json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}})
        )
        .unwrap();
        s
    }

    fn read(&mut self, id: u64) -> Value {
        let mut line = String::new();
        loop {
            line.clear();
            let n = self.reader.read_line(&mut line).expect("read stdout");
            assert!(n != 0, "the server closed stdout before answering {id}");
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                if v.get("id").and_then(Value::as_u64) == Some(id) {
                    return v;
                }
            }
        }
    }

    fn call(&mut self, tool: &str, args: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        let req = json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":tool,"arguments":args}});
        writeln!(self.child.stdin.as_mut().unwrap(), "{req}").unwrap();
        self.read(id)
    }

    /// The tool's JSON body.
    fn body(&mut self, tool: &str, args: Value) -> Value {
        let answer = self.call(tool, args);
        let text = answer
            .pointer("/result/content/0/text")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("no text in {answer}"));
        serde_json::from_str(text).unwrap_or_else(|_| panic!("not JSON: {text}"))
    }

    fn stderr(&self) -> String {
        std::fs::read_to_string(&self.stderr_path).unwrap_or_default()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        drop(self.child.stdin.take());
        stop_server(&mut self.child);
    }
}

struct Env {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    proj: PathBuf,
}

impl Env {
    fn new(project_toml: &str) -> Env {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let home = root.join("home");
        let proj = root.join("proj");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(proj.join(".iris-agentic-dev.toml"), project_toml).unwrap();
        Env {
            _tmp: tmp,
            home,
            proj,
        }
    }
    fn ws_file(&self) -> PathBuf {
        self.proj.join(".iris-agentic-dev.toml")
    }
    fn write_operator(&self, body: &str) {
        std::fs::write(self.home.join(".iris-agentic-dev.toml"), body).unwrap();
    }
    fn start(&self) -> Session {
        Session::start(&self.home, &self.proj, &[])
    }
}

/// A project file that raises writes, aimed at an address nothing listens on.
const RAISING: &str =
    "host = \"127.0.0.1\"\nweb_port = 2\nnamespace = \"PROD\"\nwrite_tools_enabled = true\n";

fn trust_entry(ws_file: &Path) -> String {
    format!(
        "[[trust]]\npath = {:?}\nformat = 1\nsettings = ['connection = \"http://127.0.0.1:2\"', 'connection.namespace = \"PROD\"', 'write_tools_enabled = true']\ntrusted_at = \"2026-10-23T00:00:00Z\"\n",
        ws_file.to_string_lossy()
    )
}

fn set_aside_lines(stderr: &str) -> Vec<&str> {
    stderr.lines().filter(|l| l.contains("set aside")).collect()
}

// T-R4a-B01
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn an_untrusted_file_that_raises_writes_leaves_them_off_and_says_so() {
    let env = Env::new(RAISING);
    let mut s = env.start();
    let cfg = s.body("check_config", json!({}));

    assert_eq!(cfg["write_tools_enabled"], false, "{cfg}");
    assert_eq!(
        cfg["config_sources"]["workspace_file"]["trust"], "untrusted",
        "{cfg}"
    );
    let ids: Vec<&str> = cfg["set_aside"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["id"].as_str())
        .collect();
    assert!(ids.contains(&"workspace:write_tools_enabled"), "{ids:?}");
    let item = cfg["set_aside"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == "workspace:write_tools_enabled")
        .unwrap();
    assert_eq!(item["kind"], "untrusted_setting");
    assert!(
        item["lift"]
            .as_str()
            .unwrap()
            .contains("iris-agentic-dev trust"),
        "{item}"
    );

    let stderr = s.stderr();
    let lines = set_aside_lines(&stderr);
    assert_eq!(lines.len(), 1, "exactly one stderr line, got: {stderr:?}");
    assert!(
        lines[0].contains("is not trusted") && lines[0].contains("trust --show"),
        "{}",
        lines[0]
    );

    // And the gate really is closed: a write is refused.
    let r = s.body(
        "iris_doc",
        json!({"mode": "put", "name": "T141.A.cls", "content": "Class T141.A {}"}),
    );
    assert_eq!(r["error_code"], "WRITE_TOOLS_DISABLED", "{r}");
}

// T-R4a-B02
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn a_trusted_file_that_raises_writes_turns_them_on_quietly() {
    let env = Env::new(RAISING);
    env.write_operator(&trust_entry(&env.ws_file()));
    let mut s = env.start();
    let cfg = s.body("check_config", json!({}));

    assert_eq!(cfg["write_tools_enabled"], true, "{cfg}");
    assert_eq!(cfg["write_tools_source"], "config_file", "{cfg}");
    assert_eq!(
        cfg["config_sources"]["workspace_file"]["trust"], "trusted",
        "{cfg}"
    );
    assert!(
        cfg["set_aside"].as_array().unwrap().is_empty(),
        "nothing is set aside: {}",
        cfg["set_aside"]
    );
    assert!(set_aside_lines(&s.stderr()).is_empty(), "{:?}", s.stderr());
    assert_eq!(cfg["config_sources"]["operator_file"]["loaded"], true);
}

// T-R4a-B03, R5 C12 (PLATFORM-20)
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn editing_a_trusted_file_mid_session_lapses_trust_and_sets_everything_aside() {
    let env = Env::new(RAISING);
    env.write_operator(&trust_entry(&env.ws_file()));
    let mut s = env.start();
    assert_eq!(
        s.body("check_config", json!({}))["write_tools_enabled"],
        true
    );

    // The agent edits the project file: one more loosening setting.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(
        env.ws_file(),
        format!("{RAISING}[policy.default]\ndataPolicy = \"allow\"\n"),
    )
    .unwrap();

    let cfg = s.body("check_config", json!({}));
    let ws = &cfg["config_sources"]["workspace_file"];
    assert_eq!(ws["trust"], "lapsed", "{cfg}");
    assert!(
        ws["changed"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["setting"] == "policy.default.dataPolicy" && c["now"] == "\"allow\""),
        "changed names the key: {ws}"
    );
    assert_eq!(
        cfg["write_tools_enabled"], false,
        "the loosening that was trusted is set aside too: {cfg}"
    );
    let ids: Vec<&str> = cfg["set_aside"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["id"].as_str())
        .collect();
    for want in [
        "workspace:write_tools_enabled",
        "workspace:policy.default.dataPolicy",
    ] {
        assert!(ids.contains(&want), "{want} missing: {ids:?}");
    }
    let r = s.body(
        "iris_doc",
        json!({"mode": "put", "name": "T141.A.cls", "content": "Class T141.A {}"}),
    );
    assert_eq!(r["error_code"], "WRITE_TOOLS_DISABLED", "{r}");
}

// T-R4a-B09
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn the_role_names_are_reported_in_check_config() {
    let env = Env::new(
        "mode = \"operate\"\n[instance.a]\nhost = \"a.example\"\nrole = \"workspace\"\n\
         [instance.b]\nhost = \"b.example\"\nrole = \"control-plane\"\n\
         [instance.c]\nhost = \"c.example\"\nrole = \"contrl-plane\"\n",
    );
    let mut s = env.start();
    let cfg = s.body("check_config", json!({}));
    let errs: Vec<String> = cfg["config_errors"]
        .as_array()
        .unwrap_or_else(|| panic!("no config_errors: {cfg}"))
        .iter()
        .map(|e| e.as_str().unwrap().to_string())
        .collect();
    for (name, needle) in [
        ("a", "workspace"),
        ("b", "control-plane"),
        ("c", "contrl-plane"),
    ] {
        assert!(
            errs.iter()
                .any(|e| e.contains(&format!("instance.{name}")) && e.contains(needle)),
            "no note for instance.{name}: {errs:?}"
        );
    }
}

// T-R4a-B10
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn the_watch_path_is_never_the_operator_file() {
    let env = Env::new("namespace = \"USER\"\n");
    env.write_operator("namespace = \"OPNS\"\n");
    let operator = env.home.join(".iris-agentic-dev.toml");

    // cwd "/" used to fall back to $HOME/.iris-agentic-dev.toml, the file the agent was told to write.
    for cwd in [Path::new("/"), env.home.as_path()] {
        let mut s = Session::start(&env.home, cwd, &[]);
        let cfg = s.body("check_config", json!({}));
        assert!(
            cfg["config_watch_path"].is_null(),
            "cwd {}: config_watch_path must be null, got {}",
            cwd.display(),
            cfg["config_watch_path"]
        );
        assert!(!cfg
            .to_string()
            .contains("write a .iris-agentic-dev.toml to that exact path"));
        assert_eq!(
            cfg["config_sources"]["operator_file"]["path"],
            operator.to_string_lossy().as_ref()
        );
    }

    // In a project, the watch path is the project's file.
    let mut s = env.start();
    let cfg = s.body("check_config", json!({}));
    assert_eq!(
        cfg["config_watch_path"].as_str().map(PathBuf::from),
        Some(env.ws_file())
    );
}

// R5 C11
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn a_broken_workspace_file_closes_the_gates_and_every_iris_tool_says_why() {
    let env = Env::new("host = \"127.0.0.1\"\nwrite_tools_enabled = = true\n");
    let mut s = env.start();

    let cfg = s.body("check_config", json!({}));
    assert_eq!(cfg["connected"], false, "{cfg}");
    assert_eq!(cfg["write_tools_enabled"], false);
    assert_eq!(cfg["destructive_tools_enabled"], false);
    let ws = env.ws_file();
    assert!(
        cfg["config_parse_error"]
            .as_str()
            .unwrap_or("")
            .contains(ws.to_str().unwrap()),
        "{cfg}"
    );

    for (tool, args) in [
        ("iris_execute", json!({"code": "Write 1"})),
        ("iris_query", json!({"query": "SELECT 1"})),
        ("iris_doc", json!({"mode": "get", "name": "A.B.cls"})),
        ("iris_compile", json!({"target": "A.B.cls"})),
        (
            "iris_global",
            json!({"action": "get", "global_name": "Zed"}),
        ),
    ] {
        let answer = s.call(tool, args);
        assert_eq!(answer["result"]["isError"], true, "{tool}: {answer}");
        let text = answer["result"]["content"][0]["text"].as_str().unwrap();
        let body: Value = serde_json::from_str(text).unwrap();
        assert_eq!(body["error_code"], "CONFIG_INVALID", "{tool}: {body}");
        assert_eq!(
            body["file"],
            ws.to_string_lossy().as_ref(),
            "{tool}: {body}"
        );
        assert_eq!(body["line"], 2, "{tool}: {body}");
        assert!(
            body["column"].as_u64().is_some_and(|c| c >= 1),
            "{tool}: {body}"
        );
    }
}

/// The additions of `contracts/check-config.schema.json` that this phase owns (`identity` is the
/// execution-identity phase's). No JSON-Schema crate is in the lock, so the schema's rules are
/// checked by hand: required keys, the `from` enum, the set-aside item shape.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn check_config_carries_the_contracted_fields() {
    let schema: Value = serde_json::from_str(
        &std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../docs/contracts/check-config.schema.json"),
        )
        .expect("the contract file"),
    )
    .unwrap();
    let from_enum: Vec<&str> = schema["$defs"]["from"]["enum"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    let set_aside_kinds: Vec<&str> = schema["properties"]["set_aside"]["items"]["properties"]
        ["kind"]["enum"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();

    let env = Env::new(&format!(
        "{RAISING}[policy.default]\ndataPolicy = \"allow\"\nglobalBlocklist = [\"^Mine*\"]\n"
    ));
    let mut s = env.start();
    let cfg = s.body("check_config", json!({}));

    for key in schema["required"].as_array().unwrap() {
        let key = key.as_str().unwrap();
        if key == "identity" {
            continue; // the execution-identity phase adds it
        }
        assert!(
            cfg.get(key).is_some(),
            "required field {key} missing: {cfg}"
        );
    }

    // config_sources
    assert!(cfg["config_sources"]["operator_file"]["path"].is_string());
    assert!(cfg["config_sources"]["operator_file"]["loaded"].is_boolean());
    let ws = &cfg["config_sources"]["workspace_file"];
    assert!(
        ["trusted", "untrusted", "lapsed", "not_needed"].contains(&ws["trust"].as_str().unwrap())
    );

    // effective_policy: one entry per server, `default` among them, every part {value, from}.
    let default = &cfg["effective_policy"]["default"];
    let required: Vec<&str> = schema["properties"]["effective_policy"]["additionalProperties"]
        ["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    for part in &required {
        let p = &default[*part];
        assert!(p.get("value").is_some(), "{part}: {p}");
        let from = &p["from"];
        let froms: Vec<&str> = match from {
            Value::String(s) => vec![s.as_str()],
            Value::Array(a) => a.iter().map(|x| x.as_str().unwrap()).collect(),
            other => panic!("{part}: from is {other}"),
        };
        for f in froms {
            assert!(
                from_enum.contains(&f),
                "{part}: from {f} is not in the contract"
            );
        }
    }
    assert_eq!(
        default["dataPolicy"]["value"], "block",
        "an untrusted `allow` is set aside"
    );
    assert_eq!(
        default["dataPolicy"]["set_aside"],
        json!(["workspace:policy.default.dataPolicy"])
    );
    assert_eq!(
        default["globalBlocklist"]["value"],
        json!(["^Mine*"]),
        "blocklists combine"
    );

    // compile_time_gate
    assert_eq!(cfg["compile_time_gate"]["enabled"], true);
    assert!(from_enum.contains(&cfg["compile_time_gate"]["from"].as_str().unwrap()));

    // set_aside items
    let items = cfg["set_aside"].as_array().unwrap();
    assert!(items.len() >= 2, "{items:?}");
    for i in items {
        for k in ["id", "kind", "setting", "reason", "lift"] {
            assert!(i.get(k).is_some_and(|v| v.is_string()), "{k} in {i}");
        }
        assert!(
            set_aside_kinds.contains(&i["kind"].as_str().unwrap()),
            "{i}"
        );
    }

    // The old string alias stays.
    assert!(cfg.get("credentials_withheld").is_some());
}

/// Every refusal carries `refusal`, whichever gate raised it: the pipeline's gates, the write gate
/// and a handler's own check (FR-039).
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn refusals_from_every_kind_of_gate_carry_the_refusal_object() {
    let env = Env::new(RAISING);
    let mut s = env.start();

    // The write gate, before dispatch: the file's `true` was set aside, so it is an untrusted setting
    // and says which item and how to lift it.
    let r = s.call(
        "iris_doc",
        json!({"mode": "put", "name": "T141.A.cls", "content": "Class T141.A {}"}),
    );
    assert_eq!(r["result"]["isError"], true);
    let body: Value =
        serde_json::from_str(r["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(body["error_code"], "WRITE_TOOLS_DISABLED");
    assert_eq!(body["refusal"]["kind"], "untrusted_setting", "{body}");
    assert_eq!(
        body["refusal"]["set_aside"],
        "workspace:write_tools_enabled"
    );
    assert_eq!(body["refusal"]["server"], "default");
    assert_eq!(body["refusal"]["lift"]["setting"], "write_tools_enabled");
    assert_eq!(
        body["refusal"]["lift"].as_object().unwrap().len(),
        1,
        "the refusal names the setting only: {body}"
    );
    let text = body.to_string();
    assert!(
        !text.contains("iris-agentic-dev.toml") && !text.contains("iris-agentic-dev trust"),
        "{text}"
    );
    // The way to lift it is the set-aside item's, which `check_config` lists.
    let cfg = s.body("check_config", json!({}));
    let item = cfg["set_aside"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == "workspace:write_tools_enabled")
        .unwrap_or_else(|| panic!("{cfg}"));
    assert!(
        item["lift"]
            .as_str()
            .unwrap()
            .contains("iris-agentic-dev trust"),
        "{item}"
    );

    // A policy gate on a named global, before dispatch: an acknowledgement.
    let r = s.body(
        "iris_global",
        json!({"action": "get", "global_name": "PAPMI"}),
    );
    assert_eq!(r["error_code"], "PHI_GATE_BLOCKED", "{r}");
    assert_eq!(r["refusal"]["kind"], "acknowledgement_required");
    assert_eq!(r["refusal"]["lift"]["argument"], "acknowledgePhi");

    // The built-in blocklist: a fixed rule.
    let r = s.body(
        "iris_global",
        json!({"action": "get", "global_name": "oddDEF"}),
    );
    assert_eq!(r["error_code"], "SYSTEM_BLOCKLIST", "{r}");
    assert_eq!(r["refusal"]["kind"], "fixed_rule");
    assert!(r["refusal"]["lift"].is_null());

    // A handler's own refusal, raised after dispatch.
    let r = s.body("iris_query", json!({"query": "GRANT %ALL TO UnknownUser"}));
    assert_eq!(r["error_code"], "SQL_WRITE_BLOCKED", "{r}");
    assert_eq!(r["refusal"]["kind"], "fixed_rule", "{r}");

    // Not a refusal: an input error carries none.
    let r = s.body("iris_query", json!({}));
    assert!(r.get("refusal").is_none(), "{r}");
}

/// FR-037: on a server whose system mode implies the dev template (no probe answer, USER), an
/// untrusted file's `mcpTemplate = "test"` tightens it and applies; `check_config` reports it as the
/// workspace file's. A looser `dev` on a live-looking namespace is set aside.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn an_untrusted_template_that_tightens_what_the_system_mode_implies_applies() {
    let env = Env::new(
        "host = \"127.0.0.1\"\nweb_port = 2\nnamespace = \"USER\"\n[policy.default]\nmcpTemplate = \"test\"\n",
    );
    let mut s = env.start();
    let cfg = s.body("check_config", json!({}));
    let t = &cfg["effective_policy"]["default"]["mcpTemplate"];
    assert_eq!(t["value"], "test", "{cfg}");
    assert_eq!(t["from"], "workspace_file", "{cfg}");
    assert!(cfg["set_aside"].as_array().unwrap().is_empty(), "{cfg}");

    let r = s.body("iris_execute", json!({"code": "Write 1"}));
    assert_eq!(r["error_code"], "ENV_GATE_BLOCKED", "{r}");
    assert_eq!(r["template"], "test");
    assert_eq!(
        r["refusal"]["kind"], "operator_limit",
        "the tightening applied, so no item: {r}"
    );

    // The same file on a production-looking namespace: `dev` would loosen live, so it is set aside.
    let env = Env::new(
        "host = \"127.0.0.1\"\nweb_port = 2\nnamespace = \"PROD\"\n[policy.default]\nmcpTemplate = \"dev\"\n",
    );
    // Writes forced on at launch, so the write gate does not answer before the template does.
    let mut s = Session::start(&env.home, &env.proj, &[("IRIS_WRITE_TOOLS_ENABLED", "1")]);
    let cfg = s.body("check_config", json!({}));
    let ids: Vec<&str> = cfg["set_aside"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["id"].as_str())
        .collect();
    assert_eq!(ids, vec!["workspace:policy.default.mcpTemplate"], "{cfg}");
    let r = s.body("iris_execute", json!({"code": "Write 1"}));
    assert_eq!(r["error_code"], "ENV_GATE_BLOCKED", "{r}");
    assert_eq!(r["refusal"]["kind"], "untrusted_setting", "{r}");
}
