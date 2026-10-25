//! A stdio MCP session against the built binary with a temporary `HOME`, shared by the spec 141
//! binary tests that edit configuration files while the server runs.

use iris_agentic_dev_core::testing::{clean_mcp_command, iad_binary_path, stop_server};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};

pub struct Session {
    child: Child,
    reader: BufReader<std::process::ChildStdout>,
    next_id: u64,
    stderr_path: PathBuf,
}

impl Session {
    pub fn start(home: &Path, cwd: &Path, env: &[(&str, &str)]) -> Session {
        let bin = iad_binary_path();
        assert!(
            bin.exists(),
            "no binary at {}; cargo build first",
            bin.display()
        );
        // One log per session, so a restart does not read the first session's lines.
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let stderr_path = home.join(format!("stderr-{n}.log"));
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

    pub fn read(&mut self, id: u64) -> Value {
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

    pub fn body(&mut self, tool: &str, args: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        let req = json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":tool,"arguments":args}});
        writeln!(self.child.stdin.as_mut().unwrap(), "{req}").unwrap();
        let answer = self.read(id);
        let text = answer
            .pointer("/result/content/0/text")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("no text in {answer}"));
        serde_json::from_str(text).unwrap_or_else(|_| panic!("not JSON: {text}"))
    }

    /// A write tool. A refused call says `WRITE_TOOLS_DISABLED`; one that passes the gate fails on
    /// the unreachable server with something else.
    pub fn put(&mut self) -> Value {
        self.body(
            "iris_doc",
            json!({"mode": "put", "name": "T141.Latch.cls", "content": "Class T141.Latch {}"}),
        )
    }

    pub fn stderr(&self) -> String {
        std::fs::read_to_string(&self.stderr_path).unwrap_or_default()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        drop(self.child.stdin.take());
        stop_server(&mut self.child);
    }
}

pub struct Env {
    _tmp: tempfile::TempDir,
    pub home: PathBuf,
    pub proj: PathBuf,
}

impl Env {
    pub fn new(operator: &str) -> Env {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let (home, proj) = (root.join("home"), root.join("proj"));
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&proj).unwrap();
        let env = Env {
            _tmp: tmp,
            home,
            proj,
        };
        env.write_operator(operator);
        env
    }
    pub fn operator_file(&self) -> PathBuf {
        self.home.join(".iris-agentic-dev.toml")
    }
    pub fn write_operator(&self, body: &str) {
        std::fs::write(self.operator_file(), body).unwrap();
    }
    /// Edit the operator file the way an agent with a file tool would, after the mtime has moved on.
    pub fn edit_operator(&self, body: &str) {
        std::thread::sleep(std::time::Duration::from_millis(1100));
        self.write_operator(body);
    }
    pub fn start(&self) -> Session {
        Session::start(&self.home, &self.proj, &[])
    }
    pub fn start_with(&self, env: &[(&str, &str)]) -> Session {
        Session::start(&self.home, &self.proj, env)
    }
}
